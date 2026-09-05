//! `csift send`: queue a message for one Claude Code lane on the csift channel.

use super::*;

/// Delivery mode. `queue` is a SUBSET of `steer`: a steer message may ride any eligible hook
/// point, a queue message only a turn-boundary one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum SendMode {
    /// Deliver at the next available hook point of any kind (the default).
    #[default]
    Steer,
    /// Deliver only at a turn boundary.
    Queue,
}

impl SendMode {
    /// The wire spelling, which is also what the channel files record.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SendMode::Steer => "steer",
            SendMode::Queue => "queue",
        }
    }
}

#[derive(Args, Debug)]
#[command(
    about = "Queue a message for one Claude Code lane, and say what will carry it",
    long_about = "csift send - queue a message for ONE Claude Code lane (a top-level session, \
        an unnamed subagent, a teammate, a workflow lane), from another lane or from any \
        process outside Claude Code.\n\n\
        WHY IT EXISTS. The official channel is structurally absent for some receivers: a \
        running workflow lane cannot be reached by the send tool at all, an unnamed subagent \
        cannot reach its parent subagent, nothing reaches an idle top-level session that \
        published no socket, and a sender outside Claude Code holds no tool to call. csift's \
        channel is a set of files csift owns under the receiving session's sidecar directory; \
        a `csift deliver` hook the USER installed carries them into the lane. csift never \
        installs a hook and never writes a transcript.\n\n\
        WHAT IT WRITES. Three files, all under `<session>/csift-channel/`: the message source, \
        the receiver's inbox line, and the sender's outbox line. Nothing else on disk is \
        touched.\n\n\
        THE VERDICT (one per send, printed first):\n  \
          OK             queued, and a named hook point will carry it\n  \
          FULL           queued, but the message needs more slots than the receiver has\n  \
          MAY-FAIL       queued, with the risk named (hooks configured but never armed, a \
        policy switch, a frozen lane, an unprovable gate)\n  \
          UNPREDICTABLE  queued, but csift cannot say WHEN: the receiver is not alive, is \
        headless, or has no delivery hook at all\n  \
          REFUSED        nothing was queued, and why (a stopped-by-user lane, a completed \
        workflow lane, a completed agent lane without --resume)\n\n\
        OFFICIAL SENDS ARE DELEGATED, NEVER PERFORMED. Where an official transport exists \
        (the teammate mailbox, the in-process queue, the cross-session socket, a resume), the \
        receipt prints the exact call for YOU to make - csift is a binary and those are model \
        tools - and still queues the message on its own channel, so a delegation that is \
        forgotten is not a lost message. `--official-only` queues nothing.\n\n\
        Exit code is 0 for every verdict, REFUSED included: a refusal is a definitive answer, \
        not a usage error. Read the verdict, not the exit code.",
    after_help = "EXAMPLES\n  \
          csift send @<agent-id> \"stop after the current file\"      # steer a running subagent\n  \
          csift send @Relay@harbor \"status?\" --mode queue           # a teammate, at its next turn boundary\n  \
          csift send @<uuid> -f note.md --ttl 2d                     # a body from a file, alive for two days\n  \
          echo \"ship it\" | csift send @<agent-id> --from @main       # a body on stdin, an exact sender\n  \
          csift send @<agent-id> \"continue\" --resume                 # respawn a completed lane (an ACTION)\n  \
          csift send @<uuid> \"ping\" --from ci-runner --format json   # from outside Claude Code\n\n\
        JSON (--format json): the envelope's `{\"kind\":\"header\"}` line, ONE \
        `{\"kind\":\"send\", id, verdict, channel, mode, receiver:{lane, routing_id, session, \
        kind, state, version, configured_slots, armed_slots}, official:{delegated, tool, to}, \
        prediction, risks:[…], settings:{sources:[{scope, path, read, note}], \
        unobservable:[…]}}` row, and the `{\"kind\":\"summary\", queued, chunks, \
        message_chars, relation, cross_project, ttl_secs, official_floor_met}` line.\n\n\
        THE SETTINGS LINE. The gate verdicts, the slot census and every hook risk above come \
        off ONE fold of the receiver's settings cascade, so the receipt's `settings` line \
        names the scopes that contributed, the scopes it tried and did not get (as `absent`), \
        any file that was there but unreadable, and the inputs that change the outcome and \
        leave nothing on disk at all (a settings file or inline JSON handed to Claude Code on \
        its command line, a restricted source set, the trust dialog, MDM) - a verdict is \
        checkable only when its sources are named.\n\n\
        CONFIGURED vs ARMED. `configured_slots` counts the `csift deliver` slot hook entries \
        installed in the receiver's settings cascade; `armed_slots` counts slots that have actually RUN in \
        that lane. Configuration is not arming: a receiver process can predate the settings \
        edit. A message queued for a lane with no armed slot is queued honestly, not \
        delivered.\n\n\
        THE SENDER. Inside Claude Code the caller is a LANE; the environment names only the \
        top-level session, so without `--from @<lane>` the send is attributed to the session \
        and says so on stderr. Outside Claude Code the caller is EXTERNAL and `--from` is a \
        free label for the receipt - csift never fills it from your environment, and it is \
        never a username.\n\n\
        SEE ALSO\n  \
          csift msg <ID>              did it land? csift's ledger joined to the receiver's proof\n  \
          csift ack <ID>              the receiver's own record that it read one\n  \
          csift deliver --recipe      the hook block the RECEIVER's owner installs to get any\n  \
          csift whoami --to @<lane>   the same prediction, with nothing queued and nothing written\n  \
          csift status @<lane>        is that lane alive, and what is it doing right now\n  \
          csift agents @<uuid>        the lane ids to address (a teammate prints both its forms)"
)]
pub struct SendArgs {
    /// The receiver lane, in any `@`-form csift accepts: `@<uuid>` (or its leading-hex
    /// prefix), `@<agent-id>`, `@<Name>@<Team>` (a teammate's routing form), `@main`,
    /// `@trap:<marker>`. It must resolve to EXACTLY one transcript.
    pub target: String,

    /// The message body. Omit it to read the body from `-f FILE` or from stdin.
    pub message: Option<String>,

    /// Read the message body from a file instead of the positional.
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    pub file: Option<PathBuf>,

    /// `steer` (default) delivers at the next hook point of any kind; `queue` waits for a
    /// turn boundary (UserPromptSubmit / Stop / SubagentStop / a resume or compact
    /// SessionStart).
    #[arg(long, value_enum, default_value_t = SendMode::Steer)]
    pub mode: SendMode,

    /// How long the message stays deliverable, in the site-wide duration grammar
    /// (`30s` `10m` `12h` `2d` `1w` `3mo` `1y`). Default 12h.
    #[arg(long, value_name = "DUR", default_value = "12h")]
    pub ttl: String,

    /// Who is sending. Inside Claude Code: `@<lane>` (`@main`, or the `a...` id `csift
    /// agents` prints). Outside: a free LABEL for the receipt, never a lane and never a
    /// username (default `unknown`).
    #[arg(long, value_name = "LANE|LABEL")]
    pub from: Option<String>,

    /// Permit the official RESUME of a completed lane. Without it a completed teammate or
    /// unnamed subagent is REFUSED, because reaching one respawns the lane instead of
    /// delivering to it.
    #[arg(long)]
    pub resume: bool,

    /// Print the official call only, and queue NOTHING on the csift channel. Meaningful only
    /// where an official transport exists; otherwise the send has nothing to delegate.
    #[arg(long = "official-only")]
    pub official_only: bool,

    /// Output format (text default; `json` emits the envelope + one `send` row).
    #[arg(long, value_enum, default_value_t)]
    pub format: OutputFormat,
}
