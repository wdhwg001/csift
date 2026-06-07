//! Command-line surface (clap derive).
//!
//! Three subcommands: `list`, `search`, `whoami`. Each carries example-rich help
//! (`--help`) keyed off the SPEC §6.1–§6.3 baseline invocations.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

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
          search   regex over transcripts, returning the complete x exchange per hit\n  \
          whoami   identify the calling CC session via $CLAUDE_CODE_SESSION_ID\n\n\
        A target is EITHER a real filesystem cwd (it gets path-encoded) OR an \
        already-encoded `-Users-...` projects-dir token; with no target, every \
        project under ~/.claude/projects is scanned.",
    after_help = "EXAMPLES\n  \
          csift list                                  # index every session, all projects\n  \
          csift list .                                # just this cwd's project\n  \
          csift search \"carry\"                        # smart-case regex, all projects\n  \
          csift search \"\" -t user --since 2h --path . # pure filter: user turns, last 2h, here\n  \
          csift whoami                                # who am I (this CC session)?\n\n\
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
    /// Regex-search sessions, returning complete request/response x exchanges.
    Search(SearchArgs),
    /// Identify the calling Claude Code session (via CLAUDE_CODE_SESSION_ID).
    Whoami(WhoamiArgs),
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
    /// leading `-` token as an unknown flag (SPEC §6.1 baseline invocation).
    #[arg(value_name = "PATH", allow_hyphen_values = true)]
    pub paths: Vec<PathBuf>,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

#[derive(Debug, Args)]
#[command(
    long_about = "Regex-search transcripts, returning the COMPLETE round-trip (x) \
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
        `3d`, `90m`, `45s`, `1w`) meaning \"that long ago\" in Australia/Sydney.\n\n\
        `--max-count` caps emitted exchanges but reports the dropped count — there is \
        NO silent truncation anywhere.",
    after_help = "EXAMPLES\n  \
          csift search \"carry\"                                  # all projects, smart-case\n  \
          csift search -i \"askuserquestion\" -t tool             # tool_use blocks naming AUQ\n  \
          csift search \"\" -t user --since 2h --path .            # user turns, last 2h, this project\n  \
          csift search \"tail.read\" --multiline --session 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d\n  \
          csift search \"panic\" -t agent -t thinking --turn-range 10..20 --max-count 50\n  \
          csift search \"persisted-output\" --resolve-persisted --format json"
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
    /// leading `-` value as an unknown flag (`-U …`). Same fix as `list`'s PATH.
    #[arg(long = "path", value_name = "PATH", allow_hyphen_values = true)]
    pub paths: Vec<PathBuf>,

    /// Restrict to a single session id (uuid).
    #[arg(long, value_name = "SESSION_ID")]
    pub session: Option<String>,

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
