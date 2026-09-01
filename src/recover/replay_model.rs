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
