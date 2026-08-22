//! FilesArgs + RecoverMode + RecoverArgs.

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

#[derive(Debug, Args)]
#[command(
    about = "Reconstruct a file's history from the transcript: segmented diff-patches, \
             point-in-time partial snapshot, or coverage scoping",
    long_about = "Reconstruct a single file's history from a session transcript. Unlike \
        `files` (which only ROLLS UP that a file was touched), `recover` rebuilds the \
        file's CONTENT line-by-line from the transcript's Reads / Writes / Edits, in \
        transcript order, with every output line carrying the JSONL LINE NUMBER so an \
        LLM can `Read` the raw jsonl directly.\n\n\
        FIVE MUTUALLY-EXCLUSIVE MODES (exactly one; default = restore):\n  \
          (default, no mode flag) RESTORE the file's FINAL content as RAW restorable \
        bytes: what you'd `> file` to put it back. Restore SUCCEEDS only when the \
        session saw the WHOLE file (a full Read, or it authored the file outright); it \
        then prints the reconstructed content with NO line numbers / banners (clean for \
        piping). When the session observed just PART of the file (a windowed read + a few \
        edits), restore FAILS LOUDLY; it never hands back a holey file, naming the \
        line ranges it CAN and CANNOT recover and pointing at `--salvage`.\n  \
          --salvage   restore's never-fails sibling: the best-effort, line-numbered \
        FINAL-state fragment. Dumps whatever survived (each known line numbered) with the \
        unrecoverable lines left as explicit `??? lines A..B unknown` gaps. For a file \
        that is GONE, only-partially-read, and barely-edited, where rewinding isn't the \
        goal and salvaging the surviving proportion is. Identical output to `--at @latest`, \
        framed as the dead-file salvage front door. (Content invalidated by a \
        `modified since read` boundary is dropped, not shown stale.)\n  \
          --patches   segmented unified-diff history of `--file`. The range \
        is split at INTEGRITY BOUNDARIES: points where reconstruction across them is \
        invalid (a `File has been modified since read` harness error, an `originalFile` \
        that disagrees with the replayed buffer, an external `edited_text_file`, or a \
        heuristic Bash mutation). Each segment + boundary carries its jsonl line / turn \
        / timestamp. This is the CHANGES view: rewind a still-present file to an older \
        state over a `--since`/`--until` window; most useful when ONLY this session \
        touched the file and it did NOT read it in full.\n  \
          --at <WHEN> the PARTIAL, line-numbered \"in the LLM's eyes\" snapshot of \
        `--file` as of <WHEN> (the SAME relative/ISO/bare-date grammar as --since: \
        `45s`/`90m`/`2h`/`3d`/`1w`, ISO8601, bare-date=local-midnight, PLUS the \
        recover-only `@turn:<N>` / `@line:<N>` / `@latest`). Unlike restore, `--at` will \
        dump a PARTIAL snapshot: known lines carry their number; unknown regions are \
        marked `??? lines A..B unknown`; gaps are NEVER fabricated. `--at @latest` is \
        the partial-tolerant sibling of the default restore (final state, holes shown).\n  \
          --coverage  (alias --dry-run) scope a recovery WITHOUT dumping content: which \
        line ranges are recoverable, where the integrity boundaries sit, and per-op \
        counts (reads / edits / writes / bash / external-edits).\n\n\
        The TARGET selects the session(s): `@<uuid>` for one, or a project \
        PATH/encoded-dir for every session under it. `--no-subagents` restricts to the \
        top-level session (OMC fan-out edits happen in subagents, so default ON).\n\n\
        WINDOWING: `--turn (N|A..B|N..|-k)` (inclusive, 0-based genuine-user order) \
        INTERSECTS with `--since`/`--until` (ISO8601 / relative; both filters AND). \
        `--file-lines (N|A..B|N..|-k)` further restricts to a 1-based FILE-line span. `--out <PATH>` writes \
        the reconstructed artifact (restored content / snapshot / concatenated patches) \
        verbatim to a file; in restore mode stdout then stays empty (just a stderr note), \
        in the other modes the summary still prints to stdout.\n\n\
        Reconstruction is NECESSARILY PARTIAL and NEVER fabricates: an unseen line is \
        an explicit gap, an un-anchorable edit is a coverage hole, a Bash touch is a \
        heuristic (not authoritative) boundary. No silent truncation.",
    after_help = "MODE (choose AT MOST ONE; default = restore)\n  \
          --salvage / --patches / --at <WHEN> / --coverage are MUTUALLY EXCLUSIVE; \
        passing two (e.g. `--coverage --patches`) is a parse error. With NONE, the default \
        RESTORE mode applies: it hands back the file's final content, or FAILS (never a \
        partial file) when the session saw only part of it; reach for `--salvage` then. \
        `--file` is REQUIRED for every mode; its value is an absolute path OR the magic \
        `@plan` (the session-bound plan file).\n\n\
        EXAMPLES\n  \
          csift recover @<uuid> --file /abs/app.py                  # DEFAULT: restore final content (or fail if only partial)\n  \
          csift recover @<uuid> --file /abs/app.py --out /abs/app.py # restore straight back onto disk (raw bytes, no banners)\n  \
          csift recover @<uuid> --file /abs/gone.py --salvage       # file is gone + only partly seen: dump what survived, gaps explicit\n  \
          csift recover . --file /abs/PLAN.md --coverage            # scope first: covered ranges + boundaries, no dump\n  \
          csift recover @<uuid> --file /abs/app.py --patches        # segmented unified diffs over the whole session\n  \
          csift recover @<uuid> --file /abs/app.py --since 2h       # patches for the last 2h only (rewind a window)\n  \
          csift recover @<uuid> --file /abs/app.py --at @latest     # partial-tolerant final snapshot (holes shown)\n  \
          csift recover @<uuid> --file /abs/app.py --at @turn:42    # partial snapshot as the LLM saw it at turn 42\n  \
          csift recover @<uuid> --file @plan --out /tmp/plan.md     # reconstruct the session's bound plan (even if deleted)\n  \
          csift recover @<uuid> --file /abs/x.rs --file-lines 100..200 --patches   # only patches touching FILE lines 100-200\n\n\
        WINDOW ACCOUNTING (every mode)\n  \
          Recover never pretends a window is clean. Beside the replayed events it \
        counts, per window: parsed bash mutations OF the file (each disclosed as a \
        boundary), OPAQUE mutating-class commands (a formatter run, a package-manager \
        install, an archive/patch extraction, an interpreter write with an \
        unextractable target: commands that mutate files they never name, shown as \
        `fmt:`/`pkg:`/`extract:`/`interp:` markers), and PowerShell commands (their \
        command text is never lexically parsed). When any are present the output says \
        so and prints a ready-to-run, time-bounded `csift search` that lists the \
        window's tool calls touching the file. `complete` therefore means complete \
        FROM THE TOOL STREAM; only a clean window note implies nothing else ran. A \
        `--turn`/`--since`/`--until` window that excludes integrity-relevant events \
        prints a note naming how many fell outside.\n\n\
        JSON SCHEMA (per --format json)\n  \
          EVERY mode (restore included) emits the envelope: one `{kind:\"header\",…}` \
        line first, one `{kind:\"summary\", file, mode, sessions, skipped_lines}` line \
        last. RESTORE emits one `{kind:\"restore\", file, complete, lines, \
        boundaries, bash_events, opaque_commands, powershell_commands, \
        suggested_search}` row carrying `content` (or `path` + `wrote` with `--out`); \
        a partial or no-history run still emits its `{kind:\"restore\", complete:false, \
        reason}` row + the summary, then errors on stderr with a non-zero exit. \
        `--patches` emits `{kind:\"segment\",…}` + `{kind:\"boundary\",…}` rows; \
        `--coverage` emits `{kind:\"coverage\", covered_ranges, boundaries, \
        hard_boundaries, soft_boundaries, events, fragments, recoverable_lines, \
        opaque_commands, powershell_commands, suggested_search, …}`; `--at` and \
        `--salvage` emit `{kind:\"snapshot\", lines, gaps, seen_total_lines, \
        boundaries, opaque_commands, powershell_commands, suggested_search, …}`. A \
        boundary object is `{line, source_session_id, source_line, turn_index, \
        ts_utc, ts_local, cause, confidence, detail}`: `line` is the replay/cutoff \
        coordinate (feed it to `--at @line:<N>`), `source_line` the REAL jsonl line in \
        `source_session_id` (feed those to `csift show`); the two pairs differ only \
        after a cross-transcript merge. Every per-session row carries the id-domain \
        discriminators `{session_id, is_subagent, parent_session_id}` (is_subagent \
        flags a bare-hex subagent record; re-feed parent_session_id, never the bare \
        session_id)."
)]
pub struct RecoverArgs {
    /// Project target(s) (actual cwd or encoded dir) whose session(s) to reconstruct
    /// from, OR an `@<uuid>` session token. Repeatable; with none, every project is
    /// scanned. `@<uuid>` (8-4-4-4-12 hex) scopes to one session, so `csift recover @<uuid>
    /// --file …` works as the EXAMPLES show; `@<agent-id>` scopes to one subagent + its
    /// subtree (ids from `csift agents`: a subagent's WRITE gap closes from tool_use input);
    /// `@main`/`@trap:<marker>` resolve the calling session, and a `*.jsonl` file scopes to
    /// that transcript.
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

    /// The ABSOLUTE file path whose history to reconstruct, matched against the path
    /// exactly as written in the transcript (with a basename-suffix fallback). REQUIRED
    /// for every mode (`--patches` / `--at` / `--coverage`). The MAGIC value `@plan`
    /// (bash-safe, no escaping) instead reconstructs the session-BOUND plan file: the
    /// path from the session's `plan_mode` attachment, so a deleted plan is recoverable
    /// from the transcript alone; locate it without dumping via `csift plan`.
    #[arg(long, value_name = "ABS_PATH")]
    pub file: Option<String>,

    /// Exclude subagent transcripts: reconstruct only from the top-level session. Subagent
    /// transcripts are spanned by default; this is the only span flag.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Span subagent transcripts: the DEFAULT here; the explicit flag exists so every
    /// span command answers the same two switches (`--subagents` / `--no-subagents`).
    #[arg(long = "subagents", conflicts_with = "no_subagents")]
    pub subagents: bool,

    /// Best-effort line-numbered FINAL-state fragment: restore's never-fails sibling. Where
    /// the default restore REFUSES a partial file, `--salvage` dumps whatever survived (known
    /// lines numbered, unrecoverable lines left as explicit `??? lines A..B unknown` gaps). For
    /// a file that is gone, only-partially-read, and barely-edited: salvage the surviving
    /// proportion instead of rewinding. Identical output to `--at @latest`.
    #[arg(long, group = "mode")]
    pub salvage: bool,

    /// Segmented unified-diff history of `--file`: the CHANGES view (rewind a still-present
    /// file over a `--since`/`--until` window; best when only this session touched it and did
    /// not read it whole). NOT the default: with no mode flag, `recover` RESTOREs the final
    /// content instead (and fails loudly, never emitting a partial file).
    #[arg(long, group = "mode")]
    pub patches: bool,

    /// Point-in-time partial snapshot of `--file` as of `<WHEN>`. WHEN uses the SAME
    /// grammar as `--since`: a relative `Ns`/`Nm`/`Nh`/`Nd`/`Nw` (`45s`, `90m`, `2h`, `3d`,
    /// `1w`) = that long ago, an ISO8601 datetime (`2026-06-01T05:00:00Z`), or a bare date
    /// (`2026-06-01`) = LOCAL MIDNIGHT, PLUS the recover-only forms `@turn:<N>` (snapshot as
    /// of the first line after genuine-user turn N; discover N from the `<id>·t<n>` header in
    /// `csift search` text, or `turn_index` in any `--format json` record) and `@line:<N>`
    /// (snapshot as of JSONL TRANSCRIPT line N: the `line_no` shown in this tool's output,
    /// NOT a file line of `--file`; for a 1-based FILE-line span of `--file` use `--file-lines`
    /// instead), and `@latest` (the file's FINAL reconstructed state; no cutoff; the clean way
    /// to ask for "its last form" without guessing a timestamp past the last write). A datetime
    /// bound is INCLUSIVE of events AT that instant.
    /// Setting this both selects the mode AND supplies its cutoff.
    #[arg(long, value_name = "WHEN", group = "mode")]
    pub at: Option<String>,

    /// Coverage / scoping summary (no content dump). Alias: `--dry-run`.
    #[arg(long, visible_alias = "dry-run", group = "mode")]
    pub coverage: bool,

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

    /// Restrict to a 1-based, inclusive FILE-line span of `--file`: the shared range grammar
    /// (`N` · `A..B` · `N..` · `-k` = the last k lines) (filters the reconstructed line space, independent of the turn/time
    /// window). Applies in `--patches` / `--at` / `--coverage`. (Named `--file-lines`
    /// because `line`/`L` always means a TRANSCRIPT jsonl line in csift; this is the one
    /// flag that addresses the reconstructed FILE's lines.)
    #[arg(
        long = "file-lines",
        value_name = "N|A..B|N..|-k",
        allow_hyphen_values = true
    )]
    pub line_range: Option<String>,

    /// Write the reconstructed artifact (snapshot / concatenated patches)
    /// verbatim to this file; the summary still prints to stdout. The DIFFERENCE from stdout:
    /// stdout shortens each over-long line/body to a ~400-char excerpt (a `… (+N chars)`
    /// marker) for readability, whereas `--out` writes every line in full. IGNORED in
    /// `--coverage` mode (a scoping summary; no artifact to write, so no file is created and
    /// a stderr note is printed); it writes for `--patches` / `--at`.
    #[arg(long, value_name = "PATH")]
    pub out: Option<PathBuf>,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// BATCH MODE: reconstruct MANY files in a single corpus scan. Path to a manifest listing
    /// the target absolute file paths (one per line; blank lines and `#` comments ignored).
    /// Each transcript is parsed ONCE and every listed file it touched is extracted from it;
    /// turning N separate `recover --file` runs (N whole-corpus parses) into one. Requires
    /// `--out-dir`; mutually exclusive with `--file`. Honors `--at`/`--since`/`--until` (default
    /// = the file's final reconstructed state). Each file is written under `--out-dir` mirroring
    /// its absolute path; a `recovery-report.tsv` summarizes per-file status.
    #[arg(long, value_name = "MANIFEST")]
    pub files_from: Option<PathBuf>,

    /// BATCH MODE output directory (required with `--files-from`). Each recovered file is written
    /// to `<DIR>/<abs-path-without-leading-slash>` (created as needed); existing files are not
    /// clobbered unless `--force`.
    #[arg(long, value_name = "DIR")]
    pub out_dir: Option<PathBuf>,

    /// In batch mode, overwrite an already-present output file (default: skip + report it).
    #[arg(long)]
    pub force: bool,
}

impl RecoverArgs {
    /// Whether subagent transcripts are spanned (the default). `--no-subagents` restricts to
    /// the top-level session(s). Feeds [`crate::path::SubagentScope::from`].
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.subagents || !self.no_subagents
    }

    /// Resolve the mode flags into the active [`RecoverMode`]. clap's `group = "mode"`
    /// (multiple=false) rejects more than one at parse time, so at most one is set;
    /// none set ⇒ the `Restore` default (full content, or an error if only partial).
    #[must_use]
    pub fn mode(&self) -> RecoverMode {
        if self.at.is_some() {
            RecoverMode::At
        } else if self.coverage {
            RecoverMode::Coverage
        } else if self.patches {
            RecoverMode::Patches
        } else if self.salvage {
            RecoverMode::Salvage
        } else {
            RecoverMode::Restore
        }
    }
}
