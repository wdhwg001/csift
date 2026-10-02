//! RecoverArgs.

use super::*;

#[derive(Debug, Args)]
#[command(
    about = "Rebuild a file's history as segmented diffs, a partial snapshot or a coverage scope",
    long_about = "Reconstruct the history of a single file from a session transcript.\n\
        \n\
        `files` only rolls up which files a session touched. `recover` rebuilds a file's CONTENT\n\
        line by line from the transcript's Reads, Writes and Edits, replayed in transcript order.\n\
        Every output line carries the JSONL LINE NUMBER of its source record, so an LLM can `Read`\n\
        the raw jsonl at that line.\n\
        \n\
        SIX MUTUALLY-EXCLUSIVE MODES (exactly one applies)\n\
        The default RESTORE takes no flag. The other five are the flags of the mode group.\n\
        \n  \
        (default, no mode flag)\n      \
        RESTORE prints the file's FINAL content as raw restorable bytes, the bytes you would\n      \
        write with `> file` to put it back. It succeeds only when the session saw the WHOLE\n      \
        file, through a full Read or because it authored the file outright. The content then\n      \
        prints with no line numbers and no banners, so it pipes cleanly. When the session saw\n      \
        only PART of the file (a windowed read plus a few edits), restore fails loudly and\n      \
        never hands back a file with holes. The error names the line ranges it can and cannot\n      \
        recover, and it points at `--salvage`.\n  \
        --salvage\n      \
        Restore's never-fails sibling, the best-effort FINAL-state fragment. It dumps whatever\n      \
        survived, each known line numbered, and leaves every unrecoverable run as an explicit\n      \
        `??? lines A..B unknown` gap. It suits a file that is GONE, was only partly read and\n      \
        was barely edited, when the goal is salvaging what survived and rewinding is not.\n      \
        The output is identical to `--at @latest`, offered here as the dead-file salvage door.\n      \
        Content invalidated by a `modified since read` boundary is dropped, never shown stale.\n  \
        --patches\n      \
        The segmented unified-diff history of `--file`. The range is split at INTEGRITY\n      \
        BOUNDARIES, the points where reconstruction across them is invalid. Each one is a\n      \
        `File has been modified since read` harness error, an `originalFile` that disagrees\n      \
        with the replayed buffer, an external `edited_text_file`, or a heuristic Bash mutation.\n      \
        Each segment and each boundary carries its jsonl line, turn and timestamp. This is the\n      \
        CHANGES view. It rewinds a still-present file to an older state over a\n      \
        `--since`/`--until` window, and it is most useful when ONLY this session touched the\n      \
        file and never read it in full.\n  \
        --at <WHEN>\n      \
        The PARTIAL, line-numbered snapshot of `--file` as the LLM saw it at <WHEN>. WHEN takes\n      \
        the same relative, ISO and bare-date grammar as `--since` (`45s`/`90m`/`2h`/`3d`/`1w`,\n      \
        ISO8601, a bare date meaning local midnight) plus three recover-only forms,\n      \
        `@turn:<N>`, `@line:<N>` and `@latest`. Unlike restore, `--at` prints a PARTIAL\n      \
        snapshot. Known lines carry their number, unknown regions are marked\n      \
        `??? lines A..B unknown`, and gaps are NEVER fabricated. `--at @latest` is the\n      \
        partial-tolerant sibling of the default restore. It prints the final state with its\n      \
        holes shown.\n  \
        --coverage (alias --dry-run)\n      \
        Scopes a recovery WITHOUT dumping content. It reports which line ranges are\n      \
        recoverable, where the integrity boundaries sit, and the per-op counts (reads, edits,\n      \
        writes, bash, external-edits).\n  \
        --list-backups\n      \
        Lists Claude Code's OWN file-history checkpoint store for `--file` without scanning a\n      \
        transcript, one row per stored checkpoint (backup instant, bytes, @vN, store path).\n      \
        It is a listing only. Its content is never merged into a reconstruction.\n\
        \n\
        The TARGET selects the sessions. `@<uuid>` names one session, and a project path or encoded\n\
        directory names every session under it. Subagent transcripts are spanned by default, because\n\
        OMC fan-out edits happen in subagents. `--no-subagents` restricts the scan to the top-level\n\
        session.\n\
        \n\
        WINDOWING\n\
        `--turn (N|A..B|N..|-k)` is inclusive and counts turns from 0 in genuine-user order. It\n\
        intersects with `--since`/`--until` (ISO8601 or relative), so both filters apply.\n\
        `--file-lines (N|A..B|N..|-k)` further restricts the output to a 1-based span of FILE lines.\n\
        `--out <PATH>` writes the reconstructed artifact (the restored content, the snapshot or the\n\
        concatenated patches) verbatim to a file. In restore mode stdout then stays empty and a note\n\
        goes to stderr. In the other modes the report still prints to stdout.\n\
        \n\
        Reconstruction is NECESSARILY PARTIAL and NEVER fabricates. An unseen line is an explicit\n\
        gap, an edit with no anchor is a coverage hole, and a Bash touch is a heuristic boundary,\n\
        never an authoritative one. Nothing is truncated silently.",
    after_help = "MODE (at most one, and the default is restore)\n  \
        `--salvage`, `--patches`, `--at <WHEN>`, `--coverage` and `--list-backups` are MUTUALLY\n  \
        EXCLUSIVE, so passing two (`--coverage --patches`, say) is a parse error. With none of\n  \
        them the default RESTORE applies. It hands back the file's final content, or it FAILS\n  \
        without printing a partial file when the session saw only part of it, and `--salvage` is\n  \
        the mode to reach for then. `--file` is REQUIRED in every mode. Its value is an absolute\n  \
        path or the magic `@plan`, which names the session-bound plan file.\n\
        \n\
        EXAMPLES\n  \
        Restore the final content, or fail if the session saw only part of the file:\n    \
        csift recover @<uuid> --file /abs/app.py\n  \
        Restore straight back onto disk (raw bytes, no banners):\n    \
        csift recover @<uuid> --file /abs/app.py --out /abs/app.py\n  \
        The file is gone and was only partly seen. Dump what survived, with the gaps marked:\n    \
        csift recover @<uuid> --file /abs/gone.py --salvage\n  \
        Scope first. Print the covered ranges and the boundaries, with no dump:\n    \
        csift recover . --file /abs/PLAN.md --coverage\n  \
        List Claude Code's own rewind checkpoints of this path (a store listing, no transcript scan):\n    \
        csift recover --file /abs/app.py --list-backups\n  \
        Print the segmented unified diffs over the whole session:\n    \
        csift recover @<uuid> --file /abs/app.py --patches\n  \
        Restore from the events of the last 2h only (no mode flag, so this is the default restore):\n    \
        csift recover @<uuid> --file /abs/app.py --since 2h\n  \
        Print the partial-tolerant final snapshot, with the holes shown:\n    \
        csift recover @<uuid> --file /abs/app.py --at @latest\n  \
        Print the partial snapshot as the LLM saw it at turn 42:\n    \
        csift recover @<uuid> --file /abs/app.py --at @turn:42\n  \
        Reconstruct the session's bound plan, even if the plan file was deleted:\n    \
        csift recover @<uuid> --file @plan --out /tmp/plan.md\n  \
        Print only the patches that touch FILE lines 100-200:\n    \
        csift recover @<uuid> --file /abs/x.rs --file-lines 100..200 --patches\n\
        \n\
        WINDOW ACCOUNTING (every mode)\n  \
        Recover never pretends a window is clean. Beside the replayed events it counts, for each\n  \
        window, the commands that can change the file without the replay seeing it.\n    \
        bash mutations   parsed bash mutations OF the file, each disclosed as a boundary\n    \
        opaque commands  OPAQUE mutating-class commands, which mutate files they never name (a\n                     \
        formatter run, a package-manager install, an archive or patch\n                     \
        extraction, an interpreter write with an unextractable target), shown\n                     \
        as `fmt:`/`pkg:`/`extract:`/`interp:` markers\n    \
        PowerShell       PowerShell commands, whose command text is never lexically parsed\n  \
        When any are present the output says so and prints a ready-to-run, time-bounded\n  \
        `csift search` that lists the window's tool calls touching the file. `complete` therefore\n  \
        means complete FROM THE TOOL STREAM, and only a clean window note implies nothing else ran.\n  \
        A `--turn`/`--since`/`--until` window that excludes integrity-relevant events prints a\n  \
        note naming how many fell outside.\n\
        \n\
        BASH CONTENT ANCHORS (deterministic shell reads and writes join the replay)\n  \
        A small closed set of shell shapes carries the file content ITSELF. Those shapes replay as\n  \
        first-class events instead of degrading to a boundary.\n    \
        write side   a quoted-delimiter heredoc written via `cat`/`tee` (the body is\n                 \
        byte-verbatim in the transcript, and an unquoted delimiter is admitted only\n                 \
        with an expansion-free body), `echo`/`printf` with purely literal\n                 \
        arguments, and `truncate -s 0`\n    \
        read side    `cat <file>`, `head -n N <file>` and `sed -n 'A,Bp' <file>`, whose stdout\n                 \
        is the file window when the result echo is clean (empty stderr, not\n                 \
        interrupted, not externalized, exit ok)\n  \
        Every anchor needs literal operands resolved against the recording shell's cwd, in a\n  \
        segment free of substitutions and subshells.\n\
        \n  \
        A READ anchor demands a SINGLE simple command. The stdout of a pipe or a chain is a\n  \
        concatenation that nothing attributes to one file.\n\
        \n  \
        A WRITE anchor is per SEGMENT. Each admissible write inside a pipe or a `;`/`&&`/`||`/newline\n  \
        chain anchors (the write-then-run shape), provided the whole command's result is clean (no\n  \
        error, empty stderr, not interrupted) and no other segment WRITES the same path. A\n  \
        redirect or `tee` into the path, `cp` or `mv` onto it, `mv` away from it, `rm`, `touch` or\n  \
        `sed -i` refuses the anchor, because segment order is not replayed within one command. A\n  \
        read or a mode change of the path (`wc -l`, `chmod +x`) leaves the anchor standing.\n\
        \n  \
        Variable targets stay in the boundary lanes. So do interpreter heredocs, because a script\n  \
        is not file content, and ssh-fed heredocs, because they write to a REMOTE filesystem. An\n  \
        admitted write anchor supersedes its own heuristic bash-mutation boundary. A byte-known\n  \
        `>>` append is PLACED only onto a complete newline-terminated buffer. Anywhere else it is\n  \
        disclosed as a `bash_append_unplaced` boundary (content known, position unknowable).\n  \
        `tail` output cannot be placed in a line-keyed buffer without the file length, so it is\n  \
        deliberately not an anchor, and `sed -i` rewrites are likewise left heuristic. Coverage\n  \
        counts the admitted anchors as `bash-read-anchor`/`bash-write-anchor`, and segment\n  \
        provenance names the class (`bash-heredoc`/`bash-cat`/`bash-write`).\n\
        \n\
        FILE-HISTORY SNAPSHOTS (Claude Code's own backups, adopted as rebase anchors)\n  \
        Claude Code snapshots every Edit/Write-tracked file per prompt and bumps its `version`\n  \
        only when the bytes changed. The version sequence therefore records DISK TRUTH, including\n  \
        writes with NO tool record (the harness rewriting its settings on /model or /config, an\n  \
        outside editor). recover reads the sequence from the transcript. Where the store blob\n  \
        under `<claude-home>/file-history/` survives AND its mtime matches the recorded backup\n  \
        instant, recover COMPARES the replayed buffer against the snapshot content at each version\n  \
        change. A blob that fails the mtime check is refused, because the @vN name collides across\n  \
        a mid-session counter reset. A disagreement is an authoritative `external_write` boundary,\n  \
        and the replay REBASES on the snapshot bytes. The reconstruction then continues from what\n  \
        the disk held, not from a buffer no file ever carried.\n\
        \n  \
        Without the blob, a version jump with no tool write since the previous snapshot still\n  \
        discloses the same boundary without content, worded\n    \
        `external write (inferred, snapshot vN->vM, no tool record since L<line>)`\n  \
        It names the interval it bounds so you can go and read it. Nothing is rebased, and trust\n  \
        ends there. Both the per-prompt `file-history-snapshot` table and the per-write\n  \
        `file-history-delta` line feed that sequence, so the interval is as tight as the harness's\n  \
        own bookkeeping allows.\n\
        \n  \
        This runs for the `--file` TARGET on ANY path. `csift files` reports the same inference\n  \
        only for the settings family, because a global timeline row per tracked path would flood\n  \
        the output. A tool write and a silent write inside ONE snapshot interval merge into one\n  \
        bump. The content comparison is the complete detector, and the version number alone is\n  \
        not. The motivating producer is /rewind \"Restore code\", which writes NOTHING to the\n  \
        transcript. The next prompt's `edited_text_file` attachment is the only other trace, and\n  \
        it stays the authoritative boundary it already was.\n\
        \n\
        THE SURVIVAL AXIS\n  \
        The replay runs in FILE ORDER and stays that way. An Edit made on a branch the operator\n  \
        later rewound past DID hit the disk, so it is replayed like any other write. Only the\n  \
        reporting changes. Turn numbers are the LIVE ones, the same numbers `search` and `show`\n  \
        print and the ones `--turn` and `--at @turn:` resolve against. An event or boundary from a\n  \
        record off the surviving conversation is stamped `turn abandoned (root L<n>)` instead of a\n  \
        number. JSON gives it a NULL `turn_index` plus `survival` and `abandoned_root_line`, and a\n  \
        one-line disclosure says how many such events the window held. A `--turn` window still\n  \
        admits them, by the live turn they physically follow. A window that silently dropped a\n  \
        real write would reconstruct a state the disk never had.\n\
        \n\
        FRESHNESS SIGNALS (Claude Code's own, adopted as boundaries)\n  \
        `staleReadFileStateHint` on a Bash result\n      \
        Claude Code itself reporting, BY NAME, that the command modified files in its read\n      \
        set. The paths are relative to the shell cwd and are resolved before matching. The\n      \
        hint becomes an authoritative HARD `hint_modified` boundary, and it is the one signal\n      \
        that attributes a formatter-class rewrite to concrete files.\n  \
        `staleRecovered` on a successful Edit\n      \
        The disk had drifted since the last read, and the edit still applied. It becomes an\n      \
        authoritative `stale_recovered` annotation. Nothing is invalidated, and other changes\n      \
        exist outside the stream.\n  \
        `edited_text_file` external-edit boundary\n      \
        When the boundary's window also ran a formatter-class command, the boundary's detail\n      \
        names that command. The change may be the formatter's rewrite, so check the project's\n      \
        conventions.\n  \
        Failed ops (`String to replace not found`, `File does not exist`) are counted in the event\n  \
        ledger and never replayed.\n\
        \n\
        JSON SCHEMA (per --format json)\n  \
        EVERY mode, restore included, emits the envelope. One `{kind:\"header\",…}` line comes first\n  \
        and one `{kind:\"summary\", file, mode, sessions, skipped_lines}` line comes last. That\n  \
        summary carries the crossing set {crossing_budget, boundaries_crossed, budget_exhausted,\n  \
        boundaries_above_floor, floor_line, floor_cause} whenever `--cross-compactions` is\n  \
        non-zero. The set records what the survival walk stepped over to decide which replayed\n  \
        events are on the surviving conversation, and where it stopped. At `--cross-compactions 0`\n  \
        the keys are absent. The text form of the set is two lines in every mode. Those lines go\n  \
        to stdout in the four reporting modes, and to STDERR in the default restore mode, whose\n  \
        stdout is the restored file.\n\
        \n  \
        The rows between the header and the summary depend on the mode.\n    \
        restore           One `{kind:\"restore\", file, complete, lines, boundaries, bash_events,\n                      \
        opaque_commands, powershell_commands, suggested_search}` row carrying\n                      \
        `content`, or `path` + `wrote` with `--out`. A partial or no-history\n                      \
        run still emits its `{kind:\"restore\", complete:false, reason}` row and\n                      \
        the summary, then errors on stderr with a non-zero exit.\n    \
        --patches         `{kind:\"segment\",…}` and `{kind:\"boundary\",…}` rows.\n    \
        --coverage        `{kind:\"coverage\", covered_ranges, boundaries, hard_boundaries,\n                      \
        soft_boundaries, events, abandoned_events, fragments,\n                      \
        recoverable_lines, opaque_commands, powershell_commands,\n                      \
        suggested_search, …}`\n    \
        --at, --salvage   `{kind:\"snapshot\", lines, gaps, seen_total_lines, abandoned_events,\n                      \
        boundaries, opaque_commands, powershell_commands, suggested_search, …}`\n  \
        `abandoned_events` counts the replayed events that came from records off the surviving\n  \
        conversation. It is a share of what produced the coverage above, never subtracted from it.\n\
        \n  \
        A boundary object is `{line, source_session_id, source_line, turn_index, survival,\n  \
        abandoned_root_line, ts_utc, ts_local, cause, confidence, detail}`. `turn_index` is NULL\n  \
        on an abandoned record, and `abandoned_root_line` names its branch head. `line` is the\n  \
        replay/cutoff coordinate to feed to `--at @line:<N>`. `source_line` is the REAL jsonl line\n  \
        in `source_session_id`, and that pair is what you feed to `csift show`. The two pairs\n  \
        differ only after a cross-transcript merge.\n\
        \n  \
        Every per-session row carries the id-domain discriminators `{session_id, is_subagent,\n  \
        parent_session_id}`. `is_subagent` flags a bare-hex subagent record. Re-feed\n  \
        `parent_session_id`, never the bare `session_id`."
)]
pub struct RecoverArgs {
    /// Project targets (actual cwd or encoded dir) whose sessions to reconstruct from.
    ///
    /// A target is a path or an `@` session token, and the argument repeats. With none, every
    /// project is scanned. `@<uuid>` (8-4-4-4-12 hex) scopes to one session, which is how
    /// `csift recover @<uuid> --file …` in the EXAMPLES works. `@<agent-id>` scopes to one
    /// subagent and its subtree. The ids come from `csift agents`, and a subagent's WRITE gap
    /// closes from the tool_use input. `@main` and `@trap:<marker>` resolve the calling
    /// session, and a `*.jsonl` file scopes to that transcript.
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

    /// The ABSOLUTE path of the file whose history to reconstruct. REQUIRED in every mode.
    ///
    /// The target is matched against each path exactly as the transcript wrote it, and against
    /// a relative path resolved through its record's shell cwd. A line that carries no cwd (a
    /// file-history snapshot key) resolves through the transcript's first cwd, the directory
    /// such a key is relative to. In a session that changed directory before its first write,
    /// that join can name a path no record wrote. A basename-suffix match is the fallback.
    ///
    /// A suffix that names MORE THAN ONE distinct file in scope is refused before anything is
    /// replayed. A relative record path counts as the file its cwd resolves it to. The refusal
    /// prints the count and the first ten candidates, with the rest counted. Replayed as one
    /// file, the candidates would stitch their edits into one buffer, or keep one file's
    /// history and drop the others'. Pass one of the candidates, or a longer suffix that names
    /// exactly one. Every candidate the refusal prints replays when passed back. A candidate
    /// that other candidates end with (a relative path from a transcript with no cwd) replays
    /// that file alone, with a note on stderr.
    ///
    /// Paths are compared LEXICALLY, the target and every record path alike. `.` segments are
    /// dropped, and a run of `/` collapses to one (a leading `//` stays). So `./a/lib.rs` and
    /// `a/lib.rs`, or `/p//q` and `/p/q`, are one file, and any of its spellings replays its
    /// whole history. Nothing that needs the filesystem is resolved. A `..` stays as written,
    /// except inside a relative path joined to its record's cwd, which takes the shell join's
    /// lexical `..` as a Bash operand does. `/tmp/x` and the `/private/tmp/x` a symlink
    /// resolves it to are two files, refused as two. Each one passed back replays only the
    /// events recorded under that spelling, which can read `complete` on content that a write
    /// through the other spelling replaced.
    ///
    /// The MAGIC value `@plan` (bash-safe, no escaping) reconstructs the session-BOUND plan
    /// file instead, the path named by the session's `plan_mode` attachment. A deleted plan is
    /// therefore recoverable from the transcript alone. To locate it without dumping, use
    /// `csift plan`.
    #[arg(long, value_name = "ABS_PATH", verbatim_doc_comment)]
    pub file: Option<String>,

    /// Exclude subagent transcripts and reconstruct from the top-level session only.
    ///
    /// Subagent transcripts are spanned by default. This is the one flag that narrows the span.
    /// `--subagents` only restates the default.
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

    /// Best-effort, line-numbered FINAL-state fragment, the never-fails sibling of restore.
    ///
    /// Where the default restore REFUSES a partial file, `--salvage` dumps whatever survived.
    /// Known lines are numbered, and unrecoverable lines are left as explicit
    /// `??? lines A..B unknown` gaps. It suits a file that is gone, was only partly read and
    /// was barely edited, when the goal is to salvage the surviving proportion instead of
    /// rewinding. The output is identical to `--at @latest`.
    #[arg(long, group = "mode", verbatim_doc_comment)]
    pub salvage: bool,

    /// Segmented unified-diff history of `--file`, the CHANGES view.
    ///
    /// It rewinds a still-present file over a `--since`/`--until` window, and it works best
    /// when only this session touched the file and did not read it whole. It is NOT the
    /// default. With no mode flag, `recover` RESTOREs the final content instead, and it fails
    /// loudly and never emits a partial file.
    #[arg(long, group = "mode", verbatim_doc_comment)]
    pub patches: bool,

    /// Point-in-time partial snapshot of `--file` as of `<WHEN>`.
    ///
    /// Setting this flag both selects the mode AND supplies its cutoff. WHEN uses the SAME
    /// grammar as `--since`, plus three recover-only forms.
    ///   `Ns`/`Nm`/`Nh`/`Nd`/`Nw`   that long ago (`45s`, `90m`, `2h`, `3d`, `1w`)
    ///   ISO8601 datetime          for example `2026-06-01T05:00:00Z`
    ///   bare date                 `2026-06-01` means LOCAL MIDNIGHT
    ///   `@turn:<N>`               the snapshot as of the first line after genuine-user turn N.
    ///                             Find N in the `<id>·t<n>` header of `csift search` text, or
    ///                             in `turn_index` in any `--format json` record.
    ///   `@line:<N>`               the snapshot as of JSONL TRANSCRIPT line N, the `L<n>` this
    ///                             tool's text prints and the `line` key its JSON carries. It
    ///                             is NOT a file line of `--file`. For a 1-based FILE-line span
    ///                             of `--file`, use `--file-lines` instead.
    ///   `@latest`                 the FINAL reconstructed state, with no cutoff. It is the
    ///                             clean way to ask for the last form without guessing a
    ///                             timestamp past the last write.
    /// A datetime bound is INCLUSIVE of events AT that instant.
    #[arg(long, value_name = "WHEN", group = "mode", verbatim_doc_comment)]
    pub at: Option<String>,

    /// Print a coverage and scoping summary, with no content dump. `--dry-run` is an alias.
    #[arg(long, visible_alias = "dry-run", group = "mode", verbatim_doc_comment)]
    pub coverage: bool,

    /// List Claude Code's OWN file-history checkpoint store for `--file`.
    ///
    /// The store holds the rewind feature's snapshots under
    /// `<claude-home>/file-history/<session>/<hash>@vN`. The listing prints one row per stored
    /// checkpoint (backup instant, bytes, @vN, store path), ordered by backup instant. The
    /// store key is the sha256 of the absolute path (first 16 hex), so `--file` must be a
    /// literal absolute path. `@plan` cannot be hashed.
    ///
    /// The listing has provenance bounds. Only the structured file tools and an approved
    /// in-place `sed` preview write to the store, so other shell writes and manual edits never
    /// land there. The store is PRUNED over time, and the `@vN` counters reset per session dir,
    /// so they are never an order key. Absence therefore proves nothing, and a listing is NOT a
    /// history. csift never merges checkpoint content into a reconstruction, because that
    /// content has no transcript anchor. Inspect or copy a row's store path yourself.
    #[arg(
        long = "list-backups",
        group = "mode",
        conflicts_with = "files_from",
        verbatim_doc_comment
    )]
    pub list_backups: bool,

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
    ///   `Ns`/`Nm`/`Nh`/`Nd`/`Nw`   that many seconds, minutes, hours, days or weeks AGO
    ///                             (`45s`, `90m`, `2h`, `3d`, `1w`)
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

    /// Restrict to a 1-based, inclusive span of FILE lines of `--file`.
    ///
    /// The span takes the shared range grammar (`N`, `A..B`, `N..`, `-k` for the last k lines)
    /// and filters the reconstructed line space, independently of the turn and time window. It
    /// applies in `--patches`, `--at` and `--coverage`. The flag is named `--file-lines`
    /// because `line` and `L` always mean a TRANSCRIPT jsonl line in csift. This is the one
    /// flag that addresses the reconstructed FILE's lines.
    #[arg(
        long = "file-lines",
        value_name = "N|A..B|N..|-k",
        allow_hyphen_values = true,
        verbatim_doc_comment
    )]
    pub line_range: Option<String>,

    /// Write the reconstructed artifact verbatim to this file.
    ///
    /// The artifact is the restored content, the snapshot or the concatenated patches. In the
    /// default restore mode stdout stays empty and a note goes to stderr. With `--salvage`,
    /// `--at` and `--patches` the report still prints to stdout. Stdout shortens each over-long
    /// line or body to a ~400-char excerpt (a `… (+N chars)` marker) for readability, while
    /// `--out` writes every line in full. `--out` is IGNORED in `--coverage` mode, a scoping
    /// summary with no artifact to write. No file is created there, and a stderr note says so.
    #[arg(long, value_name = "PATH", verbatim_doc_comment)]
    pub out: Option<PathBuf>,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// Reconstruct MANY files in a single corpus scan (BATCH MODE).
    ///
    /// The value is a manifest listing the target absolute file paths, one per line, with blank
    /// lines and `#` comments ignored. Each transcript is parsed ONCE, and every listed file it
    /// touched is extracted from it. That turns N separate `recover --file` runs (N
    /// whole-corpus parses) into one. The flag requires `--out-dir` and is mutually exclusive
    /// with `--file`. It honors `--at`/`--since`/`--until`, and the default is each file's
    /// final reconstructed state. Each file is written under `--out-dir`, mirroring its
    /// absolute path, and a `recovery-report.tsv` summarizes per-file status. A manifest line
    /// follows `--file`'s rule. A line that names several files is not replayed. Its report row
    /// reads `ambiguous`, stderr names the manifest line and its candidates, and the summary
    /// counts it beside the recovered files, not among them.
    #[arg(long, value_name = "MANIFEST", verbatim_doc_comment)]
    pub files_from: Option<PathBuf>,

    /// Output directory for BATCH MODE, required with `--files-from`.
    ///
    /// Each recovered file is written to `<DIR>/<abs-path-without-leading-slash>`, with
    /// directories created as needed. Existing files are not clobbered unless `--force` is
    /// given.
    #[arg(long, value_name = "DIR", verbatim_doc_comment)]
    pub out_dir: Option<PathBuf>,

    /// In batch mode, overwrite an output file that already exists.
    ///
    /// Without it, an existing file is skipped and reported.
    #[arg(long, verbatim_doc_comment)]
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
    /// none set => the `Restore` default (full content, or an error if only partial).
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
