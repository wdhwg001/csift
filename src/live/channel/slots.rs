//! Slot sequencing: keeping N hook processes that fire at once from printing their
//! chunks out of order.
//!
//! The channel is thin-hook: the user pastes N identical lines `csift deliver --slot k`
//! into one event's hook array, and Claude Code runs them as N separate processes with
//! no ordering guarantee between them. Chunk 2 of a message arriving before chunk 1 is
//! not fatal but it is confusing, so each slot waits for its predecessor's marker before
//! emitting.
//!
//! Two rules keep the wait honest:
//! - A TIMEOUT NEVER BLOCKS a delivery. Slot k waits at most [`SLOT_WAIT_MS`]; if the
//!   marker never appears it emits anyway and prefixes its first chunk with
//!   [`disorder_warning`], so the receiver is told the order may be disturbed rather
//!   than losing the message to a wedged predecessor.
//! - The directory lives under the system temp dir, NOT under the sidecar. It is
//!   per-process scratch keyed by the parent pid, it must not survive a reboot, and the
//!   sidecar is the durable record of the channel.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

/// How long slot k waits for slot k-1 before emitting anyway.
pub(crate) const SLOT_WAIT_MS: u64 = 5000;

/// The poll interval while waiting.
pub(crate) const SLOT_POLL_MS: u64 = 50;

/// One slot's handle on the per-event sequencing directory.
#[derive(Debug, Clone)]
pub(crate) struct SlotChain {
    pub(crate) dir: PathBuf,
    pub(crate) slot: u32,
}

/// What the wait for the predecessor ended in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaitOutcome {
    /// Slot 1 (or 0): nothing to wait for.
    First,
    /// The predecessor's marker appeared.
    Ready,
    /// The deadline passed with no marker. The caller emits anyway, with the warning.
    TimedOut,
}

/// The sequencing directory for one (parent process, event, lane) triple.
///
/// The parent pid keys it to the Claude Code process that fired the hooks, so two
/// sessions delivering at the same instant never share a chain; the event keys it so a
/// PreToolUse chain and a Stop chain do not wait on each other.
pub(crate) fn seq_dir(ppid: u32, event: &str, lane: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "csift-deliver-{ppid}-{}-{}",
        sanitize_component(event),
        sanitize_component(lane)
    ))
}

/// Fold anything that is not a plain identifier character into `-`. A lane id is
/// already validated to that alphabet; the event name comes from the hook input, so it
/// is folded rather than trusted.
fn sanitize_component(s: &str) -> String {
    let folded: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if folded.is_empty() {
        "x".to_string()
    } else {
        folded
    }
}

/// Enter the chain as `slot`.
///
/// Slot 1 WIPES and recreates the directory: it is the head of a fresh chain, and a
/// leftover marker from the previous firing of the same event would otherwise let slot 2
/// run immediately against a stale predecessor. Every other slot only ensures the
/// directory exists.
pub(crate) fn open_slot(ppid: u32, event: &str, lane: &str, slot: u32) -> Result<SlotChain> {
    let dir = seq_dir(ppid, event, lane);
    if slot <= 1 {
        let _ = std::fs::remove_dir_all(&dir);
    }
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating slot chain directory {}", dir.display()))?;
    Ok(SlotChain { dir, slot })
}

/// The marker file of one slot inside a chain directory.
fn marker_path(dir: &Path, slot: u32) -> PathBuf {
    dir.join(format!("s{slot}.done"))
}

/// Wait for the predecessor slot, at most [`SLOT_WAIT_MS`].
pub(crate) fn wait_prev(chain: &SlotChain) -> WaitOutcome {
    wait_prev_deadline(chain, SLOT_WAIT_MS)
}

/// [`wait_prev`] with an explicit deadline in milliseconds, so a test can exercise the
/// timeout path without waiting five seconds for it.
pub(crate) fn wait_prev_deadline(chain: &SlotChain, deadline_ms: u64) -> WaitOutcome {
    if chain.slot <= 1 {
        return WaitOutcome::First;
    }
    let target = marker_path(&chain.dir, chain.slot - 1);
    let started = Instant::now();
    let deadline = Duration::from_millis(deadline_ms);
    loop {
        if target.exists() {
            return WaitOutcome::Ready;
        }
        if started.elapsed() >= deadline {
            return WaitOutcome::TimedOut;
        }
        std::thread::sleep(Duration::from_millis(SLOT_POLL_MS));
    }
}

/// Record that this slot has emitted, releasing its successor.
pub(crate) fn mark_done(chain: &SlotChain) -> Result<()> {
    let path = marker_path(&chain.dir, chain.slot);
    std::fs::write(&path, b"").with_context(|| format!("writing slot marker {}", path.display()))
}

/// Remove the chain directory once the last slot has emitted. A failure is ignored on
/// purpose: the directory is scratch, and a delivery must never fail because a temp
/// directory could not be removed.
pub(crate) fn cleanup_if_last(chain: &SlotChain, max_slot: u32) {
    if chain.slot >= max_slot {
        let _ = std::fs::remove_dir_all(&chain.dir);
    }
}

/// The line a slot prefixes to its first chunk when it gave up waiting.
pub(crate) fn disorder_warning(slot: u32) -> String {
    format!(
        "[csift-channel warning: slot {slot} emitted before slot {}; order may be disturbed]",
        slot.saturating_sub(1)
    )
}
