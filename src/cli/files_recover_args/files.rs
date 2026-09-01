//! FilesArgs + RecoverMode.

use super::*;

#[derive(Debug, Args)]
#[command(
    about = "Which files/dirs a session modified, when (Edit/Write/Notebook + heuristic Bash)",
    long_about = "Show which FILES and DIRECTORIES a session modified, and when. csift \
        extracts file mutations from a session's transcript (spanning its subagents by \
        default, since OMC fan-out edits happen in subagents):\n  \
          • AUTHORITATIVE  Edit / Write / MultiEdit (input.file_path) + NotebookEdit \
        (input.notebook_path), with create-vs-edit resolved from the paired \
        tool_result (`type:\"create\"` = a new file).\n  \
          • HEURISTIC      Bash file mutations, parsed LEXICALLY from the command \
        string (rm/mv/cp/mkdir/touch/tee/sed -i/git/redirection). Bash carries no path \
        field in its result, so these are best-effort and ALWAYS labelled `(heuristic)`.\n\n\
        DETAIL LEVEL: `--by <summary|dir|file|timeline>` (DEFAULT `summary`; STRICTLY \
        coarsening):\n  \
          --by summary   (DEFAULT) coarse TOP-LEVEL-PREFIX rollup (first few dir segments; a\n              \
                         whole project tree collapses to one row); the smallest output\n  \
          --by dir       one row per distinct directory (the FULL parent path, finer than\n              \
                         summary) with per-op + distinct-file counts + first/last\n  \
          --by file      one row per distinct file (per-op counts + first/last touch)\n  \
          --by timeline  full chronological list, one line per mutation (HEAVY; opt-in only)\n\n\
        PATH FILTERS (optional, combinable with each other AND with --by; matched against \
        each mutation's FULL absolute path):\n  \
          --regex <RE>   keep a path iff the Rust `regex` pattern matches ANYWHERE in it\n  \
          --glob <PAT>   keep a path iff the glob matches the full path (`**` crosses `/`)\n  \
        Both filters AND together (a path must satisfy EVERY supplied filter) and apply \
        BEFORE the --by rollup, so summary/dir/file/timeline AND the Edit-before-Read \
        boundary section all reflect the filtered set; an invalid pattern is a hard error \
        and a filter that removes everything yields the normal empty output.\n\n\
        The TARGET selects the session(s): `@<uuid>` for one, or a project \
        PATH/encoded-dir for every session under it; with neither, all projects are \
        scanned. SUBAGENT SCOPE (default spans subagents, since OMC fan-out edits happen \
        in subagents): `--no-subagents` restricts to the TOP-LEVEL session only.\n\n\
        WINDOWING: `--turn (N|A..B|N..|-k)` (inclusive, 0-based on genuine-user order) \
        INTERSECTS with `--since`/`--until` (both filters AND). Time bounds accept ISO8601 \
        (`2026-06-01`, `2026-06-01T05:00:00Z`) or a relative form (`2h`, `3d`, `90m`, \
        `45s`, `1w`) meaning \"that long ago\" in the system-local timezone; a mutation \
        with no timestamp never falls inside a bounded window.\n\n\
        No silent truncation: skipped malformed lines are counted and surfaced.",
    after_help = "DETAIL LEVEL: `--by <summary|dir|file|timeline>` (default summary, strictly coarsening)\n  \
          --by summary (coarse top-level-prefix rollup) < --by dir (full parent dir) < --by file \
        (per file) < --by timeline (per mutation). PATH FILTERS: --regex <RE> (Rust regex, \
        matches anywhere in the full path) and --glob <PAT> (full-path glob, `**` crosses `/`) \
        are optional, combinable with each other and with --by, and AND together over the \
        full absolute path BEFORE the rollup. SUBAGENT SCOPE: --no-subagents restricts to the \
        top-level session (default spans subagents).\n\n\
        EXAMPLES\n  \
          csift files @<uuid>                          # default summary: coarse top-level-prefix op rollup\n  \
          csift files @<uuid> --by file                # per-file op counts + first/last touch\n  \
          csift files @<uuid> --by file --regex '\\.rs$'   # ONLY .rs paths, per-file\n  \
          csift files @<uuid> --by timeline --glob '**/src/**'  # mutations anywhere under a src/ dir\n  \
          csift files @<uuid> --by timeline --since 2h # full chronological, last 2h (heavy)\n  \
          csift files . --format json --by dir         # machine-readable per-dir rollup\n\n\
        ACID TEST: \"how many distinct gap docs touched / how many /tmp docs created?\"\n  \
          csift files @<uuid> --by file                       # count rows ending in gaps-style docs\n  \
          csift files @<uuid> --by timeline --format json     # filter is_create==true (path under /tmp)\n  \
          (there is NO `op` value `create`: `op` is one of {bash,edit,write,multi_edit,\n   \
           notebook_edit}; create-vs-edit is the SEPARATE `is_create` boolean, so a /tmp-doc\n   \
           CREATE test filters `is_create==true` [optionally AND op in {write,multi_edit,\n   \
           notebook_edit}], never `op==\"create\"`.)\n  \
        (NOTE: BOTH the per-mutation `op` AND `is_create` keys live ONLY in `--by timeline`\n   \
         JSON; a `--by file` row carries per-op COUNT fields (write/edit/bash/multi_edit/\n   \
         notebook_edit/total) + first/last timestamps, NOT `op`/`is_create`, so use\n   \
         `--by timeline` to test create-vs-edit or filter by op.)\n\n\
        JSON SCHEMAS (per --format json)\n  \
          --by timeline : one object per mutation: {session_id, is_subagent, parent_session_id,\n             \
                       path, op, ts_utc, ts_local, turn_index, is_create, heuristic,\n             \
                       resolution, path_verbatim, command_errored} + a\n             \
                       trailing summary object. (session_id is the transcript's own id: a\n             \
                       top-level uuid, or a bare SUBAGENT hex when is_subagent=true, which is\n             \
                       NOT a re-feedable @<uuid> target; re-feed parent_session_id, always the\n             \
                       owning top-level uuid. heuristic=true ONLY for a bash-derived mutation: a\n             \
                       guessed path/op lexically parsed from a shell command, lower confidence;\n             \
                       false = a definitive Edit/Write/Notebook/MultiEdit tool call with an\n             \
                       exact file_path. Filter heuristic==false for confirmed mutations only.\n             \
                       A bash row's `path` is RESOLVED against the recording shell's cwd, the\n             \
                       record's own top-level `cwd` field: `resolution` names the class\n             \
                       (absolute = typed absolute; cwd-joined = joined to the record cwd, no\n             \
                       inference; cd-tracked = joined through literal in-command `cd`s, a\n             \
                       lexical inference; unresolved = kept exactly as typed, `~`/`$VAR`/an\n             \
                       unknowable cwd, never joined). `path_verbatim` keeps the typed spelling\n             \
                       when it differs from `path`; both are null on structured tools and on\n             \
                       class markers. A CLASS MARKER is a pseudo-path, not a file: it flags\n             \
                       what kind of mutation ran when the file set is not in the command\n             \
                       text. `git:<sub>` = a mutating git subcommand; `fmt:<tool>` = a\n             \
                       formatter run naming no files (cargo fmt, a prettier write run, ...);\n             \
                       `interp:<lang>` = an interpreter payload with a write whose target\n             \
                       could not be extracted; `pkg:<manager>` = a package-manager\n             \
                       install/update; `extract:<tool>` = an archive or patch extraction.\n             \
                       An `external write` row is a SNAPSHOT-INFERRED mutation: the\n             \
                       tracked file's file-history version jumped with no tool record\n             \
                       in the interval - Claude Code itself rewrote the file (/model,\n             \
                       /config, plugin toggles) or an outside editor did. Reported ONLY\n             \
                       for the settings family (`.claude/settings*.json`); the tracked\n             \
                       set spans thousands of ordinary paths and the harness writes\n             \
                       bookkeeping constantly, so wider reporting would flood every\n             \
                       timeline. The row's detail names the version transition and the\n             \
                       uncovered interval; a version-counter RESET (process restart)\n             \
                       starts a new generation and is never reported as a write.\n             \
                       `command_errored=true` flags a mutation kept\n             \
                       from a bash chain whose tool_result errored: part of the chain failed\n             \
                       and which arms ran is unknowable, so it is disclosed instead of\n             \
                       dropped.)\n  \
          --by file  : one object per file: {session_id, is_subagent, parent_session_id,\n             \
                       file, write, edit, bash, multi_edit, notebook_edit, total,\n             \
                       distinct_files, first_utc, first_local, last_utc, last_local} + a\n             \
                       trailing summary object. (is_subagent + parent_session_id discriminate\n             \
                       the id-domain on EVERY grouped view, same as --by timeline: a subagent\n             \
                       row's session_id is a bare hex; re-feed parent_session_id.)\n  \
          --by dir / --by summary : the same per-op count keys + the same {session_id,\n             \
                       is_subagent, parent_session_id} discriminators, grouped under a\n             \
                       `dir`/`bucket` key, + a trailing summary {distinct_files,\n             \
                       total_mutations, skipped_lines, detail_level}."
)]
pub struct FilesArgs {
    /// Project target(s) (actual cwd or encoded dir) whose sessions' file mutations to
    /// report, OR an `@<uuid>` session token. Repeatable; with none, every project is
    /// scanned. `@<uuid>` (8-4-4-4-12 hex) scopes to one session (searched across all projects
    /// when no project path is given), so `csift files @<uuid>` works as the EXAMPLES show;
    /// `@<agent-id>` scopes to one subagent + its subtree (ids from `csift agents`);
    /// `@main`/`@trap:<marker>` resolve the calling session, and a `*.jsonl` file scopes to that
    /// transcript. (The parent uuid also reaches a subagent's mutations; its subagents are
    /// spanned by default.)
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

    /// Exclude subagent transcripts: report only the top-level `<uuid>.jsonl` session's
    /// mutations. Subagent mutations are attributed by default (built-in Task/Agent-tool, OMC,
    /// and Workflow agents; important because OMC fan-out edits happen in subagents); this is
    /// the only span flag.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Span subagent transcripts: the DEFAULT here; the explicit flag exists so every
    /// span command answers the same two switches (`--subagents` / `--no-subagents`).
    #[arg(long = "subagents", conflicts_with = "no_subagents")]
    pub subagents: bool,

    /// Detail level: `--by <summary|dir|file|timeline>` (DEFAULT `summary`, strictly
    /// coarsening): `summary` = coarse top-level-prefix op rollup (smallest output);
    /// `dir` = one row per distinct directory (full parent path) with per-op +
    /// distinct-file counts + first/last; `file` = one row per distinct file (per-op
    /// counts + first/last touch); `timeline` = full chronological list, one line per
    /// mutation (HEAVY; opt-in only).
    #[arg(long = "by", value_enum, default_value_t = FilesDetail::Summary)]
    pub by: FilesDetail,

    /// Keep only mutated paths whose FULL absolute path matches this Rust `regex` pattern
    /// ANYWHERE (used as-is, no smart-case). Combinable with `--glob` (ANDed) and `--by`;
    /// applied BEFORE the rollup so every view + the boundary section reflect the filtered
    /// set. An invalid pattern is a hard error.
    #[arg(long = "regex", value_name = "RE")]
    pub regex: Option<String>,

    /// Keep only mutated paths whose FULL absolute path matches this glob (`**` crosses
    /// `/`). Combinable with `--regex` (ANDed) and `--by`; applied BEFORE the rollup so
    /// every view + the boundary section reflect the filtered set. An invalid pattern is a
    /// hard error.
    #[arg(long = "glob", value_name = "PAT")]
    pub glob: Option<String>,

    /// Inclusive turn-index range in the shared grammar: `N` (one turn) · `A..B` · `N..` (to the end) · `..N` · `-k` from the end (`-3..` = the last 3) - 0-BASED: turn 0 is the pre-first-user
    /// lead (the session's opening context), so `1..N` SKIPS it. A turn opens on a genuine
    /// user message, an answered AskUserQuestion, or a plan-rejection-with-message.
    /// Discover turn indices from the `<id>·t<n>` exchange header in `csift search` text output, or the
    /// `turn_index` field in any `--format json` record. Intersects (AND) with `--since` /
    /// `--until`.
    #[arg(
        long = "turn",
        value_name = "N|A..B|N..|-k",
        allow_hyphen_values = true
    )]
    pub turn_range: Option<String>,

    /// Lower time bound. WHEN grammar (system-local tz): a relative `Ns`/`Nm`/`Nh`/`Nd`/`Nw`
    /// = that many seconds/minutes/hours/days/weeks AGO (`45s`, `90m`, `2h`, `3d`, `1w`);
    /// an ISO8601 instant (`2026-06-01T05:00:00Z` / `…+10:00`); a BARE datetime
    /// (`2026-06-01T05:00:00`) = that LOCAL wall-clock time; or a bare date (`2026-06-01`)
    /// = LOCAL MIDNIGHT that day. Intersects (AND) with --turn (both filters apply).
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as --since). Intersects (AND) with --turn (both filters apply).
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl FilesArgs {
    /// Whether subagent transcripts are spanned (the default). `files` now matches every
    /// other default-on command: `--no-subagents` restricts to the top-level session;
    /// otherwise subagents are spanned. Feeds [`crate::path::SubagentScope::from`].
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.subagents || !self.no_subagents
    }

    /// The active [`FilesDetail`], selected directly by `--by` (clap validates the value
    /// against the `ValueEnum`, defaulting to `Summary`).
    #[must_use]
    pub fn detail(&self) -> FilesDetail {
        self.by
    }
}

/// The reconstruction mode for `recover` (exactly one is active; default restore).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoverMode {
    /// DEFAULT (no mode flag): hand back the file's FINAL content as RAW restorable bytes;
    /// but ONLY when it is fully recoverable. If the session saw just PART of the file (a
    /// windowed read + a few edits), restore ERRORS instead of emitting a holey file, naming what
    /// it can/can't recover and pointing at `--salvage`.
    Restore,
    /// Best-effort, line-numbered FINAL-state fragment of `--file`. Explicit (`--salvage`).
    /// Restore's never-fails sibling: when the session only ever saw PART of the file, this
    /// dumps what DID survive (each known line numbered) with the unrecoverable lines left as
    /// explicit `??? lines A..B unknown` gaps; for a file that is gone, only-partially-read,
    /// and barely-edited, where rewinding isn't the goal and salvaging the surviving proportion
    /// is. Same output as `--at @latest`, framed as the dead-file salvage front door.
    Salvage,
    /// One or more unified-diff patches over the range, segmented at integrity boundaries.
    /// Explicit (`--patches`). The diff/rewind view: shows the changes a session made (compose
    /// with `--since`/`--until`/`--turn` to extract a time window; to rewind a file you
    /// still have to an older state, even when the session only partially read it).
    Patches,
    /// The partial, line-numbered point-in-time snapshot as of `--at <WHEN>` (gaps are explicit).
    At,
    /// Coverage / scoping summary: recoverable ranges + boundaries + counts, no dump.
    Coverage,
}
