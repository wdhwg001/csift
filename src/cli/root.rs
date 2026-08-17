//! Cli root + the Command enum (11 subcommands + the hidden turns tombstone).

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
          agents   list a session's subagents (kind, start/completion, status) + time-window filter\n  \
          whoami   identify the calling CC session via $CLAUDE_CODE_SESSION_ID\n  \
          files    which files/dirs a session modified, when (Edit/Write/Notebook + heuristic Bash)\n  \
          recover  reconstruct a file's history from the transcript: segmented diff-patches,\n           \
                   point-in-time partial snapshot, or coverage scoping\n  \
          plan     locate the Plan-Mode plan file bound to a session (recover --file @plan dumps it)\n  \
          verbatim RESTORE the verbatim user/assistant back-and-forth a compaction summary\n           \
                   CLIPPED, within a char budget (live-tail peek is `show --turn`)\n  \
          image    list + extract the inline images a session carries (pastes/screenshots)\n\n\
        list/search/stats/files/recover/plan/image span each session's subagent transcripts by \
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
        leading-hex prefix, e.g. `@13d9645a`) scopes to one session; `@<agent-id>` to one \
        subagent + its subtree (a bare hex ≥12 OR a teammate `aName-<hex>` id; exactly what \
        `csift agents` prints); `@main` is the CALLING top-level session (read from \
        $CLAUDE_CODE_SESSION_ID); a `*.jsonl` path scopes to that one transcript. A BARE uuid \
        (no `@`) is NOT special. `--sessions-from <FILE|->` scopes to a LIST of ids (one per \
        line, e.g. piped from `search -l`) on every multi-target subcommand.\n\n\
        @trap:<marker> answers \"which subagent am I?\" on EVERY subcommand (including `whoami`, which \
        reports your bare hex + parent uuid). CC withholds a subagent's OWN id from its Bash \
        environment, so a running subagent cannot name itself via env. Instead you INVENT a marker \
        and embed it LITERALLY in this very csift command; csift finds the transcript whose Bash \
        `csift` command carries that marker and scopes to that agent's subtree. TIMING: a \
        subagent's transcript records the command mid-run, so a first try resolves; the MAIN \
        conversation flushes its own record only AFTER the command completes, so a top-level \
        FIRST use always misses; from the main thread use `@main` instead (env-based, no race), \
        or re-run the SAME command with the SAME marker. \
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
        explaining what was searched and, under a label filter, where the pattern DOES occur.\n  \
          Ranges       Every range flag shares one grammar: N | A..B | N.. | ..N | .. | -k,\n               \
        endpoints inclusive; `-k` counts from the end, so `-3..` means \"the last 3\". Turns\n               \
        are 0-based logical indices (the `tN` search prints); lines are 1-based physical\n               \
        jsonl lines (the `Lnnnn`). Different axes: read both from output, don't compute.\n  \
          Subagents    Most commands include a session's subagent transcripts automatically;\n               \
        `--no-subagents` restricts. `verbatim` is the one opt-in (`--subagents`, because its\n               \
        budget multiplies per session), and `agents` lists subagents instead of spanning.\n  \
          Caps         No silent truncation, ever: every cap prints what was dropped and how\n               \
        to get the rest. `list` caps an unscoped run at 50 rows; `show` caps at 200 record\n               \
        units and prints the exact continuation command; `--max-count 0` = uncapped, always.\n  \
          Time         Every timestamp prints in YOUR local timezone with the offset inline,\n               \
        like `2026-07-11 15:33:37 AEST(UTC+10)`, derived per instant (DST-correct; fractional\n               \
        zones render like `IST(UTC+05:30)`). Raw UTC lives in JSON `ts_utc`, never in text.\n\n\
        JSON OUTPUT (--format json)\n  \
          Every command emits the same envelope: one `{\"kind\":\"header\",…}` line, then\n  \
        kind-tagged rows, then one `{\"kind\":\"summary\",…}` line, even for zero matches.\n  \
        Select rows with `jq 'select(.kind==\"…\")'`; the summary is always `tail -1`. Do\n  \
        the select BEFORE projecting fields; a field template applied to the mixed stream\n  \
        stamps the header/summary rows as all-null rows. Rows\n  \
        carry the id trio (`session_id` / `is_subagent` / `parent_session_id`), INCLUDING\n  \
        every search hit inside `.hits[]` (v0.6.5), so bare `.hits[]` flattening carries\n  \
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
          - Text-mode excerpts keep a record's LITERAL newlines (a multiline Bash command\n    \
        renders as-is), so piping text output through `head -N` can cut mid-record and\n    \
        hide the overflow pointers. The line-safe machine form is `--format json` (one\n    \
        object per line); the honest caps are `--max-count` and the built-in drop reports.\n\n\
        WHAT csift WILL NOT DO\n  \
          Semantic/BM25 search (regex is the tool; broaden the pattern, or census first);\n  \
        ad-hoc aggregation languages (the closed `--count-by` axes, `stats`, and\n  \
        `files --by` are the built-ins; anything else: `--raw | jq`); diffs (fetch both\n  \
        sides with `show` / `recover --at`, diff outside); writing or terminating\n  \
        anything. csift only reads.\n\n\
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
}

#[derive(Debug, Subcommand)]
pub enum Command {
    // Per-command about/long_about/after_help live on each *Args struct's #[command(...)]
    // block. A variant doc comment here SHADOWS the struct's about + long_about (clap
    // derive precedence) - that shadowing kept every subcommand's long_about invisible
    // until v0.6.2. Keep the variants bare.
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
    // HIDDEN catch-all for the REMOVED `turns` name (→ `verbatim`, v0.5). Exists only so
    // the rename gets the pointed successor error the `-t thinking` legacy values get,
    // instead of clap's teach-nothing "unrecognized subcommand". Never works, always bails
    // (main.rs). Kept bare like every variant (the shadowing rule above); a plain comment,
    // not a doc comment, so nothing ever renders for it.
    #[command(hide = true)]
    Turns(TurnsRenamedArgs),
}

/// Argument sink for the hidden [`Command::Turns`] tombstone: swallows EVERY token (flags
/// included) so the rename error always wins - a flag-parse error must never preempt it.
#[derive(Args, Debug)]
pub struct TurnsRenamedArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., hide = true)]
    pub rest: Vec<std::ffi::OsString>,
}
