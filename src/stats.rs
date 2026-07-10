//! `stats` subcommand — one-scan aggregates per session (and a scope total).
//!
//! Absorbs the questions that otherwise force hand-rolled jsonl parsing: "how many
//! tokens did this session burn (per model)?", "which tools ran, how often?", "how
//! many turns / compactions?", "when did it start/stop?". One fixed, rich shape —
//! no view modes, no tuning flags; `--since`/`--until` bound the counted records by
//! timestamp (a record with no timestamp never falls inside a bounded window).

use std::collections::BTreeMap;
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
}

/// Entry point for `csift stats`.
pub fn run_stats(args: &StatsArgs) -> Result<()> {
    let window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;
    let turn_range = args
        .turn_range
        .as_deref()
        .map(|s| crate::text::parse_range_spec(s, "--turn-range", false))
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
    // silent — the drop is reported. Keep the MOST RECENTLY active, then restore the
    // deterministic id order for display (the scope TOTAL then covers the shown subset).
    let mut dropped = 0usize;
    if let Some(n) = args.max_count {
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
/// cover it too — kept explicit for clarity, not reach).
fn line_is_stats_candidate(line: &[u8]) -> bool {
    memchr::memmem::find(line, br#""role":"user""#).is_some()
        || memchr::memmem::find(line, br#""role":"assistant""#).is_some()
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

    let (records, skipped): (Vec<Record>, usize) = scan_lines_parallel(bytes, |line, _| {
        if !line_is_stats_candidate(line) {
            return LineVerdict::Ignore;
        }
        match crate::parse::parse_line(line) {
            Ok(Some(rec)) => LineVerdict::Keep(rec),
            Ok(None) => LineVerdict::Ignore,
            Err(_) => LineVerdict::Skip,
        }
    });
    out.skipped_lines = skipped;

    // `--turn-range`: per-record turn membership on the FULL transcript's genuine-turn
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
                let model = msg.model_id().unwrap_or("(unknown)").to_string();
                let sums = out.tokens.entry(model).or_default();
                sums.input += u.input_tokens.unwrap_or(0);
                sums.output += u.output_tokens.unwrap_or(0);
                sums.cache_read += u.cache_read_input_tokens.unwrap_or(0);
                sums.cache_creation += u.cache_creation_input_tokens.unwrap_or(0);
            }
        }
        if let Some(blocks) = rec.blocks() {
            for b in blocks {
                if let Block::ToolUse { name, .. } = b {
                    let name = name.as_deref().unwrap_or("(unnamed)").to_string();
                    *out.tools.entry(name).or_insert(0) += 1;
                }
            }
        }
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

fn merged_tools(rows: &[SessionStats]) -> BTreeMap<String, usize> {
    let mut total: BTreeMap<String, usize> = BTreeMap::new();
    for r in rows {
        for (name, n) in &r.tools {
            *total.entry(name.clone()).or_insert(0) += *n;
        }
    }
    total
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
        if !r.tools.is_empty() {
            // Descending by count, then name — the "what ran here" glance.
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
    // Scope TOTAL block (only when >1 session — a single session IS its own total).
    if rows.len() > 1 {
        let tokens = merged_tokens(rows);
        let tools = merged_tools(rows);
        println!(
            "TOTAL  {} sessions ({} top-level + {} subagent)",
            rows.len(),
            top,
            sub
        );
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
            "user_records": r.user_records,
            "assistant_records": r.assistant_records,
            "turns": r.turns,
            "compactions": r.compactions,
            "tools": r.tools,
            "tokens": tokens_json(&r.tokens),
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
        "turns": rows.iter().map(|r| r.turns).sum::<usize>(),
        "tools": merged_tools(rows),
        "tokens": tokens_json(&merged_tokens(rows)),
        "skipped_lines": rows.iter().map(|r| r.skipped_lines).sum::<usize>(),
        "dropped_by_cap": dropped,
    }));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
