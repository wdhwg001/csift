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
        SUBCOMMANDS\n  \
          list     a fast \"which session is this?\" index showing the first and last user\n           \
                   message and the last agent message\n  \
          search   regex search over transcripts that returns the complete round-trip\n           \
                   exchange of each hit\n  \
          show     fetch the records you name in ONE transcript, by `--line`, `--turn` or\n           \
                   `--uuid`, rendered in full or as `--raw` bytes. `--turn -3..` fetches the\n           \
                   last 3 turns (the live-tail peek).\n  \
          stats    one-scan aggregates per session (tokens by model, tool calls, turns, span)\n  \
          whoami   identify the calling Claude Code session from $CLAUDE_CODE_SESSION_ID\n  \
          agents   list a session's subagents with kind, start and completion time and\n           \
                   status, filtered by a time window\n  \
          files    the files and directories a session modified, and when (Edit, Write and\n           \
                   Notebook calls, plus a heuristic reading of Bash)\n  \
          recover  reconstruct a file's history from the transcript as segmented\n           \
                   diff-patches, a point-in-time partial snapshot, or a coverage scope\n  \
          plan     locate the Plan-Mode plan file bound to a session (`recover --file @plan`\n           \
                   prints its text)\n  \
          verbatim restore, within a character budget, the verbatim user/assistant\n           \
                   back-and-forth a compaction summary CLIPPED. The live-tail peek is\n           \
                   `show --turn`.\n  \
          image    list and extract the inline images a session carries (pastes and\n           \
                   screenshots)\n  \
          status   a one-shot LIVE verdict on a session, with the evidence named. The verdict\n           \
                   is one of running, waiting-children, waiting-hitl, idle-background-open,\n           \
                   idle-eot, stale-dead or unknown.\n  \
          wait     block until a session condition fires (stop, hitl, auq, notification,\n           \
                   tool:, write:, verdict:). Exits 124 when --timeout elapses.\n  \
          send     queue a message for ONE lane on the csift channel, carried by a\n           \
                   `csift deliver` hook the user installed. Each send prints one verdict.\n           \
                   An official send is printed for you to make and never performed.\n  \
          msg      reconcile one csift-channel message, joining the ledger's INTENT to the\n           \
                   receiver transcript's FACT in one verdict. With no id it lists the\n           \
                   lane's ledger.\n  \
          ack      record that the calling lane READ a channel message (appends an ack line)\n  \
          deliver  the hook entry. Claude Code calls it through the installed hook. By hand\n           \
                   you run only `--recipe`, which prints the block you paste.\n\n\
        Most commands read a session's subagent transcripts too. list, search, stats, files,\n\
        recover, plan, image, status and wait span them by default (built-in Task/Agent-tool\n\
        agents, OMC agents and Workflow agents), and `--no-subagents` restricts them to\n\
        top-level sessions. `verbatim` is the exception among the file-operating commands. It\n\
        reads only the TOP-LEVEL thread by default (single-thread recovery), opts INTO\n\
        spanning with `--subagents`, and REQUIRES a target. `agents` LISTS subagents as its\n\
        targets instead of spanning them as inputs, so it rejects both span flags.\n\n\
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
          A top-level FIRST use races that flush and normally misses. From the main thread use\n  \
          `@main` instead (it reads the environment and has no race), or re-run the SAME\n  \
          command with the SAME marker.\n\n  \
          DISCIPLINE (enforced). Invent the marker yourself, once, right now. It is an\n  \
          imaginative, literary, random, CONTEXT-INDEPENDENT token of EXACTLY 3 CamelCase\n  \
          words and 4 random digits, shaped like `@trap:JollyShinyBrook4283`. That exact\n  \
          literal is the RESERVED doc example and csift refuses it, because a copied example\n  \
          collides with itself into ambiguity. Invent your OWN.\n    \
            - NEVER generate it with a script. The generator would be another `csift`-like\n      \
              command carrying the marker, and the match would be ambiguous.\n    \
            - Never build it from a shell variable or by string concatenation. It must\n      \
              appear VERBATIM in the recorded command.\n    \
            - Never reuse a previous one.\n  \
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
            goes away (`| head`), and every surface whose rows go through the shared output\n    \
            buffer reports `csift: error: Broken pipe` and exits 1. It still exits 1 under\n    \
            `2>&1`, where that line shares the closed pipe and is lost. The surfaces that\n    \
            still print row by row abort with 101 and a std panic message, with a backtrace\n    \
            only under `RUST_BACKTRACE=1`. The rule follows the WRITER that makes the failing\n    \
            write, never the subcommand. On a buffered surface a stderr note that cannot land\n    \
            is dropped, and the run does not abort.\n\n  \
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
                file only through its sidecar, so it never reads a copy with no sidecar\n        \
                directory beside it, and that run finishes normally.\n    \
            Every merge is named on stderr and counted in the JSON header\n    \
            (`duplicate_transcripts`, `duplicate_records_dropped`), beside the copies it did\n    \
            NOT merge (`divergent_transcripts`) and the ones it could not read\n    \
            (`unreadable_transcripts`). `show` still refuses an address that resolves to two\n    \
            files. Target a `*.jsonl` path to read one copy. A path reads the file it names,\n    \
            never another copy of its id, and names the others on stderr. One agent id can\n    \
            also sit at BOTH sidecar locations of one session, the built-in one and a workflow\n    \
            run's (its conversation continued there). The same rule reads both and names both.\n\n  \
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
            come from `--count-by <axis>`, and `stats`' tool tallies count each CALL. These are\n    \
            three units (exchange, record, call). A `--count-by tool` reading about twice a\n    \
            `stats` tally (the use plus its result carrier) is that unit difference and\n    \
            nothing more.\n  \
          - `search -l` prints the owning SESSION uuids, which is what `--sessions-from -`\n    \
            consumes. The JSON summary's `transcript_ids` is the finer per-transcript list.\n    \
            The two answer different questions.\n  \
          - Line numbers are per FILE. Fetch a subagent's line at the subagent's own id, never\n    \
            at the parent uuid. The printed `refetch` command already gets this right.\n  \
          - Reading recent turns is `show <target> --turn -3..`. `verbatim` is only for turns\n    \
            a compaction summary already clipped, and it tells you when nothing was.\n  \
          - A pending AskUserQuestion, ExitPlanMode or MCP prompt is invisible in the\n    \
            transcript until it is answered. csift merges pending ones from a hook-written\n    \
            sidecar when one is present, and `list` reports `sidecar_present`, so \"nothing\n    \
            pending\" and \"no hook installed\" read differently. A sidecar entry is dropped\n    \
            automatically when the NATIVE transcript already shows its elicitation closed,\n    \
            because the native record outranks the sidecar. A REJECTED plan is one such case.\n    \
            Claude Code fires no PostToolUse for a rejection, so the hook never wrote its\n    \
            resolved marker.\n  \
          - A text-mode excerpt is ONE LINE per record. Whitespace runs collapse to single\n    \
            spaces, so a multiline Bash command renders flat, and `--no-truncate` alone will\n    \
            not bring a table or a paragraph break back. The JSON `body` keeps the newlines of\n    \
            a render csift did not already flatten. It is NULL unless the excerpt cap was\n    \
            lifted (by `search --no-truncate`, or by a `show --line`/`--uuid` address), so a\n    \
            default stream carries a fragment per hit and no second full copy of every record.\n    \
            An opener csift RECONSTRUCTS (a user message, an answered AskUserQuestion, a\n    \
            rejection) is flattened a layer earlier. Its `body` is flat even then, and `--raw`\n    \
            is the only way to its bytes.\n  \
          - To bound the OUTPUT, use `--max-count`. A capped run keeps every note and names\n    \
            what it dropped, while a pipe into `head` or `tail` amputates one end of the\n    \
            ledger.\n\n\
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
          `deliver` and `ack`) write the csift-owned `<session>/csift-channel/` and nothing\n  \
          else. They never write a transcript, the team mailbox, the messaging socket, the\n  \
          session registry or a settings file. Arming the channel is an edit you make\n  \
          yourself. Three commands also write the file YOU name with an output flag.\n    \
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
        Check `jq '.cleanupPeriodDays // 30' ~/.claude/settings.json` and consider raising\n  \
        it. csift can only read what survives.\n\n\
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

    /// How many compaction boundaries the survival walk may step OVER, following the
    /// `logicalParentUuid` each boundary records. It applies to EVERY chain-walking
    /// subcommand (`search`, `show`, `stats`, `files`, `recover`, `verbatim`, `image`),
    /// since any of them can be asked about a session that compacted.
    ///
    /// Claude Code's own loader never crosses. It reads `parentUuid` only, which is null on
    /// a boundary, so on resume the model sees the summary and not the turns above it.
    /// csift crosses because a transcript is an archive and the turns above the cut were
    /// real. The records above a cut belong to the same conversation, and a turn the
    /// operator rewound past up there is one csift cannot name until the walk reaches it.
    /// Above a CROSSED boundary, records resolve by the ordinary rules (live or abandoned).
    /// Above the FLOOR where the walk stopped, they stay `pre-cut`.
    ///
    /// `0` restores the loader's reading, and csift then says nothing about crossing at
    /// all. `all` follows every boundary a transcript carries.
    ///
    /// `0` ends the walk AT every boundary with no exception. That includes the one shape
    /// where a BUDGET and a GUARD on the `logicalParentUuid` arm decide differently. A
    /// boundary whose OWN `parentUuid` is non-null and names no record is climbable by that
    /// arm as a guard, while a budget of `0` buys no hop at all, so the records above it
    /// read `pre-cut` instead of resolving. No transcript of the measured corpus (32
    /// streams over 8 transcripts) carries that shape. On a transcript that does, read both
    /// budgets and do not assume they agree.
    ///
    /// The footer names what was crossed, what was not, and what you can do about the
    /// rest. It offers a flag value only where raising the flag would MOVE the floor, and
    /// that takes two conditions together. The BUDGET must be what stopped the walk, and
    /// the boundary it stopped at must still name a record the file carries. A boundary
    /// naming a record no line carries is a floor at every budget. The offered value
    /// promises that every boundary here gets TRIED. It does not promise that every one of
    /// them opens.
    ///
    /// The floor's own reason rides JSON as `floor_cause`, which takes EIGHT values.
    ///
    ///   `budget-exhausted`        the only one a larger budget can move, and only when
    ///                             that boundary's door is open
    ///
    /// THREE mean a boundary whose door is shut, and each names the fact that shut it.
    ///
    ///   `unresolvable-boundary`   its `logicalParentUuid` is on no line of this file
    ///   `relink-removed`          it is on a line, which you can open, that the
    ///                             pre-boundary relink cut took out of the uuid map
    ///   `boundary-loop`           it names a record the walk had already reached, so its
    ///                             door leads back into the conversation, not above the cut
    ///
    /// The remaining four mean the walk ended BELOW the boundaries and never reached them.
    ///
    ///   `head`                    the conversation's own head
    ///   `dangling-parent`         a `parentUuid` naming no record here
    ///   `cycle`                   the parent links loop
    ///   `no-leaf`                 nothing resolved, so there is no walk to extend
    ///
    /// The DEFAULT of 3 is the measured value. On an idle host, budget 3 ran inside the
    /// same-binary noise band against budget 0 on every one of 95 chain-walking rows (a
    /// compacted transcript and the 397 MiB landmark, 21 repetitions each). The medians
    /// were 1.001 and 1.002 and the widest slowdown 10 ms, so the crossing costs nothing a
    /// clock can see. Pass the number explicitly wherever a reading must not depend on the
    /// default.
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
