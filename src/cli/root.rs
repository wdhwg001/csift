//! Cli root + the Command enum (17 subcommands + the hidden turns tombstone).

use super::*;

/// ripgrep for Claude Code session transcripts.
#[derive(Debug, Parser)]
#[command(
    name = "csift",
    version,
    about = "ripgrep for Claude Code session transcripts",
    long_about = "csift - \"ripgrep for Claude Code session transcripts\".\n\n\
        csift lists and regex-searches the session transcripts Claude Code writes as `.jsonl`\n\
        files under ~/.claude/projects/<encoded>/. It is a fast Rust CLI built for an LLM\n\
        consumer: a Claude Code agent searching or recovering its own session or a peer's.\n\
        Output is clean, token-efficient and regex-driven. Text is the default, readable by a\n\
        person or a model, and `--format json` is the machine form.\n\n\
        Matching is pure regex, as in ripgrep. There is no BM25, no embedding and no semantic\n\
        search, by design.\n\n\
        A lane is one running Claude Code conversation, the top-level session or one subagent,\n\
        each with its own transcript.\n\n\
        SUBCOMMANDS\n  \
          list     a fast \"which session is this?\" index giving per session its times, the\n           \
                   first and last message the person typed, and the last agent message\n  \
          search   regex search over transcripts that returns the complete round-trip\n           \
                   exchange of each hit\n  \
          show     fetch the records you name in ONE transcript, by `--line`, `--turn` or\n           \
                   `--uuid`, rendered in full or as `--raw` bytes. `--turn -3..` fetches the\n           \
                   last 3 turns (the live-tail peek), and `--branch-points` lists where the\n           \
                   conversation forked.\n  \
          stats    one-scan aggregates per session, counting records, turns, tool calls by\n           \
                   name, tokens by model, time span, compactions, and the turns that were\n           \
                   abandoned or rewound\n  \
          whoami   identify the calling Claude Code session from $CLAUDE_CODE_SESSION_ID, or\n           \
                   from $CODEX_COMPANION_SESSION_ID when that is unset\n  \
          agents   list a session's subagents with kind, start and completion time and\n           \
                   status, filtered by a time window\n  \
          files    the files and directories a session modified, and when (Edit, Write and\n           \
                   Notebook calls, plus a heuristic reading of Bash)\n  \
          recover  reconstruct a file's history from the transcript as segmented\n           \
                   diff-patches, a point-in-time partial snapshot, or a coverage scope\n  \
          plan     locate the Plan-Mode plan file bound to a session. That is the path its\n           \
                   `plan_mode` attachment names or, with no attachment, the file named by the\n           \
                   first plan slug its records carry (the first-slug law).\n           \
                   `recover --file @plan` prints the plan's text.\n  \
          verbatim restore, within a character budget, the verbatim user/assistant\n           \
                   back-and-forth a compaction summary CLIPPED. The live-tail peek is\n           \
                   `show --turn`.\n  \
          image    list the inline images a session carries (pastes and screenshots) and\n           \
                   extract them to files\n  \
          status   a one-shot LIVE verdict on a session, with the evidence named. The verdict\n           \
                   is one of running, waiting-children, waiting-hitl (a dialog waits on a\n           \
                   person, so a human is in the loop), idle-background-open, idle-eot (idle at\n           \
                   the end of a turn), stale-dead or unknown.\n  \
          wait     block until a session condition fires. The conditions are stop, hitl,\n           \
                   auq (an AskUserQuestion is waiting for its answer), notification, tool:,\n           \
                   write: and verdict:. Exits 124 when --timeout elapses.\n  \
          send     queue a message for ONE lane on the csift channel, carried by a\n           \
                   `csift deliver` hook the user installed. Each send prints one verdict.\n           \
                   An official send is printed for you to make and never performed.\n  \
          msg      reconcile one csift-channel message, joining the INTENT in csift's ledger\n           \
                   (`csift-channel/ledger/<lane>.jsonl`) to the receiver transcript's FACT in\n           \
                   one verdict. With no id it lists the lane's ledger.\n  \
          ack      record that the calling lane READ a channel message (appends an ack line)\n  \
          deliver  the hook entry. Claude Code calls it through the installed hook. By hand\n           \
                   you run only `--recipe`, which prints the block you paste.\n\n\
        Most commands read a session's subagent transcripts too. Those live in the session's\n\
        sidecar directory, the `<session-id>/` folder beside its transcript, which also holds\n\
        externalised tool output (`tool-results/`), the elicitation file (`elicitations.jsonl`)\n\
        and csift's channel files (`csift-channel/`). Agents the Task or Agent tool started sit\n\
        under `subagents/`, and workflow agents under `subagents/workflows/<run>/`. list, search,\n\
        stats, files, recover, plan, image, status and wait span them by default, and\n\
        `--no-subagents` restricts them to top-level sessions. `verbatim` is the exception among\n\
        the file-operating commands. It reads only the TOP-LEVEL thread by default (single-thread\n\
        recovery), opts INTO spanning with `--subagents`, and REQUIRES a target. `agents` LISTS\n\
        subagents as its targets instead of spanning them as inputs, so it rejects both span\n\
        flags.\n\n\
        TARGETS\n  \
          A target is always a positional argument. With no target, every project under\n  \
          ~/.claude/projects is scanned.\n\n    \
            <cwd>              a real filesystem cwd, which csift path-encodes\n    \
            -Users-...         an already-encoded projects-dir token\n    \
            @<uuid>            one session, also by its leading-hex prefix (`@1a2b3c4d`)\n    \
            @<agent-id>        one subagent and its subtree. The id is a bare hex of at least\n                       \
                               12 digits or a teammate's `aName-<hex>` id, exactly what\n                       \
                               `csift agents` prints.\n    \
            @<Name>@<Team>     one teammate, by its ROUTING form\n    \
            @main              the CALLING top-level session, read from $CLAUDE_CODE_SESSION_ID\n    \
            @trap:<marker>     the CALLING subagent (next section)\n    \
            <file>.jsonl       that one transcript\n\n  \
          The routing form is the `routing:` field `agents` prints beside a teammate's\n  \
          transcript id, and a form TaskStop takes. SendMessage refuses a `to` carrying `@`.\n  \
          The form fails loud when two teammates share it. A BARE uuid (no `@`) is NOT\n  \
          special. `--sessions-from <FILE|->` scopes any multi-target subcommand to a LIST of\n  \
          ids, one per line (for example piped from `search -l`).\n\n\
        WHICH SUBAGENT AM I? (@trap:<marker>)\n  \
          Claude Code withholds a subagent's OWN id from its Bash environment, so a running\n  \
          subagent cannot name itself from the environment. `@trap:<marker>` answers on EVERY\n  \
          subcommand, `whoami` included, which reports your bare hex id and the parent uuid.\n  \
          You INVENT a marker and write it LITERALLY into this very csift command. csift finds\n  \
          the transcript whose Bash `csift` command carries that marker and scopes to that\n  \
          agent's subtree.\n\n  \
          TIMING. A subagent's transcript flushes per content block, so its command is on disk\n  \
          at dispatch and a first try resolves. The MAIN conversation's record is an async\n  \
          flush of the completed assistant message, landing about 1 to 3.4 s after dispatch.\n  \
          A top-level FIRST use races that flush and normally misses. From the main thread\n  \
          `@main` answers with no race (it reads the environment), and a re-run of the SAME\n  \
          command with the SAME marker resolves.\n\n  \
          THE MARKER (csift checks its shape). A marker is invented by hand, once, for this\n  \
          one command. It is an imaginative, literary, random, CONTEXT-INDEPENDENT token of\n  \
          EXACTLY 3 CamelCase words and 4 random digits, shaped like\n  \
          `@trap:JollyShinyBrook4283`. That exact literal is the RESERVED doc example and\n  \
          csift refuses it, because a copied example collides with itself into ambiguity.\n    \
            - A marker generated by a script never resolves. The generator is another\n      \
              `csift`-like command carrying the marker, so the match is ambiguous.\n    \
            - A marker built from a shell variable or by string concatenation does not appear\n      \
              VERBATIM in the recorded command, so nothing matches it.\n    \
            - A reused marker also matches the earlier command that carried it.\n  \
          csift REJECTS lazy markers: not EXACTLY 3 words, single-letter or ALLCAPS \"words\"\n  \
          (`HTML`, `USB`, ...), not exactly 4 trailing digits, or trivial digits (1111, 1234,\n  \
          9876, 1357, 2468).",
    after_help = "EXAMPLES\n  \
          csift list                                  # index every session, all projects\n  \
          csift list .                                # only this cwd's project\n  \
          csift search \"carry\"                        # smart-case regex, all projects\n  \
          csift search \"\" -t user --since 2h .        # pure filter: user turns, last 2h, here\n  \
          csift agents @<uuid> --since 2h             # subagents TRIGGERED in the last 2h\n  \
          csift whoami                                # which Claude Code session am I?\n  \
          csift files @<uuid> --by file               # the files this session modified, and when\n  \
          csift recover @<uuid> --file /abs/app.py    # segmented diff-patch history of a file\n  \
          csift recover . --file /abs/app.py --at @turn:42   # partial snapshot as of turn 42\n  \
          csift verbatim . --budget 40000             # restore what a summary clipped\n  \
          csift plan @<uuid>                          # the plan file bound to a session\n  \
          csift image @<uuid> --out /tmp/imgs         # extract every pasted image to a dir\n  \
          csift show @<uuid> --line 46550             # the exact record a hit cited, in full\n  \
          csift show @<uuid> --line 46550 --raw       # ...or its verbatim raw jsonl bytes\n  \
          csift stats @main --no-subagents            # this session's token/tool/turn totals\n  \
          csift search \"panic\" -l | csift stats --sessions-from -   # stats for the matches\n  \
          csift search \"\" @<uuid> -t agent -T agent.thinking       # the agent minus thinking\n\n\
        THE RULES EVERY COMMAND FOLLOWS\n\n  \
          Exit codes\n    \
            An address you NAMED that does not resolve (a line, a turn, a uuid, a pinned id, a\n    \
            file) is a hard error, exit != 0. A filter that matches nothing is an honest\n    \
            empty, exit 0. A zero-match search also prints a diagnosis on stderr that says\n    \
            what was searched and, under a label filter, where the pattern DOES occur.\n    \
            A write that cannot LAND is an ordinary error too. Pipe csift into a reader that\n    \
            goes away (`| head`), and a command that buffers its output reports\n    \
            `csift: error: Broken pipe` and exits 1. The buffered ones are `search` (except\n    \
            `-c`, `-l` and `--count-by`), `show` (except `--branch-points`), `files`, `agents`,\n    \
            `image` and `verbatim`. A buffered command still exits 1 under `2>&1`, where that\n    \
            line shares the closed pipe and is lost. Every other command prints row by row and aborts with\n    \
            101 and a std panic message, with a backtrace only under `RUST_BACKTRACE=1`. On a\n    \
            buffered command a stderr note that cannot land is dropped, and the run does not\n    \
            abort.\n\n  \
          Ranges\n    \
            Every range flag shares one grammar, `N | A..B | N.. | ..N | .. | -k`, with\n    \
            inclusive endpoints. `-k` counts from the end, so `-3..` means \"the last 3\".\n    \
            Turns are 0-based logical indices (the `tN` search prints). Lines are 1-based\n    \
            physical jsonl lines (the `Lnnnn`). The two are different axes, so read both from\n    \
            the output and never compute one from the other.\n\n  \
          Subagents\n    \
            Most commands include a session's subagent transcripts automatically, and\n    \
            `--no-subagents` restricts them. `verbatim` is the one opt-in (`--subagents`),\n    \
            because its budget multiplies per session. `agents` lists subagents instead of\n    \
            spanning them.\n\n  \
          Copies\n    \
            One session id can sit on disk under TWO project directories. A renamed project\n    \
            whose directory was copied produces that shape, and so do Claude Code's own\n    \
            project-dir-name override and its relocation fallback. csift merges the copies by\n    \
            comparing their BYTES.\n      \
              - Identical copies, or one that is a strict byte PREFIX of the other, collapse\n        \
                onto the LONGEST.\n      \
              - Copies that DIFFER merge into nothing, and both stay in scope.\n      \
              - A copy csift could not READ stays in scope too, and the stderr note names it\n        \
                with the error. A command that reads every file in its scope then dies on\n        \
                that copy, after the note and before its own header. `agents` opens a session\n        \
                file only through its sidecar directory, so it never reads a copy with no\n        \
                sidecar directory beside it, and that run finishes normally.\n    \
            Every merge is named on stderr. The JSON header of `list`, `search`, `stats`,\n    \
            `files`, `recover`, `verbatim`, `image` and `agents` counts it\n    \
            (`duplicate_transcripts`, `duplicate_records_dropped`), beside the copies it did\n    \
            NOT merge (`divergent_transcripts`) and the ones it could not read\n    \
            (`unreadable_transcripts`). `show` still refuses an address that resolves to two\n    \
            files. A `*.jsonl` path reads one copy, the file it names, never another copy of\n    \
            its id, and it names the others on stderr. One agent id can also sit in BOTH\n    \
            subagent folders of one sidecar directory, `subagents/` and a workflow run's\n    \
            `subagents/workflows/<run>/` (its conversation continued there). The same rule\n    \
            reads both and names both.\n\n  \
          Caps\n    \
            Nothing is truncated silently. Every cap prints what it dropped and how to get the\n    \
            rest. `list` caps an unscoped run at 50 rows. `show` caps at 200 record units and\n    \
            prints the exact continuation command. `--max-count 0` means uncapped, always.\n\n  \
          Time\n    \
            Every timestamp prints in YOUR local timezone with the offset inline, as in\n    \
            `2026-07-11 15:33:37 AEST(UTC+10)`. The offset is derived per instant, so it is\n    \
            DST-correct, and a fractional zone renders like `IST(UTC+05:30)`. Raw UTC appears\n    \
            only in JSON `ts_utc`, never in text.\n\n\
        JSON OUTPUT (--format json)\n  \
          Every command but two emits the same envelope. It opens with one\n  \
          `{\"kind\":\"header\",…}` line, carries the kind-tagged rows, and closes with one\n  \
          `{\"kind\":\"summary\",…}` line, even for zero matches. The two exceptions:\n    \
            wait      prints ONE `{\"kind\":\"wait\",…}` object on exit, so `jq .fired` reads\n              \
                      it whole.\n    \
            deliver   prints a hook-output object or nothing. Its stdout belongs to Claude\n              \
                      Code, which reads it at the hook point.\n  \
          Select rows with `jq 'select(.kind==\"…\")'`. The summary is always `tail -1`. Do the\n  \
          select BEFORE projecting fields. A field template applied to the mixed stream stamps\n  \
          the header and summary rows as all-null rows.\n  \
          Rows carry the id trio (`session_id`, `is_subagent`, `parent_session_id`), and so\n  \
          does every search hit inside `.hits[]`, so bare `.hits[]` flattening carries real\n  \
          ids. Each hit also carries `refetch`, a ready-to-run `csift show` command for that\n  \
          exact record, which is still the preferred single-record path. Each subcommand's\n  \
          --help carries its full row schemas.\n\n\
        PITFALLS WORTH KNOWING UP FRONT\n  \
          - An empty pattern (\"\") matches EVERYTHING. It is the base for -t, time and turn\n    \
            filters, and the on-ramp to a census (`csift search \"\" <target> --count-by label`).\n  \
          - `search -c` counts round-trip EXCHANGES, never records or lines. Per-record counts\n    \
            come from `--count-by <axis>`, and `stats`' tool tallies count each CALL. So a\n    \
            `--count-by tool` reading runs about twice a `stats` tally, since it counts the tool\n    \
            use and the record carrying its result as two records.\n  \
          - `search -l` prints the owning SESSION uuids, which is what `--sessions-from -`\n    \
            consumes. The JSON summary's `transcript_ids` is the finer per-transcript list,\n    \
            with a subagent's own id where the hit sat in a subagent.\n  \
          - Line numbers are per FILE. Fetch a subagent's line at the subagent's own id, never\n    \
            at the parent uuid. The printed `refetch` command already gets this right.\n  \
          - Reading recent turns is `show <target> --turn -3..`. `verbatim` is only for turns\n    \
            a compaction summary already clipped, and it tells you when nothing was.\n  \
          - A pending AskUserQuestion, ExitPlanMode or MCP prompt is invisible in the\n    \
            transcript until it is answered. When a hook writes them to the elicitation file\n    \
            (`elicitations.jsonl` in the sidecar directory), csift merges the pending ones in,\n    \
            and `list` reports `sidecar_present`, so \"nothing pending\" and \"no hook installed\"\n    \
            read differently. An entry of that file is dropped automatically when the NATIVE\n    \
            transcript already shows its elicitation closed, because the native record\n    \
            outranks the file. A REJECTED plan is one such case. Claude Code fires no\n    \
            PostToolUse for a rejection, so the hook never wrote its resolved marker.\n  \
          - A text-mode excerpt is ONE LINE per record. Whitespace runs collapse to single\n    \
            spaces, so a multiline Bash command renders flat, and `--no-truncate` alone will\n    \
            not bring a table or a paragraph break back. The JSON `body` keeps the newlines of\n    \
            a render csift did not already flatten. It is NULL unless the excerpt cap was\n    \
            lifted (by `search --no-truncate`, or by a `show --line`/`--uuid` address), so a\n    \
            default stream carries a fragment per hit and no second full copy of every record.\n    \
            An opener csift RECONSTRUCTS (a user message, an answered AskUserQuestion, a\n    \
            rejection) is flattened a layer earlier. Its `body` is flat even then, and `--raw`\n    \
            is the only way to its bytes.\n  \
          - `--max-count` bounds the OUTPUT. A capped run keeps every note and names what it\n    \
            dropped, while a pipe into `head` or `tail` cuts off one end of the output and\n    \
            the footer notes with it.\n\n\
        THE CHANNEL (send / msg / ack / deliver)\n  \
          The channel carries a message between Claude Code lanes, and into one from any\n  \
          process outside Claude Code. It exists because the official channel is structurally\n  \
          absent for some receivers.\n    \
            - A running workflow lane cannot be reached by the send tool at all.\n    \
            - An unnamed subagent cannot reach its parent subagent.\n    \
            - Nothing reaches an idle top-level session that published no socket.\n    \
            - A sender outside Claude Code holds no tool to call.\n  \
          `csift send @<lane> \"…\"` queues a message and prints one verdict saying what will\n  \
          carry it. Where an official transport exists, the receipt prints the call for YOU\n  \
          to make, since csift is a binary and those transports are model tools. A\n  \
          `csift deliver --slot k` hook, which the RECEIVER's owner pastes into settings.json,\n  \
          carries the message in, and `csift deliver --recipe` prints that block.\n  \
          `csift msg <ID>` says whether it landed, by joining csift's own ledger to the\n  \
          receiver transcript's proof. `csift ack <ID>` is the receiver saying it read one.\n  \
          `csift whoami --to @<lane>` runs the same prediction with nothing queued.\n\n\
        WHAT csift WILL NOT DO\n  \
          - Semantic or BM25 search. Regex is the tool, so broaden the pattern or take a\n    \
            census first.\n  \
          - Ad-hoc aggregation languages. The built-ins are the closed `--count-by` axes,\n    \
            `stats` and `files --by`. Anything else is `--raw | jq`.\n  \
          - Diffs. Fetch both sides with `show` or `recover --at` and diff them outside.\n  \
          - Terminating anything.\n\n\
        WHAT csift WRITES\n  \
          Every command reads. Two kinds of command also write. The channel three (`send`,\n  \
          `deliver` and `ack`) write the csift-owned `csift-channel/` folder of a session's\n  \
          sidecar directory and nothing else. They never write a transcript, the team\n  \
          mailbox, the messaging socket, the session registry or a settings file. Arming the\n  \
          channel is an edit you make yourself. Three commands also write the file YOU name\n  \
          with an output flag.\n    \
            recover --out <PATH>     the reconstructed artifact. It replaces the file, and\n                             \
                                     leaves it untouched when nothing was reconstructed.\n    \
            verbatim --out <PATH>    the rendered reconstruction, under the same rule\n    \
            image --out <PATH|DIR>   each selected image. It replaces a file of the same name.\n    \
            recover --files-from <F> --out-dir <DIR>\n                             \
                                     each recovered file at its absolute path under DIR,\n                             \
                                     plus `recovery-report.tsv`. An existing file is skipped\n                             \
                                     unless `--force`.\n\n\
        RETENTION\n  \
          Claude Code deletes transcripts older than `cleanupPeriodDays` (default 30!).\n  \
          `jq '.cleanupPeriodDays // 30' ~/.claude/settings.json` prints the current value,\n  \
          and a larger value keeps transcripts longer. csift can only read what survives.\n\n\
        `csift <subcommand> --help` gives each subcommand's flags, JSON schemas and examples."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Override the Claude Code config dir: `~/.claude`, or wherever it was relocated.
    /// The flag applies to EVERY subcommand, because it decides where session transcripts
    /// and plan files are read from. It has the highest priority.
    ///
    /// The `$CLAUDE_CONFIG_DIR` env var, Claude Code's own relocation mechanism, is honored
    /// too. The default is a bare `.claude` under the OS home dir (`$HOME` on Unix,
    /// `%USERPROFILE%` on Windows), the same resolution Claude Code uses.
    ///
    /// Point the flag at the dir that IS the `.claude` equivalent. Transcripts are read
    /// from `<DIR>/projects/<encoded>/*.jsonl`.
    #[arg(
        long = "claude-home",
        value_name = "DIR",
        global = true,
        verbatim_doc_comment
    )]
    pub claude_home: Option<PathBuf>,

    /// How many compaction boundaries the conversation chain may cross. It applies to EVERY
    /// subcommand that walks the chain (`search`, `show`, `stats`, `files`, `recover`,
    /// `verbatim`, `image`), since any of them can be asked about a session that compacted.
    ///
    /// THE CHAIN. Claude Code rebuilds a conversation by picking one leaf record and walking
    /// its `parentUuid` links back to the head, and only what that walk reaches is the
    /// conversation. csift calls that walk the chain. A record the walk reaches is `live`. A
    /// turn opener it does not reach, such as a draft the person recalled or a turn the person
    /// rewound past, is `abandoned`. A record above a compaction cut that the walk could not
    /// resolve is `pre-cut`.
    ///
    /// A compaction writes a boundary record whose `parentUuid` is null, so Claude Code's own
    /// walk ends there, and on resume the model sees the summary and not the turns above it.
    /// The boundary also names, in `logicalParentUuid`, the record the conversation continued
    /// from, and csift follows that link across up to N boundaries. It crosses because a
    /// transcript is an archive and the turns above the cut were real. Above a CROSSED
    /// boundary, records read `live` or `abandoned` by the same rule as below it, so a turn the
    /// person rewound past up there gets named. Above the FLOOR, the point where the walk
    /// stopped, they stay `pre-cut`.
    ///
    /// `0` is Claude Code's own reading. The walk ends AT every boundary, and csift then says
    /// nothing about crossing at all. `all` follows every boundary a transcript carries.
    ///
    /// One shape reads differently at `0` than at any larger budget. It is a boundary whose
    /// OWN `parentUuid` is non-null and names no record. At any budget above `0` the walk steps
    /// past it through its `logicalParentUuid`. At `0` it steps nowhere, so the records above it
    /// read `pre-cut` instead of resolving. On a transcript that carries this shape, the two
    /// budgets give different readings.
    ///
    /// The text footer names what was crossed, what was not, and what a larger budget would
    /// do. It offers a flag value only where raising the flag would MOVE the floor, and that
    /// takes two conditions together. The BUDGET must be what stopped the walk, and the
    /// boundary it stopped at must still name a record the file carries. A boundary naming a
    /// record no line carries is a floor at every budget. The offered value is the budget at
    /// which every boundary in the file gets TRIED, and a tried boundary can still stop the walk
    /// for one of the three reasons below.
    ///
    /// JSON `floor_cause` says why the walk stopped. It takes EIGHT values.
    ///
    ///   `budget-exhausted`        the budget ran out on a boundary. The only cause a larger
    ///                             budget can move, and only when that boundary still names a
    ///                             record the walk can step to.
    ///
    /// THREE mean a boundary the walk cannot step past at any budget, each named for the fact
    /// that stops it.
    ///
    ///   `unresolvable-boundary`   its `logicalParentUuid` is on no line of this file
    ///   `relink-removed`          its `logicalParentUuid` is on a line you can open, but
    ///                             before the walk starts Claude Code removes records above a
    ///                             compaction from the set it walks, and that record was one
    ///   `boundary-loop`           it names a record the walk had already reached, so stepping
    ///                             past it leads back into the conversation below the cut
    ///
    /// The remaining four mean the walk ended BELOW the boundaries and never reached them.
    ///
    ///   `head`                    the conversation's own head
    ///   `dangling-parent`         a `parentUuid` naming no record here
    ///   `cycle`                   the parent links loop
    ///   `no-leaf`                 nothing resolved, so there is no walk to extend
    ///
    /// The DEFAULT is 3. Measured on an idle host, crossing three boundaries took no time a
    /// clock can tell apart from crossing none. A reading that must not depend on the default
    /// names the number explicitly.
    #[arg(
        long = "cross-compactions",
        value_name = "N|all",
        global = true,
        verbatim_doc_comment,
        default_value_t = crate::model::CROSS_COMPACTIONS_DEFAULT,
        value_parser = crate::model::CrossingBudget::parse
    )]
    pub cross_compactions: crate::model::CrossingBudget,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    // Per-command about/long_about/after_help live on each *Args struct's #[command(...)]
    // block. A variant doc comment here SHADOWS the struct's about + long_about (clap
    // derive precedence), and that shadowing makes every subcommand's long_about
    // invisible. Keep the variants bare.
    List(ListArgs),
    Search(SearchArgs),
    Show(ShowArgs),
    Stats(StatsArgs),
    Whoami(WhoamiArgs),
    Agents(AgentsArgs),
    Files(FilesArgs),
    Recover(RecoverArgs),
    Plan(PlanArgs),
    Verbatim(VerbatimArgs),
    Image(ImageArgs),
    Status(StatusArgs),
    Wait(WaitArgs),
    // The channel four, in the order the hand-written SUBCOMMANDS block reads them and in the
    // order a caller meets them: queue one, ask whether it landed, say you read it, and the
    // hook that carries it. clap lists subcommands by variant position, so the two lists
    // agree only while this order holds (`cli::tests::docs` pins that).
    Send(SendArgs),
    Msg(MsgArgs),
    Ack(AckArgs),
    // HIDDEN catch-all for the REMOVED `turns` name (the command is `verbatim`). Exists
    // only so it gets the pointed successor error the retired `-t` spellings get,
    // instead of clap's teach-nothing "unrecognized subcommand". Never works, always bails
    // (main.rs). Kept bare like every variant (the shadowing rule above); a plain comment,
    // not a doc comment, so nothing ever renders for it.
    #[command(hide = true)]
    Turns(TurnsRenamedArgs),
    // The csift-channel hook entry. Bare like every other variant (the shadowing rule
    // above): its about/long_about live on `DeliverArgs`.
    Deliver(DeliverArgs),
}

/// Argument sink for the hidden [`Command::Turns`] tombstone: swallows EVERY token (flags
/// included) so the rename error always wins - a flag-parse error must never preempt it.
#[derive(Args, Debug)]
pub struct TurnsRenamedArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., hide = true)]
    pub rest: Vec<std::ffi::OsString>,
}
