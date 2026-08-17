//! SessionSummary + MessagePreview row types.

use super::*;

/// Max characters of a message excerpt shown inline before truncation. Truncation
/// is ALWAYS explicit (`… (+N chars)`) - never silent (SPEC §0, §8.1).
///
/// Deliberately SHORTER than `search`'s 400-char cap (`search::EXCERPT_MAX`): `list`
/// is a scannable identity index (many one-line previews at a glance), whereas
/// `search` shows the matched exchange where more surrounding context is useful. The
/// two caps are intentionally different - not an oversight.
pub(crate) const EXCERPT_MAX: usize = 200;

/// One row of `list` output.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub session_id: String,
    /// True when this row is a SUBAGENT transcript (so `session_id` is a bare hex, NOT a
    /// re-feedable `@<uuid>` target). Discriminates the id-domain - the SAME shape
    /// `search`/`files`/`turns`/`recover` carry, so a `list` JSON consumer can tell a
    /// subagent row from a top-level uuid row without string-parsing `path`.
    pub is_subagent: bool,
    /// The re-feedable PARENT session uuid (the owning top-level session). Equals
    /// `session_id` for a top-level row; for a subagent row it is the uuid you re-feed
    /// (`csift verbatim <parent_session_id>` works; the bare hex does not).
    pub parent_session_id: String,
    /// Absolute path to the session jsonl.
    pub path: PathBuf,
    /// Decoded human-readable cwd (read from the data, §2.4), if present.
    pub cwd: Option<String>,
    pub version: Option<String>,
    pub git_branch: Option<String>,
    pub first_user: Option<MessagePreview>,
    pub last_user: Option<MessagePreview>,
    pub last_agent: Option<MessagePreview>,
    /// Count of malformed lines skipped while reading this session (never hidden).
    pub skipped_lines: usize,
    /// One-line renders of this session's UNRESOLVED-pending elicitations (§3.10) merged from
    /// the hook-written sidecar - what the session is currently blocked on (AskUserQuestion /
    /// ExitPlanMode / MCP), MISSING from the native transcript. Empty for a top-level session
    /// with no sidecar / no pending, and ALWAYS empty for a subagent row (the sidecar is keyed
    /// by the top-level session). Drives the `with elicitation sidecar` annotation.
    pub pending_elicitations: Vec<String>,
    /// True when the session's elicitation SIDECAR FILE exists at all (= the csift hook
    /// is installed for this session - resolved pairs stay in the file). The tri-state a
    /// consumer needs: present+pending / present+none (safe to conclude "not blocked on
    /// an elicitation") / absent (hook unknown - CANNOT conclude anything). Always false
    /// for a subagent row (the sidecar is keyed by the top-level session).
    pub sidecar_present: bool,
}

/// A short, timestamped preview of one message for the `list` view.
#[derive(Debug, Clone)]
pub struct MessagePreview {
    /// Raw ISO8601 UTC timestamp, if the record had one.
    pub timestamp_utc: Option<String>,
    /// One-line excerpt of the message text (already whitespace-normalized).
    pub excerpt: String,
}

impl MessagePreview {
    pub(crate) fn from(timestamp_utc: Option<String>, text: &str) -> Self {
        MessagePreview {
            timestamp_utc,
            excerpt: truncate_excerpt(text),
        }
    }
}

/// Truncate to [`EXCERPT_MAX`] chars with the shared explicit `… (+N chars)` marker.
pub(crate) fn truncate_excerpt(s: &str) -> String {
    crate::text::truncate_excerpt(s, EXCERPT_MAX)
}
