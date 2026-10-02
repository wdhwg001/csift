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
    long_about = "csift send queues a message for ONE Claude Code lane (a top-level session, an\n\
        unnamed subagent, a teammate or a workflow lane). The sender is another lane or any\n\
        process outside Claude Code.\n\n\
        WHY IT EXISTS\n  \
          The official channel is structurally absent for some receivers.\n    \
            - A running workflow lane cannot be reached by the send tool at all.\n    \
            - An unnamed subagent cannot reach its parent subagent.\n    \
            - Nothing reaches an idle top-level session that published no socket.\n    \
            - A sender outside Claude Code holds no tool to call.\n  \
          csift's channel is a set of files csift owns under the receiving session's sidecar\n  \
          directory. A `csift deliver` hook the USER installed carries them into the lane.\n  \
          csift never installs a hook and never writes a transcript.\n\n\
        WHAT IT WRITES\n  \
          It writes three files, all under `<session>/csift-channel/`. They are the message\n  \
          source, the receiver's inbox line and the sender's outbox line. Nothing else on disk\n  \
          is touched.\n\n\
        THE VERDICT (one per send, printed first)\n  \
          OK             queued, and a named hook point will carry it\n  \
          FULL           queued, but the message needs more slots than the receiver has\n  \
          MAY-FAIL       queued, with the risk named (hooks configured but never armed, a\n                 \
                         policy switch, a frozen lane, an unprovable gate)\n  \
          UNPREDICTABLE  queued, but csift cannot say WHEN. The receiver is not alive, is\n                 \
                         headless, or has no delivery hook at all.\n  \
          REFUSED        nothing was queued, and the reason is printed (a stopped-by-user\n                 \
                         lane, a completed workflow lane, a completed agent lane without\n                 \
                         --resume)\n\n\
        OFFICIAL SENDS ARE DELEGATED, NEVER PERFORMED\n  \
          Where an official transport exists (the teammate mailbox, the in-process queue, the\n  \
          cross-session socket, a resume), the receipt prints the exact call for YOU to make,\n  \
          because csift is a binary and those are model tools. The message is still queued on\n  \
          csift's own channel, so a delegation that is forgotten loses no message.\n  \
          `--official-only` queues nothing.\n\n\
        EXIT CODE\n  \
          The exit code is 0 for every verdict, REFUSED included. A refusal is a definitive\n  \
          answer and is never treated as a usage error. Read the verdict, since the exit code\n  \
          says nothing about it.",
    after_help = "EXAMPLES\n  \
          csift send @<agent-id> \"stop after the current file\"      # steer a running subagent\n  \
          csift send @Relay@harbor \"status?\" --mode queue           # a teammate, at its next turn boundary\n  \
          csift send @<uuid> -f note.md --ttl 2d                     # a body from a file, alive for 2 days\n  \
          echo \"ship it\" | csift send @<agent-id> --from @main       # a body on stdin, an exact sender\n  \
          csift send @<agent-id> \"continue\" --resume                 # respawn a completed lane (an ACTION)\n  \
          csift send @<uuid> \"ping\" --from ci-runner --format json   # from outside Claude Code\n\n\
        JSON (--format json)\n  \
          The envelope's `{\"kind\":\"header\"}` line, then ONE `send` row:\n    \
            {\"kind\":\"send\", id, verdict, channel, mode,\n     \
             receiver:{lane, routing_id, session, kind, state, version, configured_slots,\n               \
                       armed_slots},\n     \
             official:{delegated, tool, to}, prediction, risks:[…],\n     \
             settings:{sources:[{scope, path, read, note}], unobservable:[…]}}\n  \
          then the summary line:\n    \
            {\"kind\":\"summary\", queued, chunks, message_chars, message_utf16_units,\n     \
             chunk_budget_utf16_units, relation, cross_project, ttl_secs, official_floor_met}\n  \
          The chunk budget is counted in UTF-16 code units, the unit Claude Code's inline cap\n  \
          measures, so an emoji is one char and two units.\n\n\
        THE SETTINGS LINE\n  \
          The gate verdicts, the slot census and every hook risk above come off ONE fold of the\n  \
          receiver's settings cascade. The receipt's `settings` line therefore names\n    \
            - the scopes that contributed,\n    \
            - the scopes it tried and did not get (as `absent`),\n    \
            - any file that was there but unreadable,\n    \
            - the inputs that change the outcome and leave nothing on disk at all (a settings\n      \
              file or inline JSON handed to Claude Code on its command line, a restricted\n      \
              source set, the trust dialog, MDM).\n  \
          A verdict is checkable only when its sources are named.\n\n\
        CONFIGURED vs ARMED\n  \
          `configured_slots` counts the `csift deliver` slot hook entries installed in the\n  \
          receiver's settings cascade. `armed_slots` counts the slots that have actually RUN in\n  \
          that lane. Configuration is not arming, because a receiver process can predate the\n  \
          settings edit. A message queued for a lane with no armed slot is queued honestly and\n  \
          not delivered.\n\n\
        THE SENDER\n  \
          Inside Claude Code the caller is a LANE. The environment names only the top-level\n  \
          session, so without `--from @<lane>` the send is attributed to the session, and\n  \
          stderr says so. Outside Claude Code the caller is EXTERNAL, and `--from` is a free\n  \
          label for the receipt. csift never fills it from your environment, and it is never a\n  \
          username.\n\n\
        SEE ALSO\n  \
          csift msg <ID>              did it land? csift's ledger joined to the receiver's proof\n  \
          csift ack <ID>              the receiver's own record that it read one\n  \
          csift deliver --recipe      the hook block the RECEIVER's owner installs to get any\n  \
          csift whoami --to @<lane>   the same prediction, with nothing queued and nothing written\n  \
          csift status @<lane>        is that lane alive, and what is it doing right now\n  \
          csift agents @<uuid>        the lane ids to address (a teammate prints both its forms)"
)]
pub struct SendArgs {
    /// The receiver lane, in any `@`-form csift accepts (`@<uuid>` or its leading-hex
    /// prefix, `@<agent-id>`, `@<Name>@<Team>` for a teammate's routing form, `@main`,
    /// `@trap:<marker>`). It must resolve to EXACTLY one transcript.
    #[arg(verbatim_doc_comment, value_parser = parse_lane_target)]
    pub target: String,

    /// The message body. Omit it to read the body from `-f FILE` or from stdin.
    pub message: Option<String>,

    /// Read the message body from a file instead of the positional.
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    pub file: Option<PathBuf>,

    /// `steer` (the default) delivers at the next hook point of any kind. `queue` waits for
    /// a turn boundary (UserPromptSubmit, Stop, SubagentStop, or a resume or compact
    /// SessionStart).
    #[arg(verbatim_doc_comment, long, value_enum, default_value_t = SendMode::Steer)]
    pub mode: SendMode,

    /// How long the message stays deliverable, in the site-wide duration grammar
    /// (`30s` `10m` `12h` `2d` `1w` `3mo` `1y`). Default 12h.
    #[arg(verbatim_doc_comment, long, value_name = "DUR", default_value = "12h")]
    pub ttl: String,

    /// Who is sending. Inside Claude Code it is `@<lane>` (`@main`, or the `a...` id
    /// `csift agents` prints). Outside, it is a free LABEL for the receipt, never a lane and
    /// never a username (default `unknown`).
    #[arg(verbatim_doc_comment, long, value_name = "LANE|LABEL", value_parser = parse_sender)]
    pub from: Option<String>,

    /// Permit the official RESUME of a completed lane. Without it a completed teammate or
    /// unnamed subagent is REFUSED, because reaching one respawns the lane instead of
    /// delivering to it.
    #[arg(verbatim_doc_comment, long)]
    pub resume: bool,

    /// Print the official call only, and queue NOTHING on the csift channel. The flag means
    /// something only where an official transport exists. Otherwise the send has nothing to
    /// delegate.
    #[arg(verbatim_doc_comment, long = "official-only")]
    pub official_only: bool,

    /// Output format. Text is the default, and `json` emits the envelope plus one `send`
    /// row.
    #[arg(verbatim_doc_comment, long, value_enum, default_value_t)]
    pub format: OutputFormat,
}
