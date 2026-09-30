//! Cli root + the Command enum (17 subcommands + the hidden turns tombstone).

use super::*;

/// ripgrep for Claude Code session transcripts.
#[derive(Debug, Parser)]
#[command(
    name = "csift",
    version,
    about = "ripgrep for Claude Code session transcripts",
    long_about = "csift - \"ripgrep for Claude Code session transcripts\".\n\n\
        A fast Rust CLI to list and regex-search Claude Code session `.jsonl` files \
        under ~/.claude/projects/<encoded>/. Built for an LLM consumer (a CC agent \
        searching or recovering its own / a peer session): output is clean, \
        token-efficient and regex-driven. Human/LLM-readable text by default; pass \
        `--format json` for machine use.\n\n\
        Pure regex/ripgrep only: no BM25, no embeddings, no semantic search, by design.\n\n\
        SUBCOMMANDS\n  \
          list     fast \"which session is this?\" index (first/last user + last agent)\n  \
          search   regex over transcripts, returning the complete round-trip exchange per hit\n  \
          show     FETCH the record(s) you name: `--line` / `--turn` / `--uuid` of ONE transcript\n           \
                   (`--turn -3..` = the last 3 turns, the live-tail peek), rendered full or `--raw` bytes\n  \
          stats    one-scan aggregates per session: tokens by model, tool calls, turns, span\n  \
          whoami   identify the calling CC session via $CLAUDE_CODE_SESSION_ID\n  \
          agents   list a session's subagents (kind, start/completion, status) + time-window filter\n  \
          files    which files/dirs a session modified, when (Edit/Write/Notebook + heuristic Bash)\n  \
          recover  reconstruct a file's history from the transcript: segmented diff-patches,\n           \
                   point-in-time partial snapshot, or coverage scoping\n  \
          plan     locate the Plan-Mode plan file bound to a session (recover --file @plan dumps it)\n  \
          verbatim RESTORE the verbatim user/assistant back-and-forth a compaction summary\n           \
                   CLIPPED, within a char budget (live-tail peek is `show --turn`)\n  \
          image    list + extract the inline images a session carries (pastes/screenshots)\n  \
          status   one-shot LIVE verdict on a session: running / waiting-children /\n           \
                   waiting-hitl / idle-background-open / idle-eot / stale-dead /\n           \
                   unknown, with the evidence named\n  \
          wait     block until a session condition fires (stop / hitl / auq / notification /\n           \
                   tool: / write: / verdict:), exit 124 on --timeout\n  \
          send     queue a message for ONE lane on the csift channel (a user-installed\n           \
                   `csift deliver` hook carries it); one verdict per send, official sends\n           \
                   are printed for you to make, never performed\n  \
          msg      reconcile a csift-channel message: the ledger's INTENT joined to the\n           \
                   receiver transcript's FACT, one verdict; with no id, the lane's ledger\n  \
          ack      record that the calling lane READ a channel message (appends an ack line)\n  \
          deliver  hook entry; called by Claude Code through the installed hook, not by\n           \
                   hand (except --recipe, which prints the block you paste)\n\n\
        list/search/stats/files/recover/plan/image/status/wait span each session's subagent \
        transcripts by \
        default (built-in Task/Agent-tool, OMC, and Workflow agents); pass `--no-subagents` \
        to restrict to top-level sessions. `verbatim` is the exception among the file-operating \
        commands: it defaults to the TOP-LEVEL thread only (single-thread recovery), \
        opts INTO spanning via `--subagents`, and REQUIRES a target. (`agents` LISTS \
        subagents as its targets, not as inputs to span, so it rejects both \
        span flags.)\n\n\
        A target is EITHER a real filesystem cwd (it gets path-encoded) OR an \
        already-encoded `-Users-...` projects-dir token; with no target, every \
        project under ~/.claude/projects is scanned.\n\n\
        TARGETING a session/agent (always positional): `@<uuid>` (or its \
        leading-hex prefix, e.g. `@1a2b3c4d`) scopes to one session; `@<agent-id>` to one \
        subagent + its subtree (a bare hex ≥12 OR a teammate `aName-<hex>` id; exactly what \
        `csift agents` prints); `@<Name>@<Team>` is a teammate's ROUTING form (the `routing:` \
        field `agents` prints beside that transcript id, a form TaskStop takes; SendMessage \
        refuses a `to` carrying `@`) and resolves to that one teammate, failing loud when two \
        teammates share it; \
        `@main` is the CALLING top-level session (read from \
        $CLAUDE_CODE_SESSION_ID); a `*.jsonl` path scopes to that one transcript. A BARE uuid \
        (no `@`) is NOT special. `--sessions-from <FILE|->` scopes to a LIST of ids (one per \
        line, e.g. piped from `search -l`) on every multi-target subcommand.\n\n\
        @trap:<marker> answers \"which subagent am I?\" on EVERY subcommand (including `whoami`, which \
        reports your bare hex + parent uuid). CC withholds a subagent's OWN id from its Bash \
        environment, so a running subagent cannot name itself via env. Instead you INVENT a marker \
        and embed it LITERALLY in this very csift command; csift finds the transcript whose Bash \
        `csift` command carries that marker and scopes to that agent's subtree. TIMING: a \
        subagent's transcript flushes per content block, so its command is on disk at dispatch \
        and a first try resolves; the MAIN conversation's record is an async flush of the \
        completed assistant message landing ~1-3.4s after dispatch, so a top-level FIRST use \
        races it and normally misses; from the main thread use `@main` instead (env-based, no \
        race), or re-run the SAME command with the SAME marker. \
        DISCIPLINE (enforced): invent the marker ONE-SHOT, by you, right now: an imaginative, \
        literary, random, CONTEXT-INDEPENDENT token of EXACTLY 3 CamelCase words + 4 random digits, \
        shaped like `@trap:JollyShinyBrook4283`. That exact literal is the RESERVED doc \
        example, refused by csift (a copied example self-collides into ambiguity), so invent your \
        OWN. NEVER generate it with a script (a generator would be \
        another `csift`-ish command carrying the marker → ambiguity); never build it from a \
        shell variable or string concatenation (it must appear VERBATIM in the recorded \
        command); never reuse a previous one. csift REJECTS lazy markers: not EXACTLY 3 words, \
        single-letter or ALLCAPS \"words\" (`HTML`/`USB`…), not exactly 4 trailing digits, or \
        trivial digits (1111 / 1234 / 9876 / 1357 / 2468).",
    after_help = "EXAMPLES\n  \
          csift list                                  # index every session, all projects\n  \
          csift list .                                # just this cwd's project\n  \
          csift search \"carry\"                        # smart-case regex, all projects\n  \
          csift search \"\" -t user --since 2h .        # pure filter: user turns, last 2h, here\n  \
          csift agents @<uuid> --since 2h             # subagents TRIGGERED in the last 2h\n  \
          csift whoami                                # who am I (this CC session)?\n  \
          csift files @<uuid> --by file               # which files this session modified, when\n  \
          csift recover @<uuid> --file /abs/app.py    # segmented diff-patch history of a file\n  \
          csift recover . --file /abs/app.py --at @turn:42  # partial snapshot as the LLM saw it at turn 42\n  \
          csift verbatim . --budget 40000                # restore the verbatim back-and-forth a summary clipped\n  \
          csift plan @<uuid>                          # locate the plan file bound to a session\n  \
          csift image @<uuid> --out /tmp/imgs         # extract every pasted image to a dir\n  \
          csift show @<uuid> --line 46550             # fetch the exact record a hit cited, in full\n  \
          csift show @<uuid> --line 46550 --raw       # …or its verbatim raw jsonl bytes\n  \
          csift stats @main --no-subagents            # this session's token/tool/turn aggregates\n  \
          csift search \"panic\" -l | csift stats --sessions-from -   # aggregate exactly the sessions that matched\n  \
          csift search \"\" @<uuid> -t agent -T agent.thinking       # the agent role minus its thinking\n\n\
        THE RULES EVERY COMMAND FOLLOWS\n  \
          Exit codes   An address you NAMED that doesn't resolve (a line, a turn, a uuid, a\n               \
        pinned id, a file) is a hard error (exit != 0). A filter that matches nothing is an\n               \
        honest empty (exit 0); a zero-match search additionally prints a diagnosis on stderr\n               \
        explaining what was searched and, under a label filter, where the pattern DOES\n               \
        occur. A write that cannot LAND is an ordinary error too: pipe csift into a reader\n               \
        that goes away (`| head`) and every surface whose rows go through the shared output\n               \
        buffer reports `csift: error: Broken pipe` and exits 1, and still 1 under `2>&1`,\n               \
        where that line shares the closed pipe and is lost; the ones that still print row\n               \
        by row abort with 101 and a std panic message, a backtrace only under\n               \
        `RUST_BACKTRACE=1`. The rule follows the WRITER that makes the failing write, never\n               \
        the subcommand. On a buffered surface a stderr note that cannot land is dropped\n               \
        rather than aborting the run.\n  \
          Ranges       Every range flag shares one grammar: N | A..B | N.. | ..N | .. | -k,\n               \
        endpoints inclusive; `-k` counts from the end, so `-3..` means \"the last 3\". Turns\n               \
        are 0-based logical indices (the `tN` search prints); lines are 1-based physical\n               \
        jsonl lines (the `Lnnnn`). Different axes: read both from output, don't compute.\n  \
          Subagents    Most commands include a session's subagent transcripts automatically;\n               \
        `--no-subagents` restricts. `verbatim` is the one opt-in (`--subagents`, because its\n               \
        budget multiplies per session), and `agents` lists subagents instead of spanning.\n  \
          Copies       One session id can be on disk under TWO project directories (a\n               \
        renamed project whose directory was copied; Claude Code's own project-dir-name\n               \
        override and its relocation fallback reach the same shape). csift merges the copies\n               \
        and decides by the BYTES: identical, or one a strict byte PREFIX of the other,\n               \
        collapse onto the LONGEST; copies that DIFFER merge into nothing and both stay in\n               \
        scope, and so does a copy csift could not READ (the stderr note names that one and\n               \
        the error). A command that reads every file in its scope then dies on that copy,\n               \
        after the note and before its own header; `agents` opens a session file only\n               \
        through its sidecar, so a copy with no sidecar directory beside it is never read\n               \
        and that run finishes normally. Every merge is named on stderr and counted in the\n               \
        JSON header (`duplicate_transcripts`, `duplicate_records_dropped`), beside the\n               \
        copies it did NOT merge (`divergent_transcripts`) and the ones it could not read\n               \
        (`unreadable_transcripts`). `show` still refuses an address that resolves\n               \
        to two files: target a `*.jsonl` path to read one copy - a path reads the file it\n               \
        names, never another copy of its id, and names the others on stderr. One agent id\n               \
        can also sit at BOTH sidecar locations of one session, the built-in one and a\n               \
        workflow run's (its conversation continued there): the same rule reads and names\n               \
        both.\n  \
          Caps         No silent truncation, ever: every cap prints what was dropped and how\n               \
        to get the rest. `list` caps an unscoped run at 50 rows; `show` caps at 200 record\n               \
        units and prints the exact continuation command; `--max-count 0` = uncapped, always.\n  \
          Time         Every timestamp prints in YOUR local timezone with the offset inline,\n               \
        like `2026-07-11 15:33:37 AEST(UTC+10)`, derived per instant (DST-correct; fractional\n               \
        zones render like `IST(UTC+05:30)`). Raw UTC lives in JSON `ts_utc`, never in text.\n\n\
        JSON OUTPUT (--format json)\n  \
          Every command but two emits the same envelope: one `{\"kind\":\"header\",…}` line,\n  \
        then kind-tagged rows, then one `{\"kind\":\"summary\",…}` line, even for zero matches.\n  \
        The two: `wait` prints ONE `{\"kind\":\"wait\",…}` object on exit (so `jq .fired` reads\n  \
        it whole), and `deliver` prints a hook-output object or nothing - its stdout belongs\n  \
        to Claude Code, which reads it at the hook point.\n  \
        Select rows with `jq 'select(.kind==\"…\")'`; the summary is always `tail -1`. Do\n  \
        the select BEFORE projecting fields; a field template applied to the mixed stream\n  \
        stamps the header/summary rows as all-null rows. Rows\n  \
        carry the id trio (`session_id` / `is_subagent` / `parent_session_id`), INCLUDING\n  \
        every search hit inside `.hits[]`, so bare `.hits[]` flattening carries\n  \
        real ids; each hit also carries `refetch`: a ready-to-run `csift show` command for\n  \
        that exact record (still the preferred single-record path). Full\n  \
        row schemas live in each subcommand's --help.\n\n\
        PITFALLS WORTH KNOWING UP FRONT\n  \
          - An empty pattern (\"\") matches EVERYTHING: it is the base for -t/time/turn\n    \
        filters and the census on-ramp (`csift search \"\" <target> --count-by label`).\n  \
          - `search -c` counts round-trip EXCHANGES, not records or lines; per-record\n    \
        counts are `--count-by <axis>`; `stats`' tool tallies are per CALL. Three units\n    \
        (exchange, record, call), so `--count-by tool` reading ≈2× a `stats` tally\n    \
        (use + result carrier) is a unit difference, not a discrepancy.\n  \
          - `search -l` prints owning SESSION uuids (what `--sessions-from -` consumes);\n    \
        the JSON summary's `transcript_ids` is the finer per-transcript list. Two\n    \
        different questions, two different answers.\n  \
          - Line numbers are per FILE: fetch a subagent's line at the subagent's own id,\n    \
        never at the parent uuid. The printed `refetch` command already has this right.\n  \
          - Reading recent turns is `show <target> --turn -3..`; `verbatim` is only for\n    \
        turns a compaction summary already clipped (it tells you when nothing was).\n  \
          - A pending AskUserQuestion / ExitPlanMode / MCP prompt is invisible in the\n    \
        transcript until answered. csift merges pending ones from a hook-written sidecar\n    \
        when present, and `list` reports `sidecar_present`, so you can tell \"nothing\n    \
        pending\" apart from \"no hook installed\". A sidecar pending whose elicitation the\n    \
        NATIVE transcript already shows closed (e.g. a REJECTED plan: Claude Code fires\n    \
        no PostToolUse for a rejection, so the hook never got to write its resolved\n    \
        marker) is auto-dropped: the native record outranks the sidecar.\n  \
          - A text-mode excerpt is ONE LINE per record: whitespace runs collapse to single\n    \
        spaces, so a multiline Bash command renders flat and `--no-truncate` alone will\n    \
        not give a table or a paragraph break back. The JSON `body` keeps the newlines\n    \
        of a render csift did not already flatten, and it is NULL unless the excerpt cap\n    \
        was lifted - `search --no-truncate`, or a `show --line`/`--uuid` address - so a\n    \
        default stream carries a fragment per hit and not a second full copy of every\n    \
        record. An opener csift RECONSTRUCTS (a user message, an answered\n    \
        AskUserQuestion, a rejection) is flattened a layer earlier, so its `body` is flat\n    \
        even there and `--raw` is the only way to its bytes. To bound the\n    \
        OUTPUT reach for `--max-count` rather than a pipe into `head`/`tail`: a capped\n    \
        run keeps every note and names what it dropped, while a pipe amputates one end\n    \
        of the ledger.\n\n\
        THE CHANNEL (send / msg / ack / deliver)\n  \
          A message channel between Claude Code lanes, and into one from any process\n  \
        outside Claude Code. It exists because the official channel is structurally\n  \
        absent for some receivers: a running workflow lane cannot be reached by the send\n  \
        tool at all, an unnamed subagent cannot reach its parent subagent, nothing\n  \
        reaches an idle top-level session that published no socket, and a sender outside\n  \
        Claude Code holds no tool to call. `csift send @<lane> \"…\"` queues a message and\n  \
        prints one verdict saying what will carry it (where an official transport exists\n  \
        the receipt prints the call for YOU to make, since csift is a binary and those\n  \
        are model tools). A `csift deliver --slot k` hook, which the RECEIVER's owner\n  \
        pastes into settings.json, carries it in; `csift deliver --recipe` prints that\n  \
        block. `csift msg <ID>` says whether it landed, joining csift's own ledger to the\n  \
        receiver transcript's proof; `csift ack <ID>` is the receiver saying it read one.\n  \
        `csift whoami --to @<lane>` runs the same prediction with nothing queued.\n\n\
        WHAT csift WILL NOT DO\n  \
          Semantic/BM25 search (regex is the tool; broaden the pattern, or census first);\n  \
        ad-hoc aggregation languages (the closed `--count-by` axes, `stats`, and\n  \
        `files --by` are the built-ins; anything else: `--raw | jq`); diffs (fetch both\n  \
        sides with `show` / `recover --at`, diff outside); terminating anything.\n  \
          Every command reads. Two kinds of command also write. The channel three - `send`,\n  \
        `deliver` and `ack` - write the csift-owned `<session>/csift-channel/` and nothing\n  \
        else: never a transcript, never the team mailbox, never the messaging socket, never\n  \
        the session registry, never a settings file. Arming the channel is an edit you make\n  \
        yourself. And three commands write the file YOU name with an output flag:\n    \
        recover --out <PATH>     the reconstructed artifact; replaces the file, and\n                             \
        leaves it untouched when nothing was reconstructed\n    \
        verbatim --out <PATH>    the rendered reconstruction, same rule\n    \
        image --out <PATH|DIR>   each selected image; replaces a file of the same name\n    \
        recover --files-from <F> --out-dir <DIR>\n                             \
        each recovered file at its absolute path under DIR, plus\n                             \
        `recovery-report.tsv`; an existing file is skipped unless `--force`\n\n\
        RETENTION\n  \
          Claude Code deletes transcripts older than `cleanupPeriodDays` (default 30!).\n  \
        Check `jq '.cleanupPeriodDays // 30' ~/.claude/settings.json` and consider raising\n  \
        it; csift can only read what survives.\n\n\
        Run `csift <subcommand> --help` for per-subcommand flags, JSON schemas + examples."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Override the Claude Code config dir: the `~/.claude` directory, or wherever it has
    /// been relocated. Applies to EVERY subcommand (it determines where session
    /// transcripts and plan files are read from). Highest priority; the `$CLAUDE_CONFIG_DIR`
    /// env var (Claude Code's own relocation mechanism) is honored too, and a bare
    /// `.claude` under the OS home dir (`$HOME` on Unix, `%USERPROFILE%` on Windows; the
    /// same resolution Claude Code uses) is the default. Point this at the dir that IS the
    /// `.claude` equivalent: transcripts are read from `<DIR>/projects/<encoded>/*.jsonl`.
    #[arg(long = "claude-home", value_name = "DIR", global = true)]
    pub claude_home: Option<PathBuf>,

    /// How many compaction boundaries the survival walk may step OVER, following the
    /// `logicalParentUuid` each boundary records. Applies to EVERY chain-walking
    /// subcommand (`search`, `show`, `stats`, `files`, `recover`, `verbatim`, `image`),
    /// since any of them can be asked about a session that compacted.
    ///
    /// Claude Code's own loader never crosses: it reads `parentUuid` only, which is null
    /// on a boundary, so on resume the model sees the summary and not the turns above it.
    /// csift crosses because a transcript is an archive and the turns above the cut were
    /// real: the records above a cut are the same conversation, and a turn the operator
    /// rewound past up there is one csift could not name until the walk got to it. Above a
    /// CROSSED
    /// boundary records resolve by the ordinary rules (live / abandoned); above the FLOOR
    /// where the walk stopped they stay `pre-cut`.
    ///
    /// `0` restores the loader's reading and makes csift say nothing about crossing at
    /// all; `all` follows every boundary a transcript carries. `0` ends the walk AT every
    /// boundary with no exception, including the one shape where a BUDGET and a GUARD on
    /// the `logicalParentUuid` arm decide differently: a boundary whose OWN `parentUuid`
    /// is non-null and names no record is climbable by that arm as a guard, while a budget
    /// of `0` buys no hop at all, so the records above it read `pre-cut` instead of
    /// resolving. No transcript of the measured corpus - 32 streams over 8 transcripts -
    /// carries that shape, so read both budgets rather than assume they agree on one that
    /// does. The footer names what was crossed, what was not, and what you can do about the
    /// rest - it offers a flag value only where raising the flag would MOVE the floor,
    /// which needs both that the BUDGET is what stopped the walk and that the boundary it
    /// stopped at still names a record the file carries; a boundary naming a record no
    /// line carries is a floor at every budget. What the offered value promises is that
    /// every boundary here gets TRIED, not that every one of them opens.
    ///
    /// The floor's own reason rides JSON as `floor_cause`, over EIGHT values. Only
    /// `budget-exhausted` is one a larger budget can move, and only when that boundary's
    /// door is open. THREE are a boundary whose door is shut, and each names which fact
    /// shut it: `unresolvable-boundary` (its `logicalParentUuid` is on no line of this
    /// file), `relink-removed` (it is on a line, which you can open, that the pre-boundary
    /// relink cut took out of the uuid map) and `boundary-loop` (it names a record the walk
    /// had already reached, so its door leads back into the conversation rather than above
    /// the cut). The remaining four mean the walk ended BELOW the boundaries and never
    /// reached them at all: `head` (the conversation's own head), `dangling-parent` (a
    /// `parentUuid` naming no record here), `cycle` (the parent links loop) and `no-leaf`
    /// (nothing resolved, so there is no walk to extend).
    ///
    /// The DEFAULT of 3 is the measured value: on an idle host, budget 3 ran inside the
    /// same-binary noise band against budget 0 on every one of 95 chain-walking rows (a
    /// compacted transcript and the 397 MiB landmark, 21 repetitions each; medians 1.001
    /// and 1.002, the widest slowdown 10 ms), so the crossing costs nothing a clock can
    /// see. Pass the number explicitly wherever a reading must not depend on the default.
    #[arg(
        long = "cross-compactions",
        value_name = "N|all",
        global = true,
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
