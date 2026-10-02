//! StatusArgs + WaitArgs - the live-truth command pair.

use super::*;

/// The background-task LENS shared by `status` and `wait`: which OPEN background tasks
/// count toward the verdict (and toward `--until stop`). Every task is still listed;
/// an excluded one is marked with the rule that excluded it.
#[derive(Args, Debug, Default, Clone)]
pub struct BackgroundLensArgs {
    /// Count only background tasks launched at or after WHEN, and list earlier ones as
    /// ignored. WHEN takes the shared time grammar (`2026-09-02`, `2h`, `1d`, `2mo`, `-30m`)
    /// or `now`, this command's own start instant. With `now`, every task already dangling
    /// when the command started is ignored, which is what a session supervising another one
    /// usually wants from `wait`.
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
        A lane is one running Claude Code conversation, the top-level session or one subagent,\n\
        each with its own transcript. Child transcripts and workflow journals join in by default,\n\
        as on every command that spans subagents, and `--no-subagents` leaves the main lane only.\n\
        The elicitation file covers the human-in-the-loop (HITL) states, where a dialog blocks\n\
        the session on a person. It is `elicitations.jsonl` in the session's sidecar directory,\n\
        the `<session-id>/` folder beside its transcript. Each child lane is classified in one of\n\
        three states, and settled lanes FOLD to a count so live work stays visible.\n    \
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
          which way the task entered (`entered by ctrl+b`, `entered by timeout after 2m`,\n  \
          `entered to deliver a message`), and so does JSON `entered_by`.\n\n  \
          A row whose launching record cannot be found keeps the receipt instant and says so.\n  \
          It never invents a launch time. csift counts a receipt only when the whole sentence\n  \
          is there, from its opening words through a real task id (`b` plus 8 characters) to\n  \
          the closing words that kind of receipt ends with. A transcript that only quotes the\n  \
          receipt wording, with a placeholder for the id, therefore cannot fabricate a task. A\n  \
          fifth way in, a plugin's turn abort, writes the ordinary sentence and stays invisible.\n\n  \
          The harness itself writes NOTHING about a still-running shell at end of turn. The\n  \
          REPL's \"N shell still running\" lives in process memory. The end-of-turn record\n  \
          carries a duration and a message count, and its pending counts cover background\n  \
          AGENTS only. `status` therefore scans the whole main transcript, which holds the\n  \
          launches from every lane. Completions normally land in the main file. One\n  \
          addressed to the owning agent lands in that agent's lane, which is read too.\n\n  \
          For every OPEN task it lists the kind (shell, agent or monitor), the id, the launch\n  \
          instant and age, the description or command, and the output file's size and last\n  \
          write. Closed tasks fold to one count per state. The states are completed, failed,\n  \
          killed, stopped and timed out, plus blocked, which only a remote agent's completion\n  \
          notice reports, and any unknown status, which is disclosed and never booked as\n  \
          completed.\n\n  \
          A Monitor is armed as an immediately-paired tool call and shares the shell id\n  \
          namespace. Its event pulses never close it. Only its termination notice or its\n  \
          timeout does, and a PERSISTENT monitor never returns by design, so name it with\n  \
          --ignore-background.\n\n  \
          AN OPEN TASK MAY ALREADY BE DEAD. Claude Code's own orphan summary says a UI stop, a\n  \
          Monitor timeout or agent teardown leaves no transcript marker, and it reconciles\n  \
          those tasks only at the next session start. A long session commonly carries several\n  \
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
            `idle-eot`               truly stopped at the end of turn (EOT, the assistant's\n                             \
                                     end_turn record)\n    \
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
          and reading the last message is a check shallower than any human would accept, so\n  \
          no trust in the work may rest on it. A model with a partial context tends to\n  \
          believe it read everything, even past an explicit tool error.\n\n\
        THIS IS A LIVE-TRUTH COMMAND\n  \
          It is point-in-time and explicitly NON-reproducible. search, show and stats give the\n  \
          same answer every time they read the same files. status answers \"now\" and says so.\n  \
          It reads outside projects/ (the registry) and honors --claude-home.\n\n\
        HONESTY LIMITS (stated in the output when they bite)\n  \
          - A pending PERMISSION prompt leaves no transcript trace. Only a CURRENT registry\n    \
            row shows it, as status `waiting`, which the harness sets for any blocking dialog\n    \
            (a question, a permission prompt, a plan approval, a sandbox or worker request).\n    \
            That row is transition-written, so only the pid probe refutes a stale one.\n  \
          - A multi-question AskUserQuestion was measured on disk while its dialog was open,\n    \
            and the tail reads it as waiting-hitl. A single-question one stays buffered until\n    \
            it is answered. The difference depends on when Claude Code writes the record to\n    \
            disk, and no rule keys on the question count. The elicitation file covers the\n    \
            buffered shape.\n  \
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
          The tasks section reads the harness's task store, which is named after the id the\n  \
          PROCESS started with, and a `/clear` never renames it. When the session's own\n  \
          id finds nothing, csift tries the root of the `cleared_from` chain (see\n  \
          `csift list`), then the team file written within 5s of the registry row's\n  \
          startedAt. Each store that answered prints as `tasks store: <dir> (via <candidate>)`,\n  \
          and an EMPTY one is marked `- empty`. The directory existing answers where the list\n  \
          came from, whether or not it holds anything. A store found through the chain root or\n  \
          the team file fills `tasks` and `tasks_completed` like the session's own, and with no\n  \
          store found both are null.\n\n\
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
          At startup `wait` snapshots every watched file's byte length and observes ONLY events\n  \
          strictly AFTER those offsets. A condition already satisfied by history alone never\n  \
          fires, because history is `search`'s job. After the snapshot one readiness line\n  \
          prints to stderr (`csift: watching N file(s) ...`), so a scripted caller can order its\n  \
          own actions after the baseline.\n\n\
        CONDITIONS (repeatable, OR-ed, the first hit wins)\n  \
          A lane is one running Claude Code conversation, the top-level session or one\n  \
          subagent, each with its own transcript, and `wait` watches the target's lanes.\n  \
          stop                      the verdict becomes idle-eot (stopped at the end of turn)\n                            \
                                    or stale-dead, a TRUE stop. `idle-background-open`\n                            \
                                    never satisfies it. A narrower lens lets it fire. The\n                            \
                                    lens is `--background-since` and `--ignore-background`,\n                            \
                                    which decide which open background tasks count. Without\n                            \
                                    one, the timeout elapses and the report shows what\n                            \
                                    stayed open.\n  \
          hitl                      the verdict becomes waiting-hitl (HITL, human in the\n                            \
                                    loop, where a dialog blocks the session on a person)\n  \
          auq                       the elicitation file (`elicitations.jsonl` in the\n                            \
                                    session's `<session-id>/` folder) gains an unanswered\n                            \
                                    AskUserQuestion (AUQ)\n  \
          notification[:REGEX]      a task-notification lands in any watched lane\n  \
          tool:NAME[:REGEX]         a tool_use of NAME whose serialized input matches\n  \
          write:PATH_RE[:LINE_RE]   a Write/Edit whose path matches (and a content line, if\n                            \
                                    given)\n  \
          verdict:V                 any verdict `csift status --help` lists\n\n  \
          Regexes take the same syntax as search (RE2-class, no backrefs, no lookaround).\n  \
          Polling is adaptive, from a 200ms floor backing off toward 2s when quiet, and\n  \
          `--interval` pins it. Reads are incremental (only appended bytes) and guarded\n  \
          against a torn tail.\n\n\
        --timeout IS REQUIRED\n  \
          A background task can be designed never to return (a dev server, a watcher, a\n  \
          tail -f), so a wait on `stop` with no bound might never end. A call without\n  \
          --timeout is rejected with that reason.\n\n\
        A SHELL THE MODEL RAN IN THE FOREGROUND CAN HOLD `stop` OPEN\n  \
          Ctrl+b, a timeout or a message delivery moves such a shell into the background. It\n  \
          then counts like any other open task, and its row and JSON carry `entered_by`. A\n  \
          task that may stay running is left out of the count by --ignore-background or a\n  \
          narrower --background-since. Without that, a 124 exit means the task was still open\n  \
          when the timeout elapsed, and the session itself may not have stalled.\n\n\
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
                  exit code outside csift's 0-or-non-zero rule, because a monitor's timeout\n          \
                  is a normal outcome a script must branch on.\n  \
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
