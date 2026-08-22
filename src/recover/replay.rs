//! Replay: confidence, boundaries, segment accounting, the event replay core.

use super::*;

/// The confidence of an integrity boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Confidence {
    Authoritative,
    Heuristic,
}

impl Confidence {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Confidence::Authoritative => "AUTHORITATIVE",
            Confidence::Heuristic => "HEURISTIC",
        }
    }
    pub(crate) fn json(self) -> &'static str {
        match self {
            Confidence::Authoritative => "authoritative",
            Confidence::Heuristic => "heuristic",
        }
    }
}

/// A detected integrity boundary - a point where reconstruction across it is invalid.
#[derive(Debug, Clone)]
pub(crate) struct Boundary {
    pub(crate) line_no: usize,
    pub(crate) turn_index: usize,
    pub(crate) timestamp_utc: Option<String>,
    pub(crate) kind: &'static str,
    pub(crate) confidence: Confidence,
    pub(crate) detail: String,
}

/// Per-op counts for the coverage report.
#[derive(Debug, Default, Clone)]
pub(crate) struct EventCounts {
    pub(crate) read_full: usize,
    pub(crate) read_windowed: usize,
    pub(crate) edit: usize,
    pub(crate) edit_unanchorable: usize,
    pub(crate) write: usize,
    pub(crate) bash: usize,
    pub(crate) external_edit: usize,
    pub(crate) history_snapshot: usize,
    pub(crate) integrity_error: usize,
}

/// One reconstructed segment (a maximal run of events with no hard boundary inside).
#[derive(Debug, Clone)]
pub(crate) struct Segment {
    pub(crate) index: usize,
    pub(crate) line_no_start: usize,
    pub(crate) line_no_end: usize,
    pub(crate) turn_start: usize,
    pub(crate) turn_end: usize,
    pub(crate) ts_start: Option<String>,
    pub(crate) ts_end: Option<String>,
    /// The buffer state at the END of this segment.
    pub(crate) end_buffer: SparseBuffer,
    /// The buffer state at the START of this segment (its pre-state).
    pub(crate) start_buffer: SparseBuffer,
    /// False when this segment opened after a boundary with no fresh full anchor.
    pub(crate) pre_state_known: bool,
    /// The kind of full anchor (if any) that opened/seeded this segment.
    pub(crate) anchor_source: Option<SnapSource>,
}

/// The full replay outcome for one session's `--file` events.
#[derive(Debug, Default)]
pub(crate) struct Replay {
    pub(crate) segments: Vec<Segment>,
    pub(crate) boundaries: Vec<Boundary>,
    pub(crate) counts: EventCounts,
    /// The final buffer after replaying ALL events (used by `--coverage`/`--at`).
    pub(crate) final_buffer: SparseBuffer,
    pub(crate) coverage_holes: Vec<(usize, usize, usize)>, // (line_no, turn, jsonl-ish marker)
}

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
                    SnapSource::FullRead => out.counts.read_full += 1,
                    SnapSource::Write => out.counts.write += 1,
                    SnapSource::FileAttachment => out.counts.read_full += 1,
                }
                let opened_here = seg_open.is_none();
                // A WRITE is a creation/whole-file event → its segment's pre-state is the
                // buffer BEFORE the write (so the diff shows the write as a real change). A
                // full READ / file attachment is an OBSERVATION of existing state → its
                // segment's pre-state is the anchor content itself (post-read), so the diff
                // shows only the edits made AFTER the read, not a spurious "creation".
                let pre_before_reset = buf.clone();
                anchor_source = Some(*source);
                buf.reset_to_full(content, *total_lines, e.line_no);
                had_full_anchor = true;
                if opened_here {
                    seg_start = match source {
                        SnapSource::Write => pre_before_reset,
                        SnapSource::FullRead | SnapSource::FileAttachment => buf.clone(),
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
                }
            }
            EventKind::ExternalEdit { snippet } => {
                out.counts.external_edit += 1;
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
                out.boundaries.push(Boundary {
                    line_no: e.line_no,
                    turn_index: e.turn_index,
                    timestamp_utc: e.timestamp_utc.clone(),
                    kind: "external_edit",
                    confidence: Confidence::Authoritative,
                    detail: "edited_text_file attachment (file changed outside the tool stream)"
                        .to_string(),
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
                had_full_anchor = false;
                anchor_source = None;
            }
            EventKind::HistorySnapshotMarker => {
                out.counts.history_snapshot += 1;
                // A coverage annotation only - not an anchor, not a boundary.
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
