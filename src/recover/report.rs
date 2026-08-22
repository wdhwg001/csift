//! Report fragments: range strings, session headers, coverage + patches text.

use super::*;

/// Compress sorted line numbers to a compact `1-50, 52, 60-72` range string (`none` when empty).
pub(crate) fn ranges_str(nums: &[usize]) -> String {
    if nums.is_empty() {
        return "none".to_string();
    }
    let mut sorted: Vec<usize> = nums.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let fmt = |a: usize, b: usize| {
        if a == b {
            a.to_string()
        } else {
            format!("{a}-{b}")
        }
    };
    let mut parts = Vec::new();
    let (mut start, mut prev) = (sorted[0], sorted[0]);
    for &n in &sorted[1..] {
        if n == prev + 1 {
            prev = n;
        } else {
            parts.push(fmt(start, prev));
            start = n;
            prev = n;
        }
    }
    parts.push(fmt(start, prev));
    parts.join(", ")
}

/// The `1..=total` line numbers NOT present in `known`, as a compact range string.
pub(crate) fn missing_ranges_str(known: &[(usize, String)], total: usize) -> String {
    let present: std::collections::HashSet<usize> = known.iter().map(|(n, _)| *n).collect();
    let missing: Vec<usize> = (1..=total).filter(|n| !present.contains(n)).collect();
    ranges_str(&missing)
}

/// Print the per-transcript header. A SUBAGENT transcript is branded
/// `SUBAGENT <hex> · parent SESSION <uuid>` (mirroring list/files/search/turns text) - its
/// `session_id` is a bare hex, NOT a re-feedable `@<uuid>` target, so it must never be
/// tokened a bare `SESSION`. A top-level transcript prints `SESSION <uuid>`.
pub(crate) fn session_header(first: &mut bool, s: &ScanResult) {
    if !*first {
        println!();
    }
    *first = false;
    if s.is_subagent {
        println!(
            "SUBAGENT {}  ·  parent SESSION {}",
            s.session_id, s.parent_session_id
        );
    } else {
        println!("SESSION {}", s.session_id);
    }
}

pub(crate) fn render_coverage_text(ctx: &RenderCtx, sessions: &[ScanResult]) -> Result<()> {
    let file = ctx.file.as_deref().unwrap_or("(none)");
    let mut first = true;
    let mut any = false;
    for s in sessions {
        if s.events.is_empty() {
            continue;
        }
        any = true;
        let rep = replay(&s.events, None);
        session_header(&mut first, s);
        println!("  file: {file}");
        let ranges = apply_line_range(rep.final_buffer.known_lines(), ctx.line_range);
        let known = ranges.len();
        let total = rep.final_buffer.seen_total_lines.unwrap_or(known);
        let pct = if total > 0 { known * 100 / total } else { 0 };
        let fragments = rep.boundaries_hard_count() + 1;
        println!("  recoverable: {known}/{total} lines ({pct}%)  fragments: {fragments}");
        let covered = covered_spans(&ranges);
        println!("  covered line ranges: {}", fmt_spans(&covered));
        println!("  events: {}", fmt_counts(&rep.counts));
        if rep.boundaries.is_empty() {
            println!("  integrity boundaries: (none)");
        } else {
            println!(
                "  integrity boundaries: {} ({} hard · {} soft)",
                rep.boundaries.len(),
                rep.boundaries_hard_count(),
                rep.boundaries_soft_count()
            );
            print_boundary_lines(s, &rep.boundaries);
        }
        if rep.counts.edit_unanchorable > 0 {
            println!(
                "  un-anchorable edits (coverage holes): {}",
                rep.counts.edit_unanchorable
            );
        }
        print_window_disclosure(s, ctx);
    }
    if !any {
        println!("no recoverable history for {file} in range");
    }
    print_footer(ctx);
    Ok(())
}

/// The shared per-boundary text lines (coverage / at / salvage), locations routed
/// through [`boundary_loc`] so a merged group shows real transcript lines.
pub(crate) fn print_boundary_lines(s: &ScanResult, boundaries: &[Boundary]) {
    for b in boundaries {
        let sym = if b.confidence == Confidence::Authoritative {
            "⚠"
        } else {
            "~"
        };
        println!(
            "    {sym} {}  turn {}  {}  {} ({})",
            boundary_loc(s, b.line_no),
            b.turn_index,
            format_timestamp(b.timestamp_utc.as_deref()),
            b.detail,
            b.confidence.label()
        );
    }
}

/// The shared per-session window-disclosure text (coverage / at / salvage / patches):
/// the opaque command counts, the first few commands themselves (real transcript
/// lines, capped with an explicit remainder), and the paste-runnable inspection
/// search. Prints nothing when the window is clean of opaque commands.
pub(crate) fn print_window_disclosure(s: &ScanResult, ctx: &RenderCtx) {
    let d = window_disclosure(s, ctx.file.as_deref());
    if let Some(note) = opaque_note(&d) {
        println!("  opaque in window: {note}");
        const CAP: usize = 5;
        for o in s.opaque.iter().take(CAP) {
            // An opaque row keeps its REAL transcript line; name the transcript when
            // it is not the row's own (a merged group).
            let loc = if o.session_id == s.session_id {
                format!("L{}", o.line_no)
            } else {
                format!("L{} in {}", o.line_no, o.session_id)
            };
            println!(
                "    · {loc}  turn {}  {}  {}",
                o.turn_index,
                format_timestamp(o.timestamp_utc.as_deref()),
                o.marker
            );
        }
        if s.opaque.len() > CAP {
            println!("    (+{} more; use the search below)", s.opaque.len() - CAP);
        }
        if let Some(cmd) = &d.suggested {
            println!("  inspect the window: {cmd}");
        }
    }
}

pub(crate) fn render_patches_text(
    ctx: &RenderCtx,
    sessions: &[ScanResult],
    out_path: Option<&Path>,
) -> Result<()> {
    let file = ctx.file.as_deref().unwrap_or("(none)");
    let mut first = true;
    let mut any = false;
    let mut out_blob = String::new();

    for s in sessions {
        if s.events.is_empty() {
            continue;
        }
        let rep = replay(&s.events, None);
        if rep.segments.is_empty() && rep.boundaries.is_empty() {
            continue;
        }
        any = true;
        session_header(&mut first, s);
        println!("  file: {file}");

        // Interleave segments + boundaries in jsonl-line order.
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
                    let pre = match (seg.pre_state_known, seg.anchor_source) {
                        (_, Some(src)) => format!("pre-state: {} anchor", src.label()),
                        (true, None) => "pre-state: known".to_string(),
                        (false, None) => "pre-state PARTIALLY UNKNOWN after boundary".to_string(),
                    };
                    println!(
                        "  ─ SEGMENT {}  L{}..L{}  turns {}..{}  {}..{}  ({pre}) ─",
                        seg.index,
                        seg.line_no_start,
                        seg.line_no_end,
                        seg.turn_start,
                        seg.turn_end,
                        format_timestamp(seg.ts_start.as_deref()),
                        format_timestamp(seg.ts_end.as_deref()),
                    );
                    let old = filter_lines(&seg.start_buffer, ctx.line_range);
                    let new = filter_lines(&seg.end_buffer, ctx.line_range);
                    let diff = unified_diff(&old, &new, usize::MAX);
                    if diff.is_empty() {
                        println!("  (no change in this segment)");
                    } else {
                        for line in diff.lines() {
                            println!("  {line}");
                        }
                    }
                    out_blob.push_str(&diff);
                }
                TimelineItem::Bound(b) => {
                    let sym = if b.confidence == Confidence::Authoritative {
                        "⚠"
                    } else {
                        "~"
                    };
                    println!(
                        "  {sym} INTEGRITY BOUNDARY  {}  turn {}  {}  {} ({})",
                        boundary_loc(s, b.line_no),
                        b.turn_index,
                        format_timestamp(b.timestamp_utc.as_deref()),
                        b.detail,
                        b.confidence.label()
                    );
                }
            }
        }
        print_window_disclosure(s, ctx);
    }

    if !any {
        println!("no recoverable history for {file} in range");
    }
    if let Some(p) = out_path {
        if write_out_guarded(p, &out_blob)? {
            println!();
            println!("(wrote concatenated patches to {})", p.display());
        }
    }
    print_footer(ctx);
    Ok(())
}

pub(crate) fn render_at_text(
    ctx: &RenderCtx,
    sessions: &[ScanResult],
    out_path: Option<&Path>,
) -> Result<()> {
    let file = ctx.file.as_deref().unwrap_or("(none)");
    let when = ctx.at.as_deref().unwrap_or("");
    let mut first = true;
    let mut any = false;
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
        any = true;
        session_header(&mut first, s);
        println!("  file: {file}");
        if let Some(c) = cutoff {
            println!("  as of: jsonl line {c}");
        }
        let total = rep.final_buffer.seen_total_lines.unwrap_or(0);
        let rendered = render_snapshot_body(&known, total, true);
        for line in rendered.lines() {
            println!("  {line}");
        }
        // The snapshot's integrity context: what invalidated or may have touched the
        // replayed content up to the cutoff, plus the window's opaque commands.
        if !rep.boundaries.is_empty() {
            println!(
                "  integrity boundaries: {} ({} hard · {} soft)",
                rep.boundaries.len(),
                rep.boundaries_hard_count(),
                rep.boundaries_soft_count()
            );
            print_boundary_lines(s, &rep.boundaries);
        }
        print_window_disclosure(s, ctx);
        // The --out artifact: known lines + explicit gap markers (honest).
        out_blob.push_str(&render_snapshot_body(&known, total, false));
        out_blob.push('\n');
    }

    if !any {
        // Salvage runs with an empty `when` (no cutoff): name the state it asked for.
        if when.is_empty() {
            println!("no recoverable history for {file} at the latest state");
        } else {
            println!("no recoverable history for {file} as of {when}");
        }
    }
    if let Some(p) = out_path {
        if write_out_guarded(p, &out_blob)? {
            println!();
            println!("(wrote partial snapshot to {})", p.display());
        }
    }
    print_footer(ctx);
    Ok(())
}

/// Render a partial snapshot body: known lines numbered, gaps explicit. `inline_trunc`
/// truncates long lines for human stdout (false for the verbatim `--out` artifact).
pub(crate) fn render_snapshot_body(
    known: &[(usize, String)],
    total: usize,
    inline_trunc: bool,
) -> String {
    let mut out = String::new();
    let mut prev: usize = 0;
    let last_known = known.last().map(|(n, _)| *n).unwrap_or(0);
    for (n, text) in known {
        if *n > prev + 1 {
            out.push_str(&format!("??? lines {}..{} unknown\n", prev + 1, n - 1));
        }
        let body = if inline_trunc {
            truncate_excerpt(text)
        } else {
            text.clone()
        };
        out.push_str(&format!("{n:>5}  {body}\n"));
        prev = *n;
    }
    // Trailing gap up to the last-seen total.
    if total > last_known && last_known > 0 {
        out.push_str(&format!(
            "??? lines {}..{} unknown\n",
            last_known + 1,
            total
        ));
    } else if known.is_empty() {
        if total > 0 {
            out.push_str(&format!(
                "??? lines 1..{total} unknown (no content seen in range)\n"
            ));
        } else {
            out.push_str("(no content seen for this file in range)\n");
        }
    }
    out
}
