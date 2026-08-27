//! ImageArgs + PlanArgs.

use super::*;

#[derive(Debug, Args)]
#[command(
    about = "List + extract the images a session carries (inline base64 blocks → files)",
    long_about = "List and EXTRACT the images a session carries. A pasted/attached image (and a \
        tool-result screenshot) is stored INLINE on a record as a base64 image block, so `image` \
        decodes it straight back to a file; nothing was externalised.\n\n\
        TWO ADDRESSES: `#N`: the session's own `[Image #N]` handle (`verbatim`/`search` show it \
        inline; an ambiguous `#N` errors with the occurrence list, disambiguate via `--since`/\
        `--turn`/`--uuid`); and `L<line>i<n>`: the exact locator (carrying record's JSONL \
        line + ordinal within it).\n\n\
        Default action is to LIST (id · media-type · ~size · time). Pass `--out <PATH>` to EXTRACT: \
        a DIRECTORY keeps each image's SOURCE format (auto-named); a FILE path's extension CONVERTS \
        the single image to that format (`convert in.png out.jpg` idiom). A URL-source image has no \
        inline bytes; it is reported, never fabricated.",
    after_help = "EXAMPLES\n  \
          csift image @<uuid>                             # list every image (deduped)\n  \
          csift image . --format json                     # machine-readable listing\n  \
          csift image @<uuid> --no-subagents --id '32,33,34,36' --out /tmp/imgs  # re-share by handle\n  \
          csift image @<uuid> --no-subagents --id '1' --since 1h --out /tmp/imgs    # disambiguate a reused #1\n  \
          csift image @<uuid> --no-subagents --id L6812i2 --out /tmp/shot.jpg        # one image -> a file, convert\n\n\
        JSON SCHEMA (per --format json)\n  \
          Envelope: header → listing rows → summary. Listing rows: {kind:\"image\", handle \
        (the display `#N` / locator), seq, id, line, img_index, session_id, is_subagent, \
        parent_session_id, source_kind, media_type, b64_len, est_bytes, url, record_uuid, \
        ts_utc, ts_local}. With `--out`, each written file adds {kind:\"extract\", handle, \
        seq, id, session_id, is_subagent, parent_session_id, path, bytes, media_type, \
        source_media_type, converted, notes}."
)]
pub struct ImageArgs {
    /// Project target(s) (actual cwd or encoded dir) whose session(s) to scan for images,
    /// OR an `@<uuid>` session token. With none, every project is scanned. `@<uuid>` scopes to
    /// one session, `@main`/`@trap:<marker>` resolve the calling session, and a `*.jsonl` file scopes to
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

    /// ADDRESS specific images by the `#N` session handle (`--id 32,33`) or the exact
    /// `L<line>i<n>` locator (`--id L6812i1`). Repeatable + comma-delimited. Without `--out`,
    /// filters the LISTING to these; with `--out`, extracts only these. Both forms are
    /// per-transcript, so `--id` needs a single transcript in scope (pin with `@<uuid>
    /// --no-subagents`). If a `#N` is AMBIGUOUS (CC reuses `#N` across prompts, so it names >1
    /// distinct image), `image` ERRORS with the occurrence list; disambiguate with the exact
    /// `L<line>i<n>`, or narrow scope via `--since`/`--until` / `--turn` / `--uuid`. `#N` is
    /// inherited from CC's paste-time `[Image #N]` numbering, NOT a dense 1..N index csift
    /// assigns: a transcript's handles can start past #1 and carry HOLES (a number whose
    /// image never landed in this transcript); a `--id` miss therefore ERRORS naming the
    /// handles that DO exist, and the plain listing shows them all.
    #[arg(long, value_name = "ID", value_delimiter = ',')]
    pub id: Vec<String>,

    /// Lower time bound (ISO8601 or relative `2h`/`3d`/…, system-local); narrows the image set
    /// so an ambiguous `#N` can resolve in a window where it is unique. A `#N` disambiguator.
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as `--since`). A `#N` disambiguator.
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Restrict to images in this turn range: the shared grammar (`N` · `A..B` · `N..` · `-k` from the end), 0-based inclusive. A per-transcript
    /// `#N` disambiguator; needs a single transcript in scope.
    #[arg(
        long = "turn",
        value_name = "N|A..B|N..|-k",
        allow_hyphen_values = true
    )]
    pub turn_range: Option<String>,

    /// Restrict to images carried by the record whose uuid starts with this (a `#N`
    /// disambiguator: the uuid shown in the ambiguity error / `--format json`).
    #[arg(long, value_name = "UUID")]
    pub uuid: Option<String>,

    /// EXTRACT: decode the selected image(s) to this PATH. The path's EXTENSION drives the
    /// format (the `convert in.png out.jpg` idiom): a **directory** (or any path WITHOUT an
    /// `png`/`jpg`/`jpeg`/`gif`/`webp` extension) writes each image auto-named
    /// `<session>[-img<N>]-L<line>i<n>.<ext>` in its SOURCE format; a path WITH one of those
    /// extensions writes the (single) selected image to exactly that file, CONVERTING to that
    /// format if it differs from the source (→jpeg lossy q90, →gif dithered palette, →webp lossy
    /// q90; an animated GIF → a still format yields its first frame + a warning). Without
    /// `--out`, `image` only LISTS.
    #[arg(long, value_name = "PATH")]
    pub out: Option<PathBuf>,

    /// Exclude subagent transcripts: scan only the top-level session. Subagent transcripts are
    /// scanned by default (a tool screenshot may live in one); this is the only span flag.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Span subagent transcripts: the DEFAULT here; the explicit flag exists so every
    /// span command answers the same two switches (`--subagents` / `--no-subagents`).
    #[arg(long = "subagents", conflicts_with = "no_subagents")]
    pub subagents: bool,

    /// Emit JSON (one object per image + a trailing summary) instead of the text listing.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl ImageArgs {
    /// Whether subagent transcripts are spanned (the default). `--no-subagents` restricts to
    /// the top-level session(s). Feeds [`crate::path::SubagentScope::from`].
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.subagents || !self.no_subagents
    }
}

#[derive(Debug, Args)]
#[command(
    about = "Locate the Plan-Mode plan file BOUND to a session (via its `plan_mode` attachment)",
    long_about = "Locate the Plan-Mode PLAN FILE bound to a session. Claude Code stores plans \
        flat under `~/.claude/plans/<three-words>.md` (a subagent's gets an `-agent-<hex>` \
        suffix); the random name is bound to the session by the `plan_mode` ATTACHMENT the \
        transcript writes on entering Plan Mode. That attachment is the authoritative binding \
       : a session may also Edit/Write OTHER sessions' plan files, but those are not its own \
        plan, so this never path-guesses.\n\n\
        TARGET: a project PATH / encoded-dir (positional), or an `@<uuid>` session token. \
        With NO target, the CALLING session is resolved from `CLAUDE_CODE_SESSION_ID` \
        (like `whoami`): `csift plan` answers \"what is MY plan file\". Subagents are spanned \
        by default (their own plans surface, flagged); `--no-subagents` restricts to the \
        top-level session.\n\n\
        To DUMP the plan's content (even after it was deleted), feed it to recover: \
        `csift recover @<uuid> --file @plan` reconstructs the bound plan from the \
        transcript's Writes/Edits.",
    after_help = "EXAMPLES\n  \
          csift plan                                  # the calling session's bound plan file\n  \
          csift plan @<uuid>                          # a specific session's plan file\n  \
          csift plan . --format json                  # every session under this project (NDJSON)\n  \
          csift recover @<uuid> --file @plan          # DUMP the bound plan's reconstructed content\n\n\
        JSON SCHEMA (per --format json)\n  \
          Envelope: header → {kind:\"plan\", plan_file, session_id, is_subagent, \
        parent_session_id, plan_exists, line, slug} rows → summary. `plan_exists` says whether \
        the bound file is still on disk: a DELETED plan is still locatable here, and \
        `csift recover <target> --file @plan` rebuilds its content from the transcript. \
        `slug` is the session's plan slug (one stable value per session, minted at Plan-Mode \
        entry; the harness derives the plan file name from it and re-keys recovery on it) - \
        null when the binding record predates the field."
)]
pub struct PlanArgs {
    /// Project target(s) (actual cwd or encoded dir) whose session(s) to resolve the bound
    /// plan file for, OR an `@<uuid>` session token (`@main`/`@trap:<marker>` resolve the calling
    /// session; a `*.jsonl` file scopes to that transcript). With no target, the calling
    /// session is resolved from the environment.
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

    /// REVERSE lookup: given a PLAN FILE, find the session(s) BOUND to it (the inverse of the
    /// default session→plan direction). Scans the resolved scope (default every project, or
    /// narrow with a PATH target) for transcripts whose `plan_mode` attachment names this exact
    /// plan file, and prints the bound session/subagent id(s). Useful when you have a plan file
    /// (e.g. from `~/.claude/plans/`) and need to know which conversation owns it. The path is
    /// matched by absolute identity (relative / `~` inputs are absolutized first).
    #[arg(long, value_name = "PLAN_FILE")]
    pub reverse: Option<PathBuf>,

    /// AUDIT the target scope's PLAN-FILE edits against plan BINDINGS: find every
    /// structured mutation (Write/Edit/MultiEdit/NotebookEdit) the scope made to a file
    /// that SOME session binds as its plan, and WARN when the mutating session does not
    /// bind that file itself. Why it matters: after a compaction only the session's own
    /// BOUND plan is re-injected in full; content parked in another session's plan file
    /// does not come back. Plan files are identified by JOINING against the corpus's
    /// plan_mode bindings (one prefiltered scan of every project), never by guessing a
    /// plans directory (plansDirectory is configurable). Bash-side edits are outside
    /// this audit (structured tools only).
    #[arg(long, conflicts_with = "reverse")]
    pub audit: bool,

    /// Exclude subagent transcripts: resolve only the top-level session's bound plan (forward),
    /// or only top-level bindings (reverse).
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Span subagent transcripts: the DEFAULT here; the explicit flag exists so every
    /// span command answers the same two switches (`--subagents` / `--no-subagents`).
    #[arg(long = "subagents", conflicts_with = "no_subagents")]
    pub subagents: bool,

    /// Emit NDJSON (one object per resolved plan) instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl PlanArgs {
    /// Subagent span is ON by default; `--no-subagents` restricts to the top-level session.
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.subagents || !self.no_subagents
    }
}
