//! ShowArgs + StatsArgs.
//!
//! The after-help bodies are written at column 0 inside multi-line literals, with every
//! line break typed out: clap renders help text without wrapping it, so the page keeps the
//! line lengths written here. The flag docs carry `verbatim_doc_comment` for the same
//! reason, and clap indents each of their lines under the flag.

use super::*;

/// `csift show`: fetch specific record(s) of ONE transcript, rendered full (or raw).
/// The `show --help` after-help body up to the record-row KEY LIST, which
/// [`show_keys::record_row_keys`] renders from the table rather than from a sentence.
/// It ends with the two-space indent plus `Record rows carry ` that
/// [`show_keys::KEY_BLOCK_FIRST_COLUMN`] counts.
const SHOW_AFTER_HELP_HEAD: &str = "\
EXAMPLES
  csift show @<uuid> --line 46550             # the record at line 46550, in full
  csift show @<uuid> --line 87,495..500,992   # several lines and ranges
  csift show @<agent-id> --line 88            # a SUBAGENT transcript (id from `csift agents`)
  csift show @<uuid> --uuid <record-uuid>     # one record, by its uuid
  csift show @<uuid> --line 46550 --raw       # the verbatim jsonl line
  csift show @<uuid> --branch-points          # where it forked (rewind / retry / parallel)

TARGET (exactly ONE transcript)
  `@<uuid>` or `@<uuid-prefix>` names a top-level transcript, and `show` never adds that
  session's subagents to it. `@<agent-id>` (an id `csift agents` prints) names one subagent
  transcript. A `*.jsonl` path names that file. A target that resolves to more or fewer than
  one transcript is a hard error, because a line number addresses one file.

  One session id can sit under two project directories. The spanning commands merge such
  copies. `show` counts the unmerged set instead and refuses, naming every copy's path. No
  `@<uuid>` and no `@<agent-id>` separates the copies, because a copied project directory
  carries the session's whole sidecar directory (the `<session-id>/` folder beside its
  transcript, subagent transcripts included), so a subagent then has two copies too. The
  way to one copy is its `*.jsonl` path.

ADDRESSING AND EXIT
  `--line` and `--turn` take one shared range grammar. Lines count from 1 and turns from 0,
  and every range is inclusive.

    N      one line or turn
    A..B   from A through B
    N..    from N to the end
    ..N    from the start through N
    -k     counted from the end (`--line -20..` is the last 20 lines, `--turn -3..` the
           last 3 turns)

  `--line` repeats and takes a comma-joined list. `--turn` takes one spec per run.

  A line or uuid named outright that resolves to no record is a HARD ERROR (exit non-zero).
  When the address lies past the file's end, the error names the file's line count. A range
  CLAMPS to the file and errors only when it yields nothing. The lines a range names past
  the end are counted in JSON `lines_past_end` and named in a footer line that gives the
  file's last line (a stderr note under `--raw`). The lines it covers that hold no record
  are counted separately, in `non_record_lines`.

  A pending-elicitation record merged from the elicitation file has no physical line. A
  `--uuid` reaches it, and it renders `(elicitation sidecar)` in place of `Lnnnn`.

RECORDS OUTSIDE THE SURVIVING CONVERSATION
  `--turn` uses the numbering `search` prints, and that numbering follows Claude Code's own
  conversation chain (the global `--cross-compactions` entry says what the chain is). A
  record the chain no longer reaches belongs to no numbered turn. Such
  records are a recalled draft, a turn the operator REWOUND past, and anything under one.

  An ADDRESS always fetches what it names, so it still renders such a record. The record
  comes back as its own annotated unit, headed `draft (superseded …)` or `rewound turn …` in
  place of a fabricated t<N>. The unit states its distance from the message that replaced
  it, in this line:

    differs from the sent message in N chars (P% of the final)

  N sums insertions and deletions in a shortest CHARACTER edit script, never a length difference.
  P can therefore exceed 100 when the draft was the longer text. A rewound turn leads the
  line with `rewound: the conversation continued from L<n> instead`.

  JSON record rows add `superseding_line`, `superseding_uuid` and the three numbers
  `diff_chars`, `diff_pct` and `diff_exact`. All five are null on any other row.
  `diff_exact` reads false when a bound stopped the walk, and both numbers then mean \"more
  than\". Every row also carries `survival` (live | pre-cut | abandoned),
  `abandoned_root_line` and `replay_copy_of`.

A SHAPE CSIFT MODELS NO LEAF FOR
  csift labels each record with leaves of its role.category.leaf taxonomy, the paths
  `search -t` selects. An address never hard-misses a PARSEABLE line, even when the line's
  shape carries no leaf. Those shapes are an isMeta pseudo-turn matching no template, a
  text-less block record, and an attachment payload type or system subtype that no csift
  label covers.
  A NON-isMeta image-only user record is one of them and reads `? unmodeled: user/image`.
  The isMeta one has a leaf, `harness.context.image`.

  Such a record comes back as ONE unit reading `? unmodeled: <type[/subtype|/attachment.type]>`.
  Its body is a key=value excerpt of the line's own top-level keys, and its JSON row carries
  `label`:null, `labels`:[] and `unmodeled`. A line that is no JSON object at all is still a
  hard miss, and the error points at `--raw`.

  That excerpt is the one rendered path that walks a line's values without knowing what they
  are. So a `headers` member is ELIDED at every depth, by exact name, and prints as
  `<elided>`. A `type:\"system\"` `api_error` line carries the failed request's whole HTTP
  response header set inside its `error` object, and `--raw` is the one path that prints it.

RAW MODE
  `--raw` prints the exact bytes of each addressed jsonl line, a malformed or torn line
  included. It cannot be combined with `--format json`, since raw output IS the file's own
  JSON. It reads the transcript file only, with no sidecar merge.

  A `--uuid` under `--raw` names the same records the rendered fetch shows. When two lines
  carry one uuid, Claude Code reads the later one as the record, and the earlier one is a
  replay copy. A replay copy and that survivor share the uuid, so both lines print, lowest
  first. A record merged from the elicitation file has no line, so it prints nothing and a
  stderr note says so (exit 0).

JSON SCHEMA (--format json)
  The envelope is {kind:\"header\", command:\"show\", session_id, is_subagent,
  parent_session_id, path}, then one {kind:\"record\", …} row per fetched record, then
  {kind:\"summary\", …}. Under `--branch-points` the header is {kind:\"header\",
  command:\"show\", mode:\"branch-points\", session_id, is_subagent, parent_session_id},
  with `mode` and no `path`.

  Record rows carry ";

/// The `show --help` after-help body after the record-row key list.
const SHOW_AFTER_HELP_TAIL: &str = ".

  `text` is one line, with every whitespace run collapsed to a single space. A `--line` or
  `--uuid` address lifts the excerpt cap, so `text` is FULL there. `--turn` is a window, and
  under it `text` is CLIPPED to ~400 chars with the explicit `… (+N chars)` marker.

  `body` carries the same rendered text with its NEWLINES intact, so a markdown table or a
  paragraph break survives. It is non-null under exactly the addresses that lift the cap
  (`--line` / `--uuid`) and null under `--turn`. It is the RENDERED form, so a record whose
  text the opener renderer already flattened (a user message) carries no newline there
  either. `--raw` stays the path to the bytes.

  `mode` and `compact_metadata` belong to the compaction pair and are null on every other
  record. `mode` is `compact` | `summarize-from-here` | `summarize-up-to-here`, because a
  `/rewind` summarize IS a compaction. A boundary learns its mode from the summary that
  follows it in the FILE. That pairing is read before any address narrows the output, so a
  boundary fetched alone still reports its mode. It stays null only where the file leaves
  the boundary unpaired, with no summary after it or a second boundary first.
  `compact_metadata` is the boundary's own object verbatim, including the
  `preservedMessages` uuid lists that the one-line excerpt only counts.

  The summary is {records, dropped_by_cap, refetch_remainder, non_record_lines,
  lines_past_end, skipped_lines, with_elicitation_sidecar}.

    refetch_remainder   the ready-to-run continuation command when the cap dropped units,
                        else null. It names the dropped units by line, or by uuid for a
                        unit with no line, so it never repeats a unit already shown.
    non_record_lines    lines that exist and render no record
    lines_past_end      lines a range named past the file's last line

  Whenever `--cross-compactions` is non-zero, the summary also carries the crossing set
  {crossing_budget, boundaries_crossed, budget_exhausted, boundaries_above_floor,
  floor_line, floor_cause}. The rendered reading of an addressed record moves with that
  budget. The same record can read `live` under one budget and `pre-cut` under another, so
  the keys say which reading you hold. Under `--cross-compactions 0` they are absent.
  `--raw` prints bytes, which no budget changes, so it prints the same disclosure on stderr
  only for a `--turn` window, whose turn numbering the budget does move.

  Under `--branch-points` the rows are {kind:\"branch-point\", uuid, line, parent_line,
  parent_type, live_child_line, children:[…], widest_gap_seconds}, and the summary is
  {branch_points, conversation_records, skipped_lines} plus the same crossing set, on the
  same terms.

  On a branch-point row `line` and `parent_line` are the SAME value, because the record IS
  the fork parent. Both are null when the parent uuid names no line in the file.
  `live_child_line` is the line of the child on the chain, and it is null unless EXACTLY one
  child is live. Null with zero live children means the whole fork sits off the surviving
  conversation. Null with two or more means the text output names every live child on the
  fork's line (`live children: L<a> L<b>`). Two children can both be live because the chain
  keeps assistant siblings that share one message.id.

  Each child is {line, uuid, record_type, survival (live | pre-cut | abandoned),
  verdict (live | rewound | draft | abandoned | pre-cut), ts_utc, ts_local}.";

/// The assembled `show --help` body. A `LazyLock<String>` because the key list is
/// BUILT from [`show_keys::RECORD_KEYS`]: a family that adds a key to a record row adds
/// a table row, and the help follows it with no prose to keep in step by hand.
pub(crate) static SHOW_AFTER_HELP: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    format!(
        "{SHOW_AFTER_HELP_HEAD}{}{SHOW_AFTER_HELP_TAIL}",
        crate::cli::show_keys::record_row_keys()
    )
});

#[derive(Args, Debug)]
#[command(
    about = "Fetch a transcript's records by line, turn or uuid (full or --raw), or \
             --branch-points",
    long_about = "\
Fetch the record(s) at specific 1-based jsonl line numbers, 0-based turn indices or record
uuids, from exactly ONE transcript. The line numbers are the `Lnnnn` every csift surface
prints, and the turn indices the `tN` that `search` prints.

By default each record renders FULL, with its label, its timestamp and its complete text.
That makes `show` the permission-friendly alternative to `Read`-ing the raw jsonl. `--raw`
emits the VERBATIM jsonl lines instead, which is the way to the fields csift does not render
(usage tokens, stop_reason, model, …).

`--branch-points` reports the transcript's conversation FORK facts in place of records. A
fork is a record with more than one conversation child. A rewind, a retry and a parallel
lane each make one, because each writes a later record whose `parentUuid` names an earlier
one that already has a child. Forks rank by the widest time gap between their children. A
rewind usually shows a wide gap, and a parallel lane a gap near zero. Each fork names the
LIVE child and gives every other child the conversation chain's verdict (the global
`--cross-compactions` entry says what the chain is).",
    after_help = SHOW_AFTER_HELP.as_str()

)]
pub struct ShowArgs {
    /// ONE transcript: `@<uuid>`, `@<uuid-prefix>`, `@<agent-id>` or a `*.jsonl` path.
    // Declared as a Vec DELIBERATELY (the run handler enforces exactly-one): a single
    // non-Vec positional with `allow_hyphen_values` makes clap consume a mistyped
    // long flag AS the target and report the user's VALID target as the surplus
    // "unexpected argument" - the wrong-hypothesis rabbit hole. The Vec absorbs the
    // bad token instead, so `parse_project_target` rejects it BY NAME with the same
    // "did you mistype a flag?" error every sibling command gives.
    #[arg(value_name = "TARGET", value_parser = parse_project_target, allow_hyphen_values = true, required = true, num_args = 1..)]
    pub target: Vec<std::path::PathBuf>,

    /// 1-based jsonl line(s) in the shared range grammar (`N`, `A..B`, `N..`, `..N`, or `-k`
    /// counted from the end, so `-20..` is the last 20 lines). Repeatable or comma-joined,
    /// as in `--line 87,495..500,992`.
    #[arg(
        long,
        value_name = "SPEC",
        value_delimiter = ',',
        allow_hyphen_values = true,
        verbatim_doc_comment
    )]
    pub line: Vec<String>,

    /// Record uuid(s), repeatable or comma-joined.
    #[arg(long, value_name = "UUID", value_delimiter = ',')]
    pub uuid: Vec<String>,

    /// 0-based TURN index or range, the `tN` that `search` prints, in the grammar `--line`
    /// takes (`N`, `A..B`, `N..`, `..N`, `-k` from the end). `-3..` is the last 3 turns,
    /// and `42..` runs from turn 42 to the end. The numbering matches the `·t<N>` in
    /// `search`'s exchange headers exactly.
    ///
    /// The fetch returns every user and assistant record of the named turns, so it reads a
    /// turn's whole back-and-forth. The lines a turn carries that only an explicit label
    /// reaches belong to no turn fetch. They are the `harness.turn.*` lines (`duration`,
    /// `stop-hooks`, `recap`), the `harness.system.*` records, and the
    /// `harness.bookkeeping.*` lines, the `queue` and `file-history` ones among them. An
    /// address by `--line` or `--uuid` reaches them, and so does naming their leaf:
    /// `search -t 'harness.bookkeeping.*' -t 'harness.turn.*' -t 'harness.system.*'`.
    ///
    /// Mutually exclusive with `--line` and `--uuid`, so a run picks ONE addressing mode.
    #[arg(
        long,
        value_name = "N|A..B|N..|-k",
        allow_hyphen_values = true,
        conflicts_with_all = ["line", "uuid"],
        verbatim_doc_comment
    )]
    pub turn: Option<String>,

    /// Cap the emitted record units (default 200). The cap guards a reader's context
    /// against an open range, since `--line ..` or `--turn ..` addresses a whole transcript.
    /// `0` means uncapped, as on every csift command that takes `--max-count`.
    ///
    /// The cap keeps whole records (at least one) on the LOWEST lines, whatever order they
    /// render in. A `--turn` window keeps its earliest turns, and the lowest lines within a
    /// turn.
    ///
    /// The drop is ALWAYS reported, with a continuation command that names the dropped
    /// units by their lines. The lines are sorted, and ranges that overlap or touch are
    /// merged (`--line 3..4,9..14`). A dropped record merged from the elicitation file has no
    /// line and is named by `--uuid`. That file is `elicitations.jsonl` in the session's
    /// sidecar directory (the `<session-id>/` folder beside its transcript), where a hook
    /// records a pending prompt the transcript does not yet show. Every address form
    /// continues this way, `--turn` included, so the footer always says which records this
    /// page left out. Following the continuations at one cap returns every unit of the
    /// uncapped fetch exactly once.
    #[arg(long, value_name = "N", verbatim_doc_comment)]
    pub max_count: Option<usize>,

    /// Emit the VERBATIM raw jsonl line(s) instead of the rendered record view.
    /// Mutually exclusive with `--format json`.
    #[arg(long, verbatim_doc_comment)]
    pub raw: bool,

    /// Report the transcript's conversation FORK facts in place of records. A fork is a
    /// record with MORE THAN ONE conversation child. A rewind, a retry and a parallel lane
    /// each make one, because each writes a later record whose `parentUuid` names an earlier
    /// one that already has a child. Each child is listed with its line, timestamp and
    /// verdict, and forks rank by the WIDEST time gap between children (a rewind usually
    /// shows hours, a parallel lane near zero).
    ///
    /// Tool-result carriers, isMeta records and compaction summaries never count as
    /// children, so a parallel tool fan-out is no fork. The PARENT can be any user,
    /// assistant, attachment or system record carrying a uuid. A fork parented to a hook's
    /// attachment line prints that line and its type, and `parent uuid not in this file` is
    /// kept for a uuid no line here carries.
    ///
    /// Each fork names its LIVE child from Claude Code's own conversation chain, and every
    /// child gets one of five verdicts:
    ///
    ///   live        on the chain
    ///   rewound     an opener that was answered, then rewound past
    ///   draft       an opener nothing ever answered
    ///   abandoned   off the chain without being an opener (a retried assistant record)
    ///   pre-cut     above a compaction boundary, where the chain stops and csift keeps
    ///               reading
    #[arg(
        long = "branch-points",
        conflicts_with_all = ["line", "uuid", "turn", "raw"],
        verbatim_doc_comment
    )]
    pub branch_points: bool,

    /// Emit JSON instead of the rendered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// HIDDEN no-op subagent-span flag. `show` has NO span control: it fetches from exactly
    /// ONE transcript, never a set, but ten sibling commands accept the span pair, so a
    /// muscle-memory `--no-subagents` here is a REAL observed slip. Accepted only
    /// to emit a pointed error naming the actual rule, instead of the misleading generic
    /// "did you mistype a flag?" the TARGET value-parser would give. See
    /// [`ShowArgs::span_flag_error`].
    #[arg(long = "no-subagents", hide = true)]
    pub no_subagents: bool,

    /// Hidden no-op twin (see `no_subagents`): both span switches are accepted-then-rejected.
    #[arg(long = "subagents", hide = true)]
    pub subagents: bool,
}

impl ShowArgs {
    /// The pointed rejection for the hidden span pair (the `agents` precedent): name the
    /// actual rule and the correct next move, not a typo hypothesis.
    #[must_use]
    pub fn span_flag_error(&self) -> Option<&'static str> {
        if self.no_subagents || self.subagents {
            Some(
                "`show` has no subagent-span flag: it fetches record(s) from exactly ONE \
                 transcript and never spans a session's subagents (line numbers are \
                 per-FILE). Drop the span flag. To read a subagent transcript, target it \
                 directly: `csift show @<agent-id> --line N` (the id `agents`/`search` \
                 print); to search across a session AND its subagents use `csift search`.",
            )
        } else {
            None
        }
    }
}

/// `csift stats`: one-scan aggregates per session (and a scope total).
#[derive(Args, Debug)]
#[command(
    about = "Count a session's records, turns, tools by name, tokens by model, span and \
             compactions",
    long_about = "\
One scan of each transcript answers the aggregation questions that otherwise need
hand-rolled jsonl parsing. Every session row has the same fixed shape:

  lines         the physical line count
  records       the user and assistant record counts
  tokens        token burn per model, from message.usage
  narration     narration-block counts per model (agent.thinking.narration)
  tools         tool-call counts by name
  turns         the turn count
  span          the first and last timestamps, and the duration between them
  compactions   the compaction count
  malformed     the malformed-line count
  types         a whole-file LINE-TYPE census

Tokens are counted ONCE per API message. Claude Code repeats the identical usage object on
every per-block record of one message, so sums dedupe by message.id per transcript. A
per-record sum over-reads 2-3.5x.

Narration blocks are summaries the API issues. Their token split is not derivable, so only
blocks are counted.

A tool call counts once per CALL, one per invocation. `search --count-by tool` counts
RECORDS, the use plus the record carrying its result, so it reads ≈2× these tallies.

The line-type census counts every physical line by its top-level `type` (`user`,
`assistant`, `attachment`, `file-history-snapshot`, `system`, …). It answers \"what else is
in this jsonl besides the conversation\". Non-record lines are the majority of many
transcripts' bytes, and no other surface parses them.

Subagents are spanned by default. Each transcript is its own row, and the scope TOTAL block
sums them. `--since`/`--until` bound the counted records by timestamp.

`turns` counts the LIVE turns. That is the numbering that follows Claude Code's own
conversation chain, the one `search` prints as `tN` and `show --turn` addresses. A `stats`
turn count therefore equals the number of numbered rows (`t0` .. `tN`) that
`search --count-by turn` prints over the same scope, and a `--turn` window resolves against
that numbering. When the transcript carries records off the chain, the census prints one
row more, `t? (abandoned)`, which counts those records and is no turn.

What LEFT the numbering is disclosed beside it, on a line printed only when there is
something to say:

  chain  abandoned turns N (M rewound) · replay copy lines K

The record that opens a turn is its opener, and it is a message the person typed, an
answered AskUserQuestion, a tool rejection carrying typed text, an inbound peer message or
a `<task-notification>` pulse. An ABANDONED turn is one
whose opener the conversation chain no longer reaches (the global `--cross-compactions`
entry says what the chain is), such as a recalled draft or a turn the operator rewound
past. The REWOUND ones are the subset that drew a reply before the rewind. A REPLAY COPY
is a line whose uuid a later line also carries. A compaction writes such later lines when
it appends a block of records already written, with their uuids kept.

Under a `--turn` or time window every figure windows EXCEPT `lines`, the `types` census and
the three chain totals. They stay file facts (the physical line count, the per-type line
counts and the whole-chain counts). The chain totals cannot window, because an abandoned
opener carries no turn index to window on.",
    after_help = "\
EXAMPLES
  csift stats @<uuid>                                        # one session + its subagents
  csift stats @<uuid> --no-subagents                         # just the top-level thread
  csift stats . --since 1d                                   # this project, last 24h
  csift stats @<uuid> --format json | tail -1 | jq .tokens   # scope token totals

JSON SCHEMA (--format json)
  The envelope is a header, then one {kind:\"session\", …} row per session, then the
  summary.

  Session rows carry {session_id, is_subagent, parent_session_id, lines,
  line_types:{<type>:count}, user_records, assistant_records, turns, abandoned_turns,
  rewound_turns, replay_copies, compactions, first_utc, first_local, last_utc, last_local,
  tokens:{<model>:{input, output, cache_read, cache_creation}},
  narration_blocks:{<model>:count}, unknown_thinking_tags, tools:{<name>:count},
  skipped_lines}.

  The summary adds the scope totals {sessions, line_types, tokens, narration_blocks,
  unknown_thinking_tags, tools, turns, abandoned_turns, rewound_turns, replay_copies,
  dropped_by_cap, skipped_lines}. `tail -1 | jq .tokens` is the one-liner for total burn.

  `unknown_thinking_tags` counts thinking blocks whose signature carries a tag that is
  neither thinking nor narration, a tag value the API added that csift does not yet name.
  The text output prints the count only when it is above 0.

  Each session row and the summary ALSO carry the crossing set {crossing_budget,
  boundaries_crossed, budget_exhausted, boundaries_above_floor, floor_line, floor_cause}
  whenever `--cross-compactions` is non-zero. The set says how many compaction boundaries
  the conversation chain crossed, how many it did not, and where the walk stopped (the
  floor). Under `--cross-compactions 0` csift crossed nothing, and the keys are absent.

  `skipped_lines` here is a FULL-SCAN census, because `stats` parses every line. It is the
  corruption-census authority for \"does this transcript carry a torn or corrupt line
  anywhere\". The field of the same name on `list` covers only the head and tail lines
  `list` reads."
)]
pub struct StatsArgs {
    /// Project path(s), `@<uuid>`, `@<agent-id>` or `*.jsonl`, in the targeting grammar every
    /// subcommand shares. With no PATH, every project.
    #[arg(
        value_name = "PATH",
        value_parser = parse_project_target,
        allow_hyphen_values = true,
        verbatim_doc_comment
    )]
    pub paths: Vec<std::path::PathBuf>,

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

    /// Lower time bound. WHEN is ISO8601, or a relative N units AGO in seven units, `s`
    /// seconds, `m` minutes, `h` hours, `d` days, `w` weeks, `mo` months (30 days) and `y`
    /// years (365 days), as in `2h`, `3d` or `2mo`. Records outside it are not counted.
    #[arg(long, value_name = "WHEN", verbatim_doc_comment)]
    pub since: Option<String>,

    /// Upper time bound (the same WHEN grammar as --since).
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Count ONLY records in this inclusive turn-index window, in the shared range grammar
    /// (`N`, `A..B`, `N..`, `..N`, `-k` from the end). Turns are 0-based in the
    /// transcript's turn order, and turn 0 holds the first opener's turn. It is the axis
    /// `search` and `files` use, and their output shows the indices.
    ///
    /// The window intersects (AND) with `--since`/`--until`. The token burn of the last N
    /// turns is `csift stats @main --turn -N..`, with no need to look up the current turn
    /// index.
    #[arg(
        long = "turn",
        value_name = "N|A..B|N..|-k",
        allow_hyphen_values = true,
        verbatim_doc_comment
    )]
    pub turn_range: Option<String>,

    /// Cap the emitted per-session rows (default: unlimited) to bound an unscoped run's
    /// output. The kept rows are the most recently active. The drop is reported, never
    /// silent, and the scope TOTAL block then covers the shown subset. A target or
    /// `--since` chooses WHICH sessions instead.
    #[arg(long, value_name = "N", verbatim_doc_comment)]
    pub max_count: Option<usize>,

    /// Exclude subagent transcripts and count only the top-level session(s).
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Span subagent transcripts, the DEFAULT here. The explicit flag exists so every span
    /// command answers the same two switches (`--subagents` / `--no-subagents`).
    #[arg(
        long = "subagents",
        conflicts_with = "no_subagents",
        verbatim_doc_comment
    )]
    pub subagents: bool,

    /// Emit JSON instead of the text blocks.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl StatsArgs {
    /// Whether subagent transcripts are spanned (the default).
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.subagents || !self.no_subagents
    }
}
