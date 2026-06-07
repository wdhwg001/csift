//! Command-line surface (clap derive).
//!
//! Three subcommands: `list`, `search`, `whoami`. The flag set here is the
//! Phase-1 skeleton — it parses and `--help`s correctly, but the handlers it
//! feeds are still `todo!()`. Example-rich `--help` text is filled out as the
//! handlers land (SPEC.md § CLI).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// ripgrep for Claude Code session transcripts.
///
/// csift lists and regex-searches Claude Code `.jsonl` session files under
/// `~/.claude/projects/<encoded>/`. Output is LLM-friendly by default (clear
/// session / turn / category / timestamp headers); pass `--json` for machine use.
#[derive(Debug, Parser)]
#[command(name = "csift", version, about, long_about = None)]
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
pub struct SearchArgs {
    /// Regex pattern (ripgrep-like, default smart-case). MAY be empty for a
    /// pure filter (use `--category` / time / turn filters alone).
    #[arg(value_name = "PATTERN", default_value = "")]
    pub pattern: String,

    /// Restrict to a specific project target (actual cwd or encoded dir).
    /// Repeatable; combine with `--session` to narrow further.
    #[arg(long = "path", value_name = "PATH")]
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
pub struct WhoamiArgs {
    /// Print the resolved jsonl path in addition to the session id.
    #[arg(long)]
    pub path: bool,

    /// Emit JSON instead of text.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}
