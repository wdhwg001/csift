//! Recover event model: FileEvent / EventKind / SnapSource / hunk + scan types.

/// A file-touching event extracted from ONE jsonl line, in transcript order.
#[derive(Debug, Clone)]
pub(crate) struct FileEvent {
    /// 1-based jsonl line (the new capability).
    pub(crate) line_no: usize,
    /// Genuine-user turn index (`group_turn_indices`).
    pub(crate) turn_index: usize,
    pub(crate) timestamp_utc: Option<String>,
    pub(crate) kind: EventKind,
}

/// What a [`FileEvent`] does to the reconstructed buffer.
#[derive(Debug, Clone)]
pub(crate) enum EventKind {
    /// Full ground-truth content (an anchor): a Write result, a full Read
    /// (`startLine==1 && numLines==totalLines`), or a `file` attachment.
    FullSnapshot {
        content: String,
        total_lines: usize,
        source: SnapSource,
    },
    /// Windowed Read: lines `[start_line, start_line+lines.len())` are known;
    /// `total_lines` is the file length the model saw (for gap detection).
    PartialRead {
        start_line: usize,
        lines: Vec<String>,
        total_lines: usize,
    },
    /// An Edit/MultiEdit applied old→new. `structured_patch` (when present) gives exact
    /// line positions; `original_file` (when present) is used ONLY to cross-check for a
    /// boundary, never to paper over drift.
    Edit {
        hunks: Vec<EditHunk>,
        original_file: Option<String>,
        structured_patch: Option<Vec<PatchHunk>>,
    },
    /// An integrity violation the harness surfaced.
    IntegrityError { kind: IntegrityKind, raw: String },
    /// A heuristic external mutation (Bash redirect/sed -i/tee/...). SOFT signal only.
    /// `path` is the RESOLVED spelling that matched `--file` (the operand joined to the
    /// recording shell's cwd; see `bash_mutations::cwd`), `resolution` its class wire
    /// spelling (`absolute`/`cwd-joined`/`cd-tracked`/`unresolved`).
    BashTouch {
        verb: String,
        path: String,
        resolution: &'static str,
    },
    /// An external/user edit captured as an `edited_text_file` attachment snippet.
    ExternalEdit { snippet: Vec<(usize, String)> },
    /// A `file-history-snapshot` recorded a disk backup of `--file` at this time. The
    /// on-disk blob name is NOT derivable from the record (the real `backupFileName` is
    /// frequently null), so this is a COVERAGE ANNOTATION only - never a content anchor.
    HistorySnapshotMarker,
}

/// The provenance of a [`EventKind::FullSnapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnapSource {
    Write,
    FullRead,
    FileAttachment,
}

impl SnapSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            SnapSource::Write => "write",
            SnapSource::FullRead => "full-read",
            SnapSource::FileAttachment => "file-attachment",
        }
    }
}

/// The two harness integrity-error shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntegrityKind {
    /// "File has been modified since read, …" - a HARD boundary (disk drift detected).
    ModifiedSinceRead,
    /// "File has not been read yet. …" - the edit never landed; NOT a boundary.
    NotReadYet,
}

/// One hunk of an Edit (old→new strings), from the tool_use `input`.
#[derive(Debug, Clone)]
pub(crate) struct EditHunk {
    pub(crate) old_string: String,
    pub(crate) new_string: String,
    pub(crate) replace_all: bool,
}

/// One structured-patch hunk (`toolUseResult.structuredPatch[]`): a mirror of CC's
/// `{oldStart, oldLines, newStart, newLines, lines:[" ","-","+", …]}`. `newStart` is not
/// retained - replay derives the new position from `oldStart` + the running line offset.
#[derive(Debug, Clone)]
pub(crate) struct PatchHunk {
    pub(crate) old_start: usize,
    pub(crate) old_lines: usize,
    pub(crate) new_lines: usize,
    /// Each line prefixed by ` ` (context), `-` (removed), or `+` (added).
    pub(crate) lines: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-file scan
// ─────────────────────────────────────────────────────────────────────────────

/// One shell command in a scanned transcript whose touched-file set the lexical layer
/// cannot name: a mutating-CLASS marker (`fmt:cargo`, `interp:python`, `pkg:npm`,
/// `extract:tar`, `git:<sub>`), or a `PowerShell` tool call (never parsed lexically,
/// marker `powershell`). These are SCOPE accounting, not `--file` events: they cannot
/// be joined to a target, so recover COUNTS and discloses them per window instead of
/// pretending the window is clean.
#[derive(Debug, Clone)]
pub(crate) struct OpaqueCommand {
    /// The transcript the command ran in (its own id, so a merged group's rows stay
    /// attributable) and the record's REAL 1-based jsonl line there. Opaque rows are
    /// never re-stamped by the reconstruction merge: they are pointers for
    /// inspection, not replay coordinates.
    pub(crate) session_id: String,
    pub(crate) line_no: usize,
    pub(crate) turn_index: usize,
    pub(crate) timestamp_utc: Option<String>,
    /// The class marker (`fmt:cargo`, `interp:python`, ...) or `powershell`.
    pub(crate) marker: String,
}

/// Per-session scan result before global merge.
#[derive(Debug)]
pub(crate) struct ScanResult {
    pub(crate) session_id: String,
    /// True when this transcript is a SUBAGENT (so `session_id` is a bare hex, NOT a
    /// re-feedable `@<uuid>` target) - the r5 id-domain discriminator, now also on recover.
    pub(crate) is_subagent: bool,
    /// The re-feedable PARENT session uuid (= `session_id` for a top-level file).
    pub(crate) parent_session_id: String,
    pub(crate) events: Vec<FileEvent>,
    /// Commands in THIS transcript whose file set is not lexically knowable (class
    /// markers + PowerShell calls) - per-window disclosure input, never content.
    pub(crate) opaque: Vec<OpaqueCommand>,
    /// Set ONLY by `merge_groups_for_reconstruction`: the re-stamped synthetic event
    /// line (1..N over the merged stream, the `--at @line:` cutoff coordinate) mapped
    /// back to (source transcript id, its REAL jsonl line). Empty on an un-merged
    /// result, where `line_no` IS the real line. Renderers use it so a boundary's
    /// displayed location stays a real, inspectable transcript line.
    pub(crate) merged_line_origin: std::collections::BTreeMap<usize, (String, usize)>,
    pub(crate) skipped_lines: usize,
}
