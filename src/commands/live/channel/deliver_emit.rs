//! How a chunk reaches the model: the vehicle choice, the block-cap arithmetic behind it,
//! and the single JSON object a delivery is allowed to print.
//!
//! Two vehicles exist and they are not interchangeable. `additionalContext` rides the
//! hook's stdout at exit 0 and works at every delivery event. Exit 2 exists only in the
//! Stop family, where it also BLOCKS the turn from ending, which is what makes a queue
//! message arrive as steering rather than as a note the model reads after it has already
//! finished. The harness ends the turn itself once a Stop hook has blocked it
//! `CLAUDE_CODE_STOP_HOOK_BLOCK_CAP` times in a row (8 unless the environment says
//! otherwise, and a non-positive value disables the vehicle here rather than inviting the
//! override), so the arithmetic keeps one block of headroom under that ceiling and never
//! spends two blocks on one message.

use std::io::Write as _;

use anyhow::Result;
use serde_json::json;

use super::{consecutive_exit2_blocks, Chunk, HookInput, LedgerLine, Mode, Vehicle};

/// The block ceiling the harness applies when the environment names none.
pub(crate) const DEFAULT_BLOCK_CAP: i64 = 8;

/// The environment variable that moves it.
pub(crate) const BLOCK_CAP_ENV: &str = "CLAUDE_CODE_STOP_HOOK_BLOCK_CAP";

/// The `held` reason recorded beside an emit whose turn-blocking vehicle was withheld.
pub(crate) const HELD_BLOCK_CAP: &str = "block-cap";

/// The chosen vehicle plus what the ledger has to record about the choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VehicleChoice {
    pub(crate) vehicle: Vehicle,
    /// The consecutive-block position this emit occupies, recorded only for exit 2.
    pub(crate) block_count: Option<u32>,
    /// True when the exit-2 vehicle was withheld and the chunk fell back to
    /// `additionalContext`: the message still goes out, so this is a `held` line ALONGSIDE
    /// the emit, recording the downgrade rather than a message nobody sent.
    pub(crate) held_block_cap: bool,
}

/// Read the ceiling from a raw environment value. An unset or unreadable value is the
/// harness default, never zero: guessing zero would silently disable a vehicle the
/// receiver's harness still honours.
pub(crate) fn block_cap(raw: Option<&str>) -> i64 {
    raw.and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_BLOCK_CAP)
}

/// Whether one more turn-blocking emit fits.
///
/// Four conditions, each closing a different hole: the vehicle has to be enabled at all; a
/// turn that is ALREADY continuing because a stop hook blocked it is the harness asking
/// this hook to stop blocking; the lane needs a block of headroom left under the ceiling;
/// and one message never chains two blocks, so a second part rides `additionalContext`.
pub(crate) fn exit2_fits(
    cap: i64,
    consecutive: usize,
    prior_exit2: bool,
    stop_hook_active: bool,
) -> bool {
    if cap <= 0 || stop_hook_active || prior_exit2 {
        return false;
    }
    let used = i64::try_from(consecutive).unwrap_or(i64::MAX);
    used.saturating_add(1) < cap
}

/// Choose the vehicle for one chunk at one event.
pub(crate) fn choose_vehicle(
    hook: &HookInput,
    chunk: &Chunk,
    ledger: &[LedgerLine],
    cap: i64,
) -> VehicleChoice {
    let additional = VehicleChoice {
        vehicle: Vehicle::AdditionalContext,
        block_count: None,
        held_block_cap: false,
    };
    if !hook.is_stop_family() || chunk.mode != Mode::Queue {
        return additional;
    }
    let consecutive = consecutive_exit2_blocks(ledger);
    if exit2_fits(cap, consecutive, chunk.prior_exit2, hook.stop_hook_active) {
        return VehicleChoice {
            vehicle: Vehicle::Exit2,
            block_count: Some(u32::try_from(consecutive + 1).unwrap_or(u32::MAX)),
            held_block_cap: false,
        };
    }
    VehicleChoice {
        held_block_cap: true,
        ..additional
    }
}

/// The one JSON object a delivery prints on stdout.
pub(crate) fn hook_output(event: &str, chunk_text: &str) -> Result<String> {
    Ok(serde_json::to_string(&json!({
        "hookSpecificOutput": {
            "hookEventName": event,
            "additionalContext": chunk_text,
        }
    }))?)
}

/// Put the chunk on the wire.
///
/// `additionalContext` prints one JSON line and returns; exit 2 prints the chunk on
/// STDERR and leaves the process immediately with code 2, because that code is the whole
/// signal - a `Result` returned here would be mapped to the ordinary success exit and the
/// turn would end. Both streams are flushed first: the harness reads what the process
/// wrote, not what it buffered.
pub(crate) fn emit(event: &str, chunk_text: &str, vehicle: Vehicle) -> Result<()> {
    match vehicle {
        Vehicle::AdditionalContext => {
            println!("{}", hook_output(event, chunk_text)?);
            Ok(())
        }
        Vehicle::Exit2 => {
            eprintln!("{chunk_text}");
            let _ = std::io::stdout().flush();
            let _ = std::io::stderr().flush();
            std::process::exit(2);
        }
    }
}
