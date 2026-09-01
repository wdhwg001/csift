//! StatusArgs + WaitArgs - the live-truth command pair.

use super::*;

/// `csift status`: one-shot liveness verdict for a session.
#[derive(Args, Debug)]
#[command(
    about = "One-shot LIVE verdict on a session: running, waiting on children, waiting on \
             a human, idle, or dead - with the evidence",
    long_about = "Answer \"has this session truly stopped?\" RIGHT NOW. A three-way join \
        produces one verdict: the harness's own session registry \
        (`<claude-home>/sessions/<pid>.json` - status transitions land sub-second), the \
        transcript's tail state machine (an unpaired shell/tool call at the tail = a tool \
        in flight; the last assistant record's stop_reason), and owner-process liveness \
        (a `ps`-based probe, falling back to `/proc/<pid>` where ps lacks the flags, plus \
        a process-start-time guard against pid reuse). Child \
        transcripts and workflow journals join in by default (span law); the elicitation \
        sidecar covers human-in-the-loop states. Each child lane is classified \
        `in-flight` (unreturned call), `generating` (a tail record younger than 300s \
        with no end_turn yet - a long generation writes nothing for minutes, so recency \
        plus a non-final stop_reason is the live signal), or `settled`; settled lanes \
        FOLD to a count so live work stays visible. The session's harness task list \
        (`<claude-home>/tasks/`) renders open tasks (in_progress first, then pending, \
        with blockers) and folds completed ones to a count.\n\n\
        VERDICTS (a closed set): `running` (generating, or a tool in flight) · \
        `waiting-children` (main idle, subagent/workflow lanes live) · `waiting-hitl` \
        (blocked on a human: a pending AskUserQuestion/ExitPlanMode/MCP elicitation) · \
        `idle-eot` (truly stopped at end of turn) · `stale-dead` (the owning process is \
        gone; the tail shape then says HOW it died) · `unknown` (evidence insufficient or \
        contradictory - stated, never guessed). Every verdict lists the evidence rows \
        that produced it.\n\n\
        THIS IS A LIVE-TRUTH COMMAND: point-in-time, explicitly NON-reproducible - a \
        deliberate, documented departure from the forensic contract (search/show/stats \
        stay reproducible; status answers \"now\" and says so). It reads outside \
        projects/ (the registry), honoring --claude-home.\n\n\
        HONESTY LIMITS, stated in the output when they bite: a pending PERMISSION prompt \
        lives only in Claude Code process memory (no sidecar installed = it masquerades \
        as idle; the note says so); the registry covers top-level interactive sessions \
        only (a subagent target degrades to tail + pid evidence, never a fabricated \
        row); on non-unix hosts the pid probe is unavailable and the verdict says so.",
    after_help = "EXAMPLES\n  \
          csift status @<uuid>                    # is it truly stopped?\n  \
          csift status @main                      # my own session (env)\n  \
          csift status @<uuid> --format json | jq .verdict\n  \
          csift status @<uuid> --no-subagents     # the main lane only\n\n\
        JSON SCHEMA (per --format json)\n  \
          Envelope: {kind:\"header\", command:\"status\", session_id, is_subagent, \
        parent_session_id} → one {kind:\"verdict\", verdict, \
        evidence:[{surface, value, age_secs}], children:[{session_id, state, detail} - \
        live lanes only], settled_children, tasks:[{id, subject, status, blocked_by}] \
        (null when the session has no tasks dir), tasks_completed, \
        pending:[...], notes:[...]} → {kind:\"summary\", verdict}."
)]
pub struct StatusArgs {
    /// ONE session: `@<uuid>` | `@<uuid-prefix>` | `@main` | `@<agent-id>` | a `*.jsonl`
    /// path. (Same grammar as every command; exactly one target.)
    #[arg(
        value_name = "TARGET",
        value_parser = parse_project_target,
        allow_hyphen_values = true,
        required = true,
        num_args = 1..
    )]
    pub target: Vec<std::path::PathBuf>,

    /// Exclude subagent transcripts from the verdict: the main lane only.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Span subagent transcripts: the DEFAULT; the pair exists for uniformity.
    #[arg(long = "subagents", conflicts_with = "no_subagents")]
    pub subagents: bool,

    /// Emit JSON instead of the text verdict.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl StatusArgs {
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.subagents || !self.no_subagents
    }
}

/// `csift wait`: block until a condition occurs (or timeout).
#[derive(Args, Debug)]
#[command(
    about = "Block until a session condition occurs (stop, HITL, a named tool call, a \
             notification), then exit reporting which one fired",
    long_about = "The monitor half of the live-truth pair: poll a session until one of \
        the `--until` conditions fires, then exit 0 naming it (JSON carries its \
        evidence). BASELINE SEMANTICS - the difference between a monitor and a query: \
        `wait` snapshots every watched file's byte length at startup and observes ONLY \
        events strictly AFTER those offsets. A condition already satisfied by history \
        alone never fires - history is `search`'s job. After the snapshot, one readiness \
        line prints to stderr (`csift: watching N file(s) ...`) so a scripted caller can \
        order its own actions after the baseline.\n\n\
        CONDITIONS (repeatable; OR; first hit wins):\n  \
          stop                      verdict becomes idle-eot or stale-dead\n  \
          hitl                      verdict becomes waiting-hitl\n  \
          auq                       the elicitation sidecar gains an unanswered question\n  \
          notification[:REGEX]      a task-notification lands in the MAIN transcript\n  \
          tool:NAME[:REGEX]         a tool_use of NAME whose serialized input matches\n  \
          write:PATH_RE[:LINE_RE]   a Write/Edit whose path matches (and content line, if given)\n  \
          verdict:V                 any verdict from the status table\n\n\
        Regexes are the same RE2-class engine as search (no backrefs/lookaround). \
        Polling is adaptive (200ms floor backing off toward 2s when quiet; `--interval` \
        pins it); reads are incremental (only appended bytes, torn-tail guarded).\n\n\
        EXIT CODES: 0 = a condition fired · 124 = --timeout elapsed (the GNU timeout \
        convention; the ONE documented exception to the crate's 0-vs-non-zero law, \
        because a monitor's timeout is a normal outcome a script must branch on) · \
        any other non-zero = error. With no --timeout, wait waits forever and the \
        readiness line says so.",
    after_help = "EXAMPLES\n  \
          csift wait @<uuid> --until stop --timeout 300      # block until truly stopped (or 5m)\n  \
          csift wait @main --until auq                       # fire when a question lands\n  \
          csift wait @<uuid> --until tool:Read:handover      # …until it reads that file\n  \
          csift wait @<uuid> --until stop --until hitl --format json | jq .fired\n\n\
        JSON SCHEMA (per --format json)\n  \
          One object on exit: {kind:\"wait\", fired (the condition string | \"timeout\"), \
        verdict, evidence:[...], waited_secs}."
)]
pub struct WaitArgs {
    /// ONE session (same grammar as `status`).
    #[arg(
        value_name = "TARGET",
        value_parser = parse_project_target,
        allow_hyphen_values = true,
        required = true,
        num_args = 1..
    )]
    pub target: Vec<std::path::PathBuf>,

    /// A condition to wait for (repeatable; OR semantics; first hit wins).
    #[arg(long = "until", value_name = "COND", required = true)]
    pub until: Vec<String>,

    /// Give up after SECS seconds: exit 124 (the GNU timeout convention), JSON
    /// `fired:"timeout"`. Default: no timeout (wait forever; the readiness line notes it).
    #[arg(long, value_name = "SECS")]
    pub timeout: Option<u64>,

    /// Poll interval in milliseconds (default: adaptive, 200ms floor backing off to 2s).
    #[arg(long, value_name = "MS")]
    pub interval: Option<u64>,

    /// Exclude subagent transcripts from the watch set.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Span subagent transcripts: the DEFAULT; the pair exists for uniformity.
    #[arg(long = "subagents", conflicts_with = "no_subagents")]
    pub subagents: bool,

    /// Emit the exit object as JSON.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl WaitArgs {
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.subagents || !self.no_subagents
    }
}
