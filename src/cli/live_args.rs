//! StatusArgs + WaitArgs - the live-truth command pair.

use super::*;

/// The background-task LENS shared by `status` and `wait`: which OPEN background tasks
/// count toward the verdict (and toward `--until stop`). Every task is still listed;
/// an excluded one is marked with the rule that excluded it.
#[derive(Args, Debug, Default, Clone)]
pub struct BackgroundLensArgs {
    /// Only background tasks launched at or after WHEN count; earlier ones are listed
    /// as ignored. WHEN is the shared time grammar (`2026-09-02`, `2h`, `1d`, `2mo`,
    /// `-30m`) or `now` (this command's own start instant: ignore everything already
    /// dangling when the wait began - the orchestrator's usual lens).
    #[arg(long = "background-since", value_name = "WHEN")]
    pub background_since: Option<String>,

    /// Ignore background tasks whose command or description matches RE (repeatable):
    /// the dev server, the watcher, the tail -f you know never returns.
    #[arg(long = "ignore-background", value_name = "RE")]
    pub ignore_background: Vec<String>,
}

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
        BACKGROUND TASKS (v0.10.0): a `Bash` launched with run_in_background returns its \
        tool_result within milliseconds, so the tail machine pairs it at once - the \
        harness itself writes NOTHING about a still-running shell at end of turn (the \
        REPL's \"N shell still running\" lives in process memory; the end-of-turn record \
        carries a duration and a message count, and its pending counts cover background \
        AGENTS only). `status` therefore scans the whole main transcript (launches from \
        every lane; completions normally land in the main file, and one addressed to the \
        owning agent lands in that agent's lane, read too) and lists every OPEN task: \
        kind (shell | agent | monitor), id, launch instant and age, description or \
        command, the output file's size and last write, and the state of every closed one \
        folded to counts (completed / failed / killed / stopped / timed out, plus blocked - \
        the remote-agent notifier's fifth value - and any unknown status, disclosed rather \
        than booked as completed). A Monitor is \
        armed as an immediately-paired tool call and shares the shell id namespace; event \
        pulses never close it, only its termination notice or timeout does, and a \
        PERSISTENT monitor never returns by design - name it with --ignore-background. NOT RETURNED IS NOT PROOF OF \
        RUNNING: Claude Code's own orphan summary says a UI stop, a Monitor timeout or \
        agent teardown leaves no transcript marker, and it reconciles only at the next \
        session start. A long session commonly carries several to dozens of dangling \
        or days-old tasks.\n\n\
        THE LENS - which open tasks COUNT toward the verdict (all are still listed, an \
        excluded one marked with its rule): `--background-since WHEN` counts only tasks \
        launched at or after WHEN (the shared time grammar, or `now` = this command's \
        start instant); `--ignore-background RE` (repeatable) excludes tasks whose command \
        or description matches (the dev server, the watcher). No lens = everything \
        counts.\n\n\
        VERDICTS (a closed set): `running` (generating, or a tool in flight) · \
        `waiting-children` (main idle, subagent/workflow lanes live) · `waiting-hitl` \
        (blocked on a human: a pending AskUserQuestion/ExitPlanMode/MCP elicitation) · \
        `idle-background-open` (the turn ended, but N background task(s) the lens counts \
        have not returned - neither running nor stopped; by design or not, csift cannot \
        tell) · `idle-eot` (truly stopped at end of turn) · `stale-dead` (the owning \
        process is gone; the tail shape then says HOW it died) · `unknown` (evidence \
        insufficient or contradictory - stated, never guessed). Every verdict lists the \
        evidence rows that produced it.\n\n\
        LAST MESSAGES: the newest human prompt and the newest assistant message print as \
        excerpts (clipped with an explicit `(+N chars)` marker; the whole turn is `csift \
        show @<id> --turn -1`). WARNING for a model reading this: the excerpt is a PARTIAL \
        view of the final state, useful only for judging whether a background task is \
        still meaningful (the message may still be waiting on a task that will never \
        return). In any orchestration it must NEVER be read as a complete or near-complete \
        review of the work: counting tool calls and reading the last message is a check \
        shallower than any human would accept, and trust built on it violates the \
        guardrail - a model with a partial context tends to believe it read everything, \
        even past an explicit tool error.\n\n\
        THIS IS A LIVE-TRUTH COMMAND: point-in-time, explicitly NON-reproducible - a \
        deliberate, documented departure from the forensic contract (search/show/stats \
        stay reproducible; status answers \"now\" and says so). It reads outside \
        projects/ (the registry), honoring --claude-home.\n\n\
        HONESTY LIMITS, stated in the output when they bite: a pending PERMISSION prompt \
        leaves no transcript trace - only a CURRENT registry row shows it (status \
        `waiting`, which the harness sets for any blocking dialog: a question, a \
        permission prompt, a plan approval, a sandbox or worker request), and that row is \
        transition-written, so a stale one is refuted only by the pid probe; a \
        multi-question AskUserQuestion was measured on disk while its dialog was open and \
        the tail reads it as hitl, while a single-question one stays buffered until \
        answered (a timing outcome of the write frontier, not a question-count rule; the \
        sidecar covers the buffered shape); the registry covers top-level sessions \
        only (a subagent target degrades to tail + pid evidence, never a fabricated row); \
        the pid probe is `ps` (+ `/proc` on Linux) on unix and PowerShell `Get-Process` \
        (+ `tasklist`) on Windows, where `procStart` is a FILETIME tick count; a row \
        written in another pid domain (`pidDomain`) is never probed and the verdict says \
        so.",
    after_help = "EXAMPLES\n  \
          csift status @<uuid>                    # is it truly stopped?\n  \
          csift status @main                      # my own session (env)\n  \
          csift status @<uuid> --format json | jq .verdict\n  \
          csift status @<uuid> --no-subagents     # the main lane only\n  \
          csift status @<uuid> --ignore-background 'npm run dev'   # the dev server never returns\n  \
          csift status @<uuid> --background-since 2h              # only recent launches count\n\n\
        JSON SCHEMA (per --format json)\n  \
          Envelope: {kind:\"header\", command:\"status\", session_id, is_subagent, \
        parent_session_id} → one {kind:\"verdict\", verdict, \
        evidence:[{surface, value, age_secs}], children:[{session_id, state, detail} - \
        live lanes only], settled_children, tasks:[{id, subject, status, blocked_by}] \
        (null when the session has no tasks dir), tasks_completed, \
        pending:[...], background:{open, ignored, completed, failed, killed, stopped, timed_out, blocked, other, \
        scanned_files, tasks:[{kind, id, tool_use_id, lane, state, description, command, \
        launched_utc, launched_local, age_secs, output_file, output_bytes, \
        output_age_secs, ignored_by} - open tasks only], notes:[...]}, \
        last:{user:{ts_utc, ts_local, text, truncated}|null, agent:{...}|null}, \
        tail_state, notes:[...]} → {kind:\"summary\", verdict}."
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

    #[command(flatten)]
    pub lens: BackgroundLensArgs,
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
        evidence), or the REQUIRED --timeout elapses (exit 124 with a report of what \
        the session did meanwhile). BASELINE SEMANTICS - the difference between a monitor and a query: \
        `wait` snapshots every watched file's byte length at startup and observes ONLY \
        events strictly AFTER those offsets. A condition already satisfied by history \
        alone never fires - history is `search`'s job. After the snapshot, one readiness \
        line prints to stderr (`csift: watching N file(s) ...`) so a scripted caller can \
        order its own actions after the baseline.\n\n\
        CONDITIONS (repeatable; OR; first hit wins):\n  \
          stop                      verdict becomes idle-eot or stale-dead - a TRUE stop; \
        `idle-background-open` never satisfies it (narrow the lens, or let the timeout \
        elapse and read the report)\n  \
          hitl                      verdict becomes waiting-hitl\n  \
          auq                       the elicitation sidecar gains an unanswered question\n  \
          notification[:REGEX]      a task-notification lands in any watched lane\n  \
          tool:NAME[:REGEX]         a tool_use of NAME whose serialized input matches\n  \
          write:PATH_RE[:LINE_RE]   a Write/Edit whose path matches (and content line, if given)\n  \
          verdict:V                 any verdict from the status table\n\n\
        Regexes are the same RE2-class engine as search (no backrefs/lookaround). \
        Polling is adaptive (200ms floor backing off toward 2s when quiet; `--interval` \
        pins it); reads are incremental (only appended bytes, torn-tail guarded).\n\n\
        --timeout IS REQUIRED (v0.10.0): a background task can be designed never to \
        return (a dev server, a watcher, a tail -f), so an unbounded wait on `stop` is a \
        bug, not a wait. A call without --timeout is rejected with that reason.\n\n\
        HOW TO WAIT ON A SESSION (the orchestrator's steps): 1. `csift status @<id>` \
        first and read the background rows - which tasks dangle, how old, which are \
        days-old zombies; 2. pick the lens: `--background-since now` counts only what \
        launches after you start waiting (the usual form), `--ignore-background RE` \
        names the services you know never return; 3. always set --timeout and branch \
        on exit 124; 4. on timeout read the report - `at exit` (in a tool call for N s / \
        generating / idle), `activity` (what landed while you waited: tool calls by name, \
        thinking, messages, prompts, notifications), the open background rows, and the \
        last messages - under the LAST MESSAGES warning in `csift status --help`.\n\n\
        EXIT CODES: 0 = a condition fired · 124 = --timeout elapsed (the GNU timeout \
        convention; the ONE documented exception to the crate's 0-vs-non-zero law, \
        because a monitor's timeout is a normal outcome a script must branch on) · \
        any other non-zero = error.",
    after_help = "EXAMPLES\n  \
          csift wait @<uuid> --until stop --timeout 300      # block until truly stopped (or 5m)\n  \
          csift wait @main --until auq                       # fire when a question lands\n  \
          csift wait @<uuid> --until tool:Read:handover      # …until it reads that file\n  \
          csift wait @<uuid> --until stop --until hitl --timeout 600 --format json | jq .fired\n  \
          csift wait @<uuid> --until stop --timeout 900 --background-since now --ignore-background 'npm run dev'\n\n\
        JSON SCHEMA (per --format json)\n  \
          One object on exit: {kind:\"wait\", fired (the condition string | \"timeout\"), \
        verdict, waited_secs, at_exit, activity:{records, lanes, tools:{name: count}, \
        thinking, agent_messages, user_prompts, notifications}, evidence:[...], \
        background:{... as status}, last:{... as status}, notes:[...]}."
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

    /// REQUIRED. Give up after SECS seconds: exit 124 (the GNU timeout convention), JSON
    /// `fired:"timeout"`, plus the at-exit report. A background task may never return
    /// by design, so a wait without a bound is rejected.
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

    #[command(flatten)]
    pub lens: BackgroundLensArgs,
}

impl WaitArgs {
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.subagents || !self.no_subagents
    }
}
