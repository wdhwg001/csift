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
use crate::model::{
    group_turn_indices_chained, Block, Chain, ChainNode, CrossingBudget, CrossingReport, Record,
};
use crate::parse::{line_type_and_spine, mmap_bytes};
use crate::path::{self, SubagentScope};
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

mod render;

use render::{render_json, render_text};

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
    /// value (`user`, `assistant`, `attachment`, `file-history-snapshot`, ...; `(untyped)`
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
    /// Claude Code's conversation chain does not reach, how many of those were ANSWERED
    /// before the rewind, and lines a compaction re-anchor re-appended.
    abandoned_turns: usize,
    rewound_turns: usize,
    replay_copies: usize,
    /// What the walk did with this file's compaction boundaries under the crossing budget.
    /// A whole-file fact like the three above; silent under `--cross-compactions 0`.
    crossing: crate::model::CrossingReport,
}

/// Entry point for `csift stats`.
pub fn run_stats(args: &StatsArgs, cross: CrossingBudget) -> Result<()> {
    let window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;
    let turn_range = args
        .turn_range
        .as_deref()
        .map(|s| crate::text::parse_range_spec(s, "--turn", false))
        .transpose()?;
    let resolved = path::resolve_targets_with_session_list_reported(
        &args.paths,
        args.sessions_from.as_deref(),
        SubagentScope::from(args.want_subagents()),
        path::Caller::Other,
    )?;
    let files = resolved.files;
    let mut rows: Vec<SessionStats> = files
        .par_iter()
        .map(|p| stats_one_file(p, &window, turn_range, cross))
        .collect::<Result<Vec<_>>>()?;
    rows.sort_by(|a, b| a.session_id.cmp(&b.session_id));

    // The SCOPE is what the target resolved to, so it is counted before the cap below trims
    // the rows: the cap bounds what is printed and never what was asked about, the rule
    // `list` already keeps. DISTINCT session ids, through the shared rule: a divergent
    // duplicate pair is one session on disk twice, and `sessions_in_scope` is a session census.
    let (top, sub) =
        crate::subagent::scope_span(rows.iter().map(|r| (r.session_id.clone(), r.is_subagent)));

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

    match args.format {
        OutputFormat::Text => render_text(&rows, top, sub, dropped, cross, &resolved.duplicates),
        OutputFormat::Json => {
            render_json(&rows, top, sub, dropped, cross, &resolved.duplicates)?;
        }
    }
    Ok(())
}

/// Broad candidate prefilter: every countable record is a `role:user`/`role:assistant`
/// message line or an `isCompactSummary` carrier (itself role:user, so the role probes
/// cover it too - kept explicit for clarity, not reach).
fn line_is_stats_candidate(line: &[u8]) -> bool {
    // The needle law: serialization-tolerant (whitespace around the colon is the same record).
    crate::parse::line_has_role_marker(line)
}

/// One NON-CANDIDATE line: its 1-based line, its top-level `type` for the census (which
/// keeps every line accountable without building records for the non-record majority of
/// bytes), and its chain spine row when the loader admits this line type.
struct OtherLine {
    census: String,
    spine: Option<crate::parse::SpineRow>,
}

fn stats_one_file(
    path: &Path,
    window: &TimeWindow,
    turn_range: Option<crate::text::RangeSpec>,
    cross: CrossingBudget,
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

    let (records, others, skipped): (Vec<(usize, Record)>, Vec<OtherLine>, usize) =
        crate::parse::scan_lines_parallel_split(bytes, |line, line_no| {
            if !line_is_stats_candidate(line) {
                // Census every non-candidate line by its top-level `type` AND lift its chain
                // spine from the same parse (full syntax validation, subsuming the
                // malformed-line rule's shape check - [`crate::parse::line_type_and_spine`]),
                // own stream.
                return match line_type_and_spine(line_no, line) {
                    Ok(Some((census, spine))) => {
                        crate::parse::SplitVerdict::Second(OtherLine { census, spine })
                    }
                    Ok(None) => crate::parse::SplitVerdict::Ignore,
                    Err(()) => crate::parse::SplitVerdict::Skip,
                };
            }
            match crate::parse::parse_line(line) {
                Ok(Some(rec)) => crate::parse::SplitVerdict::First((line_no, rec)),
                Ok(None) => crate::parse::SplitVerdict::Ignore,
                Err(_) => crate::parse::SplitVerdict::Skip,
            }
        });
    out.skipped_lines = skipped;
    // EVERY line lands in the type census (a file fact, like `lines`); only real records
    // reach the windowed aggregates. Spine rows ride along IN FILE ORDER so the chain walk
    // sees the DAG, and are excluded from every aggregate (no message, no usage, and their
    // timestamps are not the conversation's). The merge is BORROWED: the wide records are
    // never copied and the narrow spine never enters their vector.
    for (_, rec) in &records {
        let t = rec.r#type.as_deref().unwrap_or("(untyped)");
        *out.line_types.entry(t.to_string()).or_insert(0) += 1;
    }
    let mut spine: Vec<&crate::parse::SpineRow> = Vec::new();
    for o in &others {
        *out.line_types.entry(o.census.clone()).or_insert(0) += 1;
        if let Some(r) = o.spine.as_ref() {
            spine.push(r);
        }
    }
    let mut rows: Vec<ChainNode<'_>> = Vec::with_capacity(records.len() + spine.len());
    // The jsonl line of each merged row, so the crossing report can name the floor's own
    // line; the chain answers in row indices and nothing else here carries the mapping.
    let mut line_at: Vec<usize> = Vec::with_capacity(records.len() + spine.len());
    let (mut a, mut b) = (0usize, 0usize);
    while a < records.len() || b < spine.len() {
        let take_record = match (records.get(a), spine.get(b)) {
            (Some((la, _)), Some(sb)) => *la <= sb.line(),
            (Some(_), None) => true,
            _ => false,
        };
        if take_record {
            rows.push(ChainNode::Full(&records[a].1));
            line_at.push(records[a].0);
            a += 1;
        } else {
            rows.push(ChainNode::Spine(spine[b]));
            line_at.push(spine[b].line());
            b += 1;
        }
    }

    // ONE chain, ONE grouping (section 3.3): `turns` counts LIVE numbered turns - the exact
    // numbering `search` prints as `·tN` and `show --turn` addresses - and the three
    // survival totals are whole-file facts read straight off the chain.
    let chain = Chain::build_by(&rows, |r| *r, None, cross);
    out.abandoned_turns = chain.drafts + chain.rewound_turns;
    out.rewound_turns = chain.rewound_turns;
    out.replay_copies = chain.replay_copies;
    out.crossing = chain.crossing(
        line_at.get(chain.floor).copied(),
        rows.get(chain.floor)
            .and_then(|r| r.timestamp())
            .map(str::to_string),
    );
    let groups = group_turn_indices_chained(&rows, &chain);

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
        rows[i]
            .full()
            .is_some_and(|r| window.contains(r.timestamp.as_deref()))
            && in_turn_range.as_ref().is_none_or(|k| k[i])
    };
    let admitted: Vec<&Record> = (0..rows.len())
        .filter(|&i| admit(i))
        .filter_map(|i| rows[i].full())
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
