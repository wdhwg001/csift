//! The `long_about` of `csift search --help`: the description clap prints above the usage
//! line, before the argument list and the after-help body this module's siblings assemble.
//! It lives beside them because it is the same page's prose, and because the argument
//! docs in `search_args.rs` fill that file to its line budget on their own.

/// The `long_about` of `csift search --help` (see the module doc).
pub(crate) const SEARCH_LONG_ABOUT: &str = "Regex-search transcripts, returning the COMPLETE round-trip exchange \
        containing each hit; never a bare fragment. A turn is delimited by a genuine \
        user message; the emitted exchange is the whole turn (the opening user record \
        plus every assistant/thinking/tool_use/tool_result record chained under it \
        until the next genuine user). So a matched tool_use comes WITH its \
        tool_result, a matched user turn WITH the agent's response, etc.\n\n\
        The PATTERN is ripgrep-like and defaults to SMART-CASE: case-insensitive \
        unless it spells an uppercase letter - a literal, or a character in a bracket \
        class such as `[A-Z]`, while an escape like `\\S`, `\\W`, `\\D` or `\\B` is no \
        letter; `-i` forces case-insensitive and always wins. `--multiline` lets `.` cross newlines. An EMPTY pattern is a \
        pure filter; it matches every label-eligible record, so combine it with \
        `--label` / `--since` / `--turn` (a bare empty pattern with no other \
        filter warns that it will emit a lot).\n\n\
        LABELS (`-t`, repeatable): a dotted `role.category.leaf` SELECTOR - 3 roles, 17 \
        categories, 73 leaves. A selector matches a record label iff it is a dot-SEGMENT prefix \
        of the label's path, so `-t agent.tool` covers use+result and `-t agent` the whole role. \
        The categories: user | agent.{message,thinking,tool,communication} | \
        harness.{notification,compaction,command,interrupt,schedule,resume,hook,context,enforce,\
        turn,system,bookkeeping}; `csift search --help` lists every leaf under each. A bare ROLE \
        or CATEGORY selects only the leaves the model RECEIVES; a GLOB (`-t 'user.*'`) or an \
        exact LEAF reaches every record. With none given, every leaf a default scan reaches is \
        eligible; a gated leaf still needs its own `-t` (see the leaf tree). `-T`/\
        `--label-not` EXCLUDES with the same grammar (the rg -t/-T duality): the effective set \
        is (-t selectors, or ALL) minus (-T selectors), and ONLY a -T that cancels your -t is a \
        hard error. `harness.bookkeeping` is the one category whose whole face is invisible: \
        the bare form is accepted, matches nothing by the law, and says so with the remedy. \
        Every retired spelling is a hard error naming its successors - `harness.meta` and every \
        `harness.meta.*` path, the flat `thinking`/`tool`/`tool-response`, \
        `harness.schedule.continuation`. The human turn is \
        `user.message`; an AskUserQuestion answer is `user.answer` (the full Q+options+answer \
        unit); a plan-rejection-with-message is `user.rejection` (+ a [plan: …] pointer). An \
        inbound peer/teammate message is `agent.communication.inbox` (NOT `user`); a \
        `<task-notification>` automation pulse is `harness.notification.*` (NOT `user`). A \
        tool named after harness machinery still classifies by ROLE: a `ScheduleWakeup` CALL \
        (arming a timer) is `agent.tool.use` like any other tool: `harness.schedule.wakeup` \
        is only the FIRED tick, the harness-injected marker-carrying wakeup prompt. A \
        custom-prompt tick carries no marker at all: it lands under `harness.schedule.fire`, \
        the prompt a scheduled task fires, recognised by its parent link to the scheduler's \
        own fire record rather than by its text.\n\n\
        AUTOMATION TRIGGERS: a `<task-notification>` (a background-command / workflow / \
        spawned-agent / monitor COMPLETION pulse Claude Code injects as a `type:\"user\"` record) \
        OPENS a turn like a human message but classifies under `harness.notification.<kind>` \
        (kind = background-command | workflow | subagent | monitor | task, read from the \
        summary). It renders as the parsed `[<kind> <task-id>[, <task-id>…] <status>] <summary>` \
        attribution label - every task the pulse closes, never the raw `<task-id>`/`<output-file>` \
        XML, and never the internal scan marker, which becomes a trailing \
        `(orphan reconciliation: <kind>)` instead (JSON `task_ids` / `orphan_kind`). Match it like \
        any other text (e.g. `search 'background-command' -t harness.notification.background-command`).\n\n\
        WINDOWING: `--turn` takes the shared range grammar: `N` (one turn) · `A..B` \
        (closed) · `N..` (turn N → the end) · `..N` (start → N) · `-k` = k-th FROM THE END \
        (`-3..` = the last 3 turns), 0-based on turn-boundary order, and INTERSECTS with \
        `--since`/`--until` (both filters AND). Time bounds accept ISO8601 (`2026-06-01` = local \
        midnight · `2026-06-01T05:00:00` BARE = that LOCAL wall-clock time · \
        `2026-06-01T05:00:00Z`/`…+10:00` = explicit zone) or a relative form (`2h`, `3d`, `90m`, `45s`, `1w`, `2mo`, `1y`; a leading `-` is tolerated) meaning \
        \"that long ago\" in the system local timezone.\n\n\
        ZERO MATCHES IS A DEFINITIVE ANSWER, NOT A FAILURE: a no-match search prints a stderr \
        diagnosis: \"DEFINITIVE absence (exit 0), NOT an error\", the active filters, and (when a \
        `-t`/`-T` filter was on) an active probe that NAMES the label(s) the pattern DOES occur \
        under (e.g. it was excluded by your `-t user.message` but occurs under `agent.tool.use`). \
        Under `--turn` it also counts the records the pattern and the label filter match OUTSIDE \
        turn numbering - a recalled draft, a rewound turn, a record on an abandoned branch, \
        which a turn window leaves out - on a line of its own, printed first when there are \
        any, with the two ways to reach them: drop `--turn`, or window by time with \
        `--since`/`--until`. The terminal modes (`-c`, `-l`, `--count-by`) print the same \
        stderr lines. Read the diagnosis and adjust the filter; do NOT assume a syntax error. To SEE a scope's \
        record-types before you filter, run `--count-by label` (a per-leaf census; with an empty \
        pattern it censuses the whole scope).\n\n\
        `--max-count` caps emitted exchanges but reports the dropped count (default: \
        unlimited; no cap); there is NO silent truncation anywhere.";
