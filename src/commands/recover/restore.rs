//! The default RESTORE mode: freshest-complete candidate selection, the success
//! disclosure, and the partial-failure diagnostic.

use super::*;

/// DEFAULT `recover` (no mode flag): hand back the file's FINAL content as RAW restorable bytes
/// -- but ONLY when it is fully recoverable. When the session saw just PART of the file (a
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
    /// The freshest, most-complete restore candidate so far. Newer ts - then more lines - wins.
    /// Carries the source group + replay so the result can list every boundary and
    /// disclose the window's opaque commands, and a partial result can re-derive the
    /// richest pre-change state.
    struct RestoreCandidate<'a> {
        known: Vec<(usize, String)>,
        total: usize,
        ts: Option<String>,
        source: &'a ScanResult,
        boundaries: Vec<Boundary>,
        counts: EventCounts,
    }
    let file = ctx.file.as_deref().unwrap_or("(none)");
    if json {
        // envelope v2: restore too opens with the header (then one kind:"restore" row
        // + the summary - the single stream shape every command shares). The bail
        // paths below emit their row + summary BEFORE erroring, so the envelope is
        // never left half-open.
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
    // On a hard-error path in json mode: complete the envelope (row + summary) first,
    // so a machine consumer always sees a well-formed stream beside the exit code.
    let bail_json = |reason: &str, extra: serde_json::Value| {
        if json {
            let mut row = serde_json::json!({
                "kind": "restore", "file": file, "complete": false, "reason": reason,
            });
            if let (Some(row_map), Some(extra_map)) = (row.as_object_mut(), extra.as_object()) {
                for (k, v) in extra_map {
                    row_map.insert(k.clone(), v.clone());
                }
            }
            println!("{row}");
            println!(
                "{}",
                crate::text::envelope_summary(
                    serde_json::json!({"sessions": 0, "file": file, "mode": "restore"})
                )
            );
        }
    };
    let mut best: Option<RestoreCandidate> = None;
    // Sessions whose events replayed to an EMPTY buffer (every reconstruction run was
    // invalidated, or the events carried no content at all) - so a miss is reported
    // accurately, never as "the file was never touched here".
    let mut saw_events = false;
    let mut invalidated_kinds: Vec<&'static str> = Vec::new();
    for s in sessions {
        if s.events.is_empty() {
            continue;
        }
        saw_events = true;
        let rep = replay(&s.events, None); // final state - no cutoff
        let known = rep.final_buffer.known_lines();
        if known.is_empty() {
            for b in &rep.boundaries {
                if !invalidated_kinds.contains(&b.kind) {
                    invalidated_kinds.push(b.kind);
                }
            }
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
                source: s,
                boundaries: rep.boundaries,
                counts: rep.counts,
            });
        }
    }
    let Some(cand) = best else {
        if saw_events {
            let detail = if invalidated_kinds.is_empty() {
                String::new()
            } else {
                format!(
                    " (reconstruction is invalidated at: {})",
                    invalidated_kinds.join(", ")
                )
            };
            bail_json("invalidated", serde_json::json!({}));
            bail!(
                "the transcripts in this scope DO hold events for {file}, but no line content \
                 survives the replay{detail}. The event trail is still inspectable: use \
                 --patches for the diff history, --coverage for the boundary map, or \
                 --at @line:<N> for a pre-boundary snapshot."
            );
        }
        bail_json("no-history", serde_json::json!({}));
        bail!(
            "no recoverable history for {file} in this scope: it was never Read/Written/Edited \
             here. Widen the scope (more sessions/transcripts) or check the path."
        );
    };
    let RestoreCandidate {
        known,
        total,
        source,
        boundaries,
        counts,
        ..
    } = cand;
    let disclosure = window_disclosure(source, ctx.file.as_deref());
    // Every line_no is ≤ total, so knowing `total` distinct lines ⇒ the whole 1..=total is known.
    let complete = total > 0 && known.len() == total;
    if !complete {
        bail_json(
            "partial",
            serde_json::json!({
                "lines": known.len(),
                "total_lines": total,
                "boundaries": boundaries.iter().map(|b| boundary_json_sourced(source, b)).collect::<Vec<_>>(),
                "bash_events": counts.bash,
                "opaque_commands": disclosure.opaque_classes,
                "powershell_commands": disclosure.powershell,
                "suggested_search": disclosure.suggested,
            }),
        );
        bail!(
            "{}",
            restore_partial_message(file, &known, total, &boundaries, source, &disclosure)
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
    // The success-path status report (stderr): honest per-window accounting. A clean
    // window says so positively; a window with boundaries or opaque commands lists
    // exactly what the replay did not include, plus the search that inspects it.
    let emit_status = |head: String| {
        if boundaries.is_empty() && disclosure.is_clean() {
            eprintln!(
                "{head}, complete; no bash mutation of this file and no opaque \
                 mutating-class command detected in the window)"
            );
            return;
        }
        eprintln!("{head}, complete from the tool stream; NOT verified against disk)");
        if !boundaries.is_empty() {
            eprintln!(
                "  {} change(s) in the window were disclosed as boundaries, not replayed:",
                boundaries.len()
            );
            for b in &boundaries {
                let sym = if b.confidence == Confidence::Authoritative {
                    "⚠"
                } else {
                    "~"
                };
                eprintln!(
                    "    {sym} {}  turn {}  {}  {} ({})",
                    boundary_loc(source, b.line_no),
                    b.turn_index,
                    format_timestamp(b.timestamp_utc.as_deref()),
                    boundary_detail_with_clue(source, b),
                    b.confidence.label()
                );
            }
        }
        if let Some(note) = opaque_note(&disclosure) {
            eprintln!("  also in this window: {note}");
        }
        if let Some(cmd) = &disclosure.suggested {
            eprintln!("  inspect the window: {cmd}");
        }
    };
    let restore_row = |extra: serde_json::Value| {
        let mut row = serde_json::json!({
            "kind": "restore", "file": file, "complete": true, "lines": known.len(),
            "boundaries": boundaries.iter().map(|b| boundary_json_sourced(source, b)).collect::<Vec<_>>(),
            "bash_events": counts.bash,
            "opaque_commands": disclosure.opaque_classes,
            "powershell_commands": disclosure.powershell,
            "suggested_search": disclosure.suggested,
        });
        if let (Some(row_map), Some(extra_map)) = (row.as_object_mut(), extra.as_object()) {
            for (k, v) in extra_map {
                row_map.insert(k.clone(), v.clone());
            }
        }
        row
    };
    if let Some(p) = out_path {
        let wrote = write_out_guarded(p, &content)?;
        if json {
            println!(
                "{}",
                restore_row(serde_json::json!({"path": p.to_string_lossy(), "wrote": wrote}))
            );
            println!(
                "{}",
                crate::text::envelope_summary(
                    serde_json::json!({"sessions": 1, "file": file, "mode": "restore"})
                )
            );
        } else if wrote {
            emit_status(format!(
                "(recovered {file} → {}, {} lines",
                p.display(),
                known.len()
            ));
        }
    } else if json {
        println!("{}", restore_row(serde_json::json!({"content": content})));
        println!(
            "{}",
            crate::text::envelope_summary(
                serde_json::json!({"sessions": 1, "file": file, "mode": "restore"})
            )
        );
    } else {
        print!("{content}");
        emit_status(format!("(recovered {file}: {} lines", known.len()));
    }
    Ok(())
}

/// The smart "can't fully restore the LATEST state" diagnostic. Beyond the covered/missing
/// ranges it (1) lists EVERY external-change boundary (Edit-before-Read / external edit - the
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
    source: &ScanResult,
    disclosure: &WindowDisclosure,
) -> String {
    let events: &[FileEvent] = &source.events;
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
                "modified_since_read"
                    | "external_edit"
                    | "original_file_disagreement"
                    | "hint_modified"
                    | "stale_recovered"
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
                "\n  - jsonl {} · turn {} · {} · {}",
                boundary_loc(source, b.line_no),
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

    // Always-on caveat - csift does NOT hunt for hidden boundaries.
    m.push_str(
        "\nNote: recovery can't fully guarantee a match to disk: anything that changed this file \
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
            " In fact this session ran {} bash command(s) that touched the file (first at \
             {}); treat the result as suspect.",
            bash.len(),
            boundary_loc(source, b0.line_no)
        ));
    }
    if let Some(note) = opaque_note(disclosure) {
        m.push_str(&format!(" Also in this window: {note}."));
    }
    if let Some(cmd) = &disclosure.suggested {
        m.push_str(&format!("\nInspect the window: {cmd}"));
    }
    m
}
