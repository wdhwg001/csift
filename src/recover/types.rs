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
    /// An EMPTY snippet is the attachment's over-budget degraded form (the change
    /// exceeded the 16KB budget): the signal is still authoritative, the content is
    /// not carried.
    ExternalEdit { snippet: Vec<(usize, String)> },
    /// Claude Code's own modified-file attribution: a Bash result's
    /// `staleReadFileStateHint` named this file as modified by the command ("[This
    /// command modified N file(s) you've previously read: …]"). The hint's paths are
    /// rendered RELATIVE to the recording shell's cwd and are resolved against the
    /// carrying record's own `cwd` before matching. AUTHORITATIVE (CC stat'd the
    /// read-set itself), content-less: a hard boundary at replay.
    StaleReadHint { path: String },
    /// A SUCCESSFUL Edit whose `toolUseResult.staleRecovered:true` reports the file
    /// had been modified on disk since the last read; the edit still applied cleanly
    /// (old_string stayed unique). The buffer's edited span is right, but the disk
    /// holds other changes this stream never saw: an authoritative, NON-invalidating
    /// annotation boundary.
    StaleRecovered,
    /// A WINDOWED read recovered from a gated Bash command's stdout (`sed -n 'A,Bp'`
    /// / `head -n N`): lines `[start_line, start_line + lines.len())` verbatim. The
    /// observed extent (`start_line + lines.len() - 1`) is the only total it can
    /// honestly claim; the splice floors, never shrinks, the seen length.
    BashWindowRead {
        start_line: usize,
        lines: Vec<String>,
    },
    /// A byte-known Bash APPEND (`>> file` heredoc/echo, `tee -a`). Placeable ONLY
    /// onto a COMPLETE newline-terminated buffer; otherwise it degrades to a
    /// disclosed heuristic boundary (content known, position not).
    BashAppend { content: String },
    /// A `file-history-snapshot` recorded a disk backup of `--file` at this time. A
    /// COVERAGE ANNOTATION only, never a content anchor: the named blob lives in a
    /// PRUNED tool-layer store with no transcript anchor for its content (list it with
    /// `recover --list-backups`).
    HistorySnapshotMarker,
}

/// The provenance of a [`EventKind::FullSnapshot`]. The `Bash*` variants are the
/// v0.9.4 content anchors: deterministic bash reads/writes admitted by
/// `bash_mutations::bash_anchor` plus the recover-side completeness gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnapSource {
    Write,
    FullRead,
    FileAttachment,
    /// A gated `cat`/to-EOF window read: the command's verbatim stdout.
    BashCat,
    /// A quoted-delimiter heredoc write via cat/tee: the body from the tool_use input.
    BashHeredoc,
    /// A literal `echo`/`printf` write or `truncate -s 0`.
    BashWrite,
}

impl SnapSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            SnapSource::Write => "write",
            SnapSource::FullRead => "full-read",
            SnapSource::FileAttachment => "file-attachment",
            SnapSource::BashCat => "bash-cat",
            SnapSource::BashHeredoc => "bash-heredoc",
            SnapSource::BashWrite => "bash-write",
        }
    }
}

/// The harness integrity-error shapes. Only `ModifiedSinceRead` is a boundary; the
/// rest are COUNTED annotations (the op never landed, so nothing is invalidated, but
/// a scan that saw them must say so instead of dropping them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntegrityKind {
    /// "File has been modified since read, …" - a HARD boundary (disk drift detected).
    ModifiedSinceRead,
    /// "File has not been read yet. …" - the edit never landed; NOT a boundary.
    NotReadYet,
    /// "String to replace not found in file. …" - the edit never landed; counted.
    StringNotFound,
    /// "File does not exist. …" - the op never landed; counted.
    FileDoesNotExist,
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
