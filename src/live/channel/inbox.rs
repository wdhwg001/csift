//! `inbox/<lane>.jsonl` - what was enqueued for one receiver lane.
//!
//! Append-only, and a message is NEVER removed from it. That is deliberate: the official
//! team mailbox deletes an entry when it is consumed, which makes "was this delivered?"
//! unanswerable from the file afterwards. Here the inbox stays the record of intent and
//! the per-lane ledger carries every state change, so the two together reconstruct the
//! whole life of a message long after it was read.

use std::path::Path;

use anyhow::Result;
use serde_json::{json, Value};

use super::{inbox_path, read_jsonl, str_field, Mode};

/// One enqueued message, as the receiver lane's inbox records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InboxLine {
    pub(crate) id: String,
    pub(crate) enqueued_utc: String,
    pub(crate) mode: Mode,
    /// Absent when the send carried no ttl: such a message never expires on its own.
    pub(crate) expires_utc: Option<String>,
}

impl InboxLine {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "enqueued_utc": self.enqueued_utc,
            "mode": self.mode.as_str(),
            "expires_utc": self.expires_utc,
        })
    }

    pub(crate) fn from_json(v: &Value) -> Option<Self> {
        Some(InboxLine {
            id: str_field(v, "id")?,
            enqueued_utc: str_field(v, "enqueued_utc")?,
            mode: Mode::parse(str_field(v, "mode")?.as_str())?,
            expires_utc: str_field(v, "expires_utc"),
        })
    }
}

/// Append one enqueued message to a lane's inbox.
pub(crate) fn append_inbox(root: &Path, lane: &str, line: &InboxLine) -> Result<()> {
    let path = inbox_path(root, lane)?;
    super::append_line(&path, &serde_json::to_string(&line.to_json())?)
}

/// Read a lane's inbox in append order, with the count of lines the current schema
/// could not read (a malformed line or one written by a newer csift). The count is
/// returned, never swallowed.
pub(crate) fn read_inbox(root: &Path, lane: &str) -> Result<(Vec<InboxLine>, usize)> {
    let path = inbox_path(root, lane)?;
    let (values, mut skipped) = read_jsonl(&path)?;
    let mut out = Vec::with_capacity(values.len());
    for v in &values {
        match InboxLine::from_json(v) {
            Some(line) => out.push(line),
            None => skipped += 1,
        }
    }
    Ok((out, skipped))
}
