//! MsgArgs + AckArgs - the csift channel's reconciliation pair.

use super::*;

/// `csift msg`: reconcile the channel ledger against the receiver's own transcript.
#[derive(Args, Debug)]
#[command(
    about = "Reconcile a csift-channel message: what the ledger INTENDED against what the \
             receiver's transcript PROVES",
    long_about = "Answer \"did that message actually reach the lane?\" The csift channel keeps \
        two independent records and `msg` joins them:\n\n  \
          INTENT   the per-lane ledger (`ledger/<lane>.jsonl` in the receiver session's \
        csift-channel directory). An `emit` line says csift printed a chunk at a hook point. \
        That is csift's own claim, nothing more.\n  \
          FACT     the receiver lane's TRANSCRIPT. A delivered chunk lands there as a \
        `hook_additional_context` attachment whose content opens with the envelope header, so \
        a record carrying `id=<ID>` is proof the text reached the lane's context.\n\n\
        Keeping the two apart is the whole point: a ledger with no matching record is \
        INTENT-ONLY (the hook printed into a process that crashed, or the write has not \
        flushed yet), never a delivery. The fact half is exactly \
        `csift search '<ID>' @<lane> --additional-context`; `msg` runs that join for you and \
        adds the ledger side.\n\n\
        VERDICTS (a closed set)\n  \
          DELIVERED     at least one emit AND a transcript record carrying the id\n  \
          INTENT-ONLY   emitted, no transcript record (a crash window, or a flush lag)\n  \
          QUEUED        enqueued in the lane's inbox, nothing emitted yet\n  \
          HELD          a `held` line with no later emit (the reason is printed)\n  \
          EXPIRED       an `expired` ledger line, or the ttl elapsed with nothing emitted\n  \
          ACKED         the receiver appended an `ack` line (the fact is still reported)\n  \
          REFUSED       csift declined to act on the message (the reason is printed)\n\n\
        WITHOUT AN ID this prints the lane's whole ledger view, newest first, one row per \
        message addressed to that lane, filtered by `--held` / `--sent` / `--pending`.\n\n\
        WHICH LANE: `--lane @<lane>` names it. Otherwise the calling Claude Code session is \
        used ($CLAUDE_CODE_SESSION_ID, which names the TOP-LEVEL session in every lane - so \
        inside a subagent pass `--lane @<your agent id>`). Outside Claude Code there is no \
        caller lane and `--lane` is required.",
    after_help = "EXAMPLES\n  \
          csift msg 0123456789abcdef                    # one message: intent joined to fact\n  \
          csift msg 0123456789abcdef --lane @<agent-id> # ...addressed at a subagent lane\n  \
          csift msg                                     # this lane's ledger, newest first\n  \
          csift msg --lane @<uuid> --pending            # ...only what is still waiting\n  \
          csift msg --lane @<uuid> --held               # ...only what is held, with reasons\n  \
          csift msg <ID> --format json | jq .fact       # the transcript proof, or null\n\n\
        JSON SCHEMA (per --format json)\n  \
          Envelope: header -> `msg` rows -> summary. Header: {kind:\"header\", command:\"msg\", \
        lane, session}. Rows: {kind:\"msg\", id, verdict, mode, lane, session, \
        emits:[{event, slot, part, parts, vehicle, ts_utc, ts_local}], held, held_reasons, \
        expired, acked, refused_reasons, from, relation, enqueued_utc, enqueued_local, \
        expires_utc, expires_local, fact:{line, uuid}|null}. `fact` is null for every \
        verdict but DELIVERED and an acked delivery. Summary: {kind:\"summary\", messages, \
        skipped_lines}."
)]
pub struct MsgArgs {
    /// The 16-lowercase-hex message id csift printed when the message was sent. With no
    /// id, the whole lane ledger is listed instead.
    #[arg(value_name = "ID")]
    pub id: Option<String>,

    /// The lane to read, as an `@`-target (`@<uuid>`, `@<agent-id>`, `@<Name>@<Team>`,
    /// `@main`, `@trap:<marker>`). The leading `@` may be omitted. Without it, the calling
    /// Claude Code session is used; outside Claude Code this flag is required.
    #[arg(long = "lane", value_name = "@LANE")]
    pub lane: Option<String>,

    /// List only messages HELD (a hold with no later emit), with the recorded reason.
    #[arg(long = "held", conflicts_with_all = ["sent", "pending"])]
    pub held: bool,

    /// List only messages csift has emitted at least one chunk of (DELIVERED, INTENT-ONLY
    /// or ACKED).
    #[arg(long = "sent", conflicts_with_all = ["held", "pending"])]
    pub sent: bool,

    /// List only messages still waiting: enqueued, nothing emitted, not expired, not acked.
    #[arg(long = "pending", conflicts_with_all = ["held", "sent"])]
    pub pending: bool,

    /// Emit JSON instead of the rendered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

/// `csift ack`: record that this lane read a message.
#[derive(Args, Debug)]
#[command(
    about = "Record that the calling lane READ a channel message (appends an `ack` ledger line)",
    long_about = "Append an `ack` line to the calling lane's channel ledger, so a later \
        `csift msg <ID>` reads ACKED and a delivery hook stops re-offering the message after a \
        compaction. This is the ONE statement only the receiver can make: csift can see that a \
        chunk was emitted and that a record carrying the id exists, but only the model that \
        read it can say it acted on it.\n\n\
        THE CALLER MUST BE A CLAUDE CODE LANE. An ack is a receiver's claim about its own \
        context, so a process outside Claude Code (no $CLAUDE_CODE_SESSION_ID) cannot make it \
        and csift refuses rather than writing a line nobody can attribute. Inside Claude Code, \
        the environment names the TOP-LEVEL session in every lane, so a subagent acking its own \
        deliveries passes `--lane @<its agent id>`.\n\n\
        The id must already be known to that lane (an inbox line or a ledger line): acking an \
        id the lane never received would write a record that joins to nothing, so it is a hard \
        error naming what the lane does hold.\n\n\
        csift writes only its own sidecar directory `<session>/csift-channel/`: never a \
        transcript, never the team mailbox, never a settings file."
)]
pub struct AckArgs {
    /// The 16-lowercase-hex message id to acknowledge.
    #[arg(value_name = "ID")]
    pub id: String,

    /// The acking lane, as an `@`-target (the leading `@` may be omitted). Without it the
    /// calling Claude Code session's top-level lane is used.
    #[arg(long = "lane", value_name = "@LANE")]
    pub lane: Option<String>,

    /// Emit JSON instead of the rendered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}
