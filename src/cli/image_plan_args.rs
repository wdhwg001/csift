//! ImageArgs + PlanArgs.

use super::*;

#[derive(Debug, Args)]
#[command(
    about = "List the images a session carries and extract its inline base64 blocks to files",
    long_about = "List and EXTRACT the images a session carries.\n\n\
A pasted or attached image is stored INLINE on its record as a base64 image block, so\n\
`image` decodes it straight back to a file. Nothing was externalised. The same holds for a\n\
tool-result screenshot, and for a picture pasted into a prompt that was queued and then\n\
edited or recalled, which survives only in the queued_command attachment.\n\n\
TWO ADDRESSES. `#N` is the session's own `[Image #N]` handle, which `verbatim` and `search`\n\
show inline. An ambiguous `#N` errors with the occurrence list, and `--since`, `--turn` or\n\
`--uuid` disambiguates it. `L<line>i<n>` is the exact locator, made of the carrying\n\
record's JSONL line and the image's ordinal within that record.\n\n\
The default action is to LIST each image's id, media type, approximate size and time. Pass\n\
`--out <PATH>` to EXTRACT. A DIRECTORY keeps each image's SOURCE format under an automatic\n\
name. A FILE path's extension CONVERTS the single image to that format, in the\n\
`convert in.png out.jpg` idiom. A URL-source image has no inline bytes, so it is reported\n\
and never fabricated.\n\n\
THE SURVIVAL AXIS. An image pasted into a prompt the operator later rewound past, or into\n\
one recalled and re-typed, is still LISTED and still extractable. The bytes are on disk,\n\
and `--out` writes the same file. Such an image is only MARKED (`[abandoned]` in text, JSON\n\
`survival`), so you know it is not part of the conversation the model now sees.\n\
`[pre-cut]` marks one above a compaction cut. `--turn` is the exception. It windows on\n\
NUMBERED turns, using the live numbering `search` and `show` print, and an abandoned record\n\
belongs to no numbered turn, so a turn window leaves it out.\n\n\
PDF PAGES. A `Read` of a .pdf that carries a `pages` parameter returns one image element per\n\
requested page. Those pages list and extract like any other image, and each row says WHICH\n\
page it is. The page number sits on no key of the element, because the harness labels only\n\
a page it FAILED to render. csift derives it from the carrying record's own\n\
`toolUseResult.firstPage` plus the element's position among the content elements after the\n\
leading text. Counting POSITIONS keeps a failed page in the middle from shifting the pages\n\
after it. Text prints ` p<N>` beside the media type, and the document's BASENAME once per\n\
record group (one transcript, one line, one document). The whole path is a JSON field and\n\
no listing column, and JSON carries `page` and `pdf_path`.\n\
The gate has two halves. The record's own result must read `type == \"parts\"` WITH a\n\
`firstPage`, and the record must carry exactly one `tool_result` block, because a `parts`\n\
result describes one tool call and could not say which of two blocks it belongs to.\n\
Anything else reads `page: null` and never an assumed page 1. That covers a screenshot, a\n\
`parts` result from a build before the key existed, the REPL tool's own result (which\n\
re-emits inner-Read pages with no offset of its own), and a record carrying two\n\
tool_result blocks. A whole-PDF `Read` (no `pages`) returns a `document` block instead of\n\
images and is not listed at all. The page is provenance and never an address. `--id`,\n\
`--turn` and the extracted filename are exactly what they were.",
    after_help = "EXAMPLES\n  \
csift image @<uuid>                    # list every image (deduped)\n  \
csift image . --format json            # machine-readable listing\n  \
# Re-share images by handle:\n  \
csift image @<uuid> --no-subagents --id '32,33,34,36' --out /tmp/imgs\n  \
# Disambiguate a reused #1 with a time window:\n  \
csift image @<uuid> --no-subagents --id '1' --since 1h --out /tmp/imgs\n  \
# One image to a file, converted by the file's extension:\n  \
csift image @<uuid> --no-subagents --id L6812i2 --out /tmp/shot.jpg\n\n\
JSON SCHEMA (per --format json)\n  \
The stream is a header, then listing rows, then a summary. A listing row is\n  \
{kind:\"image\", handle, seq, id, line, img_index, session_id, is_subagent,\n  \
parent_session_id, source_kind, media_type, b64_len, est_bytes, url, page, pdf_path,\n  \
record_uuid, survival, ts_utc, ts_local}. `handle` is the display `#N` or the locator.\n  \
`survival` is live | pre-cut | abandoned, the carrying record's place in the surviving\n  \
conversation, and an abandoned image is listed and extractable like any other. `page` is\n  \
the DOCUMENT page number when the image is a PDF page and null for every other image.\n  \
`pdf_path` is the document it came from, verbatim, and null beside a null page.\n  \
With `--out`, each written file adds {kind:\"extract\", handle, seq, id, session_id,\n  \
is_subagent, parent_session_id, path, bytes, media_type, source_media_type, converted,\n  \
notes}. The summary carries the crossing set {crossing_budget, boundaries_crossed,\n  \
budget_exhausted, boundaries_above_floor, floor_line, floor_cause} whenever\n  \
`--cross-compactions` is non-zero. It says what the survival walk stepped over to decide\n  \
each row's `survival`, and where it stopped. At `--cross-compactions 0` the keys are\n  \
absent."
)]
pub struct ImageArgs {
    /// Project target(s) (an actual cwd or an encoded dir) whose sessions to scan for images,
    /// or an `@<uuid>` session token. With none, every project is scanned. `@<uuid>` scopes to
    /// one session, `@main` and `@trap:<marker>` resolve the calling session, and a `*.jsonl`
    /// file scopes to that transcript.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target,
        verbatim_doc_comment
    )]
    pub paths: Vec<PathBuf>,

    /// Also scope to the session ids in FILE (`-` reads stdin). The ids are whitespace- or
    /// newline-separated uuid, uuid-prefix or agent-id tokens, bare or `@`-prefixed, exactly as
    /// csift emits them (`search -l`, JSON `transcript_ids` / `parent_session_id`). They are
    /// unioned with the positional targets, and each id resolves fail-loud like an `@`
    /// positional. An EMPTY list (an upstream stage that found nothing) scopes to NOTHING. The
    /// result is an honest empty one with exit 0, never a silent widening to every project. The
    /// resolved ids then follow this command's normal span rules, exactly as `@` positionals
    /// would. On the span-by-default commands each session EXPANDS to its subagent transcripts,
    /// and `--no-subagents` pins the scope to exactly the listed ids.
    #[arg(long = "sessions-from", value_name = "FILE|-", verbatim_doc_comment)]
    pub sessions_from: Option<std::path::PathBuf>,

    /// ADDRESS specific images by the `#N` session handle (`--id 32,33`) or by the exact
    /// `L<line>i<n>` locator (`--id L6812i1`). Repeatable and comma-delimited. Without `--out`
    /// it filters the LISTING to these images, and with `--out` it extracts only these. Both
    /// forms are per transcript, so `--id` needs a single transcript in scope (pin one with
    /// `@<uuid> --no-subagents`). Claude Code reuses `#N` across prompts, so one handle can
    /// name more than one distinct image. Such an AMBIGUOUS `#N` makes `image` fail with the
    /// occurrence list. Disambiguate with the exact `L<line>i<n>`, or narrow the scope with
    /// `--since`/`--until`, `--turn` or `--uuid`. `#N` is inherited from Claude Code's
    /// paste-time `[Image #N]` numbering. csift assigns no dense 1..N index of its own, so a
    /// transcript's handles can start past #1 and carry HOLES (a number whose image never
    /// landed in this transcript). A `--id` miss therefore fails naming the handles that DO
    /// exist, and the plain listing shows them all.
    #[arg(long, value_name = "ID", value_delimiter = ',', verbatim_doc_comment)]
    pub id: Vec<String>,

    /// Lower time bound (ISO8601 or relative `2h`/`3d`/…, system-local). It narrows the image
    /// set so an ambiguous `#N` can resolve in a window where it is unique. A `#N`
    /// disambiguator.
    #[arg(long, value_name = "WHEN", verbatim_doc_comment)]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as `--since`). A `#N` disambiguator.
    #[arg(long, value_name = "WHEN", verbatim_doc_comment)]
    pub until: Option<String>,

    /// Restrict to images in this turn range, written in the shared grammar (`N` · `A..B` ·
    /// `N..` · `-k` from the end), 0-based and inclusive. A per-transcript `#N` disambiguator,
    /// so it needs a single transcript in scope.
    #[arg(
        long = "turn",
        value_name = "N|A..B|N..|-k",
        allow_hyphen_values = true,
        verbatim_doc_comment
    )]
    pub turn_range: Option<String>,

    /// Restrict to images carried by the record whose uuid starts with this. A `#N`
    /// disambiguator. The ambiguity error and `--format json` both show the uuid.
    #[arg(long, value_name = "UUID", verbatim_doc_comment)]
    pub uuid: Option<String>,

    /// EXTRACT the selected images to this PATH. The path's EXTENSION drives the format, in the
    /// `convert in.png out.jpg` idiom. A directory, or any path WITHOUT a
    /// `png`/`jpg`/`jpeg`/`gif`/`webp` extension, gets each image in its SOURCE format,
    /// auto-named `<session>[-img<N>]-L<line>i<n>.<ext>`. A path WITH one of those extensions
    /// gets the (single) selected image at exactly that file, CONVERTED to that format when it
    /// differs from the source (→jpeg lossy q90, →gif dithered palette, →webp lossy q90).
    /// An animated GIF converted to a still format yields its first frame and a warning.
    /// Without `--out`, `image` only LISTS.
    #[arg(long, value_name = "PATH", verbatim_doc_comment)]
    pub out: Option<PathBuf>,

    /// Exclude subagent transcripts and scan only the top-level session. Subagent transcripts
    /// are scanned by default, since a tool screenshot may live in one.
    #[arg(long = "no-subagents", verbatim_doc_comment)]
    pub no_subagents: bool,

    /// Span subagent transcripts. This is the DEFAULT here. The explicit flag exists so every
    /// span command answers the same two switches (`--subagents` / `--no-subagents`).
    #[arg(
        long = "subagents",
        conflicts_with = "no_subagents",
        verbatim_doc_comment
    )]
    pub subagents: bool,

    /// Emit JSON (a header, one object per image or extracted file, a summary) instead of the
    /// text listing.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, verbatim_doc_comment)]
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
    about = "Find the plan file bound to a session by its plan_mode attachment or the first-slug law",
    long_about = "Locate the Plan Mode PLAN FILE bound to a session.\n\n\
Claude Code stores plans flat under `~/.claude/plans/<three-words>.md`, and a subagent's plan\n\
gets an `-agent-<agent id>` suffix. Two binding laws decide which file is a session's plan.\n\
They apply in precedence order and mirror Claude Code's own.\n  \
1. The `plan_mode` ATTACHMENT written on entering Plan Mode, which carries the verbatim\n     \
path.\n  \
2. When NO plan_mode exists anywhere, the FIRST record carrying a `slug` binds\n     \
`<plans-dir>/<slug>.md`. In a SUBAGENT transcript, whose records carry the session's\n     \
slug, it binds `<plans-dir>/<slug>-agent-<its agent id>.md`.\n\
`<plans-dir>` is `plansDirectory` from the merged user/project/local settings, resolved\n\
against the PROJECT ROOT (the record's cwd) and accepted only inside it, else\n\
`~/.claude/plans`. Claude Code reads the first slug in the log. A forked or background\n\
session reaches the second law by construction, because its clone strips the attachment\n\
history. Any compaction also MINTS a slug, even without Plan Mode. The output then says\n\
`slug only`, plus `MINTED at a compaction boundary` when the first carrier is the boundary\n\
itself, and Claude Code will still inject or rebuild that file. A session may also Edit or\n\
Write OTHER sessions' plan files. Those are not its own plan, so this command never guesses\n\
from a path.\n\n\
A target is a project PATH or encoded dir (positional), or an `@<uuid>` session token. With\n\
NO target, the CALLING session is resolved from `CLAUDE_CODE_SESSION_ID`, as `whoami` does,\n\
so `csift plan` answers \"what is MY plan file\". Subagents are spanned by default, and their\n\
own plans surface flagged. `--no-subagents` restricts to the top-level session.\n\n\
To DUMP the plan's content, even after the file was deleted, feed it to recover.\n\
`csift recover @<uuid> --file @plan` reconstructs the bound plan from the transcript's\n\
Writes and Edits.",
    after_help = "EXAMPLES\n  \
csift plan                               # the calling session's bound plan file\n  \
csift plan @<uuid>                       # a specific session's plan file\n  \
csift plan . --format json               # every session under this project (NDJSON)\n  \
csift recover @<uuid> --file @plan       # DUMP the bound plan's reconstructed content\n\n\
JSON SCHEMA (per --format json)\n  \
The stream is a header, then {kind:\"plan\", plan_file, session_id, is_subagent,\n  \
parent_session_id, plan_exists, line, slug} rows, then a summary. `plan_exists` says\n  \
whether the bound file is still on disk. A DELETED plan is still locatable here, and\n  \
`csift recover <target> --file @plan` rebuilds its content from the transcript. `slug` is\n  \
the session's plan slug, one stable value per session. It is minted before Plan Mode is\n  \
entered, so the first slug-carrying record precedes the plan_mode line, and the harness\n  \
derives the plan file name from it and re-keys recovery on it. `slug` is null when the\n  \
binding record predates the field.\n\n\
JSON SCHEMA under --audit\n  \
The stream is a header {mode:\"audit\"}, then three row kinds in this order, then a\n  \
summary:\n    \
{kind:\"binding\", session_id, is_subagent, parent_session_id, plan_file, line, slug,\n     \
plan_exists, slug_changes:[{line, from, to}], first_slug_line, first_slug_utc,\n     \
first_slug_local, slug_vs_plan_file (before|after|same|unknown),\n     \
slug_vs_plan_file_reason}\n    \
{kind:\"plan-unbound-text\", session_id, plan_file_reference_lines[]}, one per transcript\n     \
holding re-injected plan text with no slug anywhere\n    \
{kind:\"plan-edit\", owner_session_id, path, mutations, bound_by_owner,\n     \
binder_session_id, binder_line}\n    \
{kind:\"summary\", bindings, plan_files_touched, warnings, unbound_plan_text,\n     \
skipped_lines}\n  \
A `from` of null in `slug_changes` is the MINT. `warnings` counts the unbound-plan-text rows\n  \
alongside the unbound-edit ones."
)]
pub struct PlanArgs {
    /// Project target(s) (an actual cwd or an encoded dir) whose sessions to resolve the bound
    /// plan file for, or an `@<uuid>` session token. `@main` and `@trap:<marker>` resolve the
    /// calling session, and a `*.jsonl` file scopes to that transcript. With no target, the
    /// calling session is resolved from the environment.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target,
        verbatim_doc_comment
    )]
    pub paths: Vec<PathBuf>,

    /// Also scope to the session ids in FILE (`-` reads stdin). The ids are whitespace- or
    /// newline-separated uuid, uuid-prefix or agent-id tokens, bare or `@`-prefixed, exactly as
    /// csift emits them (`search -l`, JSON `transcript_ids` / `parent_session_id`). They are
    /// unioned with the positional targets, and each id resolves fail-loud like an `@`
    /// positional. An EMPTY list (an upstream stage that found nothing) scopes to NOTHING. The
    /// result is an honest empty one with exit 0, never a silent widening to every project. The
    /// resolved ids then follow this command's normal span rules, exactly as `@` positionals
    /// would. On the span-by-default commands each session EXPANDS to its subagent transcripts,
    /// and `--no-subagents` pins the scope to exactly the listed ids.
    #[arg(long = "sessions-from", value_name = "FILE|-", verbatim_doc_comment)]
    pub sessions_from: Option<std::path::PathBuf>,

    /// REVERSE lookup: given a PLAN FILE, find the sessions BOUND to it, the inverse of the
    /// default session-to-plan direction. It scans the resolved scope (every project by
    /// default, or narrow it with a PATH target) and resolves each transcript's binding by the
    /// same two laws the forward direction uses. The first is its latest
    /// `plan_mode` attachment's path. The second, when it carries none, is
    /// `<plans-dir>/<slug>.md` from its first valid slug, and `<slug>-agent-<agent id>.md` for
    /// a subagent transcript, so the session's plan file does not list its subagents. It prints
    /// each session or subagent id whose binding names this exact plan file, and JSON carries
    /// each one's `binding_source` (`plan_mode` | `slug-only`). A forked or background session
    /// binds by its slug alone, so it is found here too. Use it when you hold a plan file (from
    /// `~/.claude/plans/`, for example) and need the conversation that owns it. The path is
    /// matched by absolute identity, and relative or `~` inputs are absolutized first.
    #[arg(long, value_name = "PLAN_FILE", verbatim_doc_comment)]
    pub reverse: Option<PathBuf>,

    /// AUDIT the target scope's PLAN-FILE edits against plan BINDINGS. It finds every
    /// structured mutation (Write/Edit/MultiEdit/NotebookEdit) the scope made to a file that
    /// SOME session binds as its plan, and WARNS when the mutating session does not bind that
    /// file itself. This matters because after a compaction only the session's own BOUND plan
    /// is re-injected in full, and content parked in another session's plan file does not come
    /// back. Plan files are identified by JOINING against the corpus's plan_mode bindings (one
    /// prefiltered scan of every project). A plans directory is never guessed, since
    /// plansDirectory is configurable. Bash-side edits are outside this audit, which reads
    /// structured tools only.
    ///
    /// Each binding in scope is also QUALIFIED by four facts its own records carry. (a) The
    /// slug's CHANGE POINTS. `slug  L<n>  none -> <value>` is the mint, and `<a> -> <b>` reads
    /// as the binding having MOVED. The binding csift reports is the harness's, and this says
    /// when it became so. Every measured transcript has exactly one change point, so two is
    /// news. (b) Whether the bound plan file is on disk, the same `[exists]`/`[missing]`
    /// verdict the forward view prints. `[missing]` is an ordinary state, because the name is
    /// minted at Plan Mode entry and the file lands only when content is first written. (c) A
    /// `plan_file_reference` attachment (the post-compaction re-injection of the bound plan's
    /// whole content) present while NO record carries a slug. That is plan text with nothing
    /// bound to it, which nothing will re-inject. It is a warning with no corpus specimen, kept
    /// because the fork path that strips a slug can produce it. (d) The first slug-carrying
    /// record against the plan file's BIRTH instant. `before` is the ordinary order, and
    /// `after` means this transcript bound a file that already existed. `unknown` names its
    /// reason (no file, no timestamp, or a platform that records no birth time) and guesses no
    /// direction.
    #[arg(long, conflicts_with = "reverse", verbatim_doc_comment)]
    pub audit: bool,

    /// Exclude subagent transcripts. Forward, only the top-level session's bound plan resolves.
    /// Reverse, only top-level bindings are listed.
    #[arg(long = "no-subagents", verbatim_doc_comment)]
    pub no_subagents: bool,

    /// Span subagent transcripts. This is the DEFAULT here. The explicit flag exists so every
    /// span command answers the same two switches (`--subagents` / `--no-subagents`).
    #[arg(
        long = "subagents",
        conflicts_with = "no_subagents",
        verbatim_doc_comment
    )]
    pub subagents: bool,

    /// Emit NDJSON (a header, one object per resolved plan, a summary) instead of the headered
    /// text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, verbatim_doc_comment)]
    pub format: OutputFormat,
}

impl PlanArgs {
    /// Subagent span is ON by default; `--no-subagents` restricts to the top-level session.
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.subagents || !self.no_subagents
    }
}
