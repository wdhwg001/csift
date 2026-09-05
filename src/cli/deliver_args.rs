//! `DeliverArgs` - the csift-channel hook entry, plus its `--recipe` printer.

use super::*;

/// The longest delivery chain csift will name. A slot is a POSITION in one event's chain,
/// not a copy of a hook, so a three-digit slot is a typo rather than a configuration.
const MAX_SLOTS: u32 = 64;

/// Which shell form `--recipe` prints in its command entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DeliverShell {
    /// The plain command form the harness runs through its own shell.
    Bash,
    /// The Windows form, naming the PowerShell runner explicitly.
    Powershell,
}

/// A slot number: the position of this hook entry in one event's delivery chain.
fn parse_slot(s: &str) -> Result<u32, String> {
    let n: u32 = s.parse().map_err(|_| {
        format!(
            "`{s}` is not a slot number - a slot is a whole number, the position of this \
             hook entry in the delivery chain (`csift deliver --slot 1`, `--slot 2`, ...)"
        )
    })?;
    if (1..=MAX_SLOTS).contains(&n) {
        Ok(n)
    } else {
        Err(format!(
            "slot {n} is out of range - a delivery chain runs from 1 to {MAX_SLOTS}"
        ))
    }
}

/// `csift deliver`: the hook entry of the csift channel.
#[derive(Args, Debug)]
#[command(
    about = "Hook entry: called by Claude Code through the installed hook, not by hand \
             (except --recipe)",
    long_about = "Deliver whatever the csift channel is holding for THIS lane, at the hook \
        point Claude Code just fired. stdin is the hook input JSON; stdout is either one \
        hook-output object or nothing at all.\n\n\
        You do not run this command yourself. It is the line you paste into settings.json, \
        once per slot, on each delivery event - `csift deliver --recipe` prints that block \
        ready to paste. csift never writes a settings file: arming the channel is your \
        edit.\n\n\
        WHAT ONE INVOCATION DOES\n  \
        Reads the hook payload, refuses a shape it cannot place (an empty \
        `transcript_path` or a `session_id` that is not a uuid - the served/remote call \
        shape), refreshes this lane's armed marker so a sender can see that delivery hooks \
        really run here, and then, on one of the eight delivery events, emits ONE chunk of \
        what is waiting.\n\n\
        SLOTS\n  \
        The N pasted lines are one CHAIN per event: slot k emits chunk k of the \
        concatenated chunk list of every message pending for the lane, so N slots carry N \
        chunks at that event and a message longer than one chunk needs more than one slot \
        to arrive whole. Several small messages simply take a slot each, every one with \
        its own full envelope header. Slot k waits briefly for slot k-1 so the parts \
        arrive in order; a slot that gives up waiting still emits, with a one-line warning \
        that the order may be disturbed.\n\n\
        MODES AND VEHICLES\n  \
        A `steer` message rides any of the eight events (SessionStart, SubagentStart, \
        PreToolUse, PostToolUse, PostToolBatch, UserPromptSubmit, Stop, SubagentStop). A \
        `queue` message rides only a turn boundary: UserPromptSubmit, Stop, SubagentStop, \
        and the two SessionStart re-entry sources (`resume`, `compact`). The vehicle is the \
        hook's `additionalContext` at exit 0 everywhere; on Stop and SubagentStop a queue \
        message may instead go out on stderr with exit 2, which also blocks the turn from \
        ending, but only while the lane has headroom under \
        CLAUDE_CODE_STOP_HOOK_BLOCK_CAP (8 by default, non-positive disables the vehicle) \
        and never twice for one message.\n\n\
        RE-ENTRY\n  \
        SessionStart with `source=compact` offers an un-acked message once more, because \
        the compaction threw away the context the first delivery landed in; \
        `source=resume` and SubagentStart are where a message that had no lane to land in \
        finally lands.\n\n\
        WHAT IT WRITES\n  \
        Only the csift-owned sidecar directory `<session>/csift-channel/`: the per-lane \
        ledger (what was emitted, held, expired or redelivered) and the per-lane armed \
        marker. Never a transcript, never a settings file, never anything the harness owns.",
    after_help = "EXAMPLES\n  \
          csift deliver --recipe                      # the settings block to paste (4 slots, bash)\n  \
          csift deliver --recipe --slots 2            # a shorter chain\n  \
          csift deliver --recipe --shell powershell   # the Windows command form\n  \
          csift deliver --recipe > hooks.json         # the fragment alone; the note rides stderr\n\n\
        THE LINE ITSELF\n  \
          `csift deliver --slot 1` (and `--slot 2`, ... up to your chain length) as a\n  \
        `type: command` hook entry on each delivery event. It reads the hook payload on\n  \
        stdin, so running it by hand just makes it wait for a payload that never comes.\n\n\
        OUTPUT\n  \
          Exactly one JSON object on stdout when there is something to deliver:\n  \
          {\"hookSpecificOutput\":{\"hookEventName\":\"<event>\",\"additionalContext\":\"<chunk>\"}}\n  \
        and nothing at all otherwise. Diagnostics never go to stdout. The exit-2 vehicle\n  \
        prints its chunk on stderr instead and exits 2, which is how a Stop hook blocks a\n  \
        turn from ending; every other outcome exits 0.\n\n\
        SEEING WHAT HAPPENED\n  \
          The ledger is intent, the receiver's own transcript is fact:\n  \
          csift search '<message id>' @<lane> --additional-context\n\n\
        SEE ALSO\n  \
          csift send @<lane> \"…\"      queue a message this hook will carry\n  \
          csift msg <ID>              intent joined to fact, one verdict, no guessing\n  \
          csift ack <ID>              the receiving lane's own record that it read one\n  \
          csift whoami --to @<lane>   whether a lane's hooks are configured AND armed"
)]
pub struct DeliverArgs {
    /// This hook entry's position in the event's delivery chain (1-based). Slot k emits
    /// chunk k of everything pending for the lane, so the N pasted lines carry N chunks
    /// at that event. Required unless `--recipe`.
    #[arg(
        long = "slot",
        value_name = "K",
        value_parser = parse_slot,
        required_unless_present = "recipe"
    )]
    pub slot: Option<u32>,

    /// Print the settings.json `hooks` block that arms the channel (stdout), plus a
    /// two-line note about who installs it (stderr), and exit. Writes nothing.
    #[arg(long = "recipe")]
    pub recipe: bool,

    /// How many slots the printed chain has (`--recipe` only; default 4). More slots =
    /// more chunks delivered per event.
    #[arg(long = "slots", value_name = "N", value_parser = parse_slot, default_value_t = 4)]
    pub slots: u32,

    /// Which shell form the printed command entries use (`--recipe` only): `bash` (the
    /// default) or `powershell`, the Windows form.
    #[arg(long = "shell", value_name = "SHELL", value_enum, default_value_t = DeliverShell::Bash)]
    pub shell: DeliverShell,
}
