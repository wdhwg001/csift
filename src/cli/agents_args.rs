//! AgentsArgs + the agent/count/files enums (shape, order-by, CountAxis).

use super::*;

/// Which subagent kinds to surface in `agents`. Mirrors the on-disk discriminator
/// (path location), with `agentType` retained as a descriptive sub-label per row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AgentKindFilter {
    /// Built-in Task/Agent-tool subagents (`subagents/agent-<hex>.jsonl`)
    #[value(name = "builtin-task")]
    BuiltinTask,
    /// Workflow-agent transcripts (`workflows/wf_*/`)
    Workflow,
    /// Claude Code's persistent, directly-addressable team members
    Teammate,
}

#[derive(Debug, Args)]
#[command(
    about = "List a session's subagents with their kind, status, and start and completion times",
    long_about = "List a session's SUBAGENT lifecycle.\n\
        \n\
        Every subagent transcript the session spawned is listed with its id, KIND, start and\n\
        completion timestamps, duration and a determinable status. Three on-disk shapes are\n\
        discovered under `<session>/subagents/**`.\n  \
        • builtin-task  subagents/agent-<hex>.jsonl                    (Task/Agent tool)\n  \
        • workflow      subagents/workflows/wf_<id>/agent-<hex>.jsonl  (workflow agents)\n  \
        • teammate      subagents/agent-a<Name>-<hex>.jsonl            (in-process teammate,\n                                                                 \
        taskKind in_process_teammate)\n\
        Workflow `journal.jsonl` event logs are NOT transcripts. They are read only to corroborate\n\
        completion status, and they are never listed as agents.\n\
        \n\
        The TARGET selects the parent session. Pass `@<uuid>` for one session, or a project path or\n\
        encoded directory to cover every session under it, with each session's subagents grouped\n\
        under it.\n\
        \n\
        STATUS\n\
        A lane is one running Claude Code conversation, the top-level session or one subagent, each\n\
        with its own transcript. Start and completion come from the subagent transcript's first and\n\
        last record timestamps. For a workflow lane its RUN decides the status first, by the same\n\
        rule `csift status` applies to the lanes below a session.\n  \
        1. The agent's journal line decides when there is one. `result` reads `completed`, and\n     \
        `failed` reads `failed`.\n  \
        2. Otherwise the manifest an ended run writes decides. It reads `completed`, or the run's\n     \
        status verbatim (`killed`, `failed`, an open set).\n\
        A lane whose newest record is more than a minute newer than that manifest's own end instant\n\
        is the exception. Such a lane wrote after its run ended, most likely because a run resumed\n\
        under its id runs under its predecessor's manifest until it ends. It is read from its tail,\n\
        and its row names the stale manifest. A lane its run ended without completing carries no\n\
        completion instant, and its row names why in brackets (JSON `settled_by`), in the words\n\
        `status` uses.\n\
        \n\
        Any other lane is read off the transcript's NEWEST meaningful record, the newest assistant\n\
        message, tool call, tool result or typed user message (metadata, injected and system\n\
        records are skipped). The lane is `completed` when that record is an assistant text ending\n\
        its turn, and `running` or `unknown` otherwise. A returned tool_result at the tail is a\n\
        lane mid-turn, and it reads `running` with no completion instant.\n\
        \n\
        TIME WINDOW\n\
        `--since`/`--until` take ISO8601 or a relative `2h`/`3d`/… in the system local timezone. By\n\
        default they keep the subagents whose TRIGGER time falls in the window. The trigger time is\n\
        the parent tool_use timestamp, the true spawn instant. `--order-by start` uses the\n\
        transcript's first-record timestamp instead. `--order-by completion` uses the last one, the\n\
        lane's TERMINAL instant `last_activity_utc`. A FROZEN lane, one whose newest record is a\n\
        tool call that never got its result, therefore windows on its freeze instant and does not\n\
        vanish from a bounded window for never completing.\n\
        \n\
        TOPOLOGY\n\
        The TEXT output is ALWAYS the parent→child tree. Workflow runs are the parent nodes of\n\
        their agents, and a nested sub-subagent sits under its spawning agent, indented by depth.\n\
        JSON emits the SAME topology as FLAT kind-tagged rows, one `kind:\"agent\"` row per node in\n\
        tree pre-order. Rebuild the nesting from parent_agent_id + depth. `--agent <id>` grabs ONE\n\
        subagent with its returned message. `--returned-message` adds the returned message to every\n\
        row, read from one of three sources (the `returned_message` entry under JSON SCHEMA names\n\
        them). `--with-files` attaches each node's files-changed list.",
    after_help = "AT A GLANCE\n  \
        The TARGET selects the PARENT session whose subagents to list. `@<uuid>` (or `@main` or\n  \
        `@trap:<marker>`) names ONE session, and a project path or encoded directory covers every\n  \
        session under it, with each session's subagents grouped under it. The subagent shapes\n  \
        are discovered under `<session>/subagents/**`.\n    \
        • builtin-task  subagents/agent-<hex>.jsonl                    (Task/Agent tool)\n    \
        • workflow      subagents/workflows/wf_<id>/agent-<hex>.jsonl  (workflow agents)\n    \
        • teammate      subagents/agent-a<Name>-<hex>.jsonl            (in-process teammate,\n                                                                   \
        taskKind in_process_teammate)\n  \
        Workflow `journal.jsonl` event logs are NOT transcripts. They are read only to\n  \
        corroborate status, and never listed.\n\
        \n  \
        A workflow lane's run decides its status first. Its journal line comes first (`result` is\n  \
        completed, `failed` is failed), then its ended run's manifest status (`completed`, or the\n  \
        word verbatim, e.g. `killed`), unless the lane wrote past that manifest's end by more\n  \
        than a minute. Any other lane is `completed` when the transcript's newest meaningful\n  \
        record is an assistant text ending its turn, and `running` or `unknown` otherwise.\n\
        \n  \
        The TEXT output is ALWAYS the parent→child topology tree, with workflow runs as the\n  \
        parents of their agents and a nested sub-subagent under its spawning agent. JSON carries\n  \
        the SAME topology as flat pre-ordered rows.\n\
        \n  \
        `--agent <id>` grabs ONE subagent by its id as `csift agents` prints it (a bare hex, or a\n  \
        teammate's `a<Name>-<16 hex>`), with or without a leading `@`, and shows the full node and\n  \
        its returned message. It is a DIRECT lookup that IGNORES --since/--until/--order-by/--shape\n  \
        and renders just that node, a tree of one. A non-matching id is a hard error, and a plain\n  \
        listing shows the ids there are.\n\
        \n  \
        `--shape` here is the TRANSCRIPT-SHAPE filter (builtin-task | workflow | teammate). It is a\n  \
        DIFFERENT axis from the automation-trigger KIND a `<task-notification>` pulse carries\n  \
        (background-command / workflow / agent / monitor / task in its rendered label, where the\n  \
        `-t harness.notification.<leaf>` selector spells `agent` as `subagent`). The two axes\n  \
        overlap only on the token `workflow`.\n\
        \n\
        EXAMPLES\n  \
        One session's subagent tree:\n    \
        csift agents @<uuid>\n  \
        Every session under this project:\n    \
        csift agents .\n  \
        Only workflow-shape agents (NOT the automation kind):\n    \
        csift agents . --shape workflow\n  \
        Subagents TRIGGERED in the last 2h:\n    \
        csift agents @<uuid> --since 2h\n  \
        Subagents COMPLETED in the last 6h:\n    \
        csift agents @<uuid> --since 6h --order-by completion\n  \
        Order and window on the completion axis:\n    \
        csift agents @<uuid> --order-by completion\n  \
        Subagents COMPLETED since an ISO bound:\n    \
        csift agents @<uuid> --since 2026-06-01T09:00:00Z --order-by completion\n  \
        Discover the ids, then grab ONE subagent (a direct lookup that ignores time and kind):\n    \
        csift agents @<uuid>\n    \
        csift agents --agent <id>\n  \
        Each node with its files-changed list:\n    \
        csift agents @<uuid> --with-files\n  \
        Add the returned message to every row:\n    \
        csift agents @<uuid> --returned-message\n  \
        Machine-readable flat rows (kind: session|run|agent):\n    \
        csift agents . --format json\n\
        \n\
        JSON SCHEMA (per --format json)\n  \
        The output is FLAT kind-tagged rows in envelope v2, in this order.\n    \
        header    a leading {kind:\"header\", command, duplicate_transcripts,\n              \
        duplicate_records_dropped, divergent_transcripts, unreadable_transcripts}\n              \
        line. The four counts describe a transcript found under two project\n              \
        directories. They count the copies merged, the records the merge dropped,\n              \
        the copies kept apart because they differ, and the copies csift could not\n              \
        read.\n    \
        session   per session, one light {kind:\"session\", session_id, runs, agents} row\n    \
        run       per in-scope workflow run, one {kind:\"run\", session_id, run_id, task_id,\n              \
        workflow_name, status, agent_count, duration_ms, total_tokens,\n              \
        total_tool_calls, default_model, started_utc, started_local} row, followed\n              \
        by its member agent rows\n    \
        agent     every agent's OWN {kind:\"agent\", …} row, in tree PRE-ORDER\n    \
        summary   a closing {kind:\"summary\", sessions, runs, agents} terminator\n\
        \n  \
        The session row carries counts only. Its `agents` is EXACTLY the number of\n  \
        {kind:\"agent\"} rows that follow it, and the text tree prints that same set in that same\n  \
        order, so the two projections can never disagree. Two copies of one session whose\n  \
        contents DIFFER keep both sidecar directories in scope (the `<session-id>/` folder beside\n  \
        each copy, which holds its subagent transcripts). The agent copies inside them take the\n  \
        same merge as any transcript. Identical or prefix copies collapse to ONE row, and only an\n  \
        agent whose OWN copies differ gets a row per copy. Those two rows share an id, and the\n  \
        text output's `copies` line, printed beside the scope banner, says why.\n\
        \n  \
        A RUN row's `status` is the run MANIFEST's own `status` VERBATIM, read from\n  \
        `<session>/workflows/<run>.json`, which the runtime writes once the run ends. It is an\n  \
        OPEN set, not a csift enum. Observed values include `completed`, `killed` and `failed`.\n  \
        It is distinct from an AGENT row's `status`, which takes the run's word only for a lane\n  \
        its run ended without completing.\n\
        \n  \
        In the agent rows a parent row precedes its children. Rebuild the nesting from\n  \
        parent_agent_id + depth. There is no children[] array in JSON, and the tree renders in\n  \
        TEXT mode. The agent-row fields are {agent_id, path, shape, parent_session_id,\n  \
        parent_agent_id, spawn_tool_use_id, spawn_tool, workflow_id, agent_type, name,\n  \
        team_name, description, trigger_utc/_local, started_utc/_local, completed_utc/_local,\n  \
        last_activity_utc/_local, duration, depth, status, settled_by, pending_tool_use_id,\n  \
        pending_tool_name, pending_classification, pending_reason, pending_checker,\n  \
        pending_since_utc/_local, skipped_lines, fork_parent_last_uuid, fork_context_length}, plus\n  \
        control_hint and routing_id on a teammate row and only there.\n    \
        path              The node's OWN transcript. Two copies of one session carry two copies\n                      \
        of each agent transcript, and where those copies diverge both rows\n                      \
        survive sharing every other id. The path is what names which lane a\n                      \
        row is. The text tree prints a `transcript` line for exactly the ids\n                      \
        that occur twice.\n    \
        fork_*            The pair is non-null ONLY on a /fork child. It holds the parent's last\n                      \
        record uuid at fork time, feedable to\n                      \
        `csift show @<parent_session_id> --uuid <it>`, and the carried context\n                      \
        length.\n    \
        pending_*         STALENESS. `pending_classification: awaiting-execution` means slow OR\n                      \
        wedged OR abandoned, and jsonl cannot tell them apart. A lane pending\n                      \
        for hours or days usually belongs to a parent session that ended, with\n                      \
        nobody coming back, and is not in-flight work. `pending_since_utc`\n                      \
        against the current time gives the age to judge it by.\n                      \
        `pending_reason` says WHY the classification reads as it does. For a\n                      \
        Bash call Claude Code would stop to ask a person about (a dangerous\n                      \
        removal), it is Claude Code's own reason text. When the decision\n                      \
        depends on paths csift cannot see, it reads `removal target needs the\n                      \
        filesystem state at the time`. `pending_checker` names the parse path,\n                      \
        the Claude Code check the prediction followed and the Claude Code\n                      \
        generation it assumed (gen1 before 2.1.208, gen2 from 2.1.208 to\n                      \
        2.1.260, gen3 from 2.1.261), with `assumed` added when the record\n                      \
        carries no version (`gen2 assumed`). `pending_reason` is also null on\n                      \
        a Bash call predicted to run without asking, and both are null on a\n                      \
        pending call other than a Bash command.\n    \
        completed_*       completed_utc/_local and duration are non-null ONLY when status is\n                      \
        `completed`. A frozen or running lane is NOT done. Its tail instant\n                      \
        lives in last_activity_utc/_local, which every timestamped lane\n                      \
        carries, and on a frozen lane it equals pending_since_utc.\n    \
        files_changed     added by `--with-files`\n    \
        returned_message  `--returned-message`, implied by a single `--agent`, adds\n                      \
        `returned_message` + `returned_message_source`. It is the NEWEST\n                      \
        message the child EVER returned, and the source names where it was\n                      \
        read, one of `sync-tool-result` (the parent's tool result for a synchronous\n                      \
        spawn), `async-child-tail` (the child transcript's last assistant\n                      \
        text, for a background launch) or `workflow-journal` (the result\n                      \
        event of the workflow journal). On a frozen or running lane it\n                      \
        PREDATES the pending call, so a lane can carry BOTH a\n                      \
        returned_message and pending_*. The returned message describes an\n                      \
        earlier return, and the pending_* fields describe the lane's current\n                      \
        state. The TEXT render brands a non-completed\n                      \
        lane's message inline\n                      \
        (`history — predates the still-open lane, NOT the outcome`), so a\n                      \
        clean-finale-sounding tail cannot pass as the ending.\n                      \
        SEMANTICS. It answers \"what did the PARENT SESSION record as the\n                      \
        return\", not \"what did the agent conclude\". A `sync-tool-result`\n                      \
        source faithfully reports the parent's tool_result even when it is a\n                      \
        terse sign-off (`Done.`, `Complete.`) plus the harness's appended\n                      \
        continuation footer (`agentId: <id> (use SendMessage with to: '<id>',\n                      \
        summary: '<5-10 word recap>' to continue this agent)`). The harness\n                      \
        appends and never truncates. The child's own final words are always\n                      \
        `csift show @<agent-id> --turn -1..`.\n    \
        agent_type        The semantic agent ROLE string (e.g. `Explore`,\n                      \
        `oh-my-claudecode:critic`). It is DISTINCT from `shape`, the on-disk\n                      \
        transcript shape (builtin-task | workflow | teammate). `kind` is the\n                      \
        envelope discriminator exclusively.\n    \
        agent_id          ID-DOMAIN. `agent_id` IS this transcript's own id, the SAME concept\n                      \
        other commands call `session_id`. Re-feed `parent_session_id`, never\n                      \
        the bare agent id.\n    \
        skipped_lines     The malformed-line count, a head/tail WINDOW census like `list`'s,\n                      \
        because lifecycle reads only the transcript's edges. The full census\n                      \
        is `csift stats @<agent-id>`.\n  \
        A single `--agent <id>` grab emits the SAME envelope (header, session, the one agent row,\n  \
        summary), with no bare-object exception. Every `_utc` field carries a paired `_local`\n  \
        (system-local ISO). Idiom: jq 'select(.kind==\"agent\")' reaches every node.\n\
        \n\
        A TEAMMATE HAS TWO IDS AND BOTH ARE PRINTED\n  \
        Its node line carries `routing: <Name>@<Team>` beside the transcript id, and its JSON row\n  \
        carries `routing_id`. Both need a name AND a team in the meta. With either missing, the\n  \
        text line is omitted and the JSON field is null, never fabricated. The routing form is a\n  \
        form TaskStop takes (beside the bare name), and SendMessage refuses it (any `to` carrying\n  \
        `@`). SendMessage takes the bare name or the `a<Name>-<hex>` form, which is also what the\n  \
        hook payload and the transcript file carry. Only the routing form can collide, since two\n  \
        same-named teammates in one team share it. csift therefore keys on the transcript id and\n  \
        fails loud when a routing form matches more than one lane. Both ids are usable as an `@`\n  \
        target on every command.\n\
        \n\
        SEE ALSO\n  \
        csift whoami @<agent-id>    the same lane seen from itself: self, parent, topology\n  \
        csift whoami --peers        every LIVE lane in the projects root, all sessions\n  \
        csift status @<uuid>        one live verdict with its evidence, not a tree\n  \
        csift send @<agent-id> \"…\"  message one of these lanes on the csift channel"
)]
pub struct AgentsArgs {
    /// Project target (actual cwd or encoded dir) whose sessions' subagents to list.
    ///
    /// A target is a path or an `@` session token, and the argument repeats. With none, every
    /// project is scanned. `@<uuid>` (8-4-4-4-12 hex) scopes to one session, so
    /// `csift agents @<uuid>` lists its subagents. `@main` and `@trap:<marker>` resolve the
    /// calling session, and a `*.jsonl` file scopes to that transcript.
    #[arg(
        value_name = "PATH",
        verbatim_doc_comment,
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// Also scope to the session ids listed in FILE (`-` reads stdin).
    ///
    /// The ids are whitespace- or newline-separated uuid, uuid-prefix or agent-id tokens, bare
    /// or `@`-prefixed, exactly the ids csift emits (`search -l`, JSON `transcript_ids` /
    /// `parent_session_id`). They form a UNION with the positional targets, and each id
    /// resolves fail-loud like an `@` positional. An EMPTY list (an upstream stage that found
    /// nothing) scopes to NOTHING. The result is an honest empty with exit 0, never a silent
    /// widening to every project. The resolved ids then follow this command's normal span
    /// rules, exactly as `@` positionals would. On the span-by-default commands each session
    /// EXPANDS to its subagent transcripts. Add `--no-subagents` to pin the scope to exactly
    /// the listed ids.
    #[arg(long = "sessions-from", value_name = "FILE|-", verbatim_doc_comment)]
    pub sessions_from: Option<std::path::PathBuf>,

    /// HIDDEN no-op subagent-span flag. `agents` has NO subagent-span control: it DISCOVERS
    /// subagents as its primary output, so `--no-subagents` is meaningless here (unlike
    /// list/search/files/recover, which span their session's subagent transcripts). It is
    /// accepted only to emit a pointed error instead of letting `allow_hyphen_values` swallow
    /// it as a bogus PATH value (the misleading `invalid value '--no-subagents' for
    /// '[PATH]...'`). See [`AgentsArgs::span_flag_error`].
    #[arg(long = "no-subagents", hide = true)]
    pub no_subagents: bool,

    /// Hidden no-op twin (see `no_subagents`): `agents` has no span control at all, so BOTH
    /// span switches are accepted-then-rejected with the pointed error.
    #[arg(long = "subagents", hide = true)]
    pub subagents: bool,

    /// Only show subagents of this kind (repeatable). The default is all kinds.
    ///
    /// This is the subagent TRANSCRIPT-SHAPE filter, and its values are `builtin-task` |
    /// `workflow` | `teammate`. A workflow transcript sits at
    /// `subagents/workflows/wf_*/agent-<hex>.jsonl`. A teammate
    /// (`taskKind:"in_process_teammate"`) shares the built-in on-disk location
    /// (`subagents/agent-<id>.jsonl`), and the meta.json `taskKind` is the discriminator. This
    /// filter is NOT the automation-TRIGGER taxonomy
    /// (`background-command`/`agent`/`monitor`/`task`/`workflow`) that `verbatim` and
    /// `search -t harness.notification` surface. The two axes share only the literal token
    /// `workflow`, with a different meaning, so `--shape monitor` is an invalid-value error by
    /// design. The filter is ignored when `--agent <id>` is given (a direct grab).
    #[arg(long = "shape", value_enum, verbatim_doc_comment)]
    pub kinds: Vec<AgentKindFilter>,

    /// Only show subagents whose `agent_type` EXACTLY matches (repeatable).
    ///
    /// The default is all types. Examples are `--agent-type fork` and
    /// `--agent-type general-purpose`. This is the DESCRIPTIVE type axis, the meta/spawn
    /// `agentType`, an open set. It is orthogonal to `--shape`, the on-disk transcript-shape
    /// axis. A `/fork` child is `--agent-type fork` but genuinely `--shape builtin-task`. A
    /// node with no agent_type never matches an explicit filter. The filter is ignored when
    /// `--agent <id>` is given (a direct grab).
    #[arg(long = "agent-type", value_name = "TYPE", verbatim_doc_comment)]
    pub agent_types: Vec<String>,

    /// Lower time bound, read in the system-local time zone.
    ///
    ///   `N<unit>`                 that many units AGO. The seven units are `s` seconds, `m`
    ///                             minutes, `h` hours, `d` days, `w` weeks, `mo` months
    ///                             (30 days) and `y` years (365 days), as in `45s`, `90m`,
    ///                             `2h`, `3d`, `1w`, `2mo`, `1y`. A leading `-` is tolerated.
    ///   ISO8601 instant           `2026-06-01T05:00:00Z` or `…+10:00`
    ///   bare datetime             `2026-06-01T05:00:00` is that LOCAL wall-clock time
    ///   bare date                 `2026-06-01` is LOCAL MIDNIGHT
    /// The bound filters by TRIGGER time by default. `--order-by start|completion` switches the
    /// axis.
    #[arg(long, value_name = "WHEN", verbatim_doc_comment)]
    pub since: Option<String>,

    /// Upper time bound, in the same WHEN grammar as `--since`.
    ///
    /// It uses the same axis as `--since`.
    #[arg(long, value_name = "WHEN", verbatim_doc_comment)]
    pub until: Option<String>,

    /// The ORDERING axis, the timestamp that sorts the tree AND bounds `--since`/`--until`.
    ///
    ///   trigger      the DEFAULT, the true parent-tool_use spawn instant (the correct "when
    ///                was it triggered" axis). It falls back to the start timestamp for a
    ///                subagent whose spawn could not be located.
    ///   start        the subagent's first transcript record (the child-head timestamp), which
    ///                LAGS the trigger by seconds
    ///   completion   the last record
    /// `files --by` is a different flag, choosing how `files` rolls up its rows.
    #[arg(
        long = "order-by",
        value_enum,
        default_value_t = AgentTimeAxis::Trigger,
        verbatim_doc_comment
    )]
    pub order_by: AgentTimeAxis,

    /// Grab ONE subagent by its id, bare or `@`-prefixed like every other id position.
    ///
    /// The id is the `agent_id` column, a bare hex or a teammate's `a<Name>-<16 hex>`. The
    /// grab prints the full node, including the returned message (it implies
    /// `--returned-message`), and with `--with-files` its files-changed list. This is a DIRECT
    /// id lookup. It BYPASSES `--since`/`--until`/`--order-by` and `--shape`, so a known id
    /// resolves regardless of when it ran or its shape, and just the matched node is
    /// rendered, a tree of one and never the whole workflow tree. An id that matches nothing
    /// in scope is a hard ERROR with discovery guidance, never the ambiguous
    /// `no subagents found`. `csift agents @<uuid>` lists the ids (the `agent_id` column or
    /// JSON field).
    #[arg(long, value_name = "ID", verbatim_doc_comment)]
    pub agent: Option<String>,

    /// Attach each node's files-changed list.
    ///
    /// It reuses the `files` extractors over the subagent's own transcript. It is off by
    /// default, because it re-scans each transcript.
    #[arg(long = "with-files", verbatim_doc_comment)]
    pub with_files: bool,

    /// Include each subagent's RETURNED MESSAGE, with the source it was read from.
    ///
    /// The three sources are listed under `returned_message` in JSON SCHEMA. The message is
    /// omitted by default, because it can be large. A single `--agent` grab always includes
    /// it.
    #[arg(long = "returned-message", verbatim_doc_comment)]
    pub returned_message: bool,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl AgentsArgs {
    /// The error string when the (no-op) `--no-subagents` span flag is passed to `agents`, or
    /// `None` when it was not. `agents` has no subagent-span control (it lists subagents AS its
    /// output), so the flag is meaningless, but accepting + rejecting it gives a pointed
    /// message instead of the misleading `allow_hyphen_values` PATH-swallow error.
    #[must_use]
    pub fn span_flag_error(&self) -> Option<&'static str> {
        if self.no_subagents || self.subagents {
            Some(
                "`agents` has no subagent-span flag: it DISCOVERS a session's subagents as its \
                 primary output (there is nothing to span over). Drop the span flag. To list \
                 a session's subagents run `csift agents @<uuid>`; to scope another subcommand's \
                 subagent span use that subcommand's flag (e.g. `csift files --no-subagents \
                 @<uuid>`).",
            )
        } else {
            None
        }
    }
}

/// Which lifecycle timestamp the `agents` ordering axis uses (`--order-by`); it sorts the
/// tree AND bounds the `--since`/`--until` window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum AgentTimeAxis {
    /// the TRUE TRIGGER instant, the parent `Task`/`Agent` tool_use (DEFAULT)
    #[default]
    Trigger,
    /// the subagent's START, its first transcript record
    Start,
    /// the subagent's COMPLETION, its last transcript record
    Completion,
}

/// The `--count-by <AXIS>` census axis: a CLOSED, documented set (deliberately NOT a
/// query DSL: aggregation beyond these axes is `stats` / `files --by` / `--raw | jq`).
/// Doubles as the clap `ValueEnum`, so the value spellings ARE the variant names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CountAxis {
    /// Per role.category.leaf leaf. The census is multi-label, so a record counts
    ///             under EVERY leaf it carries.
    #[value(verbatim_doc_comment)]
    Label,
    /// Per tool name, over tool_use and tool_result hits. Non-tool records are
    ///             excluded and counted.
    #[value(verbatim_doc_comment)]
    Tool,
    /// Per turn, in ASCENDING turn order (a histogram). The keys are `t<N>`, or
    ///             `<transcript>·t<N>` when more than one transcript is in scope.
    #[value(verbatim_doc_comment)]
    Turn,
    /// Per transcript (`session_id`: a top-level uuid or a subagent agent-id).
    Session,
    /// Per tool pairing state, `paired` | `pending` | `orphan`. Non-tool records
    ///             are excluded.
    #[value(verbatim_doc_comment)]
    Pairing,
    /// Per assistant model (records without a model excluded + counted).
    Model,
    /// Per attachment payload type. It implies the `--attachments` gate.
    ///             Non-attachment records are excluded and counted.
    #[value(verbatim_doc_comment)]
    Attachment,
    /// Per Claude Code version stamp, the record's top-level `version`. Records
    ///             without one are excluded and counted.
    #[value(verbatim_doc_comment)]
    Version,
    /// Per tool-result error state, `ok` | `error`. Non-result records are
    ///             excluded and counted. Pairing answers "did a result come
    ///             back", and this axis answers "was it good".
    #[value(verbatim_doc_comment)]
    Result,
    /// Per PROVENANCE relation, the `for` edges the scope carries. A record
    ///             counts once under every distinct relation word its edges name,
    ///             the way the `label` axis counts it under every leaf. It counts
    ///             whether or not csift could resolve the target, because the
    ///             census answers which relations exist and a hit's own `for[]`
    ///             says what each resolved to. A record carrying no edge at all
    ///             is excluded and counted.
    #[value(verbatim_doc_comment)]
    ForKind,
}

impl CountAxis {
    /// The axis slug used in text footers and the JSON `axis` field.
    pub fn slug(self) -> &'static str {
        match self {
            CountAxis::Label => "label",
            CountAxis::Tool => "tool",
            CountAxis::Turn => "turn",
            CountAxis::Session => "session",
            CountAxis::Pairing => "pairing",
            CountAxis::Model => "model",
            CountAxis::Attachment => "attachment",
            CountAxis::Version => "version",
            CountAxis::Result => "result",
            CountAxis::ForKind => "for-kind",
        }
    }
}

/// The aggregation detail level for `files`, selected by `--by <summary|dir|file|timeline>`
/// (exactly one is active; default `summary`). Doubles as the clap `ValueEnum` for `--by`,
/// so the value spellings (`summary`/`dir`/`file`/`timeline`) ARE the variants' value names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum FilesDetail {
    /// a coarse top-level-prefix op rollup, the smallest output (DEFAULT)
    #[default]
    #[value(name = "summary")]
    Summary,
    /// one row per distinct directory (the FULL parent path)
    #[value(name = "dir")]
    ByDir,
    /// one row per distinct file, with first/last touch timestamps
    #[value(name = "file")]
    ByFile,
    /// the full chronological list, one line per mutation (opt-in)
    #[value(name = "timeline")]
    Timeline,
}
