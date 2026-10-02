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
    about = "Hook entry Claude Code calls through the installed hook. By hand, run only --recipe",
    long_about = "Delivers whatever the csift channel holds for THIS lane, at the hook point Claude\n\
        Code just fired. A lane is one running Claude Code conversation, the top-level session\n\
        or one subagent, each with its own transcript. stdin is the hook input JSON. stdout is\n\
        either one hook-output object or nothing at all.\n\n\
        You do not run this command yourself. It is the line you paste into settings.json,\n\
        once per slot, on each delivery event, and `csift deliver --recipe` prints that block\n\
        ready to paste. csift never writes a settings file, so arming the channel is your\n\
        edit.\n\n\
        WHAT ONE INVOCATION DOES\n  \
          1. Reads the hook payload, and refuses a shape it cannot place, an empty\n     \
             `transcript_path` or a `session_id` that is not a uuid. A hook fired for a\n     \
             served or remote caller carries exactly that shape\n     \
             (`session_id:\"served:<caller>\"` beside an empty `transcript_path`) and names\n     \
             no lane csift can find.\n  \
          2. Refreshes this lane's armed marker, so a sender can see that delivery hooks\n     \
             really run here.\n  \
          3. On one of the eight delivery events, emits ONE chunk of what is waiting.\n\n\
        SLOTS\n  \
          The N pasted lines form one CHAIN per event. Slot k emits chunk k of the\n  \
          concatenated chunk list of every message pending for the lane, so N slots carry N\n  \
          chunks at that event. A message longer than one chunk needs more than one slot to\n  \
          arrive whole. Several small messages simply take a slot each, and each carries its\n  \
          own full envelope header. Slot k waits briefly for slot k-1 so the parts arrive in\n  \
          order. A slot that gives up waiting still emits, with a one-line warning that the\n  \
          order may be disturbed.\n\n\
        MODES AND VEHICLES\n  \
          A `steer` message rides any of the eight events (SessionStart, SubagentStart,\n  \
          PreToolUse, PostToolUse, PostToolBatch, UserPromptSubmit, Stop, SubagentStop). A\n  \
          `queue` message rides only a turn boundary (UserPromptSubmit, Stop, SubagentStop,\n  \
          and the two SessionStart re-entry sources `resume` and `compact`).\n\n  \
          The vehicle is the hook's `additionalContext` at exit 0 everywhere. On Stop and\n  \
          SubagentStop a queue message may instead go out on stderr with exit 2, which also\n  \
          blocks the turn from ending. That happens only while the lane has headroom under\n  \
          CLAUDE_CODE_STOP_HOOK_BLOCK_CAP (8 by default, and a non-positive value disables\n  \
          the vehicle), and never twice for one message.\n\n\
        RE-ENTRY\n  \
          SessionStart with `source=compact` offers an un-acked message once more, because\n  \
          the compaction threw away the context the first delivery landed in. A message\n  \
          queued while its lane was not running is delivered when the lane runs again, at\n  \
          SessionStart with `source=resume` for a session or, for a steer message, at\n  \
          SubagentStart for a subagent.\n\n\
        WHAT IT WRITES\n  \
          It writes only csift's own `csift-channel/` directory inside the session's sidecar\n  \
          directory (the `<session-id>/` folder beside its transcript). That directory holds\n  \
          the per-lane ledger (what was emitted, held, expired or redelivered) and the\n  \
          per-lane armed marker. It never writes a transcript, a settings file, or anything\n  \
          the harness owns.",
    after_help = "EXAMPLES\n  \
          csift deliver --recipe                      # the settings block to paste (4 slots, bash)\n  \
          csift deliver --recipe --slots 2            # a shorter chain\n  \
          csift deliver --recipe --shell powershell   # the Windows command form\n  \
          csift deliver --recipe > hooks.json         # the fragment alone (the note goes to stderr)\n\n\
        THE LINE ITSELF\n  \
          `csift deliver --slot 1` (and `--slot 2`, ... up to your chain length) goes in as a\n  \
          `type: command` hook entry on each delivery event. It reads the hook payload on\n  \
          stdin, so running it by hand just makes it wait for a payload that never comes.\n\n\
        OUTPUT\n  \
          Exactly one JSON object goes to stdout when there is something to deliver.\n    \
            {\"hookSpecificOutput\":{\"hookEventName\":\"<event>\",\"additionalContext\":\"<chunk>\"}}\n  \
          Otherwise nothing is printed. Diagnostics never go to stdout. The exit-2 vehicle\n  \
          prints its chunk on stderr instead and exits 2, which is how a Stop hook blocks a\n  \
          turn from ending. Every other outcome exits 0.\n\n\
        SEEING WHAT HAPPENED\n  \
          The ledger records what csift emitted. The receiver's own transcript holds what\n  \
          actually arrived, and this search finds it.\n    \
            csift search '<message id>' @<lane> -t harness.hook.context\n\n\
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
        verbatim_doc_comment,
        long = "slot",
        value_name = "K",
        value_parser = parse_slot,
        required_unless_present = "recipe"
    )]
    pub slot: Option<u32>,

    /// Print the settings.json fragment that arms the channel, and exit. The fragment is a
    /// top-level `hooks` object to merge with the file's own, printed on stdout. A two-line
    /// note on stderr says who installs it and how. Nothing is written.
    #[arg(verbatim_doc_comment, long = "recipe")]
    pub recipe: bool,

    /// How many slots the printed chain has (`--recipe` only, default 4). More slots
    /// deliver more chunks per event.
    #[arg(verbatim_doc_comment, long = "slots", value_name = "N", value_parser = parse_slot, default_value_t = 4)]
    pub slots: u32,

    /// Which shell form the printed command entries use (`--recipe` only). `bash` is the
    /// default, and `powershell` is the Windows form.
    #[arg(verbatim_doc_comment, long = "shell", value_name = "SHELL", value_enum, default_value_t = DeliverShell::Bash)]
    pub shell: DeliverShell,
}
