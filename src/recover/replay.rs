//! The event replay core (see `replay_model` for the outcome types).

use super::*;

/// Replay a session's ordered `--file` events into segments + boundaries + counts.
/// `cutoff_line` (when `Some`) stops replay at jsonl line ≤ cutoff (for `--at`).
pub(crate) fn replay(events: &[FileEvent], cutoff_line: Option<usize>) -> Replay {
    let mut out = Replay::default();
    let mut buf = SparseBuffer::default();
    let mut seg_start = buf.clone();
    let mut seg_open: Option<(usize, usize, Option<String>)> = None; // (line_no, turn, ts)
    let mut seg_last: Option<(usize, usize, Option<String>)> = None;
    let mut pre_state_known = true;
    let mut had_full_anchor = false;
    let mut anchor_source: Option<SnapSource> = None;
    // Snapshot-instrument state (v0.9.4): the last seen file-history version (a
    // DECREASE = a generation reset, never an anomaly) and whether any mutation
    // event landed since the previous marker (the content-less silent-write test).
    let mut last_snap_version: Option<u64> = None;
    let mut writes_since_marker = false;

    #[allow(clippy::too_many_arguments)]
    let close_segment = |out: &mut Replay,
                         start_buf: &SparseBuffer,
                         end_buf: &SparseBuffer,
                         open: &Option<(usize, usize, Option<String>)>,
                         last: &Option<(usize, usize, Option<String>)>,
                         pre_known: bool,
                         anchor: Option<SnapSource>| {
        if let (Some((ls, tts, tss)), Some((le, tte, tse))) = (open, last) {
            out.segments.push(Segment {
                index: out.segments.len() + 1,
                line_no_start: *ls,
                line_no_end: *le,
                turn_start: *tts,
                turn_end: *tte,
                ts_start: tss.clone(),
                ts_end: tse.clone(),
                start_buffer: start_buf.clone(),
                end_buffer: end_buf.clone(),
                pre_state_known: pre_known,
                anchor_source: anchor,
            });
        }
    };

    for e in events {
        if let Some(c) = cutoff_line {
            if e.line_no > c {
                break;
            }
        }
        let here = (e.line_no, e.turn_index, e.timestamp_utc.clone());

        match &e.kind {
            EventKind::FullSnapshot {
                content,
                total_lines,
                source,
            } => {
                match source {
                    // HistorySnapshot never arrives as a FullSnapshot event (it is
                    // set directly by the marker arm's rebase); read-family defensively.
                    SnapSource::FullRead
                    | SnapSource::FileAttachment
                    | SnapSource::HistorySnapshot => {
                        out.counts.read_full += 1;
                    }
                    SnapSource::Write => out.counts.write += 1,
                    SnapSource::BashCat => out.counts.bash_read_anchor += 1,
                    SnapSource::BashHeredoc | SnapSource::BashWrite => {
                        out.counts.bash_write_anchor += 1;
                    }
                }
                let opened_here = seg_open.is_none();
                // A WRITE is a creation/whole-file event → its segment's pre-state is the
                // buffer BEFORE the write (so the diff shows the write as a real change). A
                // full READ / file attachment is an OBSERVATION of existing state → its
                // segment's pre-state is the anchor content itself (post-read), so the diff
                // shows only the edits made AFTER the read, not a spurious "creation".
                let pre_before_reset = buf.clone();
                if matches!(
                    source,
                    SnapSource::Write | SnapSource::BashHeredoc | SnapSource::BashWrite
                ) {
                    writes_since_marker = true;
                }
                anchor_source = Some(*source);
                buf.reset_to_full(content, *total_lines, e.line_no);
                had_full_anchor = true;
                if opened_here {
                    // A write-family anchor is a CHANGE (pre-state = the buffer before
                    // it); a read-family anchor is an OBSERVATION (pre-state = itself).
                    seg_start = match source {
                        SnapSource::Write | SnapSource::BashHeredoc | SnapSource::BashWrite => {
                            pre_before_reset
                        }
                        SnapSource::FullRead
                        | SnapSource::FileAttachment
                        | SnapSource::BashCat
                        | SnapSource::HistorySnapshot => buf.clone(),
                    };
                    seg_open = Some(here.clone());
                    pre_state_known = true;
                }
                seg_last = Some(here);
            }
            EventKind::PartialRead {
                start_line,
                lines,
                total_lines,
            } => {
                out.counts.read_windowed += 1;
                if seg_open.is_none() {
                    seg_start = buf.clone();
                    seg_open = Some(here.clone());
                }
                buf.splice(*start_line, lines, *total_lines, e.line_no);
                seg_last = Some(here);
            }
            EventKind::Edit {
                hunks,
                original_file,
                structured_patch,
            } => {
                out.counts.edit += 1;
                if seg_open.is_none() {
                    seg_start = buf.clone();
                    seg_open = Some(here.clone());
                }
                // Boundary cross-check: originalFile vs replayed buffer.
                if let Some(orig) = original_file {
                    if had_full_anchor && buffer_disagrees_with_original(&buf, orig) {
                        out.boundaries.push(Boundary {
                            line_no: e.line_no,
                            turn_index: e.turn_index,
                            timestamp_utc: e.timestamp_utc.clone(),
                            kind: "original_file_disagreement",
                            confidence: Confidence::Authoritative,
                            detail: "edit originalFile disagrees with replayed buffer".to_string(),
                        });
                    }
                }
                writes_since_marker = true;
                let outcome = apply_edit(&mut buf, hunks, structured_patch, e.line_no);
                if outcome == EditOutcome::UnAnchorable {
                    out.counts.edit_unanchorable += 1;
                    out.coverage_holes
                        .push((e.line_no, e.turn_index, e.line_no));
                }
                seg_last = Some(here);
            }
            EventKind::IntegrityError { kind, raw } => {
                out.counts.integrity_error += 1;
                match kind {
                    IntegrityKind::ModifiedSinceRead => {
                        // HARD boundary: close the current segment.
                        close_segment(
                            &mut out,
                            &seg_start,
                            &buf,
                            &seg_open,
                            &seg_last,
                            pre_state_known,
                            anchor_source,
                        );
                        out.boundaries.push(Boundary {
                            line_no: e.line_no,
                            turn_index: e.turn_index,
                            timestamp_utc: e.timestamp_utc.clone(),
                            kind: "modified_since_read",
                            confidence: Confidence::Authoritative,
                            detail: first_line(raw),
                        });
                        seg_open = None;
                        seg_last = None;
                        pre_state_known = false;
                        had_full_anchor = false;
                        anchor_source = None;
                        // The file changed out from under us - the harness rejected the edit and
                        // demanded a fresh Read. Everything known so far is now SUSPECT, so
                        // invalidate the buffer: only content RE-READ / re-written after this
                        // point counts toward the final state. Pre-boundary lines become explicit
                        // gaps, never silently-stale lines presented as "current".
                        buf.known.clear();
                        buf.seen_total_lines = None;
                    }
                    IntegrityKind::NotReadYet => { /* not a boundary; the edit never landed */ }
                    // Counted annotations: the op never landed, nothing invalidated,
                    // but the failed attempt is part of the honest event ledger.
                    IntegrityKind::StringNotFound | IntegrityKind::FileDoesNotExist => {}
                }
            }
            EventKind::ExternalEdit { snippet } => {
                out.counts.external_edit += 1;
                writes_since_marker = true;
                // HARD boundary: close current segment, then splice the external snippet.
                close_segment(
                    &mut out,
                    &seg_start,
                    &buf,
                    &seg_open,
                    &seg_last,
                    pre_state_known,
                    anchor_source,
                );
                // An EMPTY snippet is the attachment's over-budget degraded form: the
                // change exceeded the 16KB budget, so the signal arrives content-less.
                let detail = if snippet.is_empty() {
                    "edited_text_file attachment (file changed outside the tool stream; \
                     no snippet: the change exceeded the attachment budget)"
                } else {
                    "edited_text_file attachment (file changed outside the tool stream)"
                };
                out.boundaries.push(Boundary {
                    line_no: e.line_no,
                    turn_index: e.turn_index,
                    timestamp_utc: e.timestamp_utc.clone(),
                    kind: "external_edit",
                    confidence: Confidence::Authoritative,
                    detail: detail.to_string(),
                });
                for (n, text) in snippet {
                    buf.known.insert(
                        *n,
                        LineCell {
                            text: text.clone(),
                            last_line_no: e.line_no,
                        },
                    );
                }
                // Open a fresh segment anchored on the external snippet.
                seg_start = buf.clone();
                seg_open = Some(here.clone());
                seg_last = Some(here);
                pre_state_known = false;
                had_full_anchor = false;
                anchor_source = None;
            }
            EventKind::BashTouch {
                verb,
                path,
                resolution,
            } => {
                out.counts.bash += 1;
                writes_since_marker = true;
                // SOFT boundary: close current segment + flag heuristic.
                close_segment(
                    &mut out,
                    &seg_start,
                    &buf,
                    &seg_open,
                    &seg_last,
                    pre_state_known,
                    anchor_source,
                );
                out.boundaries.push(Boundary {
                    line_no: e.line_no,
                    turn_index: e.turn_index,
                    timestamp_utc: e.timestamp_utc.clone(),
                    kind: "bash_mutation",
                    confidence: Confidence::Heuristic,
                    detail: format!(
                        "bash `{verb}` on {path} [{resolution}] (reconstruction across \
                         this point may be invalid)"
                    ),
                });
                seg_open = None;
                seg_last = None;
                pre_state_known = false;
                // `had_full_anchor` survives a SOFT boundary on purpose: originalFile
                // is disk ground truth, so if the bash touch really changed the file,
                // the next Edit's originalFile-vs-buffer cross-check is exactly the
                // detector that catches it. Disarming here silenced that check for
                // the rest of the session after any single bash touch.
                anchor_source = None;
            }
            EventKind::StaleReadHint { path } => {
                out.counts.stale_hint += 1;
                writes_since_marker = true;
                // HARD boundary: Claude Code itself stat'd the read set and named this
                // file as modified by a shell command. Content-less (nothing to
                // splice), so the buffer stays, but reconstruction across the point no
                // longer matches disk and a fresh anchor is required.
                close_segment(
                    &mut out,
                    &seg_start,
                    &buf,
                    &seg_open,
                    &seg_last,
                    pre_state_known,
                    anchor_source,
                );
                out.boundaries.push(Boundary {
                    line_no: e.line_no,
                    turn_index: e.turn_index,
                    timestamp_utc: e.timestamp_utc.clone(),
                    kind: "hint_modified",
                    confidence: Confidence::Authoritative,
                    detail: format!(
                        "Claude Code reported {path} modified by this shell command \
                         (staleReadFileStateHint)"
                    ),
                });
                seg_open = None;
                seg_last = None;
                pre_state_known = false;
                had_full_anchor = false;
                anchor_source = None;
            }
            EventKind::StaleRecovered => {
                out.counts.stale_recovered += 1;
                // Authoritative ANNOTATION: the edit applied cleanly, but the disk had
                // drifted since the last read - other changes exist outside this
                // stream. Nothing is invalidated (the edited span is right), so no
                // segment close and no state reset.
                out.boundaries.push(Boundary {
                    line_no: e.line_no,
                    turn_index: e.turn_index,
                    timestamp_utc: e.timestamp_utc.clone(),
                    kind: "stale_recovered",
                    confidence: Confidence::Authoritative,
                    detail: "the edit applied cleanly, but the file had been modified \
                             on disk since the last read (changes outside this stream \
                             exist)"
                        .to_string(),
                });
            }
            EventKind::BashWindowRead { start_line, lines } => {
                out.counts.bash_read_anchor += 1;
                if seg_open.is_none() {
                    seg_start = buf.clone();
                    seg_open = Some(here.clone());
                }
                // The observed extent is the honest total; `splice` takes the max with
                // the prior seen length (a window can floor it, never shrink it). The
                // separator/terminator normalization inside `splice` can undercount
                // this observed extent by one on a newline-terminated file - re-floor
                // to the highest line this window actually saw.
                let extent = start_line + lines.len() - 1;
                buf.splice(*start_line, lines, extent, e.line_no);
                buf.seen_total_lines = Some(buf.seen_total_lines.unwrap_or(0).max(extent));
                seg_last = Some(here);
            }
            EventKind::BashAppend { content } => {
                // Placeable ONLY when the whole file is known and newline-terminated
                // (appended bytes start a NEW line then; a non-terminated tail would
                // CONCATENATE and the split is unknowable) - or the buffer is a known
                // EMPTY file. Anything else: a disclosed heuristic boundary (the
                // content is known, its line position is not).
                let total = buf.seen_total_lines;
                let complete = total.is_some_and(|t| {
                    t > 0 && buf.known.len() == t && buf.known.keys().next_back() == Some(&t)
                });
                let placeable =
                    matches!(total, Some(0)) || (complete && buf.content_ends_with_newline);
                if placeable {
                    out.counts.bash_write_anchor += 1;
                    writes_since_marker = true;
                    if seg_open.is_none() {
                        seg_start = buf.clone();
                        seg_open = Some(here.clone());
                    }
                    let base = total.unwrap_or(0);
                    for (i, text) in crate::recover::split_lines(content).into_iter().enumerate() {
                        buf.known.insert(
                            base + 1 + i,
                            LineCell {
                                text,
                                last_line_no: e.line_no,
                            },
                        );
                    }
                    buf.seen_total_lines = Some(buf.known.keys().next_back().copied().unwrap_or(0));
                    buf.content_ends_with_newline = content.ends_with('\n');
                    seg_last = Some(here);
                } else {
                    out.counts.bash += 1;
                    writes_since_marker = true;
                    close_segment(
                        &mut out,
                        &seg_start,
                        &buf,
                        &seg_open,
                        &seg_last,
                        pre_state_known,
                        anchor_source,
                    );
                    out.boundaries.push(Boundary {
                        line_no: e.line_no,
                        turn_index: e.turn_index,
                        timestamp_utc: e.timestamp_utc.clone(),
                        kind: "bash_append_unplaced",
                        confidence: Confidence::Heuristic,
                        detail: "bash append with byte-known content, but the buffer is                                  not a complete newline-terminated file here - the                                  append point is unknowable, so it stays a boundary"
                            .to_string(),
                    });
                    seg_open = None;
                    seg_last = None;
                    pre_state_known = false;
                    anchor_source = None;
                }
            }
            EventKind::HistorySnapshotMarker {
                version, content, ..
            } => {
                out.counts.history_snapshot += 1;
                // The file-history INSTRUMENT (v0.9.4): Claude Code backs the tracked
                // file up per prompt and bumps `version` only when the bytes changed.
                // A version CHANGE therefore captures the disk truth at that instant -
                // including harness-side writes that leave NO tool record (measured:
                // half of all settings.json mutations corpus-wide). Three cases:
                // - verified CONTENT attached + it DISAGREES with the replayed known
                //   lines: an untracked write happened - close the segment, disclose
                //   an authoritative `external_write` boundary, and REBASE the buffer
                //   on the snapshot content (CC's own byte-exact backup outranks a
                //   replay it contradicts);
                // - no content, but the version JUMPED with NO mutation event since
                //   the previous marker: the same fact without the bytes - an
                //   authoritative content-less boundary (StaleReadHint shape: the
                //   buffer stays, trust across the point does not);
                // - a generation RESET (version decreased: the counter restarts on
                //   process restart, 148 real cases) or an unchanged version:
                //   bookkeeping only - cross-generation comparison is invalid by
                //   construction.
                let action = snapshot_action(
                    last_snap_version,
                    *version,
                    content.as_deref(),
                    &buf,
                    writes_since_marker,
                );
                if version.is_some() {
                    last_snap_version = *version;
                }
                match action {
                    SnapAction::Nothing => {}
                    SnapAction::Rebase => {
                        let snap_content = content.as_deref().unwrap_or_default();
                        let snap_lines = split_lines(snap_content);
                        {
                            out.counts.snapshot_rebase += 1;
                            close_segment(
                                &mut out,
                                &seg_start,
                                &buf,
                                &seg_open,
                                &seg_last,
                                pre_state_known,
                                anchor_source,
                            );
                            out.boundaries.push(Boundary {
                                line_no: e.line_no,
                                turn_index: e.turn_index,
                                timestamp_utc: e.timestamp_utc.clone(),
                                kind: "external_write",
                                confidence: Confidence::Authoritative,
                                detail: format!(
                                    "the replayed buffer disagrees with Claude Code's own file-history \
                                     snapshot v{} - a write with no tool record happened in this \
                                     window; the replay is REBASED on the snapshot content",
                                    version.map_or_else(|| "?".to_string(), |v| v.to_string())
                                ),
                            });
                            buf.reset_to_full(snap_content, snap_lines.len(), e.line_no);
                            anchor_source = Some(SnapSource::HistorySnapshot);
                            had_full_anchor = true;
                            seg_start = buf.clone();
                            seg_open = Some(here.clone());
                            seg_last = Some(here);
                            pre_state_known = false;
                        }
                    }
                    SnapAction::ContentlessBoundary => {
                        close_segment(
                            &mut out,
                            &seg_start,
                            &buf,
                            &seg_open,
                            &seg_last,
                            pre_state_known,
                            anchor_source,
                        );
                        out.boundaries.push(Boundary {
                            line_no: e.line_no,
                            turn_index: e.turn_index,
                            timestamp_utc: e.timestamp_utc.clone(),
                            kind: "external_write",
                            confidence: Confidence::Authoritative,
                            detail: format!(
                                "file-history version jumped to v{} with NO tool write of this \
                                 file since the previous snapshot - a harness-side or \
                                 out-of-band write; the snapshot content is unavailable \
                                 (pruned, unverifiable, or never stored), so nothing is \
                                 rebased and trust across this point ends",
                                version.map_or_else(|| "?".to_string(), |v| v.to_string())
                            ),
                        });
                        seg_open = None;
                        seg_last = None;
                        pre_state_known = false;
                        had_full_anchor = false;
                        anchor_source = None;
                    }
                }
                writes_since_marker = false;
            }
        }
    }

    // Close the trailing open segment.
    close_segment(
        &mut out,
        &seg_start,
        &buf,
        &seg_open,
        &seg_last,
        pre_state_known,
        anchor_source,
    );
    out.final_buffer = buf;
    out
}

/// True when the replayed buffer, over the lines `originalFile` describes, DISAGREES with
/// `originalFile` (an out-of-band change happened). Compares the contiguous known region
/// that overlaps `originalFile`'s first lines; if nothing is comparably known, returns
/// false (we cannot prove a disagreement, so we never flag a false boundary).
pub(crate) fn buffer_disagrees_with_original(buf: &SparseBuffer, original_file: &str) -> bool {
    let orig_lines = split_lines(original_file);
    if orig_lines.is_empty() {
        return false;
    }
    // Compare line-by-line over the known cells that fall within the original's length.
    let mut compared = 0usize;
    let mut mismatches = 0usize;
    for (k, cell) in &buf.known {
        if *k >= 1 && *k <= orig_lines.len() {
            compared += 1;
            if orig_lines[*k - 1] != cell.text {
                mismatches += 1;
            }
        }
    }
    // Require a reasonable comparison base (≥1 line) and ANY mismatch to flag - but only
    // when we compared enough to be meaningful (avoid a single fluke). A mismatch ratio
    // over a small threshold is a real disagreement.
    compared > 0 && mismatches > 0 && (mismatches * 4 >= compared)
}

// ─────────────────────────────────────────────────────────────────────────────
// Unified diff (in-crate, safe Rust)
// ─────────────────────────────────────────────────────────────────────────────

/// What a snapshot marker means for the replay - the PURE decision half of the
/// marker arm (the effects live in `replay`):
/// - verified content that DISAGREES with the replayed known lines (or a complete
///   buffer with a different length) => `Rebase` - valid across a generation reset
///   too, since the bytes are mtime-verified against THIS marker's backupTime;
/// - no content, but a same-generation version JUMP with no mutation event since
///   the previous marker => `ContentlessBoundary` (the silence signal is
///   generation-bound: a version DECREASE is a counter restart, not a write);
/// - anything else => `Nothing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapAction {
    Rebase,
    ContentlessBoundary,
    Nothing,
}

fn snapshot_action(
    last_version: Option<u64>,
    version: Option<u64>,
    content: Option<&str>,
    buf: &SparseBuffer,
    writes_since_marker: bool,
) -> SnapAction {
    if let Some(snap_content) = content {
        let snap_lines = split_lines(snap_content);
        let known_disagrees = buf
            .known
            .iter()
            .any(|(n, cell)| snap_lines.get(n - 1).is_none_or(|l| *l != cell.text));
        let complete_len_differs = buf
            .seen_total_lines
            .is_some_and(|t| buf.known.len() == t && t != snap_lines.len());
        if !buf.known.is_empty() && (known_disagrees || complete_len_differs) {
            return SnapAction::Rebase;
        }
        return SnapAction::Nothing;
    }
    let jumped = matches!((last_version, version), (Some(p), Some(v)) if v > p);
    let reset = matches!((last_version, version), (Some(p), Some(v)) if v < p);
    if jumped && !reset && !writes_since_marker {
        SnapAction::ContentlessBoundary
    } else {
        SnapAction::Nothing
    }
}
