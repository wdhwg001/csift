//! MsgArgs + AckArgs - the csift channel's reconciliation pair.

use super::*;

/// `csift msg`: reconcile the channel ledger against the receiver's own transcript.
#[derive(Args, Debug)]
#[command(
    about = "Reconcile a channel message's ledger INTENT against the receiver transcript's FACT",
    long_about = "Answers \"did that message actually reach the lane?\" A lane is one running Claude\n\
        Code conversation, the top-level session or one subagent, each with its own transcript.\n\
        The csift channel keeps two independent records, and `msg` joins them.\n  \
          INTENT   the per-lane ledger, `ledger/<lane>.jsonl` in the receiver session's\n           \
                   csift-channel directory. An `emit` line says csift printed a chunk at a hook\n           \
                   point. That is csift's own claim, nothing more.\n  \
          FACT     the receiver lane's TRANSCRIPT. A delivered chunk lands there as a\n           \
                   `hook_additional_context` attachment whose content opens with the envelope\n           \
                   header, so a record carrying `id=<ID>` is proof the text reached the lane's\n           \
                   context.\n\n\
        A ledger with no matching record is INTENT-ONLY (the hook printed into a process that\n\
        crashed, or the write has not flushed yet), and never a delivery. The fact half is what\n\
        `csift search '<ID>' @<lane> -t harness.hook.context` finds, and `msg` runs that search\n\
        and adds the ledger side (THE FACT HALF ON ITS OWN, below, says what the `-t` changes).\n\n\
        VERDICTS (a closed set)\n  \
          DELIVERED     at least one emit AND a transcript record carrying the id\n  \
          INTENT-ONLY   emitted, no transcript record (a crash window, or a flush lag)\n  \
          QUEUED        enqueued in the lane's inbox, nothing emitted yet\n  \
          HELD          a `held` line with no later emit (the reason is printed)\n  \
          EXPIRED       an `expired` ledger line, or the ttl elapsed with nothing emitted\n  \
          ACKED         the receiver appended an `ack` line (the fact is still reported)\n  \
          REFUSED       csift declined to act on the message (the reason is printed)\n\n\
        WITHOUT AN ID\n  \
          `msg` prints the lane's whole ledger view, newest first, one row per message\n  \
          addressed to that lane. `--held`, `--sent` and `--pending` filter it.\n\n\
        WHICH LANE\n  \
          `--lane @<lane>` names it. Otherwise the calling Claude Code session is used, read\n  \
          from $CLAUDE_CODE_SESSION_ID. That variable names the TOP-LEVEL session in every\n  \
          lane, so inside a subagent pass `--lane @<your agent id>`. Outside Claude Code there\n  \
          is no caller lane, and `--lane` is required.",
    after_help = "EXAMPLES\n  \
          csift msg 0123456789abcdef                    # one message: intent joined to fact\n  \
          csift msg 0123456789abcdef --lane @<agent-id> # ...addressed at a subagent lane\n  \
          csift msg                                     # this lane's ledger, newest first\n  \
          csift msg --lane @<uuid> --pending            # ...only what is still waiting\n  \
          csift msg --lane @<uuid> --held               # ...only what is held, with reasons\n  \
          csift msg <ID> --format json | jq 'select(.kind==\"msg\").fact'   # the proof, or null\n\n\
        JSON SCHEMA (--format json)\n  \
          The envelope is a header, then the `msg` rows, then a summary.\n    \
            header    {kind:\"header\", command:\"msg\", lane, session}\n    \
            row       {kind:\"msg\", id, verdict, mode, lane, session,\n               \
                       emits:[{event, slot, part, parts, vehicle, ts_utc, ts_local}], held,\n               \
                       held_reasons, expired, acked, refused_reasons, from, relation,\n               \
                       enqueued_utc, enqueued_local, expires_utc, expires_local,\n               \
                       fact:{line, uuid}|null}\n    \
            summary   {kind:\"summary\", messages, skipped_lines}\n  \
          `fact` is null for every verdict but DELIVERED and an acked delivery.\n\n\
        THE FACT HALF ON ITS OWN\n  \
          `csift search '<ID>' @<lane> -t harness.hook.context` is the transcript side of the\n  \
          join. `msg` reads the same records, the hook-context attachments whose text opens\n  \
          with the envelope header naming the id. Without `-t`, `csift search '<ID>' @<lane>`\n  \
          finds those deliveries too, labelled `agent.communication.channel` and rendered\n  \
          verbatim from their envelope, along with any other record that quotes the id (a\n  \
          person pasting it, say). The `-t harness.hook.context` keeps only the delivery\n  \
          records. A delivery is one of the THREE attachment leaves a default scan reaches,\n  \
          beside the person's mid-turn steer and the model's structured answer.\n\n\
        SEE ALSO\n  \
          csift ack <ID>              record that this lane read the message\n  \
          csift send @<lane> \"…\"      queue one\n  \
          csift deliver --recipe      the hook block a receiver needs to get any at all\n  \
          csift status @<lane>        is the lane even alive to receive\n  \
          csift whoami                which lane you are, when the lane to pass is unclear"
)]
pub struct MsgArgs {
    /// The 16-lowercase-hex message id csift printed when the message was sent. With no
    /// id, the whole lane ledger is listed instead.
    #[arg(verbatim_doc_comment, value_name = "ID")]
    pub id: Option<String>,

    /// The lane to read, as an `@`-target (`@<uuid>`, `@<agent-id>`, `@<Name>@<Team>`,
    /// `@main`, `@trap:<marker>`). The leading `@` may be omitted. Without this flag the
    /// calling Claude Code session is used. Outside Claude Code the flag is required.
    #[arg(verbatim_doc_comment, long = "lane", value_name = "@LANE", value_parser = parse_lane_target)]
    pub lane: Option<String>,

    /// List only messages HELD (a hold with no later emit), with the recorded reason.
    #[arg(long = "held", conflicts_with_all = ["sent", "pending"])]
    pub held: bool,

    /// List only messages csift has emitted at least one chunk of (DELIVERED, INTENT-ONLY
    /// or ACKED).
    #[arg(verbatim_doc_comment, long = "sent", conflicts_with_all = ["held", "pending"])]
    pub sent: bool,

    /// List only messages still waiting, meaning enqueued with nothing emitted, not
    /// expired and not acked.
    #[arg(verbatim_doc_comment, long = "pending", conflicts_with_all = ["held", "sent"])]
    pub pending: bool,

    /// Emit JSON instead of the rendered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

/// `csift ack`: record that this lane read a message.
#[derive(Args, Debug)]
#[command(
    about = "Record that the calling lane READ a channel message (appends an `ack` ledger line)",
    long_about = "Appends an `ack` line to the calling lane's channel ledger. A lane is one running\n\
        Claude Code conversation, the top-level session or one subagent, each with its own\n\
        transcript. A later `csift msg <ID>` then reads ACKED. The delivery hook offers an\n\
        un-acked message once more after each compaction (at SessionStart with\n\
        `source=compact`), and stops doing so once the message is acked.\n\n\
        csift can see that a chunk was emitted and that a record carrying the id exists. Only\n\
        the model that read the message can say it acted on it, so the ack is the receiver's\n\
        to write.\n\n\
        THE CALLER MUST BE A CLAUDE CODE LANE\n  \
          An ack is a receiver's claim about its own context. A process outside Claude Code\n  \
          (no $CLAUDE_CODE_SESSION_ID) cannot make it, and csift refuses it instead of writing\n  \
          a line nobody can attribute. Inside Claude Code the environment names the TOP-LEVEL\n  \
          session in every lane, so a subagent acking its own deliveries passes\n  \
          `--lane @<its agent id>`.\n\n\
        THE ID MUST BE KNOWN TO THAT LANE\n  \
          The lane must hold an inbox line or a ledger line for the id. Acking an id the lane\n  \
          never received would write a record that joins to nothing, so it is a hard error,\n  \
          and the error names what the lane does hold.\n\n\
        WHAT IT WRITES\n  \
          csift writes only its own `csift-channel/` directory inside the session's sidecar\n  \
          directory, the `<session-id>/` folder beside its transcript. It never writes a\n  \
          transcript, the team mailbox or a settings file.",
    after_help = "EXAMPLES\n  \
          csift ack 0123456789abcdef                     # this session's own lane read it\n  \
          csift ack 0123456789abcdef --lane @<agent-id>  # ...a subagent acking its own\n  \
          csift ack 0123456789abcdef --format json       # the machine receipt\n\n\
        JSON (--format json)\n    \
            header    {\"kind\":\"header\", command:\"ack\", lane, session}\n    \
            row       {\"kind\":\"ack\", id, lane, session, ts_utc, ts_local, already_acked}\n    \
            summary   {\"kind\":\"summary\", acked}\n  \
          The stream is the envelope's header line, ONE `ack` row and the summary line.\n  \
          `already_acked` is true when the lane had acked this id before. The new line is\n  \
          still appended, so the ledger keeps every claim.\n\n\
        SEE ALSO\n  \
          csift msg <ID>              the verdict this ack turns into ACKED\n  \
          csift send @<lane> \"…\"      queue a message for another lane\n  \
          csift deliver --recipe      the hook block that delivers them\n  \
          csift whoami                which lane you are, when you are unsure what to pass"
)]
pub struct AckArgs {
    /// The 16-lowercase-hex message id to acknowledge.
    #[arg(value_name = "ID")]
    pub id: String,

    /// The acking lane, as an `@`-target (the leading `@` may be omitted). Without this
    /// flag the calling Claude Code session's top-level lane is used.
    #[arg(verbatim_doc_comment, long = "lane", value_name = "@LANE", value_parser = parse_lane_target)]
    pub lane: Option<String>,

    /// Emit JSON instead of the rendered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}
