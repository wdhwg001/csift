//! What one firing of the delivery hooks has to say, and the rule that makes every slot
//! of that firing say the SAME thing.
//!
//! THE SHARED-VIEW PROBLEM. The N hook processes of one event each read the lane's ledger
//! and each emit chunk k of the list they build from it. If slot 1's own `emit` line were
//! visible to slot 2, slot 2 would fold a message that is "already emitting" out of the
//! pending set, its list would be one chunk shorter, and part 2 would never be sent. So
//! the chain records a BASELINE - the ledger's byte length at the head of the chain - and
//! every slot folds exactly that prefix. The baseline is written by slot 1 into the chain
//! directory, which slot 1 wipes and recreates, so a stale baseline cannot survive into
//! the next firing; a slot that cannot read one falls back to the ledger as it stands,
//! which is also what a lone slot needs.

use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use super::{
    is_expired, is_queue_event, ledger_path, read_inbox, read_message, render, states, HookInput,
    InboxLine, LedgerLine, MessageState, Mode, RedeliverSource, SlotChain, CHUNK_BUDGET,
};

/// The file inside the chain directory that carries the fold baseline.
const BASELINE_FILE: &str = "ledger.base";

/// The `held` reason for a message whose source file is gone. Recorded once per message:
/// a hole in the channel's own directory is worth saying, and worth saying only once.
pub(crate) const HELD_SOURCE_MISSING: &str = "source-missing";

/// One chunk of one message, ready for the slot whose position it occupies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chunk {
    pub(crate) id: String,
    pub(crate) part: u32,
    pub(crate) parts: u32,
    pub(crate) mode: Mode,
    pub(crate) text: String,
    /// True when this message already carries an exit2 emit, so the vehicle rule never
    /// chains a second turn-blocking emit onto one message.
    pub(crate) prior_exit2: bool,
}

/// Why a message that was NOT put on the wire is worth a ledger line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Bookkeeping {
    /// Its deadline passed before any hook point came.
    Expired(String),
    /// It is being offered again after a compaction threw away the context it landed in.
    Redelivered(String, RedeliverSource),
    /// Its `messages/<id>.json` is gone, so there is nothing to render.
    SourceMissing(String),
}

/// The whole plan of one firing: the chunks in slot order, and the state changes the head
/// of the chain records.
#[derive(Debug, Clone, Default)]
pub(crate) struct Plan {
    pub(crate) chunks: Vec<Chunk>,
    pub(crate) bookkeeping: Vec<Bookkeeping>,
}

/// Read the ledger prefix every slot of this firing agrees on.
///
/// Slot 1 writes the baseline; every other slot reads it after its predecessor's marker
/// released it. A write failure is not fatal - the chain directory is scratch, and a slot
/// that falls back to the live length still delivers, it just risks the shorter list this
/// baseline exists to prevent.
pub(crate) fn fold_baseline(chain: &SlotChain, ledger_len: u64) -> u64 {
    let path = chain.dir.join(BASELINE_FILE);
    if chain.slot <= 1 {
        let _ = std::fs::write(&path, ledger_len.to_string());
        return ledger_len;
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map_or(ledger_len, |n| n.min(ledger_len))
}

/// The byte length of a lane's ledger right now, or 0 when it does not exist yet.
pub(crate) fn ledger_len(root: &Path, lane: &str) -> Result<u64> {
    let path = ledger_path(root, lane)?;
    Ok(std::fs::metadata(&path).map_or(0, |m| m.len()))
}

/// The lane's ledger truncated to `baseline` bytes, with the count of lines the schema
/// could not read.
///
/// A trailing partial line is dropped rather than counted: the baseline is a byte offset,
/// so a cut in the middle of a line is this reader's own doing, not a malformed line.
pub(crate) fn read_ledger_prefix(
    root: &Path,
    lane: &str,
    baseline: u64,
) -> Result<(Vec<LedgerLine>, usize)> {
    let path = ledger_path(root, lane)?;
    let Ok(raw) = std::fs::read(&path) else {
        return Ok((Vec::new(), 0));
    };
    let end = usize::try_from(baseline)
        .unwrap_or(usize::MAX)
        .min(raw.len());
    let text = String::from_utf8_lossy(&raw[..end]).into_owned();
    let complete = text.ends_with('\n');
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    let mut skipped = 0usize;
    for (i, line) in lines.iter().enumerate() {
        if line.trim().is_empty() || (!complete && i + 1 == lines.len()) {
            continue;
        }
        match serde_json::from_str::<Value>(line)
            .ok()
            .as_ref()
            .and_then(LedgerLine::from_json)
        {
            Some(parsed) => out.push(parsed),
            None => skipped += 1,
        }
    }
    Ok((out, skipped))
}

/// Build the firing's plan from the lane's inbox and the agreed ledger prefix.
pub(crate) fn plan(
    root: &Path,
    lane: &str,
    hook: &HookInput,
    ledger: &[LedgerLine],
    now_utc: &str,
) -> Result<Plan> {
    // The unreadable-line count is dropped here, as it is for the ledger prefix the caller
    // folds: a hook prints one output object and no diagnostics, so there is no surface to
    // disclose it on. `csift msg` reads the same two files and reports the count there.
    let (inbox, _) = read_inbox(root, lane)?;
    let folded = states(ledger);
    let mut out = Plan::default();
    let mut seen: Vec<&str> = Vec::new();
    for line in &inbox {
        // The inbox is append-only, so one id re-enqueued is still one message.
        if seen.contains(&line.id.as_str()) {
            continue;
        }
        seen.push(line.id.as_str());
        let state = folded.get(&line.id);
        match verdict(line, state, hook, ledger, now_utc) {
            Verdict::Skip => {}
            Verdict::Expire => out.bookkeeping.push(Bookkeeping::Expired(line.id.clone())),
            Verdict::Send { redeliver } => {
                if let Some(source) = redeliver {
                    out.bookkeeping
                        .push(Bookkeeping::Redelivered(line.id.clone(), source));
                }
                append_chunks(root, line, state, &mut out)?;
            }
        }
    }
    Ok(out)
}

/// What this firing does about one enqueued message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// Nothing: acked, already booked as expired, already emitted, or the wrong event for
    /// its mode.
    Skip,
    /// Its deadline has passed and the ledger does not say so yet.
    Expire,
    Send {
        redeliver: Option<RedeliverSource>,
    },
}

fn verdict(
    line: &InboxLine,
    state: Option<&MessageState>,
    hook: &HookInput,
    ledger: &[LedgerLine],
    now_utc: &str,
) -> Verdict {
    if state.is_some_and(|s| s.acked) {
        return Verdict::Skip;
    }
    if line
        .expires_utc
        .as_deref()
        .is_some_and(|e| is_expired(e, now_utc))
    {
        return if state.is_some_and(|s| s.expired) {
            Verdict::Skip
        } else {
            Verdict::Expire
        };
    }
    if !mode_eligible(line.mode, hook) {
        return Verdict::Skip;
    }
    match state {
        // Never emitted: pending at any eligible event. A HELD message has exactly this
        // shape, so a re-entry event delivers it with no separate rule.
        None => Verdict::Send { redeliver: None },
        Some(s) if !s.first_part_emitted() => Verdict::Send { redeliver: None },
        Some(_)
            if hook.session_start_source("compact")
                && !redelivered_since_emit(ledger, &line.id) =>
        {
            Verdict::Send {
                redeliver: Some(RedeliverSource::Compact),
            }
        }
        Some(_) => Verdict::Skip,
    }
}

/// True when this message's newest `redelivered` line is NEWER than its newest `emit`.
///
/// Read off the raw lines in append order rather than the fold, because the fold keeps
/// sets and not an order. This is what makes a compaction redelivery happen once per
/// compaction and stay identical across the slots of one firing: the head of the chain
/// writes `redelivered` BEFORE its emit, so a later slot still reads the emit as the newer
/// line and reaches the same verdict, while the NEXT compaction sees the re-emitted parts
/// and offers the message again.
fn redelivered_since_emit(ledger: &[LedgerLine], id: &str) -> bool {
    for line in ledger.iter().rev() {
        if line.id() != Some(id) {
            continue;
        }
        match line {
            LedgerLine::Redelivered { .. } => return true,
            LedgerLine::Emit { .. } => return false,
            _ => {}
        }
    }
    false
}

/// A steer message rides any eligible event; a queue message only a turn boundary.
fn mode_eligible(mode: Mode, hook: &HookInput) -> bool {
    match mode {
        Mode::Steer => true,
        Mode::Queue => is_queue_event(&hook.hook_event_name, hook.source.as_deref()),
    }
}

/// Render one message and append its chunks, each stamped with the part total the
/// renderer printed into its own header.
fn append_chunks(
    root: &Path,
    line: &InboxLine,
    state: Option<&MessageState>,
    out: &mut Plan,
) -> Result<()> {
    let Some(msg) = read_message(root, &line.id)? else {
        let already =
            state.is_some_and(|s| s.held_reasons.iter().any(|r| r == HELD_SOURCE_MISSING));
        if !already {
            out.bookkeeping
                .push(Bookkeeping::SourceMissing(line.id.clone()));
        }
        return Ok(());
    };
    let rendered = render(&msg, CHUNK_BUDGET)?;
    let parts = u32::try_from(rendered.len()).unwrap_or(u32::MAX);
    let prior_exit2 = state.is_some_and(MessageState::has_exit2);
    for (i, text) in rendered.into_iter().enumerate() {
        out.chunks.push(Chunk {
            id: msg.id.clone(),
            part: u32::try_from(i + 1).unwrap_or(u32::MAX),
            parts,
            mode: line.mode,
            text,
            prior_exit2,
        });
    }
    Ok(())
}
