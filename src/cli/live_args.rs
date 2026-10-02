//! StatusArgs + WaitArgs - the live-truth command pair.

use super::*;

/// The background-task LENS shared by `status` and `wait`: which OPEN background tasks
/// count toward the verdict (and toward `--until stop`). Every task is still listed;
/// an excluded one is marked with the rule that excluded it.
#[derive(Args, Debug, Default, Clone)]
pub struct BackgroundLensArgs {
    /// Count only background tasks launched at or after WHEN, and list earlier ones as
    /// ignored. WHEN takes the shared time grammar (`2026-09-02`, `2h`, `1d`, `2mo`, `-30m`)
    /// or `now`, this command's own start instant. `now` ignores everything already
    /// dangling when the wait began, which is what a session supervising another one
    /// usually wants.
    #[arg(verbatim_doc_comment, long = "background-since", value_name = "WHEN")]
    pub background_since: Option<String>,

    /// Ignore background tasks whose command or description matches RE (repeatable), such
    /// as the dev server, the watcher, or the tail -f you know never returns.
    #[arg(verbatim_doc_comment, long = "ignore-background", value_name = "RE")]
    pub ignore_background: Vec<String>,
}

/// `csift status`: one-shot liveness verdict for a session.
#[derive(Args, Debug)]
#[command(
    about = "One-shot LIVE verdict on a session (running, waiting, idle or dead) and its evidence",
    long_about = "Answers \"has this session truly stopped?\" RIGHT NOW, as one verdict joined from\n\
        three sources.\n    \
            registry   the harness's own session registry, `<claude-home>/sessions/<pid>.json`.\n               \
                       Its status transitions land within a second.\n    \
            tail       the transcript's tail state machine. An unpaired shell or tool call at\n               \
                       the tail is a tool in flight, and the last assistant record's\n               \
                       stop_reason is read too.\n    \
            process    whether the owner process is alive. A `ps`-based probe answers, falling\n               \
                       back to `/proc/<pid>` where ps lacks the flags, with a process-start-time\n               \
                       guard against pid reuse.\n\n\
        Child transcripts and workflow journals join in by default (the span law), and the\n\
        elicitation sidecar covers the human-in-the-loop states. Each child lane is classified\n\
        in one of three states, and settled lanes FOLD to a count so live work stays visible.\n    \
            in-flight    an unreturned call\n    \
            generating   a tail record younger than 300s and no end_turn yet. A long\n                 \
                         generation writes nothing for minutes, so recency plus a non-final\n                 \
                         stop_reason is the live signal.\n    \
            settled      the rest\n\
        The session's harness task list (`<claude-home>/tasks/`) renders its open tasks,\n\
        in_progress first and then pending, with their blockers. Completed tasks fold to a\n\
        count.\n\n\
        BACKGROUND TASKS\n  \
          A `Bash` call launched with run_in_background returns its tool_result within\n  \
          milliseconds, so the tail machine pairs it at once. That flag is only ONE of four\n  \
          ways into the background. Claude Code also moves a command the model ran in the\n  \
          FOREGROUND into the background when you press ctrl+b on it, when it hits its\n  \
          timeout, or to let a queued message reach the model. On those three the launching\n  \
          tool call carries no flag and is never rewritten, so the RECEIPT sentence is the\n  \
          only trace. `status` reads it and counts the task like any other. Each row says\n  \
          which door it came through (`entered by ctrl+b`, `entered by timeout after 2m`,\n  \
          `entered to deliver a message`), and so does JSON `entered_by`.\n\n  \
          A row whose launching record cannot be found keeps the receipt instant and says so.\n  \
          It never invents a launch time. A receipt counts only as a whole sentence, meaning\n  \
          the clause, a real `b`+8 task id and that arm's own closing clause, so a transcript\n  \
          that merely renders the template cannot fabricate a task. A fifth way in, a\n  \
          plugin's turn abort, writes the ordinary sentence and stays invisible.\n\n  \
          The harness itself writes NOTHING about a still-running shell at end of turn. The\n  \
          REPL's \"N shell still running\" lives in process memory. The end-of-turn record\n  \
          carries a duration and a message count, and its pending counts cover background\n  \
          AGENTS only. `status` therefore scans the whole main transcript, which holds the\n  \
          launches from every lane. Completions normally land in the main file. One\n  \
          addressed to the owning agent lands in that agent's lane, which is read too.\n\n  \
          For every OPEN task it lists the kind (shell, agent or monitor), the id, the launch\n  \
          instant and age, the description or command, and the output file's size and last\n  \
          write. Closed tasks fold to one count per state. The states are completed, failed,\n  \
          killed, stopped and timed out, plus blocked (the remote-agent notifier's fifth\n  \
          value) and any unknown status, which is disclosed and never booked as completed.\n\n  \
          A Monitor is armed as an immediately-paired tool call and shares the shell id\n  \
          namespace. Its event pulses never close it. Only its termination notice or its\n  \
          timeout does, and a PERSISTENT monitor never returns by design, so name it with\n  \
          --ignore-background.\n\n  \
          NOT RETURNED IS NOT PROOF OF RUNNING. Claude Code's own orphan summary says a UI\n  \
          stop, a Monitor timeout or agent teardown leaves no transcript marker, and it\n  \
          reconciles only at the next session start. A long session commonly carries several\n  \
          to dozens of dangling or days-old tasks.\n\n\
        THE LENS\n  \
          The lens decides which open tasks COUNT toward the verdict. Every task is still\n  \
          listed, and an excluded one is marked with its rule. With no lens, everything\n  \
          counts.\n    \
            --background-since WHEN   counts only tasks launched at or after WHEN (the shared\n                              \
                                      time grammar, or `now` for this command's start instant)\n    \
            --ignore-background RE    excludes tasks whose command or description matches RE\n                              \
                                      (repeatable), such as the dev server or the watcher\n\n\
        VERDICTS (a closed set)\n    \
            `running`                generating, or a tool in flight\n    \
            `waiting-children`       the main lane is idle and subagent or workflow lanes\n                             \
                                     are live\n    \
            `waiting-hitl`           blocked on a human by a pending AskUserQuestion,\n                             \
                                     ExitPlanMode or MCP elicitation\n    \
            `idle-background-open`   the turn ended, but N background tasks the lens counts\n                             \
                                     have not returned. They are neither running nor\n                             \
                                     stopped, and csift cannot tell whether that is by design.\n    \
            `idle-eot`               truly stopped at end of turn\n    \
            `stale-dead`             the owning process is gone. The tail shape then says\n                             \
                                     HOW it died.\n    \
            `unknown`                the evidence is insufficient or contradictory. That is\n                             \
                                     stated, never guessed.\n  \
          Every verdict lists the evidence rows that produced it.\n\n\
        LAST MESSAGES\n  \
          The person's newest words and the newest assistant message print as excerpts,\n  \
          clipped with an explicit `(+N chars)` marker. The whole turn is\n  \
          `csift show @<id> --turn -1`.\n\n  \
          The person's words are the newest record labelled `user.message`, `user.answer` or\n  \
          `user.rejection`, whether or not it opened a turn. That covers a typed prompt, a\n  \
          slash command typed with arguments (shown as `/name args`), a message relayed or\n  \
          queued while the session worked, an answer to an AskUserQuestion, and a\n  \
          rejection's typed instruction, each in the text `search` renders for that leaf. A\n  \
          machine trigger (a `<task-notification>` pulse, a fired schedule) is not the\n  \
          person's words and never takes the slot.\n\n  \
          WARNING for a model reading this. The excerpt is a PARTIAL view of the final state,\n  \
          useful only for judging whether a background task is still meaningful. The message\n  \
          may still be waiting on a task that will never return. In any orchestration it must\n  \
          NEVER be read as a complete or near-complete review of the work. Counting tool calls\n  \
          and reading the last message is a check shallower than any human would accept, and\n  \
          trust built on it violates the guardrail. A model with a partial context tends to\n  \
          believe it read everything, even past an explicit tool error.\n\n\
        THIS IS A LIVE-TRUTH COMMAND\n  \
          It is point-in-time and explicitly NON-reproducible, a deliberate and documented\n  \
          departure from the forensic contract. search, show and stats stay reproducible.\n  \
          status answers \"now\" and says so. It reads outside projects/ (the registry) and\n  \
          honors --claude-home.\n\n\
        HONESTY LIMITS (stated in the output when they bite)\n  \
          - A pending PERMISSION prompt leaves no transcript trace. Only a CURRENT registry\n    \
            row shows it, as status `waiting`, which the harness sets for any blocking dialog\n    \
            (a question, a permission prompt, a plan approval, a sandbox or worker request).\n    \
            That row is transition-written, so only the pid probe refutes a stale one.\n  \
          - A multi-question AskUserQuestion was measured on disk while its dialog was open,\n    \
            and the tail reads it as hitl. A single-question one stays buffered until it is\n    \
            answered. The difference is a timing outcome of the write frontier, and no rule\n    \
            keys on the question count. The sidecar covers the buffered shape.\n  \
          - The registry covers top-level sessions only. A subagent target degrades to tail\n    \
            and pid evidence, and never gets a fabricated row.\n  \
          - The pid probe is `ps` (plus `/proc` on Linux) on unix, and PowerShell\n    \
            `Get-Process` (plus `tasklist`) on Windows, where `procStart` is a FILETIME tick\n    \
            count. A row written in another pid domain (`pidDomain`) is never probed, and the\n    \
            verdict says so.",
    after_help = "EXAMPLES\n  \
          csift status @<uuid>                    # is it truly stopped?\n  \
          csift status @main                      # my own session (env)\n  \
          csift status @<uuid> --format json | jq -r 'select(.kind==\"verdict\").verdict'\n  \
          csift status @<uuid> --no-subagents     # the main lane only\n  \
          csift status @<uuid> --ignore-background 'npm run dev'   # the dev server never returns\n  \
          csift status @<uuid> --background-since 2h              # only recent launches count\n\n\
        JSON SCHEMA (--format json)\n  \
          {kind:\"header\", command:\"status\", session_id, is_subagent, parent_session_id}\n  \
          then one {kind:\"verdict\"} row:\n    \
            verdict\n    \
            evidence:[{surface, value, age_secs}]\n    \
            children:[{session_id, state, detail}]       live lanes only\n    \
            settled_children\n    \
            tasks:[{id, subject, status, blocked_by}]    null when the session has no tasks dir\n    \
            tasks_completed\n    \
            tasks_stores:[{dir, via}]\n    \
            last_checkpoint:{line, kind}|null\n    \
            pending:[...]\n    \
            background:{open, ignored, completed, failed, killed, stopped, timed_out, blocked,\n                \
                        other, scanned_files, tasks:[...], notes:[...]}\n      \
              background.tasks, open tasks only: {kind, id, tool_use_id, lane, state,\n        \
                entered_by, timed_out_after_ms, launch_note, description, command,\n        \
                launched_utc, launched_local, age_secs, output_file, output_bytes,\n        \
                output_age_secs, ignored_by}\n      \
              entered_by is model|user|timeout|deliver-message, and null for an agent or a\n      \
              monitor\n    \
            last:{user:{ts_utc, ts_local, text, truncated}|null, agent:{...}|null}\n    \
            tail_state\n    \
            notes:[...]\n  \
          then {kind:\"summary\", verdict}.\n\n\
        A CHECKPOINT AT THE TAIL, AND THE TASK STORE A CLEAR LEFT BEHIND\n  \
          When the transcript's LAST line is a `cost-state` line (the cost ledger's\n  \
          checkpoint), an evidence row names it and `last_checkpoint` carries its line. It is\n  \
          EVIDENCE, never a verdict. The harness writes one at a clear, a background handover,\n  \
          an in-app resume and at exit, and most such lines sit mid-file. All a tail one says\n  \
          is that nothing was appended after it. Liveness stays the registry row's answer, and\n  \
          the verdict set is unchanged.\n\n  \
          The tasks section reads the same store it always did. That store is named after the\n  \
          id the PROCESS started with, and a `/clear` never renames it. When the session's own\n  \
          id finds nothing, csift tries the root of the `cleared_from` chain (see\n  \
          `csift list`), then the team file written within 5s of the registry row's\n  \
          startedAt. Each store that answered prints as `tasks store: <dir> (via <candidate>)`,\n  \
          and an EMPTY one is marked `- empty`. The directory existing answers where the list\n  \
          came from, whether or not it holds anything.\n\n  \
          One pre-existing JSON key moves with this. A session whose store is found through\n  \
          the root or the team file reports its tasks where an older csift reported none, so\n  \
          `tasks` goes from null to an array and `tasks_completed` from null to a count.\n\n\
        SEE ALSO\n  \
          csift whoami --peers        every live lane at once, as `id kind state` only\n  \
          csift whoami --to @<lane>   liveness turned into a reach prediction for one lane\n  \
          csift send @<lane> \"…\"      queue a message; this verdict is what it reasons over\n  \
          csift agents @<uuid>        the lane tree below a session, live and settled alike"
)]
pub struct StatusArgs {
    /// ONE session: `@<uuid>`, `@<uuid-prefix>`, `@main`, `@<agent-id>` or a `*.jsonl`
    /// path. The grammar is the one every command takes, with exactly one target.
    #[arg(
        verbatim_doc_comment,
        value_name = "TARGET",
        value_parser = parse_project_target,
        allow_hyphen_values = true,
        required = true,
        num_args = 1..
    )]
    pub target: Vec<std::path::PathBuf>,

    /// Exclude subagent transcripts from the verdict, leaving the main lane only.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Span subagent transcripts. This is the DEFAULT, and the flag exists so the pair is
    /// uniform.
    #[arg(
        verbatim_doc_comment,
        long = "subagents",
        conflicts_with = "no_subagents"
    )]
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
    about = "Block until a condition fires (stop, HITL, a tool call, a notification) and name it",
    long_about = "The monitor half of the live-truth pair. `wait` polls a session until one of the\n\
        `--until` conditions fires, then exits 0 naming it (JSON carries its evidence). If the\n\
        REQUIRED --timeout elapses first, it exits 124 with a report of what the session did\n\
        meanwhile.\n\n\
        BASELINE SEMANTICS\n  \
          This is the difference between a monitor and a query. At startup `wait` snapshots\n  \
          every watched file's byte length and observes ONLY events strictly AFTER those\n  \
          offsets. A condition already satisfied by history alone never fires, because history\n  \
          is `search`'s job. After the snapshot one readiness line prints to stderr\n  \
          (`csift: watching N file(s) ...`), so a scripted caller can order its own actions\n  \
          after the baseline.\n\n\
        CONDITIONS (repeatable, OR-ed, the first hit wins)\n  \
          stop                      the verdict becomes idle-eot or stale-dead, a TRUE stop.\n                            \
                                    `idle-background-open` never satisfies it. Narrow the\n                            \
                                    lens, or let the timeout elapse and read the report.\n  \
          hitl                      the verdict becomes waiting-hitl\n  \
          auq                       the elicitation sidecar gains an unanswered question\n  \
          notification[:REGEX]      a task-notification lands in any watched lane\n  \
          tool:NAME[:REGEX]         a tool_use of NAME whose serialized input matches\n  \
          write:PATH_RE[:LINE_RE]   a Write/Edit whose path matches (and a content line, if\n                            \
                                    given)\n  \
          verdict:V                 any verdict from the status table\n\n  \
          Regexes use the same RE2-class engine as search (no backrefs, no lookaround).\n  \
          Polling is adaptive, from a 200ms floor backing off toward 2s when quiet, and\n  \
          `--interval` pins it. Reads are incremental (only appended bytes) and guarded\n  \
          against a torn tail.\n\n\
        --timeout IS REQUIRED\n  \
          A background task can be designed never to return (a dev server, a watcher, a\n  \
          tail -f), so an unbounded wait on `stop` is a bug. A call without --timeout is\n  \
          rejected with that reason.\n\n\
        A SHELL THE MODEL RAN IN THE FOREGROUND CAN HOLD `stop` OPEN\n  \
          Ctrl+b, a timeout or a message delivery moves such a shell into the background. It\n  \
          then counts like any other open task, and its row and JSON carry `entered_by`. If\n  \
          that is the task you are willing to leave running, name it with --ignore-background\n  \
          or narrow --background-since. Do not read the 124 as a stall.\n\n\
        HOW TO WAIT ON A SESSION, IN ORDER\n  \
          1. Run `csift status @<id>` first and read the background rows. They show which\n     \
             tasks dangle, how old they are, and which are days-old zombies.\n  \
          2. Pick the lens. `--background-since now` counts only what launches after you\n     \
             start waiting (the usual form). `--ignore-background RE` names the services you\n     \
             know never return.\n  \
          3. Always set --timeout, and branch on exit 124.\n  \
          4. On timeout, read the report. It carries `at exit` (in a tool call for N s,\n     \
             generating, or idle), `activity` (the tool calls by name, thinking, messages,\n     \
             prompts and notifications that landed while you waited), the open background\n     \
             rows, and the last messages. Read them under the LAST MESSAGES warning in\n     \
             `csift status --help`.\n\n\
        EXIT CODES\n  \
          0       a condition fired\n  \
          124     --timeout elapsed. This follows the GNU timeout convention and is the ONE\n          \
                  documented exception to the crate's 0-vs-non-zero law, because a monitor's\n          \
                  timeout is a normal outcome a script must branch on.\n  \
          other   any other non-zero exit is an error",
    after_help = "EXAMPLES\n  \
          csift wait @<uuid> --until stop --timeout 300      # block until truly stopped (or 5m)\n  \
          csift wait @main --until auq --timeout 600         # fire when a question lands\n  \
          csift wait @<uuid> --until tool:Read:handover --timeout 600  # ...until it reads that file\n  \
          csift wait @<uuid> --until stop --until hitl --timeout 600 --format json | jq .fired\n  \
          csift wait @<uuid> --until stop --timeout 900 --background-since now \\\n      \
              --ignore-background 'npm run dev'\n\n\
        JSON SCHEMA (--format json)\n  \
          One object on exit:\n    \
            {kind:\"wait\", fired, verdict, waited_secs, at_exit,\n     \
             activity:{records, lanes, tools:{name: count}, thinking, agent_messages,\n               \
                       user_prompts, notifications},\n     \
             evidence:[...], background:{... as status}, last:{... as status}, notes:[...]}\n  \
          `fired` is the condition string, or \"timeout\"."
)]
pub struct WaitArgs {
    /// ONE session, in the same grammar as `status`.
    #[arg(
        value_name = "TARGET",
        value_parser = parse_project_target,
        allow_hyphen_values = true,
        required = true,
        num_args = 1..
    )]
    pub target: Vec<std::path::PathBuf>,

    /// A condition to wait for (repeatable, OR semantics, the first hit wins).
    #[arg(long = "until", value_name = "COND", required = true)]
    pub until: Vec<String>,

    /// REQUIRED. Give up after SECS seconds and exit 124 (the GNU timeout convention), with
    /// JSON `fired:"timeout"` and the at-exit report. A background task may never return by
    /// design, so a wait without a bound is rejected.
    #[arg(verbatim_doc_comment, long, value_name = "SECS")]
    pub timeout: Option<u64>,

    /// Poll interval in milliseconds. The default is adaptive, from a 200ms floor backing
    /// off to 2s.
    #[arg(verbatim_doc_comment, long, value_name = "MS")]
    pub interval: Option<u64>,

    /// Exclude subagent transcripts from the watch set.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Span subagent transcripts. This is the DEFAULT, and the flag exists so the pair is
    /// uniform.
    #[arg(
        verbatim_doc_comment,
        long = "subagents",
        conflicts_with = "no_subagents"
    )]
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
