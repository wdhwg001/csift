//! Replay outcome model: confidence, boundaries, per-op counts, segments.

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
    /// Claude Code `staleReadFileStateHint` attributions naming this file.
    pub(crate) stale_hint: usize,
    /// Successful Edits flagged `staleRecovered` (disk had drifted; edit applied).
    pub(crate) stale_recovered: usize,
    /// Gated Bash READ content anchors (cat full + windows) admitted into the replay.
    pub(crate) bash_read_anchor: usize,
    /// Gated Bash WRITE content anchors (heredoc/echo/printf/truncate/appends).
    pub(crate) bash_write_anchor: usize,
    /// Replays REBASED on a verified file-history snapshot whose content diverged
    /// from the replayed buffer (a harness-side write with no tool record).
    pub(crate) snapshot_rebase: usize,
    /// Read echoes with NO content (blanked by the harness after the fact, or a
    /// contentless result arm): counted, never replayed.
    pub(crate) blanked_read: usize,
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

/// The annotation a blanked or contentless Read echo leaves in the replay: nothing is
/// known from it and nothing is invalidated by it, so it is disclosed, never spliced.
pub(crate) fn blanked_read_boundary(e: &FileEvent, total_lines: Option<usize>) -> Boundary {
    Boundary {
        line_no: e.line_no,
        turn_index: e.turn_index,
        timestamp_utc: e.timestamp_utc.clone(),
        kind: "blanked_read",
        confidence: Confidence::Authoritative,
        detail: match total_lines {
            Some(n) => format!(
                "a Read echo with its content blanked by the harness ({n} line(s) at \
                 the time); nothing is replayed from it"
            ),
            None => {
                "a contentless Read echo (no text arm); nothing is replayed from it".to_string()
            }
        },
    }
}

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
pub(crate) enum SnapAction {
    Rebase,
    ContentlessBoundary,
    Nothing,
}

pub(crate) fn snapshot_action(
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
