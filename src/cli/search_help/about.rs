//! The `long_about` of `csift search --help`: the description clap prints above the usage
//! line, before the argument list and the after-help body this module's siblings assemble.
//! It lives beside them because it is the same page's prose, and because the argument
//! docs in `search_args.rs` fill that file to its line budget on their own.

/// The `long_about` of `csift search --help` (see the module doc).
pub(crate) const SEARCH_LONG_ABOUT: &str =
    "Regex-search transcripts and return the COMPLETE round-trip exchange around \
        each hit,\nnever a bare fragment. An exchange is one whole turn. It holds the record \
        that opened\nthe turn plus every assistant, thinking, tool_use and tool_result record \
        chained under\nit, up to the next turn opener (the `--turn` flag lists the openers). A \
        matched tool_use\ntherefore comes with its tool_result, and a matched user turn comes \
        with the agent's\nresponse.\n\n\
        The PATTERN is ripgrep-like and SMART-CASE by default. It matches case-insensitively\n\
        unless it spells an uppercase letter, as a literal or inside a bracket class such as\n\
        `[A-Z]`. An escape like `\\S`, `\\W`, `\\D` or `\\B` is no letter. `-i` forces \
        case-insensitive\nmatching and always wins. `--multiline` lets `.` cross newlines. \
        An EMPTY pattern is a\npure filter that matches every record the label filter admits \
        (with no `-t`, every leaf\na default scan reaches), so combine it with `--label`, \
        `--since` or `--turn`. A bare\nempty pattern with no other filter warns that it will \
        emit a lot.\n\n\
        LABELS (`-t`, repeatable)\n\
        A label is a dotted `role.category.leaf` path, over 3 roles, 17 categories and 73 \
        leaves.\nA SELECTOR matches a record label when it is a dot-SEGMENT prefix of the \
        label's path.\nUnder that rule `-t agent.tool` covers use and result, and `-t agent` \
        covers the whole\nrole. The categories are these, and the leaf tree below lists every leaf \
        under each:\n\n  \
          user\n  \
          agent.{message,thinking,tool,communication}\n  \
          harness.{notification,compaction,command,interrupt,schedule,resume,hook,context,\n  \
          \x20        enforce,turn,system,bookkeeping}\n\n\
        A selector reaches different records depending on its form.\n\n  \
          a bare ROLE or CATEGORY     only the leaves the model RECEIVES\n  \
          a GLOB (`-t 'user.*'`)      every record under the prefix\n  \
          an exact LEAF               every record of that leaf\n\n\
        With no `-t`, every leaf a default scan reaches is eligible. A gated leaf still \
        needs\nits own `-t` (see the leaf tree's GATE words). `-T`/`--label-not` EXCLUDES \
        with the same\ngrammar, the rg -t/-T duality. The effective set is the -t selectors \
        (or ALL) minus the\n-T selectors, and ONLY a -T that cancels your -t is a hard \
        error. The model receives no\nleaf under `harness.bookkeeping`, so a bare \
        `-t harness.bookkeeping` matches nothing. csift\naccepts it and says so on stderr \
        with the remedy. Every retired spelling is a hard error\nnaming its successors. The \
        retired spellings are `harness.meta` and every `harness.meta.*`\npath, the flat \
        `thinking`/`tool`/`tool-response`, and `harness.schedule.continuation`.\n\n\
        Where some records land:\n\n  \
          the human turn                              `user.message`\n  \
          an AskUserQuestion answer                   `user.answer`, the full \
          Q+options+answer unit\n  \
          a plan rejection with a message             `user.rejection`, plus a [plan: …] \
          pointer\n  \
          an inbound peer or teammate message         `agent.communication.inbox`, NOT \
          `user`\n  \
          a `<task-notification>` automation pulse    `harness.notification.*`, NOT \
          `user`\n\n\
        A tool named after harness machinery still classifies by ROLE. A `ScheduleWakeup` \
        CALL\n(arming a timer) is `agent.tool.use` like any other tool. \
        `harness.schedule.wakeup` is only\nthe FIRED tick, the fixed prompt the harness \
        injects when the timer fires. That prompt\nopens with one of its own marker lines, \
        such as `<<autonomous-loop-dynamic>>` or\n`# Autonomous loop check`. A tick armed \
        with a custom prompt injects that prompt instead\nand carries no marker line. It \
        lands under `harness.schedule.fire`, the prompt a scheduled\ntask fires, which is \
        recognised by its parent link to the scheduler's own fire record and\nnever by its \
        text. A `<task-notification>` pulse OPENS a turn and is labelled by its kind, as\n\
        AUTOMATION TRIGGERS below describes.\n\n\
        WINDOWING\n\
        `--turn` takes the shared range grammar, 0-based on turn-boundary order.\n\n  \
          N       one turn\n  \
          A..B    a closed range\n  \
          N..     turn N → the end\n  \
          ..N     the start → turn N\n  \
          -k      the k-th turn FROM THE END (`-3..` = the last 3 turns)\n\n\
        `--turn` INTERSECTS with `--since`/`--until`, and both filters AND. A time bound \
        is\nISO8601 or a relative form, and a relative form means \"that long ago\" in the \
        system\nlocal timezone.\n\n  \
          2026-06-01                local midnight that day\n  \
          2026-06-01T05:00:00       a BARE datetime, that LOCAL wall-clock time\n  \
          2026-06-01T05:00:00Z      an explicit zone (`…+10:00` works too)\n  \
          2h 3d 90m 45s 1w 2mo 1y   relative forms (a leading `-` is tolerated)\n\n\
        ZERO MATCHES\n\
        A no-match search exits 0 and prints a diagnosis on stderr. It reads \"DEFINITIVE\n\
        absence (exit 0), NOT an error\" and lists the active filters. When a `-t`/`-T` \
        filter was\non, an active probe NAMES the label(s) the pattern DOES occur under. \
        For example, the\npattern was excluded by your `-t user.message` but occurs under \
        `agent.tool.use`. Under\n`--turn` the diagnosis also counts the records the pattern \
        and the label filter match\nOUTSIDE turn numbering, which a turn window leaves out. \
        Those are the records Claude Code's\nconversation chain no longer reaches (the global \
        `--cross-compactions` entry says what\nthe chain is). They are a message the person \
        sent, recalled and edited before resending\nit, a turn the person rewound past, and \
        any record that hangs under one of those. The\ncount gets a line of its own, \
        printed first when there are any, and it names the two\nways to reach them. One is \
        to drop `--turn`, the other to window by time with\n`--since`/`--until`. The \
        terminal modes (`-c`, `-l`, `--count-by`) print the same stderr\nlines. An empty \
        result is never a syntax error, and the diagnosis names the filter to\nchange. \
        `--count-by label` with an empty pattern is a per-leaf census that covers the \
        whole\nscope, which shows its record types before any filter is chosen.\n\n\
        `--max-count` caps the emitted exchanges and reports how many it dropped. The \
        default is\nunlimited, with no cap. Nothing anywhere is truncated silently.";
