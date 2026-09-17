//! The `stats` render surface: the merged scope totals and the two projections.

use super::*;

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

pub(super) fn render_text(
    rows: &[SessionStats],
    top: usize,
    sub: usize,
    dropped: usize,
    duplicates: &[crate::path::DuplicateGroup],
) {
    crate::text::emit_scope_banner(top, sub);
    crate::path::emit_duplicate_banner(duplicates);
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

pub(super) fn render_json(
    rows: &[SessionStats],
    top: usize,
    sub: usize,
    dropped: usize,
    duplicates: &[crate::path::DuplicateGroup],
) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&crate::text::envelope_scope_header(
            "stats",
            top,
            sub,
            crate::path::duplicate_header_fields(duplicates)
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
