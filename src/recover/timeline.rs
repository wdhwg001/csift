//! Timeline items, span math, footers, and the Replay report impl.

use super::*;

/// A timeline item for `--patches` ordering (segments + boundaries interleaved).
#[derive(Debug)]
pub(crate) enum TimelineItem {
    Seg(Segment),
    Bound(Boundary),
}

impl TimelineItem {
    pub(crate) fn sort_key(&self) -> usize {
        match self {
            TimelineItem::Seg(s) => s.line_no_start,
            TimelineItem::Bound(b) => b.line_no,
        }
    }
}

/// Dense line vector of a buffer (gaps omitted) restricted to `--file-lines`.
pub(crate) fn filter_lines(
    buf: &SparseBuffer,
    line_range: Option<crate::text::RangeSpec>,
) -> Vec<String> {
    apply_line_range(buf.known_lines(), line_range)
        .into_iter()
        .map(|(_, t)| t)
        .collect()
}

/// The contiguous spans of a `(line_no, text)` vector, as inclusive `(lo, hi)`.
pub(crate) fn covered_spans(lines: &[(usize, String)]) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for (n, _) in lines {
        match spans.last_mut() {
            Some(last) if last.1 + 1 == *n => last.1 = *n,
            _ => spans.push((*n, *n)),
        }
    }
    spans
}

pub(crate) fn fmt_spans(spans: &[(usize, usize)]) -> String {
    if spans.is_empty() {
        return "(none)".to_string();
    }
    spans
        .iter()
        .map(|(a, b)| format!("[{a}..{b}]"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn fmt_counts(c: &EventCounts) -> String {
    let mut parts: Vec<String> = Vec::new();
    let reads = c.read_full + c.read_windowed;
    if reads > 0 {
        parts.push(format!(
            "{reads} read ({} full, {} windowed)",
            c.read_full, c.read_windowed
        ));
    }
    if c.edit > 0 {
        let unanch = if c.edit_unanchorable > 0 {
            format!(" ({} un-anchorable)", c.edit_unanchorable)
        } else {
            String::new()
        };
        parts.push(format!("{} edit{unanch}", c.edit));
    }
    if c.write > 0 {
        parts.push(format!("{} write", c.write));
    }
    if c.bash > 0 {
        parts.push(format!("{} bash (heuristic)", c.bash));
    }
    if c.external_edit > 0 {
        parts.push(format!("{} external-edit", c.external_edit));
    }
    if c.history_snapshot > 0 {
        parts.push(format!("{} history-snapshot", c.history_snapshot));
    }
    if c.integrity_error > 0 {
        parts.push(format!("{} integrity-error", c.integrity_error));
    }
    if parts.is_empty() {
        "0".to_string()
    } else {
        parts.join(" · ")
    }
}

pub(crate) fn print_footer(ctx: &RenderCtx) {
    let mode = match ctx.mode {
        RecoverMode::Patches => "patches",
        RecoverMode::At => "at",
        RecoverMode::Salvage => "salvage",
        RecoverMode::Coverage => "coverage",
        RecoverMode::Restore => "restore", // never reached - render_restore returns before the footer
    };
    println!();
    println!(
        "mode={mode}  (reconstruction is partial — unknown lines are explicit, never fabricated)"
    );
    if ctx.skipped_lines > 0 {
        println!("({})", crate::text::malformed_note(ctx.skipped_lines));
    }
}

impl Replay {
    /// Count of HARD (authoritative + heuristic-promoted) boundaries for fragment math.
    pub(crate) fn boundaries_hard_count(&self) -> usize {
        self.boundaries.len()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON rendering (NDJSON - one object per line, trailing summary)
// ─────────────────────────────────────────────────────────────────────────────

pub(crate) fn render_json(
    ctx: &RenderCtx,
    sessions: &[ScanResult],
    out_path: Option<&Path>,
) -> Result<()> {
    use serde_json::json;
    if matches!(ctx.mode, RecoverMode::Restore) {
        return render_restore(ctx, sessions, out_path, true);
    }
    let mut session_count = 0usize;

    // envelope v2: header (always) → kind-tagged rows → summary (always).
    println!(
        "{}",
        serde_json::to_string(&crate::text::envelope_scope_header(
            "recover",
            ctx.scope_top,
            ctx.scope_sub,
            json!({})
        ))?
    );

    match ctx.mode {
        RecoverMode::Restore => unreachable!("Restore handled above in render_json"),
        RecoverMode::Coverage => {
            for s in sessions {
                if s.events.is_empty() {
                    continue;
                }
                session_count += 1;
                let rep = replay(&s.events, None);
                let known = apply_line_range(rep.final_buffer.known_lines(), ctx.line_range);
                let spans = covered_spans(&known);
                let obj = json!({
                    "kind": "coverage",
                    "session_id": s.session_id,
                    "is_subagent": s.is_subagent,
                    "parent_session_id": s.parent_session_id,
                    "file": ctx.file,
                    "recoverable_lines": known.len(),
                    "seen_total_lines": rep.final_buffer.seen_total_lines,
                    "covered_ranges": spans.iter().map(|(a,b)| [*a,*b]).collect::<Vec<_>>(),
                    "fragments": rep.boundaries_hard_count() + 1,
                    "events": counts_json(&rep.counts),
                    "boundaries": rep.boundaries.iter().map(boundary_json).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string(&obj)?);
            }
        }
        RecoverMode::Patches => {
            let mut out_blob = String::new();
            for s in sessions {
                if s.events.is_empty() {
                    continue;
                }
                let rep = replay(&s.events, None);
                if rep.segments.is_empty() && rep.boundaries.is_empty() {
                    continue;
                }
                session_count += 1;
                let mut items: Vec<TimelineItem> = Vec::new();
                for seg in &rep.segments {
                    items.push(TimelineItem::Seg(seg.clone()));
                }
                for b in &rep.boundaries {
                    items.push(TimelineItem::Bound(b.clone()));
                }
                items.sort_by_key(TimelineItem::sort_key);
                for item in &items {
                    match item {
                        TimelineItem::Seg(seg) => {
                            let old = filter_lines(&seg.start_buffer, ctx.line_range);
                            let new = filter_lines(&seg.end_buffer, ctx.line_range);
                            let diff = unified_diff(&old, &new, usize::MAX);
                            out_blob.push_str(&diff);
                            let obj = json!({
                                "kind": "segment",
                                "session_id": s.session_id,
                                "is_subagent": s.is_subagent,
                                "parent_session_id": s.parent_session_id,
                                "segment_index": seg.index,
                                "line": seg.line_no_start,
                                "line_start": seg.line_no_start,
                                "line_end": seg.line_no_end,
                                "turn_start": seg.turn_start,
                                "turn_end": seg.turn_end,
                                "ts_utc": seg.ts_start,
                                "ts_local": seg.ts_start.as_deref().and_then(local_iso),
                                "pre_state_known": seg.pre_state_known,
                                "anchor_source": seg.anchor_source.map(SnapSource::label),
                                "unified_diff": diff,
                            });
                            println!("{}", serde_json::to_string(&obj)?);
                        }
                        TimelineItem::Bound(b) => {
                            let mut obj = boundary_json(b);
                            obj["kind"] = json!("boundary");
                            obj["session_id"] = json!(s.session_id);
                            obj["is_subagent"] = json!(s.is_subagent);
                            obj["parent_session_id"] = json!(s.parent_session_id);
                            println!("{}", serde_json::to_string(&obj)?);
                        }
                    }
                }
            }
            if let Some(p) = out_path {
                write_out_guarded(p, &out_blob)?;
            }
        }
        RecoverMode::At | RecoverMode::Salvage => {
            // Salvage feeds an empty `when` (ctx.at is None) → @latest (no cutoff).
            let when = ctx.at.as_deref().unwrap_or("");
            let mut out_blob = String::new();
            for s in sessions {
                if s.events.is_empty() {
                    continue;
                }
                let cutoff = resolve_cutoff(when, &s.events)?;
                let rep = replay(&s.events, cutoff);
                let known = apply_line_range(rep.final_buffer.known_lines(), ctx.line_range);
                if known.is_empty() && rep.final_buffer.seen_total_lines.is_none() {
                    continue;
                }
                session_count += 1;
                let total = rep.final_buffer.seen_total_lines;
                let gaps = gap_ranges(&known, total.unwrap_or(0));
                // Provenance: which jsonl line last set each known file line.
                let prov: BTreeMap<usize, usize> = rep
                    .final_buffer
                    .known_lines_with_provenance()
                    .into_iter()
                    .map(|(n, _, set_at)| (n, set_at))
                    .collect();
                let obj = json!({
                    "kind": "snapshot",
                    "session_id": s.session_id,
                    "is_subagent": s.is_subagent,
                    "parent_session_id": s.parent_session_id,
                    "file": ctx.file,
                    // The jsonl-line CUTOFF this snapshot reflects (`--at` resolved).
                    "line": cutoff,
                    "lines": known.iter().map(|(n,t)| json!({
                        "n": n,
                        "text": t,
                        "set_at_line": prov.get(n),
                    })).collect::<Vec<_>>(),
                    "gaps": gaps.iter().map(|(a,b)| [*a,*b]).collect::<Vec<_>>(),
                    "seen_total_lines": total,
                });
                println!("{}", serde_json::to_string(&obj)?);
                out_blob.push_str(&render_snapshot_body(&known, total.unwrap_or(0), false));
                out_blob.push('\n');
            }
            if let Some(p) = out_path {
                write_out_guarded(p, &out_blob)?;
            }
        }
    }

    // envelope v2 summary (flat - the old nested {"summary":{…}} wrapper is gone).
    let summary = crate::text::envelope_summary(json!({
        "sessions": session_count,
        "file": ctx.file,
        "mode": match ctx.mode {
            RecoverMode::Patches => "patches",
            RecoverMode::At => "at",
            RecoverMode::Salvage => "salvage",
            RecoverMode::Coverage => "coverage",
            RecoverMode::Restore => "restore",
        },
        "skipped_lines": ctx.skipped_lines,
    }));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

pub(crate) fn counts_json(c: &EventCounts) -> serde_json::Value {
    serde_json::json!({
        "read_full": c.read_full,
        "read_windowed": c.read_windowed,
        "edit": c.edit,
        "edit_unanchorable": c.edit_unanchorable,
        "write": c.write,
        "bash": c.bash,
        "external_edit": c.external_edit,
        "history_snapshot": c.history_snapshot,
        "integrity_error": c.integrity_error,
    })
}

pub(crate) fn boundary_json(b: &Boundary) -> serde_json::Value {
    serde_json::json!({
        "line": b.line_no,
        "turn_index": b.turn_index,
        "ts_utc": b.timestamp_utc,
        "ts_local": b.timestamp_utc.as_deref().and_then(local_iso),
        // WHAT invalidated the buffer (modified-since-read / external edit / bash / …) -
        // named `cause` so `kind` stays the envelope discriminator exclusively.
        "cause": b.kind,
        "confidence": b.confidence.json(),
        "detail": b.detail,
    })
}

/// The explicit gap ranges of a known-line vector up to `total` (1-based, inclusive).
pub(crate) fn gap_ranges(known: &[(usize, String)], total: usize) -> Vec<(usize, usize)> {
    let mut gaps = Vec::new();
    let mut prev = 0usize;
    let last = known.last().map(|(n, _)| *n).unwrap_or(0);
    for (n, _) in known {
        if *n > prev + 1 {
            gaps.push((prev + 1, n - 1));
        }
        prev = *n;
    }
    if total > last && last > 0 {
        gaps.push((last + 1, total));
    } else if known.is_empty() && total > 0 {
        gaps.push((1, total));
    }
    gaps
}
