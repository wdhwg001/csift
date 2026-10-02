//! ListArgs.
//!
//! The long description and the after-help body are written at column 0 inside multi-line
//! literals, with every line break typed out: clap renders help text without wrapping it,
//! so the page keeps the line lengths written here. The flag docs carry
//! `verbatim_doc_comment` for the same reason, and clap indents each of their lines under
//! the flag.

use super::*;

#[derive(Debug, Args)]
#[command(
    about = "List sessions with times, first and last genuine user message, last agent message",
    long_about = "\
List sessions, each with a quick identity read WITHOUT parsing the whole file. For every
session jsonl under the target(s) a row gives the session id, the FIRST genuine-user message,
the LAST genuine-user message and the LAST agent message, each with its time. It adds the
decoded cwd, the git branch and the Claude Code version.

`git_branch` and `version` are LAST-seen, so they say what the session is on NOW. A session
that upgraded or switched branches mid-flight reports the current value, with the opening
value in `*_first` and a `first->last` drift arrow in text. `cwd` stays FIRST-seen on
purpose. The record cwd follows the tracked shell cwd, so a last-seen value could be a
transient subdirectory, and the session's home is the opening value.

A forward HEAD read finds the first user message, and a backward TAIL read finds the last
user and agent messages. Neither parses the full file, so `list` stays fast on 200 MB+
transcripts. Files are scanned in parallel across the corpus, then sorted for deterministic
output.

A PATH is either a real cwd, which csift path-encodes, or an already-encoded `-Users-...`
token. With no PATH, every project is listed.

Genuine-user excludes tool_result carriers, isMeta pseudo-turns (\"Continue from where you
left off.\") and compaction summaries.",
    after_help = "\
EXAMPLES
  csift list                                                 # every session, all projects
  csift list .                                               # this cwd's project
  csift list /Users/testuser/Projects/widget_app_prototype   # a real path (gets encoded)
  csift list -Users-testuser-Projects-widget-app-prototype   # a pre-encoded dir token
  csift list ~/.claude/projects/-Users-testuser-Projects-widget-app-prototype
  csift list @<uuid> --no-subagents                          # JUST the one top-level row
  csift list --format json .                                 # machine-readable index

SCOPE
  `list` SPANS subagents by default, so `csift list @<uuid>` can return 1 top-level row and
  N subagent rows. The text output then leads with a banner:

    scope  N sessions in scope (1 top-level + M subagent)

  Each subagent row is branded `SUBAGENT  <hex>  ·  parent SESSION <uuid>`. A bare hex is
  NOT a re-feedable target, so the parent uuid is the one to re-feed. A top-level row keeps
  the plain `SESSION  <uuid>` header.

JSON SCHEMA (--format json)
  The standard envelope v2, the same as every command's. A header line comes first:

    {kind:\"header\", command:\"list\", sessions_in_scope, top_level_sessions,
     subagent_sessions}

  ONE {kind:\"session\", …} row per session follows:

    {session_id, is_subagent, parent_session_id, path, cwd, git_branch, git_branch_first,
     git_branch_last, version, version_first, version_last, first_user, last_user,
     last_agent, skipped_lines, sidecar_present, pending_elicitations,
     with_elicitation_sidecar, is_clone, clone_of, clone_boundary_uuid, minted_by,
     cleared_from, cleared_from_distance_ms, cleared_from_candidates, continued_in,
     session_kind, session_kind_first_line, session_kind_last_line, lineage_scanned,
     minted_elsewhere, minted_elsewhere_count, also_at, also_at_count, duplicate_relation}

  A closing {kind:\"summary\", sessions, skipped_lines, dropped_by_cap} ends the stream.

  `is_subagent` flags a bare-hex subagent row. `parent_session_id` is the re-feedable owning
  uuid (equal to session_id for a top-level row), and a subagent `session_id` is never one
  to re-feed. `first_user`, `last_user` and `last_agent` are {excerpt, ts_utc, ts_local}
  sub-objects, or null when absent. `dropped_by_cap` is above 0 when the unscoped-flood cap
  trimmed rows, and the most recent 50 are the ones kept.

DUPLICATE COPIES (`also_at` / `duplicate_relation`, text row `copies`)
  One session id can be on disk under TWO project directories. A renamed project whose
  directory was copied leaves that shape, and Claude Code's own project-dir-name override
  and its relocation fallback reach it too. The shared resolver merges the copies. The BYTES
  decide the merge, never an mtime:

    identical   no differing byte, equal sizes. Collapses onto the LONGEST copy.
    prefix      one copy is a strict byte prefix of the other, which is what copying a file
                still being written produces. Collapses onto the LONGEST copy.
    divergent   a byte differs before the shorter copy ends. Merges NOTHING, so both
                copies keep their own row and each names the other.
    unknown     csift could not READ a copy. Merges nothing either, because a read failure
                is a gap in the evidence and says nothing about sameness.

  The copy a merge keeps carries `also_at`, naming the dropped ones. For an unknown copy the
  stderr note names that copy and the error the filesystem returned. `list` reads every
  file in its scope, so it then dies on that copy before printing a row.

  `sessions_in_scope` counts DISTINCT sessions. The header carries `duplicate_transcripts`
  and `duplicate_records_dropped` beside it, plus `divergent_transcripts` (the copies csift
  did NOT merge) and `unreadable_transcripts` (the ones it could not read). A merge that
  dropped nothing reads 0 in the first two, exactly like an ordinary scope. The resolver
  also names every merge on stderr. A dropped copy is read by targeting its `*.jsonl` path.

CLEAR LINEAGE (an inference, and the row says so)
  A `/clear` mints a new session id inside the same process and writes no lineage anywhere
  on disk, so a cleared transcript names no predecessor. csift reads two facts instead.

  A transcript whose FIRST non-isMeta user record is the `/clear` wrapper was minted by that
  clear (`minted_by:\"clear\"`, text row `cleared`). Its predecessor is the sibling in the
  same project directory whose cost-ledger checkpoint closes within 2000 ms of the
  wrapper's timestamp. A checkpoint closes at `startTime + totalDuration` on a `cost-state`
  line. The join goes in `cleared_from`, with the distance in `cleared_from_distance_ms`.

  The nearest line wins. If two different files tie, both are reported in
  `cleared_from_candidates` and NEITHER is joined. The join never uses a file mtime or the
  time adjacency of ordinary records, because two busy sessions share those.

BACKGROUND HANDOFF (`continued_in`, text row `handoff`), a STATED fact
  When a session is handed to a background child (the left-arrow gesture, or the
  background-fork command), Claude Code appends a `type:\"continued-in\"` line to the PARENT
  transcript naming the child's session id. The clone lineage (`is_clone`, `clone_of`) and
  the clear lineage are inferred, and this one is not. The harness writes the child id
  itself, and csift reports it verbatim in `continued_in` (text:
  `handoff  continued in <first8> ...`).

  The line carries four keys and no `uuid`, so it is not a chain node and its only address
  is its jsonl line. An in-place `/fork` writes no such line, so its absence never means the
  session was not forked.

  WINDOW LIMIT. The line is written after the last conversation record, so the backward
  tail read normally reaches it. The tail read STOPS once it has both of its anchors, the
  last genuine-user and the last agent message. A later `--resume` of the parent that
  appends both of those below the line pushes it outside the window, and `continued_in`
  reads null. A transcript missing one anchor is walked to the top, so nothing is lost
  there. Two whole-file answers exist: `--lineage` below, and the line-type census
  `csift stats` prints (`continued-in` is one of its `types` keys).

LANE LINEAGE (`session_kind`, text row `lane`)
  Every record a BACKGROUND lane writes carries a top-level `sessionKind:\"bg\"`. A
  foreground lane's records carry no such key at all, so `session_kind` is an EMPTY array
  for an ordinary session, with no sentinel value. It is an array because the stamp is a
  per-PROCESS fact. One transcript can hold records from a background lane and, below them,
  records that a foreground resume of the same session appended without the key.

  That is also why the SPAN needs `--lineage`. The stamp can stop mid-file, and no head or
  tail window can see where. With the flag, `session_kind_first_line` and
  `session_kind_last_line` give the first and last carrier's jsonl line, and the row reads
  `lane     bg on L<a>..L<b>`. Without the flag both are null and the row says the span
  needs it. `lineage_scanned` tells a consumer which of the two it is looking at.

MINTED ELSEWHERE (`minted_elsewhere`, text row `minted`), a STATED fact
  A record carries TWO session ids under two spellings. The local writer stamps `sessionId`
  on everything it appends and never touches a snake-case `session_id`, which belongs to
  the Remote Control bridge's event envelope. So the two agree on every record this session
  minted, and they differ exactly on a record hydrated here from another session's stream.

  Where they differ, `minted_elsewhere` reports the session the record OBJECT was minted in.
  It is a SET, with `minted_elsewhere_count` beside it, because one transcript can hold runs
  from more than one session and a single value would drop the rest in silence. It is the
  fourth lineage link a row can carry, and the only one a RECORD states about itself, so
  the row carries no inference wording.

  It needs `--lineage`, like the `session_kind` span. A hydrated run sits wherever the
  bridge put it, and on the measured specimens one file's run starts below the head window.
  A window read would be right on one file and silently empty on the next.

SKIPPED_LINES (a window census)
  The count covers the read windows and gives no whole-file verdict. `list` reads only the
  head and tail lines it needs and never scans the middle of a transcript, which is what
  keeps it fast. Its `skipped_lines` therefore counts malformed lines among the LINES READ.
  0 means \"the lines read were clean\", and a mid-file tear is OUTSIDE the windows by design.
  The full-scan corruption census over the same bytes is `csift stats`, and every full-scan
  command (search, files, recover) agrees with it.

  Within the read windows every malformed line, and every sidecar marker the current schema
  cannot read, is booked exactly once. `--lineage` does NOT widen this count. Its pass is a
  key walk with no record parse, so it has no parse failure to report and cannot book a
  line the windows already booked.

  ENCODING is not malformity. A line whose JSON is sound while its bytes are invalid UTF-8
  is decoded LOSSILY and counted as a record, never as a malformed line. The undecodable
  byte renders as U+FFFD and the text stays searchable. That holds on this surface and on
  every other, and `--raw` is the way back to the original bytes."
)]
pub struct ListArgs {
    /// One or more targets, repeatable. With none, every project is listed. A target is a
    /// real filesystem cwd, a `~/.claude/projects/<encoded>` path or bare `<encoded>` dir,
    /// an `@`-prefixed session token, or a `*.jsonl` file path, which scopes to that
    /// transcript.
    ///
    /// `@<uuid>` (8-4-4-4-12 hex) scopes to that one top-level session, searched across all
    /// projects when no project path is given, so `csift list @<uuid>` identifies it. This
    /// is the SAME positional surface `files`, `recover` and `verbatim` use. `@main` and
    /// `@trap:<marker>` resolve the calling session from the environment. A bare uuid
    /// WITHOUT `@` is NOT a session here and needs the prefix (`@<uuid>`).
    ///
    /// The default still SPANS that session's subagents, so a fan-out session lists 1 + N
    /// rows, and `--no-subagents` gives just the single top-level row. One subagent is
    /// targeted directly with `@<agent-id>`, a bare hex of 12 or more digits or a teammate
    /// `aName-<hex>` id, exactly as `csift agents` prints them. That lists the subagent's row
    /// and its subtree.
    ///
    /// Every encoded dir starts with `-`, because an absolute cwd's leading `/` encodes to
    /// `-` (`csift list -Users-testuser-Projects-foo`). A real flag such as `--format json`
    /// is still parsed as a flag in any position, never taken as a PATH.
    // `allow_hyphen_values` is REQUIRED for that first point: without it clap rejects the
    // leading `-` token as an unknown flag (the SPEC section 6.1 baseline invocation). The
    // `parse_project_target` value parser NARROWS that tolerance, which is what gives the
    // second point.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target,
        verbatim_doc_comment
    )]
    pub paths: Vec<PathBuf>,

    /// Scope ALSO to the session ids in FILE (`-` reads stdin). The ids are uuid,
    /// uuid-prefix or agent-id tokens, bare or `@`-prefixed and separated by whitespace or
    /// newlines. They are exactly the ids csift emits (`search -l`, JSON `transcript_ids` /
    /// `parent_session_id`).
    ///
    /// The list is a UNION with the positional targets, and each id resolves fail-loud like
    /// an `@` positional. An EMPTY list (an upstream stage that found nothing) scopes to
    /// NOTHING. That is an honest empty result with exit 0, never a silent widening to every
    /// project.
    ///
    /// The resolved ids then follow this command's span rules exactly as `@` positionals
    /// do. On the span-by-default commands each session EXPANDS to its subagent
    /// transcripts, and `--no-subagents` pins the scope to exactly the listed ids.
    #[arg(long = "sessions-from", value_name = "FILE|-", verbatim_doc_comment)]
    pub sessions_from: Option<std::path::PathBuf>,

    /// Cap the emitted session rows. The cap is the CONTEXT-SAFETY guard against an
    /// unscoped `csift list` flooding its reader, since a large corpus holds thousands of
    /// sessions. It defaults to 50 when listing ALL projects (no target and no
    /// `--sessions-from`) and is otherwise UNLIMITED. An explicit value overrides either
    /// default.
    ///
    /// The drop is NEVER silent. It is reported with guidance, and the KEPT rows are the
    /// most recently active. To see more, raise the cap, pass a target, or add `--since`.
    #[arg(long, value_name = "N", verbatim_doc_comment)]
    pub max_count: Option<usize>,

    /// Only sessions ACTIVE in this window. A session is kept iff its [first-activity,
    /// last-activity] span INTERSECTS `[--since, --until]`, so a long-running session that
    /// straddles the window still lists. The span uses the timestamps this index already
    /// reads (head and tail, never a full scan).
    ///
    /// WHEN is ISO8601 or relative. A bare date means local midnight, a bare datetime means
    /// LOCAL wall-clock time, and `Z` or `+10:00` is explicit. A relative WHEN such as `45s`,
    /// `90m`, `2h`, `3d` or `1w` means that long ago. A session with no readable timestamp
    /// never matches a bounded window.
    #[arg(long, value_name = "WHEN", verbatim_doc_comment)]
    pub since: Option<String>,

    /// Upper bound (the same WHEN grammar as --since).
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Exclude subagent transcripts and list only the top-level `<uuid>.jsonl` sessions,
    /// the behavior from before subagents were spanned. Subagent transcripts are spanned
    /// by default.
    #[arg(long = "no-subagents", verbatim_doc_comment)]
    pub no_subagents: bool,

    /// Span subagent transcripts, the DEFAULT here. The explicit flag exists so every span
    /// command answers the same two switches (`--subagents` / `--no-subagents`).
    #[arg(
        long = "subagents",
        conflicts_with = "no_subagents",
        verbatim_doc_comment
    )]
    pub subagents: bool,

    /// Answer the session-lineage fields over the WHOLE file in place of the head and tail
    /// windows. `list` is a head/tail reader by contract, so without this flag three
    /// lineage answers are window-bounded:
    ///
    ///   the background-lane stamp's SPAN (`session_kind_first_line` /
    ///   `session_kind_last_line`, null without the flag). A lane handed back to the
    ///   foreground stops carrying the key mid-file, and no window can see where.
    ///
    ///   `continued_in`, whose handoff line a later `--resume` of the parent can push
    ///   above the tail window.
    ///
    ///   `minted_elsewhere`, a run of records hydrated from another session, which can sit
    ///   anywhere between the two windows.
    ///
    /// The pass is ONE depth-1 key walk per file that never parses a payload. It adds
    /// nothing to `skipped_lines`, whose meaning stays "the head/tail lines read", and the
    /// whole-file corruption census is still `csift stats`.
    #[arg(long = "lineage", verbatim_doc_comment)]
    pub lineage: bool,

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
