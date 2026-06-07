//! `list` subcommand — enumerate sessions with quick identity fields.
//!
//! For each session jsonl, emit: session-id, FIRST genuine-user message, LAST
//! genuine-user message, LAST agent message, and each one's timestamp — the fast
//! "which session is this?" view. Uses head-read for the first user message and a
//! backward tail-read for the last user/agent messages (never a full parse).
//! Timestamps render in Australia/Sydney local plus raw UTC.

use anyhow::Result;

use crate::cli::ListArgs;

/// One row of `list` output.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub session_id: String,
    /// Absolute path to the session jsonl.
    pub path: std::path::PathBuf,
    pub first_user: Option<MessagePreview>,
    pub last_user: Option<MessagePreview>,
    pub last_agent: Option<MessagePreview>,
}

/// A short, timestamped preview of one message for the `list` view.
#[derive(Debug, Clone)]
pub struct MessagePreview {
    /// Raw ISO8601 UTC timestamp, if the record had one.
    pub timestamp_utc: Option<String>,
    /// One-line excerpt of the message text.
    pub excerpt: String,
}

/// Entry point for `csift list`.
pub fn run_list(_args: &ListArgs) -> Result<()> {
    todo!("resolve targets, enumerate sessions, head+tail read, render; Phase 2")
}

/// Build a [`SessionSummary`] for a single session file using head + tail reads.
pub fn summarize_session(_path: &std::path::Path) -> Result<SessionSummary> {
    todo!("head_records for first user, tail_records for last user/agent; Phase 2")
}
