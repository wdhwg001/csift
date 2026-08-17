//! AgentsArgs + the agent/count/files enums (shape, order-by, CountAxis).

use super::*;

/// Which subagent kinds to surface in `agents`. Mirrors the on-disk discriminator
/// (path location), with `agentType` retained as a descriptive sub-label per row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AgentKindFilter {
    /// Built-in Task/Agent-tool subagents (`subagents/agent-<hex>.jsonl`).
    #[value(name = "builtin-task")]
    BuiltinTask,
    /// Workflow / OMC workflow-subagent transcripts
    /// (`subagents/workflows/wf_*/agent-<hex>.jsonl`).
    Workflow,
    /// "Teammate" agents (`taskKind:"in_process_teammate"`) — Claude Code's persistent,
    /// directly-addressable team members. They share the built-in on-disk location
    /// (`subagents/agent-<id>.jsonl`); the meta.json `taskKind` is the discriminator.
    Teammate,
}

#[derive(Debug, Args)]
#[command(
    about = "List a session's subagents with kind, start/completion timestamps + status",
    long_about = "List a session's SUBAGENT lifecycle: every subagent transcript the \
        session spawned, with its id, KIND, start + completion timestamps, duration, \
        and a determinable status. Three on-disk shapes are discovered under \
        `<session>/subagents/**` (verified empirically against ~/.claude/projects):\n  \
          • builtin-task  subagents/agent-<hex>.jsonl                 (Task/Agent tool)\n  \
          • workflow      subagents/workflows/wf_<id>/agent-<hex>.jsonl (OMC workflows)\n  \
          • teammate      subagents/agent-a<Name>-<hex>.jsonl         (in_process_teammate / FleetView; meta taskKind)\n\
        Workflow `journal.jsonl` event logs are NOT transcripts — they are read only \
        to corroborate completion status, never listed as agents.\n\n\
        The TARGET selects the parent session: pass `@<uuid>` for one \
        session, or a project PATH/encoded-dir to cover every session under it (each \
        session's subagents are grouped under it). Start/completion come from the \
        subagent transcript's first/last record timestamp; status is `completed` when \
        a workflow journal carries a `result` event for the agent (or the transcript \
        terminates cleanly), else `running`/`unknown`.\n\n\
        `--since`/`--until` (ISO8601 or relative `2h`/`3d`/…, in the system local \
        timezone) filter to subagents whose TRIGGER time (the parent tool_use ts — the \
        true spawn instant) falls in the window by default; `--order-by start` uses the \
        transcript's first-record ts, `--order-by completion` the last (the lane's \
        TERMINAL instant, `last_activity_utc` — so a frozen lane windows on its freeze \
        instant rather than vanishing from a bounded window for never completing).\n\n\
        TOPOLOGY: the TEXT output is ALWAYS the parent→child tree — workflow runs as \
        parent nodes of their agents, and a nested sub-subagent under its spawning agent \
        (indented by depth). JSON emits the SAME topology as FLAT kind-tagged rows (one \
        `kind:\"agent\"` row per node, tree pre-order; rebuild nesting from \
        parent_agent_id + depth). `--agent <hex>` grabs ONE subagent with its returned message; \
        `--returned-message` adds the 3-way-resolved returned message to every row; \
        `--with-files` attaches each node's files-changed list.",
    after_help = "TARGET / TOPOLOGY (scope guidance)\n  \
          The TARGET selects the PARENT session whose subagents to list: pass `@<uuid>` \
        (or `@main`/`@trap:<marker>`) for ONE session, or a project PATH/encoded-dir \
        to cover every session under it (each session's subagents grouped under it). \
        Subagent shapes discovered under `<session>/subagents/**`:\n    \
            • builtin-task  subagents/agent-<hex>.jsonl                       (Task/Agent tool)\n    \
            • workflow      subagents/workflows/wf_<id>/agent-<hex>.jsonl     (OMC workflows)\n    \
            • teammate      subagents/agent-a<Name>-<hex>.jsonl              (in_process_teammate; meta taskKind)\n  \
          Workflow `journal.jsonl` event logs are NOT transcripts (read only to corroborate \
        status, never listed). Status is `completed` when a workflow journal carries a \
        `result` event (or the transcript terminates cleanly), else `running`/`unknown`. \
        The TEXT output is ALWAYS the parent→child topology tree (workflow runs as parents \
        of their agents; a nested sub-subagent under its spawning agent); JSON carries the \
        SAME topology as flat pre-ordered rows. `--agent <hex>` grabs ONE subagent \
        by its bare-hex id (full node + returned \
        message); it is a DIRECT lookup that IGNORES --since/--until/--order-by/--shape and \
        renders just that node (a tree of one), and \
        a non-matching hex is a hard error (run a plain listing first to discover ids). NOTE: \
        `--shape` here is the TRANSCRIPT-SHAPE filter (builtin-task | workflow | teammate), a DIFFERENT \
        axis from the automation-trigger `kind` (background-command/agent/monitor/task) used \
        by `verbatim`/`search -t user` — they overlap only on the token `workflow`.\n\n\
        EXAMPLES\n  \
          csift agents @0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d              # one session's subagent tree\n  \
          csift agents .                                                   # every session under this project\n  \
          csift agents . --shape workflow                                   # only workflow-shape agents (NOT the automation kind)\n  \
          csift agents @<uuid> --since 2h                                  # subagents TRIGGERED in the last 2h\n  \
          csift agents @<uuid> --since 6h --order-by completion            # COMPLETED in the last 6h\n  \
          csift agents @<uuid> --order-by completion                       # order/window on the completion axis\n  \
          csift agents @<uuid> --since 2026-06-01T09:00:00Z --order-by completion  # COMPLETED since an ISO bound\n  \
          csift agents @<uuid>                                             # discover ids, then grab one:\n  \
          csift agents --agent <hex>                                       #   grab ONE subagent (direct lookup; ignores time/kind)\n  \
          csift agents @<uuid> --with-files                                # each node + its files-changed list\n  \
          csift agents @<uuid> --returned-message                          # add the 3-way-resolved returned message to every row\n  \
          csift agents . --format json                                     # machine-readable flat rows (kind: session|run|agent)\n\n\
        JSON SCHEMA (per --format json)\n  \
          FLAT kind-tagged rows (envelope v2, v0.5): a leading {kind:\"header\", …} line; \
        per session one light {kind:\"session\", session_id, runs, agents} row (counts \
        only); each in-scope workflow run one {kind:\"run\", session_id, run_id, task_id, \
        workflow_name, status, agent_count, duration_ms, total_tokens, total_tool_calls, \
        default_model, started_utc, started_local} row followed by its member agent rows \
        (a RUN row's `status` is the workflow journal's own last status VERBATIM — an OPEN \
        set, not a csift enum; observed values include `completed` and `killed` — distinct \
        from an AGENT row's csift-computed `status`); \
        every agent its OWN {kind:\"agent\", …} row in tree PRE-ORDER (a parent row precedes \
        its children; rebuild nesting from parent_agent_id + depth — there is no \
        children[] array in JSON, the tree renders in TEXT mode); a closing \
        {kind:\"summary\", sessions, runs, agents} terminator. Agent-row fields: {agent_id, \
        shape, parent_session_id, parent_agent_id, spawn_tool_use_id, spawn_tool, \
        workflow_id, agent_type, name, team_name, description, trigger_utc/_local, \
        started_utc/_local, completed_utc/_local, last_activity_utc/_local, duration, \
        depth, status, pending_tool_use_id, pending_tool_name, pending_classification, \
        pending_since_utc/_local, skipped_lines} (+ control_hint on a teammate; \
        STALENESS: `pending_classification: awaiting-execution` means slow OR wedged OR \
        abandoned — jsonl cannot tell them apart, and at corpus scale a lane pending for \
        hours/days is overwhelmingly \"parent session ended, nobody is coming back\", not \
        in-flight work: weigh `pending_since_utc` against now yourself; \
        completed_utc/_local and duration are non-null ONLY when status is `completed` — \
        a frozen/running lane is NOT done, and its tail instant lives in \
        last_activity_utc/_local, which every timestamped lane carries (on a frozen lane \
        it equals pending_since_utc); \
        `--with-files` adds `files_changed`; `--returned-message`, implied by a single \
        `--agent`, adds `returned_message` + `returned_message_source` — the NEWEST \
        message the child EVER returned, source-tagged; on a frozen/running lane it \
        PREDATES the pending call, so a lane can carry BOTH a returned_message and \
        pending_* — the return is history, the pending_* fields are now; the TEXT render \
        brands a non-completed lane's message inline (`history — predates the still-open \
        lane, NOT the outcome`) so a clean-finale-sounding tail cannot pass as the ending. \
        SEMANTICS: it answers \"what did the ORCHESTRATOR record as the return\", not \
        \"what did the agent conclude\" — a `sync-tool-result` source faithfully reports \
        the parent's tool_result even when the harness truncated it to a `Done. \
        agentId: …` wrapper; the child's own final words are always \
        `csift show @<agent-id> --turn -1..`). \
        `agent_type` is \
        the semantic agent ROLE string (e.g. `Explore`, `oh-my-claudecode:critic`) — \
        DISTINCT from `shape`, the on-disk transcript shape (builtin-task | workflow | \
        teammate); `kind` is the envelope discriminator exclusively. ID-DOMAIN: `agent_id` \
        IS this transcript's own id (the SAME concept other commands call `session_id`); \
        re-feed `parent_session_id`, never the bare agent id. A single `--agent <hex>` \
        grab emits the SAME envelope (header + session + the one agent row + summary) — \
        no bare-object exception. Every `_utc` field carries a paired `_local` \
        (system-local ISO). The malformed-line count rides on each agent row's \
        `skipped_lines` — a head/tail WINDOW census like `list`'s (lifecycle reads only the \
        transcript's edges; full census: `csift stats @<agent-id>`). \
        Idiom: jq 'select(.kind==\"agent\")' reaches every node."
)]
pub struct AgentsArgs {
    /// Project target (actual cwd or encoded dir) whose sessions' subagents to list, OR an
    /// `@<uuid>` session token. Repeatable; with none, every project is scanned. `@<uuid>`
    /// (8-4-4-4-12 hex) scopes to one session (so `csift agents @<uuid>` lists its subagents),
    /// `@main`/`@trap:<marker>` resolve the calling session, and a `*.jsonl` file scopes to that transcript.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// Scope ALSO to the session ids in FILE (`-` = stdin): whitespace/newline-separated
    /// uuid / uuid-prefix / agent-id tokens, bare or `@`-prefixed — exactly the ids csift
    /// emits (`search -l`, JSON `transcript_ids` / `parent_session_id`). UNION with positional
    /// targets; each id resolves fail-loud like an `@` positional. An EMPTY list (an upstream
    /// stage that found nothing) scopes to NOTHING — honest empty, exit 0, never a silent
    /// widening to every project. The resolved ids then follow this command's normal
    /// span rules, exactly as if they were `@` positionals: on the span-by-default
    /// commands each session EXPANDS to its subagent transcripts (add `--no-subagents`
    /// to pin to exactly the listed ids).
    #[arg(long = "sessions-from", value_name = "FILE|-")]
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

    /// Only show subagents of this kind (repeatable). Default: all kinds. This is the
    /// subagent TRANSCRIPT-SHAPE filter — its values are `builtin-task` | `workflow` |
    /// `teammate`. It is NOT the automation-TRIGGER taxonomy (`background-command`/`agent`/
    /// `monitor`/`task`/`workflow`) that `verbatim`/`search -t harness.notification` surface;
    /// the two axes share only the literal token `workflow` (different meaning), so
    /// `--shape monitor` is an invalid-value error by design. Ignored when `--agent <hex>`
    /// is given (a direct grab).
    #[arg(long = "shape", value_enum)]
    pub kinds: Vec<AgentKindFilter>,

    /// Lower time bound. WHEN grammar (system-local tz): relative `Ns`/`Nm`/`Nh`/`Nd`/`Nw`
    /// = that long AGO (`45s`,`90m`,`2h`,`3d`,`1w`); an ISO8601 instant
    /// (`2026-06-01T05:00:00Z` / `…+10:00`); a BARE datetime (`2026-06-01T05:00:00`) = that
    /// LOCAL wall-clock time; or a bare date (`2026-06-01`) = LOCAL MIDNIGHT. Filters by
    /// TRIGGER time by default (`--order-by start|completion` switches axis).
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as --since). Same axis as `--since`.
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// The ORDERING axis: which timestamp sorts the tree AND bounds `--since`/`--until`.
    /// `trigger` (DEFAULT — the true parent-tool_use spawn instant), `start` (the
    /// subagent's first transcript record / child-head ts, which LAGS the trigger by
    /// seconds), or `completion` (the last record). Named `--order-by` (not `--by`, which
    /// reads like a projection) because it names the sort axis. (Sibling note: `files` uses
    /// `--by` for a PROJECTION — a different meaning on a different subcommand.)
    #[arg(long = "order-by", value_enum, default_value_t = AgentTimeAxis::Trigger)]
    pub order_by: AgentTimeAxis,

    /// Grab ONE subagent by its bare-hex id: prints its full node incl. the returned
    /// message (implies `--returned-message`) and, with `--with-files`, its files-changed.
    /// This is a DIRECT id lookup: it BYPASSES `--since`/`--until`/`--order-by` and `--shape`
    /// (a known id resolves regardless of when it ran or its shape), and just the matched
    /// node is rendered (a tree of one — not the whole workflow tree). If the hex matches
    /// nothing in scope, it is a hard ERROR with discovery guidance — not the ambiguous
    /// `no subagents found`. Discover ids first with `csift agents @<uuid>` (the `agent_id`
    /// column / JSON field).
    #[arg(long, value_name = "HEX")]
    pub agent: Option<String>,

    /// Attach each node's files-changed list (reuses the `files` extractors over the
    /// subagent's own transcript). Off by default — it re-scans each transcript.
    #[arg(long = "with-files")]
    pub with_files: bool,

    /// Include each subagent's RETURNED MESSAGE (3-way resolved). Omitted by default
    /// (a returned message can be large); always included for a single `--agent` grab.
    #[arg(long = "returned-message")]
    pub returned_message: bool,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl AgentsArgs {
    /// The error string when the (no-op) `--no-subagents` span flag is passed to `agents`, or
    /// `None` when it was not. `agents` has no subagent-span control (it lists subagents AS its
    /// output), so the flag is meaningless — but accepting + rejecting it gives a pointed
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

/// Which lifecycle timestamp the `agents` ordering axis uses (`--order-by`) — it sorts the
/// tree AND bounds the `--since`/`--until` window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum AgentTimeAxis {
    /// Filter on the TRUE TRIGGER instant — the parent `Task`/`Agent` tool_use timestamp
    /// (the correct "when was it triggered" axis). The DEFAULT. Falls back to the start
    /// timestamp for a subagent whose spawn could not be located.
    #[default]
    Trigger,
    /// Filter on the subagent's START timestamp (first transcript record; lags the
    /// trigger by seconds).
    Start,
    /// Filter on the subagent's COMPLETION timestamp (last transcript record).
    Completion,
}

/// The `--count-by <AXIS>` census axis — a CLOSED, documented set (deliberately NOT a
/// query DSL: aggregation beyond these axes is `stats` / `files --by` / `--raw | jq`).
/// Doubles as the clap `ValueEnum`, so the value spellings ARE the variant names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CountAxis {
    /// Per role.class.sub leaf (multi-label: a record counts under EVERY leaf it carries).
    Label,
    /// Per tool name (tool_use / tool_result hits; non-tool records excluded + counted).
    Tool,
    /// Per turn, ASCENDING turn order (a histogram; keys `t<N>`, `<transcript>·t<N>` when
    /// more than one transcript is in scope).
    Turn,
    /// Per transcript (`session_id` — a top-level uuid or a subagent agent-id).
    Session,
    /// Per tool pairing state: `paired` | `pending` | `orphan` (non-tool records excluded).
    Pairing,
    /// Per assistant model (records without a model excluded + counted).
    Model,
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
        }
    }
}

/// The aggregation detail level for `files`, selected by `--by <summary|dir|file|timeline>`
/// (exactly one is active; default `summary`). Doubles as the clap `ValueEnum` for `--by`,
/// so the value spellings (`summary`/`dir`/`file`/`timeline`) ARE the variants' value names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum FilesDetail {
    /// Coarse TOP-LEVEL-prefix op rollup — buckets each path on its first few directory
    /// segments (so a whole project tree collapses to one row), the smallest output and the
    /// DEFAULT. Strictly coarser than `--by dir` (which keys on the FULL parent dir).
    #[default]
    #[value(name = "summary")]
    Summary,
    /// One row per distinct directory (the FULL parent path) with per-op + distinct-file
    /// counts — finer than `--by summary`'s top-level-prefix rollup.
    #[value(name = "dir")]
    ByDir,
    /// One row per distinct file with per-op counts + first/last touch timestamps.
    #[value(name = "file")]
    ByFile,
    /// Full chronological list, one line per mutation (the verbose, opt-in mode).
    #[value(name = "timeline")]
    Timeline,
}
