//! The sender's two writes: `messages/<id>.json` (the message itself, written once) and
//! `outbox.jsonl` (one line per send, in the SENDER's own session).
//!
//! Splitting them keeps the sender's own record out of the receiver's directory: the
//! outbox line carries the verdict, the channel chosen and whether an official send was
//! delegated, while the body and the endpoints stay in the message file the receiver's
//! delivery reads. Only the message file is read back - every command that reads the
//! channel is addressed at a RECEIVER lane, so the outbox has a writer and no reader.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{json, Value};

use super::{message_path, outbox_path, Message, Mode, Verdict};

/// What the send told the caller to do on the OFFICIAL channel, if anything. csift never
/// performs an official send - the official transports are tools, callable only by the
/// model - so this records a delegation, never an action csift took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OfficialRef {
    pub(crate) delegated: bool,
    pub(crate) tool: Option<String>,
    /// The `to` value the official tool needs, which is NOT always the lane id: a
    /// teammate is addressed by its routing form `Name@Team`.
    pub(crate) to_form: Option<String>,
}

impl OfficialRef {
    pub(crate) fn none() -> Self {
        OfficialRef {
            delegated: false,
            tool: None,
            to_form: None,
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "delegated": self.delegated,
            "tool": self.tool,
            "to_form": self.to_form,
        })
    }
}

/// One line of the sender's outbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutboxLine {
    pub(crate) id: String,
    pub(crate) ts_utc: String,
    pub(crate) to_lane: String,
    pub(crate) to_session: String,
    pub(crate) mode: Mode,
    pub(crate) verdict: Verdict,
    pub(crate) channel: String,
    pub(crate) official: OfficialRef,
}

impl OutboxLine {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "ts_utc": self.ts_utc,
            "to_lane": self.to_lane,
            "to_session": self.to_session,
            "mode": self.mode.as_str(),
            "verdict": self.verdict.as_str(),
            "channel": self.channel,
            "official": self.official.to_json(),
        })
    }
}

/// Append one send to the sender's outbox. The one operation the file has: nothing reads
/// it back, so nothing parses it.
pub(crate) fn append_outbox(root: &Path, line: &OutboxLine) -> Result<()> {
    super::append_line(&outbox_path(root), &serde_json::to_string(&line.to_json())?)
}

/// Write `messages/<id>.json`. The message source is written once at send and never
/// rewritten, so this goes through the atomic rewrite path: a delivery reading the file
/// while it is being written sees nothing or the whole thing.
pub(crate) fn write_message(root: &Path, msg: &Message) -> Result<PathBuf> {
    let path = message_path(root, &msg.id)?;
    super::write_atomic(&path, &serde_json::to_string(&msg.to_json())?)?;
    Ok(path)
}

/// Read one message source. `Ok(None)` when the file is absent or the current schema
/// cannot read it - the caller decides whether that is a hole worth reporting, because
/// a missing source and an unreadable one mean different things to a delivery.
pub(crate) fn read_message(root: &Path, id: &str) -> Result<Option<Message>> {
    let path = message_path(root, id)?;
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return Ok(None);
    };
    Ok(Message::from_json(&v))
}
