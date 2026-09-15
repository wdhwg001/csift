//! The on-disk layout of one channel root, plus the two write primitives every channel
//! file uses.
//!
//! Root: `<session-sidecar-dir>/csift-channel/` - the `<uuid>/` directory beside
//! `subagents/`, reached through [`crate::subagent::sidecar_dir_for_session`]. csift
//! writes NOTHING else: not a transcript, not the team mailbox, not the messaging
//! socket, not the session registry, not a settings file.
//!
//! Two write shapes, and the choice is not stylistic:
//! - APPEND (`outbox.jsonl`, `inbox/<lane>.jsonl`, `ledger/<lane>.jsonl`) opens with
//!   `append(true).create(true)`, which is `O_APPEND` on unix and `FILE_APPEND_DATA` on
//!   Windows, and writes the whole line in ONE `write_all`. Several hook processes fire
//!   concurrently on one lane, so an append that seeks would interleave halves of two
//!   lines; an atomic-append write of a line below the pipe buffer does not.
//! - REWRITE (`armed/<lane>.json`) goes through a temp file plus a rename, so a reader
//!   racing the writer sees either the old marker or the new one, never a half-written
//!   one.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::Value;

/// The directory name csift owns inside a session's sidecar dir.
pub(crate) const CHANNEL_DIR_NAME: &str = "csift-channel";

/// The channel root for a session sidecar dir (`<sidecar>/csift-channel`). The
/// directory is created on demand by the writers, never here.
pub(crate) fn channel_dir(sidecar_dir: &Path) -> PathBuf {
    sidecar_dir.join(CHANNEL_DIR_NAME)
}

/// `messages/` - the full message sources, one JSON file each.
pub(crate) fn messages_dir(root: &Path) -> PathBuf {
    root.join("messages")
}

/// `messages/<id>.json` for a validated message id.
pub(crate) fn message_path(root: &Path, id: &str) -> Result<PathBuf> {
    validate_message_id(id)?;
    Ok(messages_dir(root).join(format!("{id}.json")))
}

/// `outbox.jsonl` - the sender's own log of what it sent, one line per send.
pub(crate) fn outbox_path(root: &Path) -> PathBuf {
    root.join("outbox.jsonl")
}

/// `inbox/<lane>.jsonl` - what was enqueued for one receiver lane. Append-only: a
/// message is never removed from it, and delivery state lives in the ledger.
pub(crate) fn inbox_path(root: &Path, lane: &str) -> Result<PathBuf> {
    validate_lane_id(lane)?;
    Ok(root.join("inbox").join(format!("{lane}.jsonl")))
}

/// `ledger/<lane>.jsonl` - what csift did about those messages (emit / held / expired /
/// ack / redelivered / refused).
pub(crate) fn ledger_path(root: &Path, lane: &str) -> Result<PathBuf> {
    validate_lane_id(lane)?;
    Ok(root.join("ledger").join(format!("{lane}.jsonl")))
}

/// `armed/<lane>.json` - the runtime marker `deliver` rewrites on every event it
/// handles, so a sender can see which slots are actually installed.
pub(crate) fn armed_path(root: &Path, lane: &str) -> Result<PathBuf> {
    validate_lane_id(lane)?;
    Ok(root.join("armed").join(format!("{lane}.json")))
}

/// True for a lane id csift will name a file after: a top-level session uuid, a bare
/// `a<16 hex>` agent id, or a name-embedded teammate id `a<Name>-<16 hex>`.
///
/// The shape predicates are the crate's existing ones ([`crate::path::is_uuid`] and
/// [`crate::path::is_subagent_id`]), so every id `csift agents` prints is a usable lane
/// id and no fourth copy of the grammar can drift from them. Every accepted shape is
/// drawn from `[A-Za-z0-9-]`, which is also what makes it safe as a path component: a
/// separator, a `..` or a drive letter cannot pass.
pub(crate) fn is_lane_id(s: &str) -> bool {
    crate::path::is_uuid(s) || crate::path::is_subagent_id(s)
}

/// Fail loudly on anything that is not a lane id. A silent skip here would write a
/// message into a file nobody reads.
pub(crate) fn validate_lane_id(s: &str) -> Result<()> {
    if !is_lane_id(s) {
        bail!(
            "not a lane id: `{s}` - a lane is a top-level session uuid, a bare `a<16 hex>` \
             agent id, or a teammate id `a<Name>-<16 hex>`"
        );
    }
    Ok(())
}

/// True for a message id: exactly 16 lowercase hex characters.
pub(crate) fn is_message_id(s: &str) -> bool {
    s.len() == 16
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(crate) fn validate_message_id(s: &str) -> Result<()> {
    if !is_message_id(s) {
        bail!("not a message id: `{s}` - a message id is 16 lowercase hex characters");
    }
    Ok(())
}

/// Append one JSON line, creating the parent directories on demand.
///
/// The newline is part of the SAME `write_all` as the payload: with `O_APPEND` a single
/// write below the platform's atomic-write size lands whole, so two hook processes
/// appending to one lane's ledger interleave whole lines rather than fragments.
pub(crate) fn append_line(path: &Path, line: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating channel directory {}", parent.display()))?;
    }
    let mut buf = String::with_capacity(line.len() + 1);
    buf.push_str(line);
    buf.push('\n');
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .with_context(|| format!("opening {} for append", path.display()))?;
    f.write_all(buf.as_bytes())
        .with_context(|| format!("appending to {}", path.display()))
}

/// Rewrite a whole file atomically: write a sibling temp file, then rename over the
/// target. A reader that opens the path mid-write sees the previous contents, never a
/// truncated marker.
pub(crate) fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent directory for {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating channel directory {}", parent.display()))?;
    let stem = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("channel");
    // The temp name carries the pid and a counter so two writers on one lane cannot
    // rename each other's half-written file into place.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = parent.join(format!(".{stem}.tmp-{}-{seq}", std::process::id()));
    {
        let mut f =
            std::fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        f.write_all(contents.as_bytes())
            .with_context(|| format!("writing {}", tmp.display()))?;
    }
    std::fs::rename(&tmp, path).with_context(|| {
        format!(
            "renaming {} over {} (atomic marker rewrite)",
            tmp.display(),
            path.display()
        )
    })
}

/// Read a channel jsonl into parsed values plus the count of lines that did not parse.
///
/// A missing file is the common case (nothing sent to this lane yet) and yields an
/// empty result, not an error. A malformed line is COUNTED and returned to the caller,
/// never dropped in silence - the same never-silent rule the scanning surfaces obey.
pub(crate) fn read_jsonl(path: &Path) -> Result<(Vec<Value>, usize)> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Ok((Vec::new(), 0));
    };
    let mut out = Vec::new();
    let mut skipped = 0usize;
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(v) => out.push(v),
            Err(_) => skipped += 1,
        }
    }
    Ok((out, skipped))
}

/// Read a single-object channel file (the armed marker). A missing or unparseable file
/// yields `None`: the marker is advisory runtime state, and its absence means "this
/// lane has never run a delivery hook", which is a fact, not an error.
pub(crate) fn read_json_object(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    v.is_object().then_some(v)
}
