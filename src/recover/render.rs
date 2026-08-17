//! Render: cutoffs, line ranges, guarded writes, restore views.

use super::*;

/// Resolve an `--at <WHEN>` spec to a cutoff jsonl line over a session's events.
/// Supports `@latest` (no cutoff → the file's FINAL state), `@line:<N>`, `@turn:<N>` (first line
/// strictly after turn N), an ISO8601 / relative datetime (events with ts ≤ the bound — the bound
/// is INCLUSIVE), or returns `None` (no cutoff → replay everything) when `when` is empty.
pub(crate) fn resolve_cutoff(when: &str, events: &[FileEvent]) -> Result<Option<usize>> {
    let when = when.trim();
    // `@latest` / empty → the final reconstructed state (replay every event, no cutoff). The
    // clean way to ask for "the file's last form" without guessing a timestamp past the last
    // write (a datetime cutoff is ≤-inclusive, so a too-early ts simply yields less).
    if when.is_empty() || when == "@latest" {
        return Ok(None);
    }
    if let Some(rest) = when.strip_prefix("@line:") {
        let n: usize = rest
            .trim()
            .parse()
            .with_context(|| format!("--at @line:<N> needs an integer, got {rest:?}"))?;
        return Ok(Some(n));
    }
    if let Some(rest) = when.strip_prefix("@turn:") {
        let target: usize = rest
            .trim()
            .parse()
            .with_context(|| format!("--at @turn:<N> needs an integer, got {rest:?}"))?;
        // Cutoff = the last jsonl line whose turn_index ≤ target.
        let cutoff = events
            .iter()
            .filter(|e| e.turn_index <= target)
            .map(|e| e.line_no)
            .max();
        // If nothing is at/below the target turn, cutoff at 0 (empty snapshot).
        return Ok(Some(cutoff.unwrap_or(0)));
    }
    // Datetime bound: the cutoff is the highest line_no whose ts ≤ bound.
    let window = TimeWindow::from_args(None, Some(when))?;
    let cutoff = events
        .iter()
        .filter(|e| window.contains(e.timestamp_utc.as_deref()))
        .map(|e| e.line_no)
        .max();
    Ok(Some(cutoff.unwrap_or(0)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Render context + line-range filtering
// ─────────────────────────────────────────────────────────────────────────────

/// Everything the renderers need beyond the per-session scan results.
#[derive(Debug)]
pub(crate) struct RenderCtx {
    pub(crate) mode: RecoverMode,
    pub(crate) file: Option<String>,
    pub(crate) line_range: Option<crate::text::RangeSpec>,
    pub(crate) at: Option<String>,
    pub(crate) skipped_lines: usize,
    /// SCOPE-span counts of the resolved transcript set, captured BEFORE the
    /// reconstruction merge folds subagents into their parent (so the banner still
    /// announces the true `1 top-level + N subagent` fan-out).
    pub(crate) scope_top: usize,
    pub(crate) scope_sub: usize,
}

/// Restrict a `(line_no, text)` known-line vector to the `--file-lines` range, if any. The
/// spec's open/from-end forms (`N..`, `-20..` = the last 20) resolve against the highest known
/// line number (the reconstructed file's length), 1-based.
pub(crate) fn apply_line_range(
    lines: Vec<(usize, String)>,
    line_range: Option<crate::text::RangeSpec>,
) -> Vec<(usize, String)> {
    match line_range {
        Some(spec) => {
            let len = lines.iter().map(|(n, _)| *n).max().unwrap_or(0);
            let (lo, hi) = spec.resolve(len, true);
            lines
                .into_iter()
                .filter(|(n, _)| *n >= lo && *n <= hi)
                .collect()
        }
        None => lines,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Text rendering
// ─────────────────────────────────────────────────────────────────────────────

/// Write the `--out` artifact ONLY when it has content. An EMPTY reconstruction (no
/// recoverable history / over-budget) must NOT clobber the destination — a user reusing a
/// scratch path (the advertised `--out /tmp/restored.md` idiom) would otherwise lose
/// pre-existing content AND read a false `(wrote …)` success line. Returns `true` when a
/// write happened (so the caller prints its `(wrote …)` line) and `false` (with a stderr
/// note) when the blob was empty and the file was left untouched. Uniform across patches/at
/// + their JSON twins + turns.
pub(crate) fn write_out_guarded(p: &Path, blob: &str) -> Result<bool> {
    if blob.is_empty() {
        eprintln!(
            "note: nothing reconstructed in range — --out file {} left untouched",
            p.display()
        );
        return Ok(false);
    }
    std::fs::write(p, blob).with_context(|| format!("cannot write --out file {}", p.display()))?;
    Ok(true)
}

/// SCOPE-span counts of the resolved transcript set (one `ScanResult` per resolved file,
/// incl. empty/no-history subagents) — `(top_level, subagent)`. Drives the shared SCOPE
/// banner / JSON header so a bare `csift recover <uuid>` fan-out is announced like list/turns.
pub(crate) fn scope_span(sessions: &[ScanResult]) -> (usize, usize) {
    let sub = sessions.iter().filter(|s| s.is_subagent).count();
    (sessions.len() - sub, sub)
}

/// Chronological order of two optional ISO-8601 timestamps. Timestamped events sort before
/// timestamp-less ones; a stable sort then keeps the within-session order for ties/absent ts.
pub(crate) fn cmp_ts(a: &Option<String>, b: &Option<String>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Fold each top-level group (a session + ITS OWN subagents, keyed by `parent_session_id`)
/// into one cross-session reconstruction timeline. A group whose target-file events come
/// from <2 transcripts is returned unchanged (byte-identical legacy single-session path).
/// A group with ≥2 contributing transcripts is collapsed into ONE synthetic `ScanResult`
/// whose events are the union, stably sorted by wall-clock timestamp, with `line_no`
/// re-stamped to a monotonic 1..N over the merged order so `--at` cutoffs (`@line` / `@turn`
/// / datetime) and `replay`'s cutoff stay well-defined across the merged stream.
pub(crate) fn merge_groups_for_reconstruction(sessions: Vec<ScanResult>) -> Vec<ScanResult> {
    // Group by parent id, preserving BTreeMap key order (== session_id order for top-level
    // sessions, since a top-level's parent_session_id is its own id) for determinism.
    let mut groups: BTreeMap<String, Vec<ScanResult>> = BTreeMap::new();
    for s in sessions {
        groups
            .entry(s.parent_session_id.clone())
            .or_default()
            .push(s);
    }

    let mut out: Vec<ScanResult> = Vec::new();
    for (parent_key, group) in groups {
        let contributing = group.iter().filter(|s| !s.events.is_empty()).count();
        if contributing < 2 {
            // 0 or 1 transcript touched the file in this group → no merge; the renderers
            // skip the empty members exactly as before.
            out.extend(group);
            continue;
        }
        // Prefer the top-level session's own uuid as the merged id (re-feedable `@<uuid>`
        // target); fall back to the shared parent key if the group is subagent-only.
        let merged_id = group
            .iter()
            .find(|s| !s.is_subagent)
            .map(|s| s.session_id.clone())
            .unwrap_or_else(|| parent_key.clone());
        let mut events: Vec<FileEvent> = group
            .iter()
            .flat_map(|s| s.events.iter().cloned())
            .collect();
        events.sort_by(|a, b| cmp_ts(&a.timestamp_utc, &b.timestamp_utc));
        for (i, e) in events.iter_mut().enumerate() {
            e.line_no = i + 1;
        }
        out.push(ScanResult {
            session_id: merged_id.clone(),
            is_subagent: false,
            parent_session_id: merged_id,
            events,
            skipped_lines: 0,
        });
    }
    out
}

pub(crate) fn render_text(
    ctx: &RenderCtx,
    sessions: &[ScanResult],
    out_path: Option<&Path>,
) -> Result<()> {
    // Restore writes the RAW file to stdout (for piping) — no scope banner to pollute it.
    if matches!(ctx.mode, RecoverMode::Restore) {
        return render_restore(ctx, sessions, out_path, false);
    }
    crate::text::emit_scope_banner(ctx.scope_top, ctx.scope_sub);
    match ctx.mode {
        RecoverMode::Restore => unreachable!("handled above"),
        RecoverMode::Coverage => render_coverage_text(ctx, sessions),
        // Salvage == `--at @latest`: render_at_text reads ctx.at (None here) → empty `when`
        // → resolve_cutoff returns None → the final-state best-effort fragment.
        RecoverMode::At | RecoverMode::Salvage => render_at_text(ctx, sessions, out_path),
        RecoverMode::Patches => render_patches_text(ctx, sessions, out_path),
    }
}

/// DEFAULT `recover` (no mode flag): hand back the file's FINAL content as RAW restorable bytes
/// — but ONLY when it is fully recoverable. When the session saw just PART of the file (a
/// windowed read + edits), ERROR (never a holey file), naming the recoverable + missing line
/// ranges and pointing at the other modes. Across unrelated session groups the freshest,
/// most-complete candidate wins. Raw content goes to STDOUT (so `recover --file X > X` restores
/// it); the status note goes to STDERR.
pub(crate) fn render_restore(
    ctx: &RenderCtx,
    sessions: &[ScanResult],
    out_path: Option<&Path>,
    json: bool,
) -> Result<()> {
    /// The freshest, most-complete restore candidate so far. Newer ts — then more lines — wins.
    /// Carries the source group's events + boundaries so a partial result can re-derive the
    /// richest pre-change state and list every external-change boundary.
    struct RestoreCandidate<'a> {
        known: Vec<(usize, String)>,
        total: usize,
        ts: Option<String>,
        events: &'a [FileEvent],
        boundaries: Vec<Boundary>,
    }
    let file = ctx.file.as_deref().unwrap_or("(none)");
    if json {
        // envelope v2: restore too opens with the header (then one kind:"restore" row
        // + the summary — the single stream shape every command shares).
        println!(
            "{}",
            crate::text::envelope_scope_header(
                "recover",
                ctx.scope_top,
                ctx.scope_sub,
                serde_json::json!({})
            )
        );
    }
    let mut best: Option<RestoreCandidate> = None;
    for s in sessions {
        if s.events.is_empty() {
            continue;
        }
        let rep = replay(&s.events, None); // final state — no cutoff
        let known = rep.final_buffer.known_lines();
        if known.is_empty() {
            continue;
        }
        let total = rep
            .final_buffer
            .seen_total_lines
            .unwrap_or_else(|| known.last().map(|(n, _)| *n).unwrap_or(0));
        let ts = s
            .events
            .iter()
            .filter_map(|e| e.timestamp_utc.clone())
            .max();
        let better = match &best {
            None => true,
            Some(b) => (&ts, known.len()) > (&b.ts, b.known.len()),
        };
        if better {
            best = Some(RestoreCandidate {
                known,
                total,
                ts,
                events: &s.events,
                boundaries: rep.boundaries,
            });
        }
    }
    let Some(cand) = best else {
        bail!(
            "no recoverable history for {file} in this scope — it was never Read/Written/Edited \
             here. Widen the scope (more sessions/transcripts) or check the path."
        );
    };
    let RestoreCandidate {
        known,
        total,
        events,
        boundaries,
        ..
    } = cand;
    // Every line_no is ≤ total, so knowing `total` distinct lines ⇒ the whole 1..=total is known.
    let complete = total > 0 && known.len() == total;
    if !complete {
        bail!(
            "{}",
            restore_partial_message(file, &known, total, &boundaries, events)
        );
    }
    let mut content = known
        .iter()
        .map(|(_, t)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    if let Some(p) = out_path {
        let wrote = write_out_guarded(p, &content)?;
        if json {
            println!(
                "{}",
                serde_json::json!({"kind": "restore", "file": file, "complete": true, "lines": known.len(), "path": p.to_string_lossy(), "wrote": wrote})
            );
            println!(
                "{}",
                crate::text::envelope_summary(
                    serde_json::json!({"sessions": 1, "file": file, "mode": "restore"})
                )
            );
        } else if wrote {
            eprintln!(
                "(recovered {file} → {}, {} lines)",
                p.display(),
                known.len()
            );
        }
    } else if json {
        println!(
            "{}",
            serde_json::json!({"kind": "restore", "file": file, "complete": true, "lines": known.len(), "content": content})
        );
        println!(
            "{}",
            crate::text::envelope_summary(
                serde_json::json!({"sessions": 1, "file": file, "mode": "restore"})
            )
        );
    } else {
        print!("{content}");
        eprintln!("(recovered {file}: {} lines, complete)", known.len());
    }
    Ok(())
}

/// The smart "can't fully restore the LATEST state" diagnostic. Beyond the covered/missing
/// ranges it (1) lists EVERY external-change boundary (Edit-before-Read / external edit — the
/// file changed outside the tool stream), (2) when a richer state existed BEFORE the first such
/// change, surfaces it (complete in the session-authored case, a fuller salvage otherwise) with
/// a dump-pre-change + dump-patches-since + reconcile-by-hand recipe, and (3) ALWAYS appends the
/// caveat that csift cannot see changes made outside the visible Read/Write/Edit stream and does
/// NOT hunt for hidden boundaries (escalated when a bash mutation may have touched the file).
pub(crate) fn restore_partial_message(
    file: &str,
    known: &[(usize, String)],
    total: usize,
    boundaries: &[Boundary],
    events: &[FileEvent],
) -> String {
    let covered = ranges_str(&known.iter().map(|(n, _)| *n).collect::<Vec<_>>());
    let missing = missing_ranges_str(known, total);
    let mut m = format!(
        "cannot fully recover the LATEST {file} from this scope: recovered {}/{} line(s) \
         [{covered}], MISSING [{missing}].",
        known.len(),
        total
    );

    // External-change boundaries: the file changed OUTSIDE the tool stream and a fresh Read was
    // forced. Across these, pre-change content is no longer part of "latest".
    let ext: Vec<&Boundary> = boundaries
        .iter()
        .filter(|b| {
            matches!(
                b.kind,
                "modified_since_read" | "external_edit" | "original_file_disagreement"
            )
        })
        .collect();
    if let Some(first) = ext.iter().min_by_key(|b| b.line_no) {
        m.push_str(&format!(
            " The file changed OUTSIDE the Read/Write/Edit stream at {} point(s) (so latest can't \
             include the pre-change lines):",
            ext.len()
        ));
        for b in &ext {
            m.push_str(&format!(
                "\n  - jsonl L{} · turn {} · {} · {}",
                b.line_no,
                b.turn_index,
                format_timestamp(b.timestamp_utc.as_deref()),
                b.kind
            ));
        }
        // Richest pre-change state = just before the FIRST external boundary (before any
        // invalidation). A second replay with a cutoff there never trips the invalidation.
        let cutoff = first.line_no.saturating_sub(1);
        let pre = replay(events, Some(cutoff));
        let pre_known = pre.final_buffer.known_lines();
        let pre_total = pre
            .final_buffer
            .seen_total_lines
            .unwrap_or_else(|| pre_known.last().map(|(n, _)| *n).unwrap_or(0));
        if pre_known.len() > known.len() {
            let pre_complete = pre_total > 0 && pre_known.len() == pre_total;
            let since = first
                .timestamp_utc
                .as_deref()
                .map(|t| format!("--since '{t}'"))
                .unwrap_or_else(|| format!("(events after L{})", first.line_no));
            if pre_complete {
                m.push_str(&format!(
                    "\nBUT BEFORE that first change the file is COMPLETELY recoverable ({} lines, \
                     as of {}). Recommended (reconcile by hand): dump the pre-change version with \
                     `recover --file {file} --at @line:{cutoff}`, then the changes since with \
                     `recover --file {file} --patches {since}`.",
                    pre_known.len(),
                    format_timestamp(first.timestamp_utc.as_deref())
                ));
            } else {
                m.push_str(&format!(
                    "\nBUT BEFORE that first change MORE survives ({}/{} lines, vs {}/{} at \
                     latest). Recommended (reconcile by hand): dump that fuller fragment with \
                     `recover --file {file} --at @line:{cutoff}` (line-numbered, gaps explicit), \
                     then the changes since with `recover --file {file} --patches {since}`.",
                    pre_known.len(),
                    pre_total,
                    known.len(),
                    total
                ));
            }
        }
    } else {
        m.push_str(" This session only observed PART of the file, so a complete file can't be rebuilt here.");
    }

    m.push_str(
        " For the best-effort LATEST fragment (survivors numbered, gaps explicit) use `--salvage`; \
         for the changes use `--patches`; to scope what's recoverable use `--coverage`; or widen \
         the scope.",
    );

    // Always-on caveat — csift does NOT hunt for hidden boundaries.
    m.push_str(
        "\nNote: recovery can't fully guarantee a match to disk — anything that changed this file \
         OUTSIDE the visible Read/Write/Edit stream (a formatter like prettier, a husky/pre-commit \
         hook, git, an external editor, a bash mutation) may be invisible here; csift does not hunt \
         for hidden changes.",
    );
    let bash: Vec<&Boundary> = boundaries
        .iter()
        .filter(|b| b.kind == "bash_mutation")
        .collect();
    if let Some(b0) = bash.first() {
        m.push_str(&format!(
            " In fact this session ran {} bash command(s) that may have touched the file (first at \
             L{}) — treat the result as suspect.",
            bash.len(),
            b0.line_no
        ));
    }
    m
}
