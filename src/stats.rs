//! `stats` subcommand - one-scan aggregates per session (and a scope total).
//!
//! Absorbs the questions that otherwise force hand-rolled jsonl parsing: "how many
//! tokens did this session burn (per model)?", "which tools ran, how often?", "how
//! many turns / compactions?", "when did it start/stop?". One fixed, rich shape -
//! no view modes, no tuning flags; `--since`/`--until` bound the counted records by
//! timestamp (a record with no timestamp never falls inside a bounded window).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::Result;
use rayon::prelude::*;
use serde_json::json;

use crate::cli::{OutputFormat, StatsArgs};
use crate::model::{group_turn_indices_chained, Block, Chain, Record};
use crate::parse::{line_type_and_spine, mmap_bytes, scan_lines_parallel, LineVerdict};
use crate::path::{self, SubagentScope};
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

/// Per-model token sums (each side summed independently; absent fields count 0).
#[derive(Debug, Clone, Copy, Default)]
struct TokenSums {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_creation: u64,
}

/// One session's aggregates.
#[derive(Debug, Default)]
struct SessionStats {
    session_id: String,
    is_subagent: bool,
    parent_session_id: String,
    lines: usize,
    user_records: usize,
    assistant_records: usize,
    turns: usize,
    tools: BTreeMap<String, usize>,
    tokens: BTreeMap<String, TokenSums>,
    first_utc: Option<String>,
    last_utc: Option<String>,
    compactions: usize,
    skipped_lines: usize,
    /// Whole-file census: every parseable physical line counted by its top-level `type`
    /// value (`user`, `assistant`, `attachment`, `file-history-snapshot`, …; `(untyped)`
    /// when the field is absent). A FILE fact like `lines`: never windowed by
    /// `--since`/`--turn`.
    line_types: BTreeMap<String, usize>,
    /// Narration-tagged thinking blocks per model (`agent.thinking.narration`: an
    /// API-issued summary of the reasoning beside it). A BLOCK count only - the token
    /// split is not derivable from the jsonl (usage is per MESSAGE and covers the
    /// reasoning block and its narration sibling together).
    narration_blocks: BTreeMap<String, usize>,
    /// Thinking-signature tags that decoded to something OTHER than thinking or
    /// narration - a new tag value surfaces here without a csift release.
    unknown_thinking_tags: usize,
    /// SURVIVAL-AXIS facts of the WHOLE file (like `lines` and `line_types`, never
    /// windowed - an abandoned opener carries no turn index to window on): turn openers
    /// Claude Code's conversation chain no longer reaches, how many of those were
    /// ANSWERED before the rewind, and lines a compaction re-anchor re-appended.
    abandoned_turns: usize,
    rewound_turns: usize,
    replay_copies: usize,
}

/// Entry point for `csift stats`.
pub fn run_stats(args: &StatsArgs) -> Result<()> {
    let window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;
    let turn_range = args
        .turn_range
        .as_deref()
        .map(|s| crate::text::parse_range_spec(s, "--turn", false))
        .transpose()?;
    let files = path::resolve_targets_with_session_list(
        &args.paths,
        args.sessions_from.as_deref(),
        SubagentScope::from(args.want_subagents()),
        path::Caller::Other,
    )?;
    let mut rows: Vec<SessionStats> = files
        .par_iter()
        .map(|p| stats_one_file(p, &window, turn_range))
        .collect::<Result<Vec<_>>>()?;
    rows.sort_by(|a, b| a.session_id.cmp(&b.session_id));

    // Context-safety cap (T2.1, opt-in): bound an unscoped run's per-session rows. NEVER
    // silent - the drop is reported. Keep the MOST RECENTLY active, then restore the
    // deterministic id order for display (the scope TOTAL then covers the shown subset).
    let mut dropped = 0usize;
    // `--max-count 0` = uncapped (the crate-wide convention).
    if let Some(n) = args.max_count.filter(|&n| n > 0) {
        if rows.len() > n {
            rows.sort_by(|a, b| b.last_utc.cmp(&a.last_utc));
            dropped = rows.len() - n;
            rows.truncate(n);
            rows.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        }
    }

    let sub = rows.iter().filter(|r| r.is_subagent).count();
    let top = rows.len() - sub;
    match args.format {
        OutputFormat::Text => render_text(&rows, top, sub, dropped),
        OutputFormat::Json => render_json(&rows, top, sub, dropped)?,
    }
    Ok(())
}

/// Broad candidate prefilter: every countable record is a `role:user`/`role:assistant`
/// message line or an `isCompactSummary` carrier (itself role:user, so the role probes
/// cover it too - kept explicit for clarity, not reach).
fn line_is_stats_candidate(line: &[u8]) -> bool {
    // R13: serialization-tolerant (whitespace around the colon is the same record).
    crate::parse::line_has_role_marker(line)
}

/// One kept line from the stats scan: a fully parsed transcript record, or the top-level
/// `type` of a NON-candidate line (attachment / file-history-snapshot / system / …) - the
/// line-type census keeps every physical line accountable without building full records
/// for the non-record majority of bytes. A non-candidate line ALSO yields its structural
/// spine row when the line type is one the conversation chain walks through, so the
/// SURVIVAL AXIS sees the whole DAG without the payload ever being parsed.
enum StatsLine {
    Record(Box<Record>),
    Other(String, Option<Box<Record>>),
}

fn stats_one_file(
    path: &Path,
    window: &TimeWindow,
    turn_range: Option<crate::text::RangeSpec>,
) -> Result<SessionStats> {
    let session_id = crate::subagent::session_id_from_path(path);
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());
    let mut out = SessionStats {
        session_id,
        is_subagent,
        parent_session_id,
        ..SessionStats::default()
    };

    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(out);
    };
    let bytes: &[u8] = &mmap;
    // Total physical lines = newline count (+1 for a torn final fragment).
    out.lines = memchr::memchr_iter(b'\n', bytes).count()
        + usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n"));

    let (kept, skipped): (Vec<StatsLine>, usize) = scan_lines_parallel(bytes, |line, _| {
        if !line_is_stats_candidate(line) {
            // Census every non-candidate line by its top-level `type` AND lift its chain
            // spine from the same parse (full syntax validation, subsuming the R10 shape
            // check - see [`crate::parse::line_type_and_spine`]).
            return match line_type_and_spine(line) {
                Ok(Some((t, spine))) => LineVerdict::Keep(StatsLine::Other(t, spine.map(Box::new))),
                Ok(None) => LineVerdict::Ignore,
                Err(()) => LineVerdict::Skip,
            };
        }
        match crate::parse::parse_line(line) {
            Ok(Some(rec)) => LineVerdict::Keep(StatsLine::Record(Box::new(rec))),
            Ok(None) => LineVerdict::Ignore,
            Err(_) => LineVerdict::Skip,
        }
    });
    out.skipped_lines = skipped;
    // Split the kept lines: EVERY line lands in the type census (a file fact, like
    // `lines`); only real records go on to the windowed aggregates below. Spine rows ride
    // along IN FILE ORDER so the chain walk sees the DAG, and are excluded from every
    // aggregate (they carry no message, no usage, and their timestamps are not the
    // conversation's - counting them would widen the span).
    let mut rows: Vec<(Record, bool)> = Vec::new();
    for l in kept {
        match l {
            StatsLine::Record(rec) => {
                let t = rec
                    .r#type
                    .clone()
                    .unwrap_or_else(|| "(untyped)".to_string());
                *out.line_types.entry(t).or_insert(0) += 1;
                rows.push((*rec, false));
            }
            StatsLine::Other(t, spine) => {
                *out.line_types.entry(t).or_insert(0) += 1;
                if let Some(s) = spine {
                    rows.push((*s, true));
                }
            }
        }
    }

    // ONE chain, ONE grouping (§3.3): `turns` counts LIVE numbered turns - the exact
    // numbering `search` prints as `·tN` and `show --turn` addresses - and the three
    // survival totals are whole-file facts read straight off the chain.
    let chain = Chain::build_by(&rows, |(r, _)| r, None);
    out.abandoned_turns = chain.drafts + chain.rewound_turns;
    out.rewound_turns = chain.rewound_turns;
    out.replay_copies = chain.replay_copies;
    let groups = group_turn_indices_chained(&rows, |(r, _)| r, &chain);

    // `--turn`: per-record membership on the FULL transcript's LIVE turn order, computed
    // BEFORE the time filter so indices stay stable, then intersected (AND) with it.
    let in_turn_range: Option<Vec<bool>> = turn_range.map(|spec| {
        let (lo, hi) = spec.resolve(groups.len(), false);
        let mut keep = vec![false; rows.len()];
        for (ti, group) in groups.iter().enumerate() {
            if ti >= lo && ti <= hi {
                for &i in group {
                    keep[i] = true;
                }
            }
        }
        keep
    });

    // Windowed view for the counts; a turn counts when the window admits >=1 of ITS
    // records, so `turns` reflects the window without re-deriving the numbering.
    let admit = |i: usize| {
        let (r, spine) = &rows[i];
        !spine
            && window.contains(r.timestamp.as_deref())
            && in_turn_range.as_ref().is_none_or(|k| k[i])
    };
    let admitted: Vec<&Record> = (0..rows.len())
        .filter(|&i| admit(i))
        .map(|i| &rows[i].0)
        .collect();

    let mut usage_peak: HashMap<(String, String), [u64; 4]> = HashMap::new();
    for rec in &admitted {
        match rec.r#type.as_deref() {
            Some("user") => out.user_records += 1,
            Some("assistant") => out.assistant_records += 1,
            _ => {}
        }
        if rec.is_compact_summary.unwrap_or(false) {
            out.compactions += 1;
        }
        if let Some(ts) = rec.timestamp.as_deref() {
            if out.first_utc.as_deref().is_none_or(|f| ts < f) {
                out.first_utc = Some(ts.to_string());
            }
            if out.last_utc.as_deref().is_none_or(|l| ts > l) {
                out.last_utc = Some(ts.to_string());
            }
        }
        if let Some(msg) = rec.message.as_ref() {
            if let Some(u) = msg.token_usage() {
                // CC repeats the IDENTICAL message.usage on every per-block record of
                // one API message; summing per record over-reports 2.2-3.5x (measured).
                // Dedupe per FILE by message.id, taking the per-field MAX across the
                // id's admitted records: identical on clean data, and immune to the
                // compaction-replay shape where a replayed copy carries ZEROED usage
                // (first-wins would depend on traversal order). An id-less record
                // counts on its own, as before.
                let model = msg.model_id().unwrap_or("(unknown)").to_string();
                let vals = [
                    u.input_tokens.unwrap_or(0),
                    u.output_tokens.unwrap_or(0),
                    u.cache_read_input_tokens.unwrap_or(0),
                    u.cache_creation_input_tokens.unwrap_or(0),
                ];
                match msg.id.as_deref() {
                    Some(id) if !id.is_empty() => {
                        let peak = usage_peak
                            .entry((model, id.to_string()))
                            .or_insert([0u64; 4]);
                        for (p, v) in peak.iter_mut().zip(vals) {
                            *p = (*p).max(v);
                        }
                    }
                    _ => {
                        let sums = out.tokens.entry(model).or_default();
                        sums.input += vals[0];
                        sums.output += vals[1];
                        sums.cache_read += vals[2];
                        sums.cache_creation += vals[3];
                    }
                }
            }
        }
        if let Some(blocks) = rec.blocks() {
            for b in blocks {
                if let Block::ToolUse { name, .. } = b {
                    let name = name.as_deref().unwrap_or("(unnamed)").to_string();
                    *out.tools.entry(name).or_insert(0) += 1;
                }
                if let Block::Thinking { signature, .. } = b {
                    match crate::model::thinking_signature_tag(signature.as_deref()).as_deref() {
                        Some(crate::model::NARRATION_TAG) => {
                            let model = rec
                                .message
                                .as_ref()
                                .and_then(|m| m.model_id())
                                .unwrap_or("(unknown)")
                                .to_string();
                            *out.narration_blocks.entry(model).or_insert(0) += 1;
                        }
                        Some("thinking") | None => {}
                        Some(_) => out.unknown_thinking_tags += 1,
                    }
                }
            }
        }
    }
    for ((model, _), vals) in usage_peak {
        let sums = out.tokens.entry(model).or_default();
        sums.input += vals[0];
        sums.output += vals[1];
        sums.cache_read += vals[2];
        sums.cache_creation += vals[3];
    }
    out.turns = groups
        .iter()
        .filter(|g| g.iter().any(|&i| admit(i)))
        .count();
    Ok(out)
}

/// Human-readable duration between two ISO timestamps (best-effort; None → "-"). The
/// rendering is the shared one (`agents` prints the same shapes), so the two never drift.
fn duration_label(first: Option<&str>, last: Option<&str>) -> String {
    let (Some(f), Some(l)) = (first, last) else {
        return "-".to_string();
    };
    let (Ok(a), Ok(b)) = (f.parse::<jiff::Timestamp>(), l.parse::<jiff::Timestamp>()) else {
        return "-".to_string();
    };
    crate::subagent::fmt_secs((b - a).get_seconds().max(0))
}

/// Scope law: usage dedupe is PER FILE (each row already deduped by message.id). The
/// same id CAN recur across a session's transcripts - the spawn message is copied into
/// each child's opening context with its own usage - and each copy is a genuine
/// per-transcript fact, so the TOTAL row sums them; it never dedupes across files.
fn merged_tokens(rows: &[SessionStats]) -> BTreeMap<String, TokenSums> {
    let mut total: BTreeMap<String, TokenSums> = BTreeMap::new();
    for r in rows {
        for (model, t) in &r.tokens {
            let e = total.entry(model.clone()).or_default();
            e.input += t.input;
            e.output += t.output;
            e.cache_read += t.cache_read;
            e.cache_creation += t.cache_creation;
        }
    }
    total
}

/// Sum one per-session `key → count` census across the scope (tools, line types,
/// narration blocks - one merge, so a fourth census cannot drift into its own copy).
fn merged_counts<'a>(
    rows: &'a [SessionStats],
    pick: impl Fn(&'a SessionStats) -> &'a BTreeMap<String, usize>,
) -> BTreeMap<String, usize> {
    let mut total: BTreeMap<String, usize> = BTreeMap::new();
    for r in rows {
        for (k, n) in pick(r) {
            *total.entry(k.clone()).or_insert(0) += *n;
        }
    }
    total
}

/// The SURVIVAL-AXIS line: openers the conversation chain no longer reaches, and lines a
/// compaction re-anchor re-appended. Printed only when there is something to say, so an
/// ordinary transcript's block is unchanged.
fn chain_line(abandoned: usize, rewound: usize, replays: usize) {
    if abandoned > 0 || replays > 0 {
        println!(
            "  chain  abandoned turns {abandoned} ({rewound} rewound) · replay copy lines \
             {replays}  (whole-file facts, outside `turns`)"
        );
    }
}

/// Count-desc `key×n` census line (`types` rows; the small closed-ish type space needs no
/// cap - a cap would be silent truncation).
fn line_types_line(types: &BTreeMap<String, usize>) -> String {
    let mut lt: Vec<(&String, &usize)> = types.iter().collect();
    lt.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    lt.iter()
        .map(|(k, v)| format!("{k}×{v}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn render_text(rows: &[SessionStats], top: usize, sub: usize, dropped: usize) {
    crate::text::emit_scope_banner(top, sub);
    for r in rows {
        if r.is_subagent {
            println!(
                "SUBAGENT {}  ·  parent SESSION {}",
                r.session_id, r.parent_session_id
            );
        } else {
            println!("SESSION {}", r.session_id);
        }
        println!(
            "  lines {} · records {} user + {} assistant · turns {} · compactions {}",
            r.lines, r.user_records, r.assistant_records, r.turns, r.compactions
        );
        if !r.line_types.is_empty() {
            println!("  types  {}", line_types_line(&r.line_types));
        }
        chain_line(r.abandoned_turns, r.rewound_turns, r.replay_copies);
        if let (Some(f), Some(l)) = (r.first_utc.as_deref(), r.last_utc.as_deref()) {
            println!(
                "  span   {}  →  {}  ({})",
                format_timestamp(Some(f)),
                format_timestamp(Some(l)),
                duration_label(Some(f), Some(l))
            );
        }
        if !r.tokens.is_empty() {
            for (model, t) in &r.tokens {
                println!(
                    "  tokens {model}: in {} · out {} · cache-read {} · cache-write {}",
                    t.input, t.output, t.cache_read, t.cache_creation
                );
            }
        }
        if !r.narration_blocks.is_empty() {
            println!(
                "  narration blocks {}  (API summaries, agent.thinking.narration; token split unavailable)",
                line_types_line(&r.narration_blocks)
            );
        }
        if r.unknown_thinking_tags > 0 {
            println!(
                "  unknown thinking-signature tags {}  (neither thinking nor narration - a new API tag value)",
                r.unknown_thinking_tags
            );
        }
        if !r.tools.is_empty() {
            // Descending by count, then name - the "what ran here" glance.
            let mut tools: Vec<(&String, &usize)> = r.tools.iter().collect();
            tools.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            let line: Vec<String> = tools.iter().map(|(k, v)| format!("{k}×{v}")).collect();
            println!("  tools  {}", line.join(" · "));
        }
        if r.skipped_lines > 0 {
            println!("  ({})", crate::text::malformed_note(r.skipped_lines));
        }
        println!();
    }
    // Scope TOTAL block (only when >1 session - a single session IS its own total).
    if rows.len() > 1 {
        let tokens = merged_tokens(rows);
        let tools = merged_counts(rows, |r| &r.tools);
        println!(
            "TOTAL  {} sessions ({} top-level + {} subagent)",
            rows.len(),
            top,
            sub
        );
        let types = merged_counts(rows, |r| &r.line_types);
        if !types.is_empty() {
            println!("  types  {}", line_types_line(&types));
        }
        println!(
            "  records {} user + {} assistant · turns {} · compactions {}",
            rows.iter().map(|r| r.user_records).sum::<usize>(),
            rows.iter().map(|r| r.assistant_records).sum::<usize>(),
            rows.iter().map(|r| r.turns).sum::<usize>(),
            rows.iter().map(|r| r.compactions).sum::<usize>(),
        );
        chain_line(
            rows.iter().map(|r| r.abandoned_turns).sum(),
            rows.iter().map(|r| r.rewound_turns).sum(),
            rows.iter().map(|r| r.replay_copies).sum(),
        );
        for (model, t) in &tokens {
            println!(
                "  tokens {model}: in {} · out {} · cache-read {} · cache-write {}",
                t.input, t.output, t.cache_read, t.cache_creation
            );
        }
        let narration = merged_counts(rows, |r| &r.narration_blocks);
        if !narration.is_empty() {
            println!("  narration blocks {}", line_types_line(&narration));
        }
        if !tools.is_empty() {
            let mut ts: Vec<(&String, &usize)> = tools.iter().collect();
            ts.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            let line: Vec<String> = ts
                .iter()
                .take(15)
                .map(|(k, v)| format!("{k}×{v}"))
                .collect();
            let extra = ts.len().saturating_sub(15);
            print!("  tools  {}", line.join(" · "));
            if extra > 0 {
                print!(" · (+{extra} more tools)");
            }
            println!();
        }
    }
    let skipped: usize = rows.iter().map(|r| r.skipped_lines).sum();
    if skipped > 0 {
        println!("({})", crate::text::malformed_note(skipped));
    }
    if dropped > 0 {
        println!(
            "… (+{dropped} more session(s) not shown — the most recently active are aggregated \
             above; narrow with a target or --since, or raise --max-count)"
        );
    }
}

fn tokens_json(tokens: &BTreeMap<String, TokenSums>) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = tokens
        .iter()
        .map(|(model, t)| {
            (
                model.clone(),
                json!({
                    "input": t.input,
                    "output": t.output,
                    "cache_read": t.cache_read,
                    "cache_creation": t.cache_creation,
                }),
            )
        })
        .collect();
    serde_json::Value::Object(map)
}

fn render_json(rows: &[SessionStats], top: usize, sub: usize, dropped: usize) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&crate::text::envelope_scope_header(
            "stats",
            top,
            sub,
            json!({})
        ))?
    );
    for r in rows {
        let obj = json!({
            "kind": "session",
            "session_id": r.session_id,
            "is_subagent": r.is_subagent,
            "parent_session_id": r.parent_session_id,
            "lines": r.lines,
            "line_types": r.line_types,
            "user_records": r.user_records,
            "assistant_records": r.assistant_records,
            "turns": r.turns,
            "abandoned_turns": r.abandoned_turns,
            "rewound_turns": r.rewound_turns,
            "replay_copies": r.replay_copies,
            "compactions": r.compactions,
            "tools": r.tools,
            "tokens": tokens_json(&r.tokens),
            "narration_blocks": r.narration_blocks,
            "unknown_thinking_tags": r.unknown_thinking_tags,
            "first_utc": r.first_utc,
            "first_local": r.first_utc.as_deref().and_then(local_iso),
            "last_utc": r.last_utc,
            "last_local": r.last_utc.as_deref().and_then(local_iso),
            "skipped_lines": r.skipped_lines,
        });
        println!("{}", serde_json::to_string(&obj)?);
    }
    let summary = crate::text::envelope_summary(json!({
        "sessions": rows.len(),
        "line_types": merged_counts(rows, |r| &r.line_types),
        "turns": rows.iter().map(|r| r.turns).sum::<usize>(),
        "abandoned_turns": rows.iter().map(|r| r.abandoned_turns).sum::<usize>(),
        "rewound_turns": rows.iter().map(|r| r.rewound_turns).sum::<usize>(),
        "replay_copies": rows.iter().map(|r| r.replay_copies).sum::<usize>(),
        "tools": merged_counts(rows, |r| &r.tools),
        "tokens": tokens_json(&merged_tokens(rows)),
        "narration_blocks": merged_counts(rows, |r| &r.narration_blocks),
        "unknown_thinking_tags": rows.iter().map(|r| r.unknown_thinking_tags).sum::<usize>(),
        "skipped_lines": rows.iter().map(|r| r.skipped_lines).sum::<usize>(),
        "dropped_by_cap": dropped,
    }));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
