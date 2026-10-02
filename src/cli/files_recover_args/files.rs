//! FilesArgs + RecoverMode.

use super::*;

#[derive(Debug, Args)]
#[command(
    about = "List files and dirs a session modified and when (Edit, Write, Notebook, heuristic Bash)",
    long_about = "Show which FILES and DIRECTORIES a session modified, and when.\n\
        \n\
        csift extracts file mutations from a session's transcript. It spans the session's subagents\n\
        by default, because work fanned out to subagents and workflow agents edits files in their\n\
        transcripts. Mutations come from two sources.\n  \
        • AUTHORITATIVE  Edit / Write / MultiEdit (input.file_path) and NotebookEdit\n                   \
        (input.notebook_path). Create-vs-edit is resolved from the paired\n                   \
        tool_result, where `type:\"create\"` marks a new file.\n  \
        • HEURISTIC      Bash file mutations, parsed LEXICALLY from the command string\n                   \
        (rm/mv/cp/mkdir/touch/tee/sed -i/git/redirection). A Bash result carries\n                   \
        no path field, so these are best-effort and ALWAYS labelled `(heuristic)`.\n\
        \n\
        DETAIL LEVEL\n\
        `--by <summary|dir|file|timeline>` picks the level. The DEFAULT is `summary`, and each level\n\
        is STRICTLY coarser than the one below it.\n  \
        --by summary   (DEFAULT) a coarse TOP-LEVEL-PREFIX rollup on the first few dir segments.\n                 \
        A whole project tree collapses to one row, the smallest output.\n  \
        --by dir       one row per distinct directory (the FULL parent path, finer than\n                 \
        summary), with per-op and distinct-file counts plus first/last\n  \
        --by file      one row per distinct file, with per-op counts plus first/last touch\n  \
        --by timeline  the full chronological list, one line per mutation (HEAVY, opt-in only)\n\
        \n\
        PATH FILTERS\n\
        Both filters are optional. They combine with each other AND with `--by`, and they match\n\
        against each mutation's FULL absolute path.\n  \
        --regex <RE>   keeps a path when the Rust `regex` pattern matches ANYWHERE in it\n  \
        --glob <PAT>   keeps a path when the glob matches the full path (`**` crosses `/`)\n\
        The filters AND together, so a path must satisfy EVERY supplied filter. They apply BEFORE\n\
        the `--by` rollup, so summary, dir, file, timeline AND the Edit-before-Read boundary section\n\
        all reflect the filtered set. An invalid pattern is a hard error, and a filter that removes\n\
        everything yields the normal empty output.\n\
        \n\
        EDIT-BEFORE-READ BOUNDARIES\n\
        Every detail level closes with a section of Edit-before-Read boundaries. Each is a place\n\
        where the harness rejected an Edit or Write with `File has been modified since read`,\n\
        because the file changed outside the tool stream (a formatter, a git command, an outside\n\
        editor) after the session last read it. Each row names the file, the jsonl line, the\n\
        turn and the time. The coverage mode of `csift recover --file <path>` breaks one file's\n\
        boundaries down further.\n\
        \n\
        The TARGET selects the sessions. `@<uuid>` names one session, a project path or encoded\n\
        directory names every session under it, and with neither, all projects are scanned.\n\
        Subagents and workflow agents are spanned by default. `--no-subagents` restricts the scan\n\
        to the TOP-LEVEL session only.\n\
        \n\
        WINDOWING\n\
        `--turn (N|A..B|N..|-k)` is inclusive and counts turns from 0. The `--turn` entry below\n\
        says which records open a turn. It intersects with `--since`/`--until`, so both filters\n\
        apply. Time bounds accept ISO8601 (`2026-06-01`, `2026-06-01T05:00:00Z`) or a relative\n\
        form meaning \"that long ago\" in the system-local timezone (`45s`, `90m`, `2h`, `3d`,\n\
        `1w`, `2mo`, `1y`, with all seven units listed under `--since`). A mutation with no\n\
        timestamp never falls inside a bounded window.\n\
        \n\
        THE SURVIVAL AXIS\n\
        Turn numbers are the LIVE ones, the turns Claude Code's own conversation chain still\n\
        reaches (the global `--cross-compactions` entry says what the chain is), and the same\n\
        numbers `search` and `show` print. A mutation made on a branch the operator later rewound\n\
        past, or under a prompt that was recalled and re-typed, belongs to no numbered turn. Its\n\
        turn slot reads `abandoned (root L<n>)`, naming the branch head, and the row carries an\n\
        `[abandoned]` marker (`[pre-cut]` above a compaction cut). The ROW STAYS, because file\n\
        order is disk truth and that write really did land. The rollups count the share\n\
        separately, as `(N from abandoned branches)`. A `--turn` window admits such a row by the\n\
        live turn it physically follows, so a window over a span keeps every write made inside\n\
        it.\n\
        \n\
        Nothing is truncated silently. Skipped malformed lines are counted and surfaced.",
    after_help = "AT A GLANCE\n  \
        Detail    `--by <summary|dir|file|timeline>`, default summary, strictly coarsening.\n            \
        --by summary (coarse top-level-prefix rollup) < --by dir (full parent dir)\n            \
        < --by file (per file) < --by timeline (per mutation)\n  \
        Filters   --regex <RE> (a Rust regex matched anywhere in the full path) and --glob <PAT>\n            \
        (a full-path glob, `**` crosses `/`). Both are optional, combine with each\n            \
        other and with --by, and AND together over the full absolute path BEFORE the\n            \
        rollup.\n  \
        Scope     subagents are spanned by default. --no-subagents restricts the scan to the\n            \
        top-level session.\n\
        \n\
        EXAMPLES\n  \
        The default summary, a coarse top-level-prefix op rollup:\n    \
        csift files @<uuid>\n  \
        Per-file op counts plus first/last touch:\n    \
        csift files @<uuid> --by file\n  \
        ONLY .rs paths, per file:\n    \
        csift files @<uuid> --by file --regex '\\.rs$'\n  \
        Mutations anywhere under a src/ dir:\n    \
        csift files @<uuid> --by timeline --glob '**/src/**'\n  \
        The full chronological list for the last 2h (heavy):\n    \
        csift files @<uuid> --by timeline --since 2h\n  \
        A machine-readable per-dir rollup:\n    \
        csift files . --format json --by dir\n\
        \n\
        WORKED EXAMPLE: \"how many distinct Markdown files were touched, and how many files were\n\
        created under /tmp?\"\n  \
        One row per Markdown file, so the row count answers the first half:\n    \
        csift files @<uuid> --by file --regex '\\.md$'\n  \
        The created files under /tmp, counted from the timeline rows:\n    \
        csift files @<uuid> --by timeline --glob '/tmp/**' --format json \\\n      \
        | jq -s 'map(select(.kind==\"mutation\" and .is_create)) | length'\n  \
        There is NO `op` value `create`. `op` is one of {bash, edit, write, multi_edit,\n  \
        notebook_edit, external_write}, and create-vs-edit is the SEPARATE `is_create` boolean. A\n  \
        test for a created /tmp file therefore filters `is_create==true` (optionally AND op in\n  \
        {write, multi_edit, notebook_edit}), never `op==\"create\"`.\n  \
        BOTH the per-mutation `op` and `is_create` keys live ONLY in `--by timeline` JSON. A\n  \
        `--by file` row carries per-op COUNT fields (write/edit/bash/multi_edit/notebook_edit/total)\n  \
        plus first/last timestamps, NOT `op`/`is_create`. Use `--by timeline` to test\n  \
        create-vs-edit or to filter by op.\n\
        \n\
        JSON SCHEMAS (per --format json)\n  \
        The stream is one header line, the rows of the chosen `--by` level, the boundary rows,\n  \
        and one summary line, in that order.\n  \
        header        : {kind:\"header\", command, sessions_in_scope, top_level_sessions,\n      \
        subagent_sessions, duplicate_transcripts, duplicate_records_dropped,\n      \
        divergent_transcripts, unreadable_transcripts}. The last four describe a transcript\n      \
        found under two project directories. They count the copies merged, the records the\n      \
        merge dropped, the copies kept apart because their contents differ, and the copies\n      \
        csift could not read.\n  \
        boundary      : one {kind:\"boundary\", session_id, is_subagent, parent_session_id, path,\n      \
        line, turn_index, survival, abandoned_root_line, cause, ts_utc, ts_local} row per\n      \
        Edit-before-Read boundary, in every `--by` level. `cause` names what was refused, and\n      \
        it reads `modified_since_read`.\n  \
        summary       : {kind:\"summary\", sessions, distinct_files, total_mutations,\n      \
        edit_before_read_boundaries, skipped_lines, detail_level}. `sessions` counts the\n      \
        distinct OWNING sessions, and `detail_level` repeats the `--by` value. The summary also\n      \
        carries the crossing set {crossing_budget, boundaries_crossed, budget_exhausted,\n      \
        boundaries_above_floor, floor_line, floor_cause} whenever `--cross-compactions` is\n      \
        non-zero. The set records what the walk stepped over to decide each row's `survival`,\n      \
        and where it stopped. At `--cross-compactions 0` those keys are absent.\n  \
        --by timeline : one object per mutation: {kind:\"mutation\", session_id, is_subagent,\n      \
        parent_session_id, line, path, op, ts_utc, ts_local, turn_index, survival,\n      \
        abandoned_root_line, is_create, heuristic, resolution, path_verbatim, command_errored,\n      \
        detail}. The row keys read as follows.\n\
        \n      \
        survival         live | pre-cut | abandoned. An ABANDONED mutation was made on a\n                       \
        branch the conversation was later rewound past. It has a NULL\n                       \
        turn_index and an abandoned_root_line naming its branch head's jsonl\n                       \
        line. It is never dropped, because the write hit the disk.\n      \
        session_id       The transcript's own id. It is a top-level uuid, or a bare SUBAGENT\n                       \
        hex when is_subagent=true, and a hex is NOT a re-feedable @<uuid>\n                       \
        target. Re-feed parent_session_id, which is always the owning\n                       \
        top-level uuid.\n      \
        heuristic        true ONLY for a bash-derived mutation, a guessed path/op lexically\n                       \
        parsed from a shell command, with lower confidence. false means a\n                       \
        definitive Edit/Write/Notebook/MultiEdit tool call with an exact\n                       \
        file_path. Filter heuristic==false for confirmed mutations only.\n      \
        resolution       A bash row's `path` is RESOLVED against the recording shell's cwd,\n                       \
        the record's own top-level `cwd` field. `resolution` names one of\n                       \
        four classes.\n                         \
        absolute     typed absolute\n                         \
        cwd-joined   joined to the record cwd, no inference\n                         \
        cd-tracked   joined through literal in-command `cd`s, a lexical\n                                      \
        inference\n                         \
        unresolved   kept exactly as typed (`~`, `$VAR`, an unknowable\n                                      \
        cwd), never joined\n      \
        path_verbatim    The typed spelling, kept when it differs from `path`. It and\n                       \
        `resolution` are both null on structured tools and on class markers.\n      \
        class markers    A CLASS MARKER is a pseudo-path, not a file. It flags what kind of\n                       \
        mutation ran when the file set is not in the command text.\n                         \
        `git:<sub>`        a mutating git subcommand\n                         \
        `fmt:<tool>`       a formatter run naming no files (cargo fmt, a\n                                            \
        prettier write run, ...)\n                         \
        `interp:<lang>`    an interpreter payload with a write whose\n                                            \
        target could not be extracted\n                         \
        `pkg:<manager>`    a package-manager install/update\n                         \
        `extract:<tool>`   an archive or patch extraction\n      \
        external write   An `external write` row is a SNAPSHOT-INFERRED mutation. The tracked\n                       \
        file's file-history version jumped with no tool record in the\n                       \
        interval, so Claude Code itself rewrote the file (/model, /config,\n                       \
        plugin toggles) or an outside editor did. Such rows are reported ONLY\n                       \
        for the settings files (`.claude/settings*.json`). The tracked set\n                       \
        spans thousands of ordinary paths and the harness writes bookkeeping\n                       \
        constantly, so wider reporting would flood every timeline.\n                       \
        `csift recover --file <path>` reads the SAME instrument on any single\n                       \
        path, where that flood cannot arise. The row's detail names the\n                       \
        version transition and the uncovered interval. A version-counter\n                       \
        RESET (a process restart) starts a new generation and is never\n                       \
        reported as a write.\n      \
        command_errored  true flags a mutation kept from a bash chain whose tool_result\n                       \
        errored. Part of the chain failed and which parts ran is unknowable,\n                       \
        so the row is disclosed instead of dropped.\n\
        \n  \
        --by file  : one object per file: {kind:\"file\", session_id, is_subagent,\n      \
        parent_session_id, path, write, edit, bash, multi_edit, notebook_edit, external_write,\n      \
        abandoned, total, distinct_files, first_utc, first_local, last_utc, last_local}.\n      \
        is_subagent + parent_session_id discriminate the id domain on EVERY grouped view, as on\n      \
        --by timeline. A subagent row's session_id is a bare hex, so re-feed\n      \
        parent_session_id. `abandoned` is a SHARE of `total`, never a subtraction. It counts\n      \
        how many of this group's mutations came from records off the surviving conversation.\n\
        \n  \
        --by dir / --by summary : the same keys as --by file. Each group is one `kind:\"dir\"` /\n      \
        `kind:\"bucket\"` row with the group named in `path`."
)]
pub struct FilesArgs {
    /// Project targets (actual cwd or encoded dir) whose sessions' file mutations to report.
    ///
    /// A target is a path or an `@` session token, and the argument repeats. With none, every
    /// project is scanned. `@<uuid>` (8-4-4-4-12 hex) scopes to one session, searched across
    /// all projects when no project path is given, which is how `csift files @<uuid>` in the
    /// EXAMPLES works. `@<agent-id>` scopes to one subagent and its subtree (ids from
    /// `csift agents`). `@main` and `@trap:<marker>` resolve the calling session, and a
    /// `*.jsonl` file scopes to that transcript. The parent uuid also reaches a subagent's
    /// mutations, because its subagents are spanned by default.
    #[arg(
        value_name = "PATH",
        verbatim_doc_comment,
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// Also scope to the session ids listed in FILE (`-` reads stdin).
    ///
    /// The ids are whitespace- or newline-separated uuid, uuid-prefix or agent-id tokens, bare
    /// or `@`-prefixed, exactly the ids csift emits (`search -l`, JSON `transcript_ids` /
    /// `parent_session_id`). They form a UNION with the positional targets, and each id
    /// resolves fail-loud like an `@` positional. An EMPTY list (an upstream stage that found
    /// nothing) scopes to NOTHING. The result is an honest empty with exit 0, never a silent
    /// widening to every project. The resolved ids then follow this command's normal span
    /// rules, exactly as `@` positionals would. On the span-by-default commands each session
    /// EXPANDS to its subagent transcripts. Add `--no-subagents` to pin the scope to exactly
    /// the listed ids.
    #[arg(long = "sessions-from", value_name = "FILE|-", verbatim_doc_comment)]
    pub sessions_from: Option<std::path::PathBuf>,

    /// Report only the top-level `<uuid>.jsonl` session's mutations, excluding subagents.
    ///
    /// Subagent mutations are attributed by default (built-in Task/Agent-tool agents and the
    /// workflow agents whose transcripts sit under `subagents/workflows/<run>/`), which matters
    /// because work fanned out to them edits files in their own transcripts. This is the one
    /// flag that narrows the span. `--subagents` only restates the default.
    #[arg(long = "no-subagents", verbatim_doc_comment)]
    pub no_subagents: bool,

    /// Span subagent transcripts, which is already the DEFAULT here.
    ///
    /// The flag exists so that every span command answers the same two switches (`--subagents`
    /// / `--no-subagents`).
    #[arg(
        long = "subagents",
        conflicts_with = "no_subagents",
        verbatim_doc_comment
    )]
    pub subagents: bool,

    /// Detail level, default `summary`. Each value is STRICTLY coarser than the next.
    ///
    ///   summary    a coarse top-level-prefix op rollup that buckets each path on its first few
    ///              directory segments, so a whole project tree collapses to one row. It is the
    ///              smallest output, and it is strictly coarser than `--by dir`, which keys on
    ///              the FULL parent dir.
    ///   dir        one row per distinct directory (the full parent path), with per-op and
    ///              distinct-file counts plus first/last. It is finer than `--by summary`.
    ///   file       one row per distinct file, with per-op counts plus first/last touch
    ///              timestamps
    ///   timeline   the full chronological list, one line per mutation (HEAVY, opt-in only)
    #[arg(
        long = "by",
        value_enum,
        default_value_t = FilesDetail::Summary,
        verbatim_doc_comment
    )]
    pub by: FilesDetail,

    /// Keep only mutated paths whose FULL absolute path matches this Rust `regex` ANYWHERE.
    ///
    /// The pattern is used as-is, with no smart-case. It combines with `--glob` (ANDed) and
    /// with `--by`, and it applies BEFORE the rollup, so every view and the boundary section
    /// reflect the filtered set. An invalid pattern is a hard error.
    #[arg(long = "regex", value_name = "RE", verbatim_doc_comment)]
    pub regex: Option<String>,

    /// Keep only mutated paths whose FULL absolute path matches this glob (`**` crosses `/`).
    ///
    /// It combines with `--regex` (ANDed) and with `--by`, and it applies BEFORE the rollup, so
    /// every view and the boundary section reflect the filtered set. An invalid pattern is a
    /// hard error.
    #[arg(long = "glob", value_name = "PAT", verbatim_doc_comment)]
    pub glob: Option<String>,

    /// Inclusive turn-index range, 0-BASED, in the shared range grammar.
    ///
    ///   `N`      one turn
    ///   `A..B`   turns A through B
    ///   `N..`    turn N to the end
    ///   `..N`    the first turn through turn N
    ///   `-k`     counted from the end (`-3..` is the last 3)
    /// Turn 0 is the first opener's turn together with every record above it (the session's
    /// opening context), so `1..N` drops that context and the first human turn with it. A turn
    /// opens on a genuine typed message, an answered AskUserQuestion, a tool rejection carrying
    /// typed text, an inbound peer message, or a `<task-notification>` pulse. A slash command
    /// with arguments carries `user.message` yet opens no turn. Find turn indices in the
    /// `<id>·t<n>` exchange header of `csift search` text output, or in the `turn_index` field
    /// of any `--format json` record. The range intersects (AND) with `--since` / `--until`.
    #[arg(
        long = "turn",
        value_name = "N|A..B|N..|-k",
        allow_hyphen_values = true,
        verbatim_doc_comment
    )]
    pub turn_range: Option<String>,

    /// Lower time bound, read in the system-local time zone.
    ///
    ///   `N<unit>`                 that many units AGO. The seven units are `s` seconds, `m`
    ///                             minutes, `h` hours, `d` days, `w` weeks, `mo` months
    ///                             (30 days) and `y` years (365 days), as in `45s`, `90m`,
    ///                             `2h`, `3d`, `1w`, `2mo`, `1y`
    ///   ISO8601 instant           `2026-06-01T05:00:00Z` or `…+10:00`
    ///   bare datetime             `2026-06-01T05:00:00` is that LOCAL wall-clock time
    ///   bare date                 `2026-06-01` is LOCAL MIDNIGHT that day
    /// The bound intersects (AND) with `--turn`, so both filters apply.
    #[arg(long, value_name = "WHEN", verbatim_doc_comment)]
    pub since: Option<String>,

    /// Upper time bound, in the same WHEN grammar as `--since`.
    ///
    /// The bound intersects (AND) with `--turn`, so both filters apply.
    #[arg(long, value_name = "WHEN", verbatim_doc_comment)]
    pub until: Option<String>,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl FilesArgs {
    /// Whether subagent transcripts are spanned (the default). `files` answers the same
    /// two switches as every other default-on command: `--no-subagents` restricts to the
    /// top-level session; otherwise subagents are spanned. Feeds
    /// [`crate::path::SubagentScope::from`].
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
