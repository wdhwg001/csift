//! Command-line surface (clap derive).
//!
//! Five subcommands: `list`, `search`, `agents`, `whoami`, `files`. Each carries
//! example-rich help (`--help`) keyed off the SPEC §6.1–§6.6 baseline invocations.
//! `list`/`search`/`files` span each session's subagent transcripts by default
//! (`--no-subagents` opts out); `agents` reports a session's subagent lifecycle.
//!
//! ## argv normalization (flag-ordering fix)
//!
//! The real entrypoint is [`parse_argv`], NOT `Cli::parse` — it runs [`normalize_argv`]
//! first so a `--format`/`--kind`/… flag works in ANY position relative to a
//! leading-`-` encoded project target (clap's `allow_hyphen_values` otherwise lets a
//! `Vec` positional greedily swallow the trailing flag). See [`normalize_argv`].

use std::path::PathBuf;

use clap::{ArgAction, Args, CommandFactory, Parser, Subcommand, ValueEnum};

/// Parse + normalize the process argv into a [`Cli`] (the real entrypoint).
///
/// This wraps [`Cli::parse`] with an argv NORMALIZATION pass that fixes the
/// flag-ordering bug described on [`normalize_argv`]: a project-target positional must
/// tolerate a leading `-` (every encoded projects-dir token starts with `-`), which
/// requires clap's `allow_hyphen_values`; but that setting makes a multi-value
/// positional GREEDILY absorb every following token — including declared long flags
/// like `--format json` — so `csift list <ENCODED> --format json` used to fail with
/// `no project dir named "--format"`. We reorder declared flags ahead of the
/// leading-dash positionals BEFORE clap parses, so `--format` works in any position.
#[must_use]
pub fn parse_argv() -> Cli {
    let raw: Vec<String> = std::env::args().collect();
    let normalized = normalize_argv(raw);
    Cli::parse_from(normalized)
}

/// Permissive value parser for a project-target positional/option: accepts ANY token
/// (a real cwd, an encoded `-Users-…` token, `.`). It does NOT reject flag-shaped
/// tokens — the [`normalize_argv`] pre-pass has already routed declared flags away
/// from the positional, so anything reaching here is a genuine target. (Rejecting
/// here would hard-abort clap instead of letting it reconsider the token as a flag,
/// which is why the earlier reject-based attempt failed; see `normalize_argv`.)
fn parse_project_target(s: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(s))
}

/// Reorder argv so declared flags (and the value of value-taking ones) precede the
/// leading-`-` project-target positionals, defeating clap's greedy `allow_hyphen_values`
/// var-arg absorption WITHOUT a hardcoded flag table.
///
/// ## Why a pre-pass (vs a value parser / `allow_hyphen_values` tweak)
///
/// clap issue #3880 + empirical probing (clap 4.6): a `Vec` positional with
/// `allow_hyphen_values=true` swallows every following token — including KNOWN long
/// flags — once it starts consuming, and `num_args`/value parsers cannot undo that
/// (a value-parser `Err` hard-aborts rather than letting clap retry the token as a
/// flag). The robust, well-understood fix is to normalize argv first.
///
/// ## Zero-drift flag discovery
///
/// We introspect csift's OWN [`Cli::command`] for the active subcommand to learn its
/// long flags and which take a value (action `Set`/`Append` take a value; `SetTrue`/
/// `SetFalse`/`Count`/help/version do not). So the flag set is never duplicated — it
/// follows the derive definitions automatically.
///
/// ## Algorithm (per subcommand args, after the subcommand name)
///
/// - `--` terminator: everything after it is passed through verbatim (clap's escape).
/// - A `--flag=value` token → a flag token (its value is inline).
/// - A bare `--flag` token → a flag token; if that flag takes a value AND the next
///   token does not itself start with `-`, the next token is its value (kept paired).
/// - A `-x` short token is left in place (clap resolves declared short flags ahead of
///   an `allow_hyphen_values` positional already; a leading-dash ENCODED token is a
///   single `-` followed by alphanumerics/dashes and is treated as a positional).
/// - Everything else is a positional.
///
/// Flags (with paired values) are emitted first, then positionals, preserving each
/// group's relative order. clap then validates the result and errors clearly on any
/// genuine misuse.
#[must_use]
pub fn normalize_argv(argv: Vec<String>) -> Vec<String> {
    // argv[0] = program; argv[1] = subcommand (if any). Find the subcommand and the
    // index where its args start.
    if argv.len() < 2 {
        return argv;
    }
    let cmd = Cli::command();
    let sub_name = &argv[1];
    let Some(sub) = cmd
        .get_subcommands()
        .find(|s| s.get_name() == sub_name || s.get_all_aliases().any(|a| a == sub_name))
    else {
        // Not a recognized subcommand (e.g. `--help`, `--version`, a typo) — leave
        // argv untouched and let clap produce its normal message.
        return argv;
    };

    // Long flags of this subcommand that TAKE a value (need their following token),
    // and the full set of declared long flags (to detect a value that is actually the
    // NEXT flag, e.g. a user typo `--path --format`).
    let mut value_long: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut all_long: std::collections::HashSet<String> = std::collections::HashSet::new();
    for a in sub.get_arguments() {
        if let Some(longs) = a.get_long_and_visible_aliases() {
            let takes = flag_takes_value(a);
            for l in longs {
                let f = format!("--{l}");
                if takes {
                    value_long.insert(f.clone());
                }
                all_long.insert(f);
            }
        }
    }

    let head = argv[..2].to_vec(); // program + subcommand
    let rest = &argv[2..];

    let mut flags: Vec<String> = Vec::new();
    let mut positionals: Vec<String> = Vec::new();
    let mut passthrough: Vec<String> = Vec::new();

    let mut i = 0;
    let mut after_terminator = false;
    while i < rest.len() {
        let tok = &rest[i];
        if after_terminator {
            passthrough.push(tok.clone());
            i += 1;
            continue;
        }
        if tok == "--" {
            // Everything after the explicit terminator is verbatim positional input.
            after_terminator = true;
            passthrough.push(tok.clone());
            i += 1;
            continue;
        }
        if tok.starts_with("--") {
            // `--flag=value` carries its own value inline; a bare `--flag` may need
            // the next token if it is a value-taking flag.
            if let Some((name, _)) = tok.split_once('=') {
                let _ = name;
                flags.push(tok.clone());
                i += 1;
            } else if value_long.contains(tok) {
                flags.push(tok.clone());
                // A value-taking flag (arity 1) consumes its NEXT token as the value —
                // INCLUDING a leading-`-` encoded token like `--path -Users-foo` (the
                // `--path` option carries `allow_hyphen_values` too). The only tokens
                // that are NOT its value: the `--` terminator, or another DECLARED long
                // flag (a user typo such as `--path --format`, which we leave for clap
                // to report). A bare short `-x` or an encoded `-Users-…` IS consumed.
                if i + 1 < rest.len() && rest[i + 1] != "--" && !all_long.contains(&rest[i + 1]) {
                    flags.push(rest[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            } else {
                // A boolean long flag (or an unknown `--x`): no paired value.
                flags.push(tok.clone());
                i += 1;
            }
        } else {
            // Positional: a real path, `.`, or a leading-`-` encoded token (which is a
            // SINGLE dash; it did not match `--` above).
            positionals.push(tok.clone());
            i += 1;
        }
    }

    let mut out = head;
    out.extend(flags);
    out.extend(positionals);
    out.extend(passthrough);
    out
}

/// True when an arg consumes a value (so its following token is its value, not a
/// separate positional). `Set`/`Append` take a value; `SetTrue`/`SetFalse`/`Count`/
/// help/version do not.
fn flag_takes_value(a: &clap::Arg) -> bool {
    !matches!(
        a.get_action(),
        ArgAction::SetTrue
            | ArgAction::SetFalse
            | ArgAction::Count
            | ArgAction::Help
            | ArgAction::HelpShort
            | ArgAction::HelpLong
            | ArgAction::Version
    )
}

/// ripgrep for Claude Code session transcripts.
#[derive(Debug, Parser)]
#[command(
    name = "csift",
    version,
    about = "ripgrep for Claude Code session transcripts",
    long_about = "csift — \"ripgrep for Claude Code session transcripts\".\n\n\
        A fast Rust CLI to list and regex-search Claude Code session `.jsonl` files \
        under ~/.claude/projects/<encoded>/. Built for an LLM consumer (a CC agent \
        searching or recovering its own / a peer session): output is clean, \
        token-efficient and regex-driven. Human/LLM-readable text by default; pass \
        `--format json` for machine use.\n\n\
        Pure regex/ripgrep only — explicitly NO BM25 / embeddings / semantic search.\n\n\
        SUBCOMMANDS\n  \
          list     fast \"which session is this?\" index (first/last user + last agent)\n  \
          search   regex over transcripts, returning the complete round-trip exchange per hit\n  \
          agents   list a session's subagents (kind, start/completion, status) + time-window filter\n  \
          whoami   identify the calling CC session via $CLAUDE_CODE_SESSION_ID\n  \
          files    which files/dirs a session modified, when (Edit/Write/Notebook + heuristic Bash)\n\n\
        list/search/files span each session's subagent transcripts by default (built-in \
        Task/Agent-tool, OMC, and Workflow agents); pass `--no-subagents` to restrict \
        to top-level sessions.\n\n\
        A target is EITHER a real filesystem cwd (it gets path-encoded) OR an \
        already-encoded `-Users-...` projects-dir token; with no target, every \
        project under ~/.claude/projects is scanned.",
    after_help = "EXAMPLES\n  \
          csift list                                  # index every session, all projects\n  \
          csift list .                                # just this cwd's project\n  \
          csift search \"carry\"                        # smart-case regex, all projects\n  \
          csift search \"\" -t user --since 2h --path . # pure filter: user turns, last 2h, here\n  \
          csift agents --session <uuid> --since 2h    # subagents started in the last 2h\n  \
          csift whoami                                # who am I (this CC session)?\n  \
          csift files <uuid> --by-file                # which files this session modified, when\n\n\
        Run `csift <subcommand> --help` for per-subcommand flags + examples."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List sessions with first/last genuine-user + last-agent message and timestamps.
    List(ListArgs),
    /// Regex-search sessions, returning complete request/response round-trip exchanges.
    Search(SearchArgs),
    /// Identify the calling Claude Code session (via CLAUDE_CODE_SESSION_ID).
    Whoami(WhoamiArgs),
    /// List a session's subagents with kind, start/completion timestamps + status.
    Agents(AgentsArgs),
    /// Which files/dirs a session modified, when (Edit/Write/Notebook + heuristic Bash).
    Files(FilesArgs),
}

/// A transcript content category, used by `search --category/-t` (repeatable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Category {
    /// Assistant thinking blocks.
    Thinking,
    /// GENUINE human input + user answers to AskUserQuestion (NOT tool_result carriers).
    User,
    /// `tool_use` blocks (agent calling a tool); AskUserQuestion is a tool_use.
    Tool,
    /// `tool_result` blocks (the tool's response).
    #[value(name = "tool-response")]
    ToolResponse,
    /// Assistant visible end-of-turn text (the agent message).
    Agent,
}

/// How to render matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum OutputFormat {
    /// Human/LLM-readable with headers (default).
    #[default]
    Text,
    /// One JSON object per emitted unit, for machine consumption.
    Json,
}

#[derive(Debug, Args)]
#[command(
    long_about = "List sessions with a fast quick-identity tuple per session — WITHOUT \
        parsing the whole file. For each session jsonl under the target(s) it emits: \
        session id, the FIRST genuine-user message (+ time), the LAST genuine-user \
        message (+ time), the LAST agent message (+ time), plus the decoded cwd / git \
        branch / CC version. A forward HEAD read finds the first user; a backward \
        TAIL read finds the last user/agent — neither parses the full file, so it \
        stays fast on 200 MB+ transcripts. Files are scanned in parallel across the \
        corpus, then sorted for deterministic output.\n\n\
        A PATH is either a real cwd (path-encoded for you) or an already-encoded \
        `-Users-...` token; with no PATH, every project is listed. Note: genuine-user \
        excludes tool_result carriers, isMeta pseudo-turns (\"Continue from where you \
        left off.\"), and compaction summaries.",
    after_help = "EXAMPLES\n  \
          csift list                                                  # every session, all projects\n  \
          csift list .                                                # sessions for the current cwd's project\n  \
          csift list /Users/testuser/Projects/widget_app_prototype  # a real path (gets encoded)\n  \
          csift list -Users-testuser-Projects-widget-app-prototype  # a pre-encoded dir token\n  \
          csift list ~/.claude/projects/-Users-testuser-Projects-widget-app-prototype\n  \
          csift list --format json .                                  # machine-readable index"
)]
pub struct ListArgs {
    /// One or more targets: an actual filesystem cwd, or a direct
    /// `~/.claude/projects/<encoded>` path / bare `<encoded>` dir. Repeatable.
    /// Defaults to all projects.
    ///
    /// `allow_hyphen_values` is REQUIRED: every encoded dir starts with `-`
    /// (an absolute cwd's leading `/` encodes to `-`), e.g.
    /// `csift list -Users-testuser-Projects-foo`. Without this clap would reject the
    /// leading `-` token as an unknown flag (SPEC §6.1 baseline invocation). The
    /// `parse_project_target` value parser NARROWS that tolerance so a real flag
    /// (`--format json`) is no longer swallowed as a PATH value in any position.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// Also discover + list each session's SUBAGENT transcripts (built-in
    /// Task/Agent-tool, OMC, and Workflow agents) found under
    /// `<session>/subagents/**`. Default ON; pass `--no-subagents` to restrict to
    /// top-level sessions only. Workflow `journal.jsonl` event logs are never
    /// transcripts and are always excluded.
    #[arg(
        long = "include-subagents",
        overrides_with = "no_subagents",
        default_value_t = true
    )]
    pub include_subagents: bool,

    /// Exclude subagent transcripts — list only the top-level `<uuid>.jsonl`
    /// sessions (the pre-subagent behavior). Overrides `--include-subagents`.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl ListArgs {
    /// Resolve the include/exclude flags into a single decision (default include).
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        !self.no_subagents
    }
}

#[derive(Debug, Args)]
#[command(
    long_about = "Regex-search transcripts, returning the COMPLETE round-trip exchange \
        containing each hit — never a bare fragment. A turn is delimited by a genuine \
        user message; the emitted exchange is the whole turn (the opening user record \
        plus every assistant/thinking/tool_use/tool_result record chained under it \
        until the next genuine user). So a matched tool_use comes WITH its \
        tool_result, a matched user turn WITH the agent's response, etc.\n\n\
        The PATTERN is ripgrep-like and defaults to SMART-CASE: case-insensitive \
        unless it contains an uppercase letter; `-i` forces case-insensitive and \
        always wins. `--multiline` lets `.` cross newlines. An EMPTY pattern is a \
        pure filter — it matches every category-eligible record, so combine it with \
        `--category` / `--since` / `--turn-range` (a bare empty pattern with no other \
        filter warns that it will emit a lot).\n\n\
        CATEGORIES (`-t`, repeatable): thinking | user | tool | tool-response | agent. \
        With none given, all five are eligible. `user` is the genuine human turn PLUS \
        answers to AskUserQuestion; it excludes tool_result carriers. `tool` includes \
        AskUserQuestion tool_use calls.\n\n\
        WINDOWING: `--turn-range START..END` (inclusive, 0-based on genuine-user \
        order) is mutually exclusive with `--since`/`--until`. Time bounds accept \
        ISO8601 (`2026-06-01`, `2026-06-01T05:00:00Z`) or a relative form (`2h`, \
        `3d`, `90m`, `45s`, `1w`) meaning \"that long ago\" in the system local timezone.\n\n\
        `--max-count` caps emitted exchanges but reports the dropped count — there is \
        NO silent truncation anywhere.",
    after_help = "EXAMPLES\n  \
          csift search \"carry\"                                  # all projects, smart-case\n  \
          csift search -i \"askuserquestion\" -t tool             # tool_use blocks naming AUQ\n  \
          csift search \"\" -t user --since 2h --path .            # user turns, last 2h, this project\n  \
          csift search \"tail.read\" --multiline --session 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d\n  \
          csift search \"panic\" -t agent -t thinking --turn-range 10..20 --max-count 50\n  \
          csift search \"persisted-output\" --resolve-persisted --format json\n\n\
        REGEX DIALECT — linear-time (RE2-class)\n  \
          The pattern is the Rust `regex` crate (regex::bytes), which GUARANTEES \
        linear-time matching in the input length: NO catastrophic backtracking, ever.\n  \
          Supported: literals; character classes [...] / [^...] / \\d \\w \\s and \
        Unicode classes \\p{...}; alternation |; groups (...) and non-capturing \
        (?:...); quantifiers * + ? {m,n} (greedy + lazy *?); anchors ^ $ \\b \\B; \
        dot . (use --multiline to let it cross newlines); inline flags (?i)(?m)(?s)(?x); \
        Unicode-aware by default.\n  \
          NOT supported (these need non-linear engines): backreferences \\1; \
        lookahead/lookbehind (?=) (?!) (?<=) (?<!); atomic groups / possessive \
        quantifiers (?>...) / a*+. A pattern using these fails to COMPILE with a clear \
        error — by design, not a bug.\n  \
          Case: smart-case by default (insensitive unless the pattern has an uppercase \
        letter); -i forces insensitive. --multiline lives in the SAME dialect (it sets \
        the (?s)(?m) flags)."
)]
pub struct SearchArgs {
    /// Regex pattern (ripgrep-like, default smart-case). MAY be empty for a
    /// pure filter (use `--category` / time / turn filters alone).
    #[arg(value_name = "PATTERN", default_value = "")]
    pub pattern: String,

    /// Restrict to a specific project target (actual cwd or encoded dir).
    /// Repeatable; combine with `--session` to narrow further.
    ///
    /// `allow_hyphen_values` is REQUIRED: an encoded dir token starts with `-`
    /// (e.g. `--path -Users-testuser-Projects-foo`); without it clap rejects the
    /// leading `-` value as an unknown flag (`-U …`). Same fix as `list`'s PATH:
    /// the `parse_project_target` value parser keeps the leading-`-` tolerance for
    /// encoded tokens while refusing to absorb a following flag as a value.
    #[arg(
        long = "path",
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// Restrict to a single session id (uuid).
    #[arg(long, value_name = "SESSION_ID")]
    pub session: Option<String>,

    /// Also search each in-scope session's SUBAGENT transcripts (built-in
    /// Task/Agent-tool, OMC, and Workflow agents) under `<session>/subagents/**`,
    /// so a hit in a subagent's work is found alongside the main thread. Default
    /// ON; pass `--no-subagents` to search only top-level sessions. Workflow
    /// `journal.jsonl` event logs are not transcripts and are never searched.
    #[arg(
        long = "include-subagents",
        overrides_with = "no_subagents",
        default_value_t = true
    )]
    pub include_subagents: bool,

    /// Exclude subagent transcripts — search only the top-level `<uuid>.jsonl`
    /// sessions. Overrides `--include-subagents`.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Filter to one or more categories. Repeatable.
    #[arg(short = 't', long = "category", value_enum)]
    pub categories: Vec<Category>,

    /// Case-insensitive match (overrides smart-case).
    #[arg(short = 'i', long)]
    pub ignore_case: bool,

    /// Allow `.` to match newlines / multiline patterns.
    #[arg(long)]
    pub multiline: bool,

    /// Inclusive turn-index range `START..END` (a turn = a genuine-user message).
    /// Mutually exclusive with `--since` / `--until`.
    #[arg(long, value_name = "START..END")]
    pub turn_range: Option<String>,

    /// Lower time bound (ISO8601 or relative). Mutually exclusive with --turn-range.
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (ISO8601 or relative). Mutually exclusive with --turn-range.
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Cap emitted exchanges. NO silent truncation — the drop count is reported.
    #[arg(long, value_name = "N")]
    pub max_count: Option<usize>,

    /// Resolve `<persisted-output>` pointers to their `tool-results/<id>.txt` file.
    #[arg(long)]
    pub resolve_persisted: bool,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl SearchArgs {
    /// Resolve the include/exclude flags into a single decision (default include).
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        !self.no_subagents
    }
}

/// Which subagent kinds to surface in `agents`. Mirrors the on-disk discriminator
/// (path location), with `agentType` retained as a descriptive sub-label per row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AgentKindFilter {
    /// Built-in Task/Agent-tool subagents (`subagents/agent-<hex>.jsonl`).
    #[value(name = "builtin-task")]
    BuiltinTask,
    /// Workflow / OMC workflow-subagent transcripts
    /// (`subagents/workflows/wf_*/agent-<hex>.jsonl`).
    Workflow,
}

#[derive(Debug, Args)]
#[command(
    long_about = "List a session's SUBAGENT lifecycle: every subagent transcript the \
        session spawned, with its id, KIND, start + completion timestamps, duration, \
        and a determinable status. Three on-disk shapes are discovered under \
        `<session>/subagents/**` (verified empirically against ~/.claude/projects):\n  \
          • builtin-task  subagents/agent-<hex>.jsonl                 (Task/Agent tool)\n  \
          • workflow      subagents/workflows/wf_<id>/agent-<hex>.jsonl (OMC workflows)\n\
        Workflow `journal.jsonl` event logs are NOT transcripts — they are read only \
        to corroborate completion status, never listed as agents.\n\n\
        The TARGET selects the parent session: pass `--session <uuid>` for one \
        session, or a project PATH/encoded-dir to cover every session under it (each \
        session's subagents are grouped under it). Start/completion come from the \
        subagent transcript's first/last record timestamp; status is `completed` when \
        a workflow journal carries a `result` event for the agent (or the transcript \
        terminates cleanly), else `running`/`unknown`.\n\n\
        `--since`/`--until` (ISO8601 or relative `2h`/`3d`/…, in the system local \
        timezone) filter to subagents whose START falls in the window by default; \
        `--by completion` filters on completion time instead.",
    after_help = "EXAMPLES\n  \
          csift agents --session 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d      # one session's subagents\n  \
          csift agents .                                                   # every session under this project\n  \
          csift agents . --kind workflow                                   # only workflow agents\n  \
          csift agents --session <uuid> --since 2h                         # subagents STARTED in the last 2h\n  \
          csift agents --session <uuid> --since 09:00 --by completion      # COMPLETED since a bound\n  \
          csift agents . --format json                                     # machine-readable lifecycle rows"
)]
pub struct AgentsArgs {
    /// Project target (actual cwd or encoded dir) whose sessions' subagents to list.
    /// Optional when `--session` is given; with neither, every project is scanned.
    /// Repeatable.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// Restrict to a single parent session id (uuid). Combine with a PATH to scope
    /// the search, or use alone to scan all projects for that session.
    #[arg(long, value_name = "SESSION_ID")]
    pub session: Option<String>,

    /// Only show subagents of this kind (repeatable). Default: all kinds.
    #[arg(long = "kind", value_enum)]
    pub kinds: Vec<AgentKindFilter>,

    /// Lower time bound (ISO8601 or relative `2h`/`3d`/…). Filters by START time by
    /// default (`--by completion` switches to completion time).
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (ISO8601 or relative). Same axis as `--since`.
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Which timestamp `--since`/`--until` filter on: `start` (default) or
    /// `completion`.
    #[arg(long = "by", value_enum, default_value_t = AgentTimeAxis::Start)]
    pub by: AgentTimeAxis,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

/// Which lifecycle timestamp the `agents` time window filters on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum AgentTimeAxis {
    /// Filter on the subagent's START timestamp (first transcript record).
    #[default]
    Start,
    /// Filter on the subagent's COMPLETION timestamp (last transcript record).
    Completion,
}

/// The aggregation detail level for `files` (exactly one is active; default summary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesDetail {
    /// Per-top-level-dir op rollup (the smallest output; the DEFAULT).
    Summary,
    /// One row per distinct directory (full path) with per-op + distinct-file counts.
    ByDir,
    /// One row per distinct file with per-op counts + first/last touch timestamps.
    ByFile,
    /// Full chronological list, one line per mutation (the verbose, opt-in mode).
    Timeline,
}

#[derive(Debug, Args)]
#[command(
    long_about = "Show which FILES and DIRECTORIES a session modified, and when. csift \
        extracts file mutations from a session's transcript (spanning its subagents by \
        default, since OMC fan-out edits happen in subagents):\n  \
          • AUTHORITATIVE  Edit / Write / MultiEdit (input.file_path) + NotebookEdit \
        (input.notebook_path), with create-vs-edit resolved from the paired \
        tool_result (`type:\"create\"` = a new file).\n  \
          • HEURISTIC      Bash file mutations, parsed LEXICALLY from the command \
        string (rm/mv/cp/mkdir/touch/tee/sed -i/git/redirection). Bash carries no path \
        field in its result, so these are best-effort and ALWAYS labelled `(heuristic)`.\n\n\
        DETAIL LEVELS (mutually exclusive; exactly one applies):\n  \
          --summary   (DEFAULT) compact per-top-level-dir op rollup — the smallest output\n  \
          --by-dir    one row per distinct directory (per-op + distinct-file counts + first/last)\n  \
          --by-file   one row per distinct file (per-op counts + first/last touch)\n  \
          --timeline  full chronological list, one line per mutation (HEAVY — opt-in only)\n\n\
        The TARGET selects the session(s): `--session <uuid>` for one, or a project \
        PATH/encoded-dir for every session under it; with neither, all projects are \
        scanned. `--no-subagents` restricts to the top-level session.\n\n\
        WINDOWING: `--turn-range START..END` (inclusive, 0-based on genuine-user order) \
        is mutually exclusive with `--since`/`--until`. Time bounds accept ISO8601 \
        (`2026-06-01`, `2026-06-01T05:00:00Z`) or a relative form (`2h`, `3d`, `90m`, \
        `45s`, `1w`) meaning \"that long ago\" in the system-local timezone; a mutation \
        with no timestamp never falls inside a bounded window.\n\n\
        No silent truncation: skipped malformed lines are counted and surfaced.",
    after_help = "EXAMPLES\n  \
          csift files <uuid>                          # default summary: per-top-level-dir op rollup\n  \
          csift files <uuid> --by-file                # per-file op counts + first/last touch\n  \
          csift files <uuid> --timeline --since 2h    # full chronological, last 2h (heavy)\n  \
          csift files . --format json --by-dir        # machine-readable per-dir rollup\n\n\
        ACID TEST: \"how many distinct gap docs touched / how many /tmp docs created?\"\n  \
          csift files <uuid> --by-file                # count rows ending in gaps-style docs\n  \
          csift files <uuid> --by-file --format json  # filter path under /tmp with is_create==true"
)]
pub struct FilesArgs {
    /// Project target(s) (actual cwd or encoded dir) whose sessions' file mutations to
    /// report. Optional when `--session` is given; with neither, every project is
    /// scanned. Repeatable.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// Restrict to a single parent session id (uuid).
    #[arg(long, value_name = "SESSION_ID")]
    pub session: Option<String>,

    /// Also attribute file mutations a SUBAGENT performed (built-in Task/Agent-tool,
    /// OMC, and Workflow agents) under the session. Default ON; pass `--no-subagents`
    /// to restrict to the top-level session. Important because OMC fan-out edits happen
    /// in subagents.
    #[arg(
        long = "include-subagents",
        overrides_with = "no_subagents",
        default_value_t = true
    )]
    pub include_subagents: bool,

    /// Exclude subagent transcripts — report only the top-level `<uuid>.jsonl`
    /// session's mutations. Overrides `--include-subagents`.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// DEFAULT detail level: compact per-top-level-dir op rollup (the smallest output).
    #[arg(long, group = "detail")]
    pub summary: bool,

    /// One row per distinct directory (full path) with per-op + distinct-file counts.
    #[arg(long = "by-dir", group = "detail")]
    pub by_dir: bool,

    /// One row per distinct file with per-op counts + first/last touch timestamps.
    #[arg(long = "by-file", group = "detail")]
    pub by_file: bool,

    /// Full chronological list, one line per mutation (HEAVY — never the default).
    #[arg(long, group = "detail")]
    pub timeline: bool,

    /// Inclusive turn-index range `START..END` (a turn = a genuine-user message).
    /// Mutually exclusive with `--since` / `--until`.
    #[arg(long, value_name = "START..END")]
    pub turn_range: Option<String>,

    /// Lower time bound (ISO8601 or relative). Mutually exclusive with --turn-range.
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (ISO8601 or relative). Mutually exclusive with --turn-range.
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl FilesArgs {
    /// Resolve the include/exclude flags into a single decision (default include).
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        !self.no_subagents
    }

    /// Resolve the four detail-level bool flags into the active [`FilesDetail`]. clap's
    /// `group = "detail"` (which is `multiple = false` by default) rejects more than one
    /// at parse time, so at most one is set here; none set ⇒ the `Summary` default.
    #[must_use]
    pub fn detail(&self) -> FilesDetail {
        if self.by_dir {
            FilesDetail::ByDir
        } else if self.by_file {
            FilesDetail::ByFile
        } else if self.timeline {
            FilesDetail::Timeline
        } else {
            FilesDetail::Summary
        }
    }
}

#[derive(Debug, Args)]
#[command(
    long_about = "Identify the CALLING Claude Code session, false-positive-safe.\n\n\
        Claude Code exports `CLAUDE_CODE_SESSION_ID` into every Bash-tool environment, \
        and its value equals the calling session's own jsonl basename exactly. That is \
        the ONLY signal csift trusts: per-session, version-independent, survives bash \
        nesting, zero false positives. (The exact var name is matched — never a loose \
        /session/i regex, which would false-positive on macOS's SECURITYSESSIONID.)\n\n\
        When the var is absent/empty (an old CC build, or running outside CC), whoami \
        does NOT guess — it errors with guidance to pass `--session <uuid>`. \
        Most-recent-mtime and process-tree walking are FORBIDDEN: many CC sessions \
        may be live at once, so mtime is almost always wrong. It is acceptable for \
        whoami to often say \"ambiguous, pass --session\".",
    after_help = "EXAMPLES\n  \
          csift whoami                  # print the calling session's uuid (+ its jsonl path if found)\n  \
          csift whoami --path           # always show the resolved jsonl path (or a not-found note)\n  \
          csift whoami --format json    # {\"session_id\":\"…\",\"path\":\"…\"}"
)]
pub struct WhoamiArgs {
    /// Print the resolved jsonl path in addition to the session id.
    #[arg(long)]
    pub path: bool,

    /// Emit JSON instead of text.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// clap's own internal consistency check — catches duplicate flags, bad
    /// `overrides_with` targets, malformed value parsers at build time.
    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }

    /// Parse through the SAME normalization pass the real entrypoint uses
    /// (`parse_argv` → `normalize_argv` → clap), so the flag-ordering fix is what we
    /// actually test, not bare clap.
    fn parse(argv: &[&str]) -> Result<Cli, clap::Error> {
        let owned: Vec<String> = argv.iter().map(|s| (*s).to_string()).collect();
        Cli::try_parse_from(normalize_argv(owned))
    }

    // ── The reported flag-ordering bug (CONFIRMED against target/debug/csift) ──

    #[test]
    fn list_format_json_after_encoded_positional_now_works() {
        // The exact repro: `--format json` AFTER a leading-`-` encoded positional
        // used to be swallowed as two extra PATH values. It must now parse as the
        // format flag, with the positional intact.
        let cli = parse(&[
            "csift",
            "list",
            "-Users-testuser-Projects-widget-app-prototype",
            "--format",
            "json",
        ])
        .expect("flag after encoded positional must parse");
        match cli.command {
            Command::List(a) => {
                assert_eq!(a.format, OutputFormat::Json);
                assert_eq!(a.paths.len(), 1);
                assert_eq!(
                    a.paths[0].to_string_lossy(),
                    "-Users-testuser-Projects-widget-app-prototype"
                );
            }
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn list_format_json_before_positional_still_works() {
        // The old workaround ordering must keep working (backward-compatible).
        let cli = parse(&[
            "csift",
            "list",
            "--format",
            "json",
            "-Users-testuser-Projects-foo",
        ])
        .expect("flag before positional still parses");
        match cli.command {
            Command::List(a) => {
                assert_eq!(a.format, OutputFormat::Json);
                assert_eq!(a.paths.len(), 1);
            }
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn search_path_flag_ordering_fixed() {
        // Same latent risk on `search --path <encoded> --format json` (cli.rs:162).
        let cli = parse(&[
            "csift",
            "search",
            "carry",
            "--path",
            "-Users-testuser-Projects-foo",
            "--format",
            "json",
        ])
        .expect("search --path then --format must parse");
        match cli.command {
            Command::Search(a) => {
                assert_eq!(a.format, OutputFormat::Json);
                assert_eq!(a.paths.len(), 1);
                assert_eq!(a.pattern, "carry");
            }
            _ => panic!("expected search"),
        }
    }

    #[test]
    fn bare_encoded_token_still_accepted_as_target() {
        // The leading-`-` tolerance must remain for genuine encoded tokens.
        let cli = parse(&["csift", "list", "-a--claude-b"]).expect("encoded token parses");
        match cli.command {
            Command::List(a) => assert_eq!(a.paths[0].to_string_lossy(), "-a--claude-b"),
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn real_path_target_still_accepted() {
        let cli = parse(&["csift", "list", "/Users/testuser/Projects/foo"]).expect("real path parses");
        match cli.command {
            Command::List(a) => {
                assert_eq!(a.paths[0].to_string_lossy(), "/Users/testuser/Projects/foo")
            }
            _ => panic!("expected list"),
        }
    }

    // ── Value parser is permissive (the normalizer routes flags, not this) ──

    #[test]
    fn parse_project_target_accepts_any_target() {
        assert!(parse_project_target("-Users-testuser-Projects-foo").is_ok());
        assert!(parse_project_target("/Users/testuser/Projects/foo").is_ok());
        assert!(parse_project_target(".").is_ok());
        assert!(parse_project_target("-a--claude-b").is_ok());
    }

    // ── argv normalizer (the actual fix mechanism) ──

    #[test]
    fn normalize_moves_long_flag_with_value_ahead_of_positional() {
        let out = normalize_argv(
            ["csift", "list", "-Users-foo", "--format", "json"]
                .map(String::from)
                .to_vec(),
        );
        // program + subcommand stay first; the flag+value are pulled ahead of the
        // leading-`-` positional.
        assert_eq!(out, vec!["csift", "list", "--format", "json", "-Users-foo"]);
    }

    #[test]
    fn normalize_keeps_boolean_flag_without_grabbing_a_value() {
        let out = normalize_argv(
            ["csift", "list", "-Users-foo", "--no-subagents"]
                .map(String::from)
                .to_vec(),
        );
        // `--no-subagents` is a SetTrue flag → it must NOT consume the positional.
        assert_eq!(out, vec!["csift", "list", "--no-subagents", "-Users-foo"]);
    }

    #[test]
    fn normalize_passes_through_after_double_dash() {
        let out = normalize_argv(
            ["csift", "list", "--", "-weird-token", "--not-a-flag"]
                .map(String::from)
                .to_vec(),
        );
        // After `--`, tokens are verbatim positionals (clap's escape) — untouched.
        assert_eq!(
            out,
            vec!["csift", "list", "--", "-weird-token", "--not-a-flag"]
        );
    }

    #[test]
    fn normalize_leaves_unknown_subcommand_untouched() {
        let out = normalize_argv(["csift", "--help"].map(String::from).to_vec());
        assert_eq!(out, vec!["csift", "--help"]);
    }

    #[test]
    fn normalize_handles_equals_form() {
        let out = normalize_argv(
            ["csift", "list", "-Users-foo", "--format=json"]
                .map(String::from)
                .to_vec(),
        );
        assert_eq!(out, vec!["csift", "list", "--format=json", "-Users-foo"]);
    }

    #[test]
    fn normalize_value_flag_at_end_has_no_following_value() {
        // A value-taking flag with NOTHING after it → the `i+1 < rest.len()` FALSE arm
        // (no value consumed). clap will later report the missing value; normalize
        // just leaves the flag in place.
        let out = normalize_argv(
            ["csift", "search", "x", "--since"]
                .map(String::from)
                .to_vec(),
        );
        assert_eq!(out, vec!["csift", "search", "--since", "x"]);
    }

    #[test]
    fn normalize_value_flag_followed_by_another_flag_does_not_consume_it() {
        // `--since --format json`: `--since` is value-taking but the NEXT token is a
        // declared long flag → it must NOT be consumed as the value (the
        // `!all_long.contains(next)` FALSE arm). Both flags are emitted; clap reports
        // the missing `--since` value.
        let out = normalize_argv(
            ["csift", "search", "x", "--since", "--format", "json"]
                .map(String::from)
                .to_vec(),
        );
        // --since stays unpaired; --format json stay paired; positional `x` last.
        assert_eq!(
            out,
            vec!["csift", "search", "--since", "--format", "json", "x"]
        );
    }

    #[test]
    fn normalize_value_flag_before_double_dash_does_not_consume_terminator() {
        // `--since -- rest`: the `--` terminator must NOT be eaten as --since's value
        // (the `rest[i+1] != "--"` FALSE arm).
        let out = normalize_argv(
            ["csift", "search", "--since", "--", "-weird"]
                .map(String::from)
                .to_vec(),
        );
        // --since left unpaired; everything after `--` passes through verbatim.
        assert_eq!(out, vec!["csift", "search", "--since", "--", "-weird"]);
    }

    // ── Subagent inclusion flags (default include) ──

    #[test]
    fn list_includes_subagents_by_default() {
        let cli = parse(&["csift", "list"]).unwrap();
        match cli.command {
            Command::List(a) => assert!(a.want_subagents(), "default must include subagents"),
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn list_no_subagents_excludes() {
        let cli = parse(&["csift", "list", "--no-subagents"]).unwrap();
        match cli.command {
            Command::List(a) => assert!(!a.want_subagents()),
            _ => panic!("expected list"),
        }
    }

    #[test]
    fn search_no_subagents_excludes() {
        let cli = parse(&["csift", "search", "x", "--no-subagents"]).unwrap();
        match cli.command {
            Command::Search(a) => assert!(!a.want_subagents()),
            _ => panic!("expected search"),
        }
    }

    // ── agents subcommand parsing ──

    #[test]
    fn agents_session_and_window() {
        let cli = parse(&[
            "csift",
            "agents",
            "--session",
            "abc",
            "--since",
            "2h",
            "--by",
            "completion",
        ])
        .unwrap();
        match cli.command {
            Command::Agents(a) => {
                assert_eq!(a.session.as_deref(), Some("abc"));
                assert_eq!(a.since.as_deref(), Some("2h"));
                assert_eq!(a.by, AgentTimeAxis::Completion);
            }
            _ => panic!("expected agents"),
        }
    }

    #[test]
    fn agents_kind_filter_and_default_axis() {
        let cli = parse(&["csift", "agents", ".", "--kind", "workflow"]).unwrap();
        match cli.command {
            Command::Agents(a) => {
                assert_eq!(a.kinds, vec![AgentKindFilter::Workflow]);
                assert_eq!(a.by, AgentTimeAxis::Start, "default axis is start");
            }
            _ => panic!("expected agents"),
        }
    }

    // ── files subcommand parsing ──

    #[test]
    fn files_default_detail_is_summary() {
        let cli = parse(&["csift", "files", "--session", "abc"]).unwrap();
        match cli.command {
            Command::Files(a) => {
                assert_eq!(a.detail(), FilesDetail::Summary, "default is summary");
                assert!(a.want_subagents(), "subagents spanned by default");
                assert_eq!(a.session.as_deref(), Some("abc"));
            }
            _ => panic!("expected files"),
        }
    }

    #[test]
    fn files_by_file_selects_by_file() {
        let cli = parse(&["csift", "files", "--session", "abc", "--by-file"]).unwrap();
        match cli.command {
            Command::Files(a) => assert_eq!(a.detail(), FilesDetail::ByFile),
            _ => panic!("expected files"),
        }
    }

    #[test]
    fn files_by_dir_and_timeline_select_levels() {
        let by_dir = parse(&["csift", "files", ".", "--by-dir"]).unwrap();
        match by_dir.command {
            Command::Files(a) => assert_eq!(a.detail(), FilesDetail::ByDir),
            _ => panic!("expected files"),
        }
        let timeline = parse(&["csift", "files", ".", "--timeline", "--since", "2h"]).unwrap();
        match timeline.command {
            Command::Files(a) => {
                assert_eq!(a.detail(), FilesDetail::Timeline);
                assert_eq!(a.since.as_deref(), Some("2h"));
            }
            _ => panic!("expected files"),
        }
    }

    #[test]
    fn files_explicit_summary_flag_is_summary() {
        let cli = parse(&["csift", "files", ".", "--summary"]).unwrap();
        match cli.command {
            Command::Files(a) => assert_eq!(a.detail(), FilesDetail::Summary),
            _ => panic!("expected files"),
        }
    }

    #[test]
    fn files_two_detail_flags_conflict() {
        // The clap `group = "detail"` (multiple=false) rejects two detail flags.
        let err = parse(&["csift", "files", ".", "--by-file", "--by-dir"]);
        assert!(err.is_err(), "two detail levels must be a clap conflict");
    }

    #[test]
    fn files_no_subagents_excludes() {
        let cli = parse(&["csift", "files", ".", "--no-subagents"]).unwrap();
        match cli.command {
            Command::Files(a) => assert!(!a.want_subagents()),
            _ => panic!("expected files"),
        }
    }

    #[test]
    fn files_encoded_token_then_flag_ordering() {
        // normalize_argv must route a trailing flag ahead of a leading-`-` token.
        let cli = parse(&["csift", "files", "-Users-foo", "--format", "json"]).unwrap();
        match cli.command {
            Command::Files(a) => {
                assert_eq!(a.format, OutputFormat::Json);
                assert_eq!(a.paths.len(), 1);
                assert_eq!(a.paths[0].to_string_lossy(), "-Users-foo");
            }
            _ => panic!("expected files"),
        }
    }
}
