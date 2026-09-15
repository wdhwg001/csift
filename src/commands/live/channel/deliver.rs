//! `csift deliver` - the hook entry of the csift channel.
//!
//! One invocation is one hook process: read the payload on stdin, decide whether this lane
//! has anything waiting at this hook point, and print at most one hook-output object. The
//! whole channel is thin-hook by design, so everything a pasted line could get wrong lives
//! here instead: the refusal shapes, the armed marker, the event filter, the chunk
//! assignment across slots, the ledger and the block-cap arithmetic.
//!
//! THE REFUSAL SHAPES. A served or remote call reaches the same hook builder with
//! `session_id:"served:<caller>"` and an EMPTY `transcript_path`. Those two fields are how
//! csift finds the lane and its sidecar, so a payload missing either is refused: nothing is
//! printed, the exit is 0, and the refusal is written to the lane's ledger whenever a lane
//! and a directory can still be named. When neither can be, the refusal IS the silent
//! exit - inventing a path to record it in would be worse than not recording it.
//!
//! WHAT IT WRITES. Only `<session>/csift-channel/`: the per-lane ledger and the per-lane
//! armed marker. Never a transcript, never a settings file, never anything the harness
//! owns - and never a hook, which is why `--recipe` prints a block for a human to paste.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::cli::{DeliverArgs, DeliverShell};

use super::{
    append_ledger, block_cap, channel_dir, choose_vehicle, cleanup_if_last, fold_baseline,
    is_lane_id, is_steer_event, ledger_len, mark_done, now_utc, open_slot, plan, read_armed,
    read_ledger_prefix, refresh_armed, run_recipe, wait_prev, ArmedMarker, Bookkeeping, Chunk,
    HookInput, LedgerLine, Plan, RecipeShell, SlotChain, VehicleChoice, WaitOutcome, BLOCK_CAP_ENV,
    HELD_BLOCK_CAP, HELD_SOURCE_MISSING,
};

/// How much of the transcript tail is read to learn the Claude Code version for the armed
/// marker. A version stamp rides every record, so the newest few kilobytes always carry
/// one, and a hook must not pay for a whole-file scan to report it.
const VERSION_TAIL_BYTES: u64 = 16 * 1024;

/// The environment variable that names the running Claude Code version, used when the
/// transcript tail carries no record with a version stamp.
const VERSION_ENV: &str = "CLAUDE_CODE_VERSION";

/// The refusal reasons, both of them shapes the harness really produces.
const REFUSED_NO_TRANSCRIPT: &str = "no transcript path in the hook payload";
const REFUSED_LANE_ID: &str = "the payload's agent id is not a lane id";

/// Where one invocation is allowed to act.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// A lane and its channel root: proceed.
    Ready { root: PathBuf, lane: String },
    /// A directory csift may write to, but a payload it will not act on.
    Refused {
        root: PathBuf,
        lane: String,
        reason: &'static str,
    },
    /// Neither a lane nor a directory: exit 0, print nothing, record nothing.
    Silent,
}

/// The `deliver` entry point.
pub(crate) fn run_deliver(args: &DeliverArgs) -> Result<()> {
    if args.recipe {
        return run_recipe(args.slots, recipe_shell(args.shell));
    }
    // stdin is the hook payload. Anything that is not a JSON object says nothing about a
    // lane, so there is nothing to record and nothing to print.
    let Some(raw) = read_stdin() else {
        return Ok(());
    };
    let Some(hook) = HookInput::parse(&raw) else {
        return Ok(());
    };
    match target(&hook) {
        Target::Silent => Ok(()),
        Target::Refused { root, lane, reason } => append_ledger(
            &root,
            &lane,
            &LedgerLine::Refused {
                id: None,
                reason: reason.to_string(),
                ts_utc: now_utc(),
            },
        ),
        Target::Ready { root, lane } => handle(&hook, &root, &lane, args.slot.unwrap_or(1)),
    }
}

fn recipe_shell(shell: DeliverShell) -> RecipeShell {
    match shell {
        DeliverShell::Bash => RecipeShell::Bash,
        DeliverShell::Powershell => RecipeShell::Powershell,
    }
}

/// Read the whole hook payload. An unreadable or blank stdin yields `None`: a hook that
/// was handed nothing has nothing to deliver.
fn read_stdin() -> Option<String> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw).ok()?;
    (!raw.trim().is_empty()).then_some(raw)
}

/// Classify the payload into the place this invocation may act.
fn target(hook: &HookInput) -> Target {
    // A served or remote call names no session csift can place. There is no lane to file a
    // refusal under, so the refusal is the exit itself.
    if !crate::path::is_uuid(&hook.session_id) {
        return Target::Silent;
    }
    let Some(root) = channel_root(hook) else {
        return Target::Silent;
    };
    if hook.transcript_path.trim().is_empty() {
        return Target::Refused {
            root,
            lane: hook.session_id.clone(),
            reason: REFUSED_NO_TRANSCRIPT,
        };
    }
    let lane = hook.lane().to_string();
    if is_lane_id(&lane) {
        Target::Ready { root, lane }
    } else {
        // The session is still a lane csift can name, so the refusal is recordable there.
        Target::Refused {
            root,
            lane: hook.session_id.clone(),
            reason: REFUSED_LANE_ID,
        }
    }
}

/// `<projects>/<encoded>/<session-uuid>/csift-channel`.
///
/// The transcript path in the payload already names the project directory, so the hook
/// needs no `--claude-home` and no project resolution. When it names none, the payload's
/// `cwd` still locates the project under the resolved projects root - which is what makes
/// the empty-transcript refusal recordable rather than silent.
fn channel_root(hook: &HookInput) -> Option<PathBuf> {
    let project_dir = if hook.transcript_path.trim().is_empty() {
        let cwd = hook.cwd.as_deref()?;
        crate::path::projects_root()
            .ok()?
            .join(crate::path::encode_cwd(Path::new(cwd)))
    } else {
        Path::new(&hook.transcript_path).parent()?.to_path_buf()
    };
    Some(channel_dir(&project_dir.join(&hook.session_id)))
}

/// The three steps every handled event runs: refresh the marker, apply the event filter,
/// and deliver when the lane has an inbox at all.
fn handle(hook: &HookInput, root: &Path, lane: &str, slot: u32) -> Result<()> {
    let now = now_utc();
    // The pre-refresh marker is the only record of how long this lane's chain has been
    // observed to be, which is what the chain cleanup reads.
    let prior = read_armed(root, lane)?;
    let version = claude_code_version(hook);
    refresh_armed(
        root,
        lane,
        slot,
        &hook.hook_event_name,
        &now,
        Some(hook.session_id.as_str()),
        version.as_deref(),
    )?;
    if !is_steer_event(&hook.hook_event_name) {
        return Ok(());
    }
    // Nothing enqueued: return before touching the chain directory, so a hook on a lane
    // with an empty inbox costs one stat. Every slot makes the same call, so no slot is
    // left waiting for a predecessor that returned early.
    if inbox_is_empty(root, lane)? {
        return Ok(());
    }
    emit_for_slot(hook, root, lane, slot, &now, prior.as_ref())
}

/// True when this lane has no inbox file, or an empty one.
fn inbox_is_empty(root: &Path, lane: &str) -> Result<bool> {
    let path = super::inbox_path(root, lane)?;
    Ok(std::fs::metadata(&path).map_or(true, |m| m.len() == 0))
}

/// Take this slot's place in the chain, build the firing's plan and emit its chunk.
fn emit_for_slot(
    hook: &HookInput,
    root: &Path,
    lane: &str,
    slot: u32,
    now: &str,
    prior: Option<&ArmedMarker>,
) -> Result<()> {
    let chain = open_slot(chain_key(hook), &hook.hook_event_name, lane, slot)?;
    let outcome = wait_prev(&chain);
    let baseline = fold_baseline(&chain, ledger_len(root, lane)?);
    let (ledger, _) = read_ledger_prefix(root, lane, baseline)?;
    let firing = plan(root, lane, hook, &ledger, now)?;
    let cap = block_cap(std::env::var(BLOCK_CAP_ENV).ok().as_deref());
    let index = usize::try_from(slot)
        .unwrap_or(usize::MAX)
        .saturating_sub(1);
    let chosen = firing
        .chunks
        .get(index)
        .map(|c| (c.clone(), choose_vehicle(hook, c, &ledger, cap)));

    // The head of the chain records the firing's state changes; every slot folds the same
    // ledger prefix, so all of them reached the same decision and one writer is enough.
    if slot <= 1 {
        record_bookkeeping(root, lane, &firing, now)?;
    }
    if let Some((chunk, choice)) = &chosen {
        record_emit(root, lane, hook, slot, chunk, choice, now)?;
    }
    // Release the successor BEFORE emitting: the exit-2 vehicle leaves the process
    // immediately, and a slot that never marked itself done would leave the next one
    // waiting out its whole deadline.
    mark_done(&chain)?;
    cleanup_chain(&chain, prior);
    match chosen {
        Some((chunk, choice)) => super::emit(
            &hook.hook_event_name,
            &with_order_warning(&chunk.text, outcome, slot),
            choice.vehicle,
        ),
        None => Ok(()),
    }
}

/// Prefix the disorder warning when this slot gave up waiting for its predecessor. A
/// timeout never blocks a delivery: the receiver is told the order may be disturbed
/// instead of losing the message to a wedged slot.
pub(crate) fn with_order_warning(text: &str, outcome: WaitOutcome, slot: u32) -> String {
    match outcome {
        WaitOutcome::TimedOut => format!("{}\n{text}", super::disorder_warning(slot)),
        WaitOutcome::First | WaitOutcome::Ready => text.to_string(),
    }
}

/// Write the state changes this firing decided on, in the order a reader folds them.
fn record_bookkeeping(root: &Path, lane: &str, firing: &Plan, now: &str) -> Result<()> {
    for item in &firing.bookkeeping {
        let line = match item {
            Bookkeeping::Expired(id) => LedgerLine::Expired {
                id: id.clone(),
                ts_utc: now.to_string(),
            },
            Bookkeeping::Redelivered(id, source) => LedgerLine::Redelivered {
                id: id.clone(),
                source: *source,
                ts_utc: now.to_string(),
            },
            Bookkeeping::SourceMissing(id) => LedgerLine::Held {
                id: id.clone(),
                reason: HELD_SOURCE_MISSING.to_string(),
                ts_utc: now.to_string(),
            },
        };
        append_ledger(root, lane, &line)?;
    }
    Ok(())
}

/// Record the emit, and - when the turn-blocking vehicle was withheld - the downgrade
/// beside it. The held line comes FIRST so a reader folding in append order sees the
/// vehicle decision before the emit it applies to.
fn record_emit(
    root: &Path,
    lane: &str,
    hook: &HookInput,
    slot: u32,
    chunk: &Chunk,
    choice: &VehicleChoice,
    now: &str,
) -> Result<()> {
    if choice.held_block_cap {
        append_ledger(
            root,
            lane,
            &LedgerLine::Held {
                id: chunk.id.clone(),
                reason: HELD_BLOCK_CAP.to_string(),
                ts_utc: now.to_string(),
            },
        )?;
    }
    append_ledger(
        root,
        lane,
        &LedgerLine::Emit {
            id: chunk.id.clone(),
            event: hook.hook_event_name.clone(),
            slot,
            part: chunk.part,
            parts: chunk.parts,
            vehicle: choice.vehicle,
            ts_utc: now.to_string(),
            hook_session: Some(hook.session_id.clone()),
            hook_agent_id: hook.agent_id.clone(),
            block_count: choice.block_count,
        },
    )
}

/// Remove the chain directory once the highest slot this lane has EVER run has emitted.
///
/// The configured chain length is not in the payload, and reading the settings cascade on
/// every tool event is not a cost a hook should pay - so the armed marker's own slot set,
/// read before this event refreshed it, is the runtime answer. On a lane's first firing
/// nobody cleans up; slot 1 wipes and recreates the directory at the head of the next
/// chain, which is what bounds it either way.
fn cleanup_chain(chain: &SlotChain, prior: Option<&ArmedMarker>) {
    if let Some(max) = prior.and_then(|m| m.slots_seen.iter().max().copied()) {
        cleanup_if_last(chain, max);
    }
}

/// The key that groups the N hook processes of one firing.
///
/// On unix that is the parent process id, the Claude Code process itself, exactly as the
/// chain is specified. Windows exposes no parent pid to a std program, so the session uuid
/// stands in: it is shared by every slot of one firing and differs between sessions, which
/// is the whole requirement.
fn chain_key(hook: &HookInput) -> u32 {
    #[cfg(unix)]
    {
        let _ = hook;
        std::os::unix::process::parent_id()
    }
    #[cfg(not(unix))]
    {
        let mut h: u32 = 2_166_136_261;
        for b in hook.session_id.as_bytes() {
            h ^= u32::from(*b);
            h = h.wrapping_mul(16_777_619);
        }
        h
    }
}

/// The Claude Code version for the armed marker: the newest transcript record that
/// carries one, else the environment, else nothing. Never invented.
fn claude_code_version(hook: &HookInput) -> Option<String> {
    version_from_tail(Path::new(&hook.transcript_path)).or_else(|| {
        std::env::var(VERSION_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty())
    })
}

/// The `version` of the newest parseable record in the transcript's tail window.
///
/// A positional tail read, not a map: the transcript is live, and Claude Code rewrites it
/// in place. The window's first line is usually a fragment, which simply fails to parse
/// and is stepped over as the walk runs backwards.
fn version_from_tail(path: &Path) -> Option<String> {
    let (bytes, _) = crate::parse::read_tail(path, VERSION_TAIL_BYTES).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    text.lines().rev().find_map(|line| {
        crate::parse::parse_line(line.as_bytes())
            .ok()
            .flatten()
            .and_then(|rec| rec.version)
    })
}
