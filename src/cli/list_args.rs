//! ListArgs.

use super::*;

#[derive(Debug, Args)]
#[command(
    about = "List sessions with first/last genuine-user + last-agent message and timestamps",
    long_about = "List sessions with a fast quick-identity tuple per session, WITHOUT \
        parsing the whole file. For each session jsonl under the target(s) it emits: \
        session id, the FIRST genuine-user message (+ time), the LAST genuine-user \
        message (+ time), the LAST agent message (+ time), plus the decoded cwd / git \
        branch / CC version. `git_branch`/`version` are LAST-seen (what the session is \
        on NOW - a session that upgraded or switched branches mid-flight reports the \
        current value, with the opening value in `*_first` and a `first->last` drift \
        arrow in text); `cwd` stays FIRST-seen on purpose (the record cwd follows the \
        tracked shell cwd, so last-seen could be a transient subdirectory - the \
        session's home is the opening value). A forward HEAD read finds the first user; a backward \
        TAIL read finds the last user/agent; neither parses the full file, so it \
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
          csift list @<uuid> --no-subagents                          # JUST the one top-level session row\n  \
          csift list --format json .                                  # machine-readable index\n\n\
        SCOPE: because the default SPANS subagents, a `csift list @<uuid>` can return 1 \
        top-level + N subagent rows. The text output then leads with a `scope  N sessions in \
        scope (1 top-level + M subagent)` banner, brands each subagent row \
        `SUBAGENT <hex> · parent SESSION <uuid>` (a bare hex is NOT a re-feedable target; \
        re-feed the parent uuid), and a top-level row keeps the plain `SESSION <uuid>` header.\n\n\
        JSON SCHEMA (per --format json)\n  \
          The standard envelope v2 (same as every command): a `{kind:\"header\", \
        command:\"list\", sessions_in_scope, top_level_sessions, subagent_sessions}` line, then \
        ONE `{kind:\"session\", …}` row per session: {session_id, is_subagent, \
        parent_session_id, path, cwd, git_branch, git_branch_first, git_branch_last, version, \
        version_first, version_last, first_user, last_user, last_agent, \
        skipped_lines, sidecar_present, pending_elicitations, with_elicitation_sidecar}, then \
        a closing `{kind:\"summary\", sessions, skipped_lines, \
        dropped_by_cap}`. `is_subagent` flags a bare-hex subagent row; `parent_session_id` is \
        the re-feedable owning uuid (= session_id for a top-level row); never re-feed a \
        subagent `session_id`. The `first_user`/`last_user`/`last_agent` fields are {excerpt, \
        ts_utc, ts_local} sub-objects (or null when absent). `dropped_by_cap` > 0 when the \
        unscoped-flood cap trimmed rows (the most-recent 50 are kept).\n\n\
        SKIPPED_LINES SEMANTICS (window census, NOT a whole-file verdict)\n  \
          `list` reads only the head/tail lines it needs (the §7 fast-overview contract; it \
        never scans the middle of a transcript), so its `skipped_lines` counts malformed lines \
        among the LINES READ: 0 means \"the lines read were clean\", and a mid-file tear is \
        OUTSIDE the windows by design. The full-scan corruption census over the same bytes is \
        `csift stats` (every full-scan command, search/files/recover, agrees with it). \
        Within the read windows every malformed line, and every sidecar marker the current \
        schema cannot read, is booked exactly once."
)]
pub struct ListArgs {
    /// One or more targets: an actual filesystem cwd, a direct
    /// `~/.claude/projects/<encoded>` path / bare `<encoded>` dir, or an `@`-prefixed session
    /// token. Repeatable. Defaults to all projects. `@<uuid>` (8-4-4-4-12 hex) scopes to that
    /// one top-level session (searched across all projects when no project path is given), so
    /// `csift list @<uuid>` identifies it: the SAME positional surface `files`/`recover`/`verbatim`
    /// use; `@main`/`@trap:<marker>` resolve the calling session from the environment; a `*.jsonl` file
    /// path scopes to that transcript. NOTE: the default still SPANS that session's subagents, so
    /// a fan-out session lists 1 + N rows; add `--no-subagents` for just the single top-level row.
    /// A bare uuid WITHOUT `@` is NOT a session here: prefix it (`@<uuid>`). One subagent is
    /// targeted directly: `@<agent-id>` (a bare hex ≥12 or a teammate `aName-<hex>` id, exactly
    /// as `csift agents` prints them) lists that transcript's row (+ its subtree).
    ///
    /// `allow_hyphen_values` is REQUIRED: every encoded dir starts with `-`
    /// (an absolute cwd's leading `/` encodes to `-`), e.g.
    /// `csift list -Users-testuser-Projects-foo`. Without this clap would reject the
    /// leading `-` token as an unknown flag (SPEC §6.1 baseline invocation). The
    /// `parse_project_target` value parser NARROWS that tolerance so a real flag
    /// (`--format json`) is parsed as a flag in any position, never swallowed as a PATH value.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// Scope ALSO to the session ids in FILE (`-` = stdin): whitespace/newline-separated
    /// uuid / uuid-prefix / agent-id tokens, bare or `@`-prefixed, exactly the ids csift
    /// emits (`search -l`, JSON `transcript_ids` / `parent_session_id`). UNION with positional
    /// targets; each id resolves fail-loud like an `@` positional. An EMPTY list (an upstream
    /// stage that found nothing) scopes to NOTHING: honest empty, exit 0, never a silent
    /// widening to every project. The resolved ids then follow this command's normal
    /// span rules, exactly as if they were `@` positionals: on the span-by-default
    /// commands each session EXPANDS to its subagent transcripts (add `--no-subagents`
    /// to pin to exactly the listed ids).
    #[arg(long = "sessions-from", value_name = "FILE|-")]
    pub sessions_from: Option<std::path::PathBuf>,

    /// Cap the emitted session rows: the CONTEXT-SAFETY guard against an unscoped `csift
    /// list` flooding the reader (a large corpus is thousands of sessions). Defaults to 50
    /// when listing ALL projects (no target / `--sessions-from`), else UNLIMITED; an explicit
    /// value overrides in either case. NEVER silent: the drop is reported with guidance and
    /// the KEPT rows are the most recently active. Raise it, pass a target, or add `--since`
    /// for more.
    #[arg(long, value_name = "N")]
    pub max_count: Option<usize>,

    /// Only sessions ACTIVE in this window: keep a session iff its [first-activity,
    /// last-activity] span: the timestamps this index already reads (head+tail, never a
    /// full scan); it INTERSECTS `[--since, --until]`, so a long-running session that
    /// straddles the window still lists. WHEN = ISO8601 (bare date ⇒ local midnight; bare
    /// datetime ⇒ LOCAL wall-clock; `Z`/`+10:00` ⇒ explicit) or relative `45s 90m 2h 3d 1w`
    /// (that long ago). A session with no readable timestamp never matches a bounded window.
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper bound (same WHEN grammar as --since).
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Exclude subagent transcripts: list only the top-level `<uuid>.jsonl` sessions (the
    /// pre-subagent behavior). Subagent transcripts are spanned by default; this is the only
    /// span flag.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Span subagent transcripts: the DEFAULT here; the explicit flag exists so every
    /// span command answers the same two switches (`--subagents` / `--no-subagents`).
    #[arg(long = "subagents", conflicts_with = "no_subagents")]
    pub subagents: bool,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl ListArgs {
    /// Whether subagent transcripts are spanned (the default). `--no-subagents` restricts to
    /// the top-level session(s). Feeds [`crate::path::SubagentScope::from`].
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.subagents || !self.no_subagents
    }
}
