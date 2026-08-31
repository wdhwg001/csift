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
use crate::model::{group_turn_indices_deduped, Block, Record};
use crate::parse::{mmap_bytes, scan_lines_parallel, LineVerdict};
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

/// One kept line from the stats scan: a fully parsed transcript record, or just the
/// top-level `type` of a NON-candidate line (attachment / file-history-snapshot /
/// system / …) - the line-type census keeps every physical line accountable without
/// building full records for the non-record majority of bytes.
enum StatsLine {
    Record(Box<Record>),
    Other(String),
}

/// Minimal probe for a non-candidate line: full JSON syntax validation plus the top-level
/// `type` value, without building a `Record`. This UPGRADES the O(1) shape check to an
/// exact census for stats (the named corruption-census authority): a `{…}`-framed line
/// with an invalid INTERIOR is now counted malformed too. Blank → `Ok(None)`; a typeless
/// object → `"(untyped)"`.
fn line_type_probe(line: &[u8]) -> std::result::Result<Option<String>, ()> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    #[derive(serde::Deserialize)]
    struct TypeProbe {
        #[serde(rename = "type")]
        r#type: Option<String>,
    }
    match serde_json::from_slice::<TypeProbe>(line) {
        Ok(p) => Ok(Some(p.r#type.unwrap_or_else(|| "(untyped)".to_string()))),
        Err(_) => Err(()),
    }
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
            // Census every non-candidate line by its top-level `type` (full syntax
            // validation, subsuming the R10 shape check - see [`line_type_probe`]).
            return match line_type_probe(line) {
                Ok(Some(t)) => LineVerdict::Keep(StatsLine::Other(t)),
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
    // `lines`); only real records go on to the windowed aggregates below.
    let mut records: Vec<Record> = Vec::new();
    for l in kept {
        match l {
            StatsLine::Record(rec) => {
                let t = rec
                    .r#type
                    .clone()
                    .unwrap_or_else(|| "(untyped)".to_string());
                *out.line_types.entry(t).or_insert(0) += 1;
                records.push(*rec);
            }
            StatsLine::Other(t) => *out.line_types.entry(t).or_insert(0) += 1,
        }
    }

    // `--turn`: per-record turn membership on the FULL transcript's genuine-turn
    // order (the SAME 0-based axis `search`/`files` window on), computed BEFORE the time
    // filter so indices stay stable, then intersected (AND) with the window below.
    let in_turn_range: Option<Vec<bool>> = turn_range.map(|spec| {
        let all: Vec<&Record> = records.iter().collect();
        let groups = group_turn_indices_deduped(&all, |r| *r);
        let (lo, hi) = spec.resolve(groups.len(), false);
        let mut keep = vec![false; records.len()];
        for (ti, group) in groups.iter().enumerate() {
            if ti >= lo && ti <= hi {
                for &i in group {
                    keep[i] = true;
                }
            }
        }
        keep
    });

    // Windowed view for the counts; turn grouping runs over the SAME windowed set so
    // `turns` reflects what the window admits.
    let admitted: Vec<&Record> = records
        .iter()
        .enumerate()
        .filter(|(i, r)| {
            window.contains(r.timestamp.as_deref()) && in_turn_range.as_ref().is_none_or(|k| k[*i])
        })
        .map(|(_, r)| r)
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
    out.turns = group_turn_indices_deduped(&admitted, |r| *r).len();
    Ok(out)
}

/// Human-readable duration between two ISO timestamps (best-effort; None → "-").
fn duration_label(first: Option<&str>, last: Option<&str>) -> String {
    let (Some(f), Some(l)) = (first, last) else {
        return "-".to_string();
    };
    let (Ok(a), Ok(b)) = (f.parse::<jiff::Timestamp>(), l.parse::<jiff::Timestamp>()) else {
        return "-".to_string();
    };
    let secs = (b - a).get_seconds().max(0);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h{m:02}m")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
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

fn merged_narration(rows: &[SessionStats]) -> BTreeMap<String, usize> {
    let mut total: BTreeMap<String, usize> = BTreeMap::new();
    for r in rows {
        for (model, n) in &r.narration_blocks {
            *total.entry(model.clone()).or_insert(0) += n;
        }
    }
    total
}

fn merged_tools(rows: &[SessionStats]) -> BTreeMap<String, usize> {
    let mut total: BTreeMap<String, usize> = BTreeMap::new();
    for r in rows {
        for (name, n) in &r.tools {
            *total.entry(name.clone()).or_insert(0) += *n;
        }
    }
    total
}

fn merged_line_types(rows: &[SessionStats]) -> BTreeMap<String, usize> {
    let mut total: BTreeMap<String, usize> = BTreeMap::new();
    for r in rows {
        for (t, n) in &r.line_types {
            *total.entry(t.clone()).or_insert(0) += *n;
        }
    }
    total
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
        let tools = merged_tools(rows);
        println!(
            "TOTAL  {} sessions ({} top-level + {} subagent)",
            rows.len(),
            top,
            sub
        );
        let types = merged_line_types(rows);
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
        for (model, t) in &tokens {
            println!(
                "  tokens {model}: in {} · out {} · cache-read {} · cache-write {}",
                t.input, t.output, t.cache_read, t.cache_creation
            );
        }
        let narration = merged_narration(rows);
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
        "line_types": merged_line_types(rows),
        "turns": rows.iter().map(|r| r.turns).sum::<usize>(),
        "tools": merged_tools(rows),
        "tokens": tokens_json(&merged_tokens(rows)),
        "narration_blocks": merged_narration(rows),
        "unknown_thinking_tags": rows.iter().map(|r| r.unknown_thinking_tags).sum::<usize>(),
        "skipped_lines": rows.iter().map(|r| r.skipped_lines).sum::<usize>(),
        "dropped_by_cap": dropped,
    }));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
