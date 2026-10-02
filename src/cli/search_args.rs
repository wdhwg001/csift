//! SearchArgs (pattern + filters + terminal modes + span pair).
//!
//! Every documented argument carries `verbatim_doc_comment`: without the `wrap_help`
//! feature clap prints a reflowed paragraph as one physical line, so the line breaks and
//! the aligned tables the page needs are written into the doc comments themselves.

use super::*;

#[derive(Debug, Clone, Args, Default)]
#[command(
    about = "Regex-search sessions and return the complete round-trip exchange around each hit",
    long_about = SEARCH_LONG_ABOUT,
    after_help = SEARCH_AFTER_HELP.as_str()
)]
pub struct SearchArgs {
    /// Regex pattern (ripgrep-like, smart-case by default). It MAY be empty, which makes
    /// the search a pure filter driven by `--label`, the time bounds or `--turn` alone.
    ///
    /// PATTERN is search's FIRST positional. files, verbatim, list, agents and recover
    /// take the PATH target first. A bare uuid here is therefore a LITERAL pattern,
    /// searched verbatim. To SCOPE to a session, pass it as an `@<uuid>` POSITIONAL (a
    /// PATH target) after the pattern, the way every sibling takes a target, as in
    /// `csift search PATTERN @<uuid>`.
    #[arg(value_name = "PATTERN", default_value = "", verbatim_doc_comment)]
    pub pattern: String,

    /// Project target(s), positional and repeatable. This is the SAME scope-target surface
    /// every sibling subcommand uses, so `csift search PATTERN .` is the same gesture as
    /// `csift files .`. With none, every project is scanned. A target is one of these.
    ///
    ///   . or a real cwd           that project
    ///   -Users-…                  an encoded project dir token
    ///   @<uuid>                   one session (8-4-4-4-12 hex), as in
    ///                             `csift search PATTERN @<uuid>`
    ///   @<agent-id>               one subagent and its subtree, as a bare hex of 12 or
    ///                             more or a teammate `aName-<hex>` (the ids `csift agents`
    ///                             prints)
    ///   @main, @trap:<marker>     the calling session
    ///   *.jsonl                   one transcript file
    ///
    /// Every encoded dir starts with `-`, so `allow_hyphen_values` is REQUIRED on this
    /// argument. The `normalize_argv` pre-pass routes declared flags (LONG ones and the
    /// short `-t`/`-i`) away from the positional, so a trailing flag
    /// (`… <path> --format json` or `… <path> -t user`) is never swallowed.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target,
        verbatim_doc_comment
    )]
    pub paths: Vec<PathBuf>,

    /// Also scope to the session ids in FILE (`-` = stdin). The ids are whitespace- or
    /// newline-separated uuid, uuid-prefix or agent-id tokens, bare or `@`-prefixed. They
    /// are exactly the ids csift emits (`search -l`, JSON `transcript_ids` /
    /// `parent_session_id`). The list is a UNION with the positional targets, and each id
    /// resolves fail-loud like an `@` positional. An EMPTY list, from an upstream stage
    /// that found nothing, scopes to NOTHING. The result is an honest empty one with exit
    /// 0, never a silent widening to every project. The resolved ids then follow this
    /// command's normal span rules, exactly as `@` positionals would. On the
    /// span-by-default commands each session EXPANDS to its subagent transcripts, and
    /// `--no-subagents` pins the scope to exactly the listed ids.
    #[arg(long = "sessions-from", value_name = "FILE|-", verbatim_doc_comment)]
    pub sessions_from: Option<std::path::PathBuf>,

    /// Exclude subagent transcripts and search only the top-level `<uuid>.jsonl` sessions.
    /// Subagent transcripts are searched by default, and this is the flag of the span pair
    /// that restricts the scope. A workflow's `journal.jsonl` event log is not a transcript
    /// and is never searched.
    #[arg(long = "no-subagents", verbatim_doc_comment)]
    pub no_subagents: bool,

    /// Span subagent transcripts, which is already the DEFAULT here. The flag exists so
    /// every span command answers the same two switches, `--subagents` and
    /// `--no-subagents`.
    #[arg(
        long = "subagents",
        conflicts_with = "no_subagents",
        verbatim_doc_comment
    )]
    pub subagents: bool,

    /// Filter to records whose label matches a SELECTOR (repeatable). A label is a dotted
    /// `role.category.leaf` path, and a selector matches it by dot-SEGMENT prefix. A
    /// selector takes one of three forms.
    ///
    ///   a bare ROLE (`-t user`) or CATEGORY    the leaves the model RECEIVES
    ///   a GLOB (`-t 'user.*'`)                 every leaf under the prefix, visibility
    ///                                          ignored
    ///   an intermediate prefix or exact leaf   its full set
    ///   (`-t agent.tool`, `-t user.unsent`)
    ///
    /// A bare role or category is narrowed on THREE axes. AXIS 1 is a per-LEAF default.
    /// A bare face reaches every leaf except these model-invisible ones.
    ///
    ///   `user.unsent`, `user.rewound`     scan-layer leaves the surviving conversation
    ///                                     does not reach
    ///   `user.queued` and the whole       lines with no message
    ///   `harness.bookkeeping` category
    ///   `agent.thinking.omitted`,         textless thinking blocks
    ///   `agent.thinking.redacted`
    ///   `harness.compaction.boundary`,    message-less system records
    ///   `harness.schedule.event`,
    ///   `harness.turn.{duration,stop-hooks,recap}`,
    ///   `harness.system.{refusal,api-error,notice,kill}`
    ///   `harness.hook.outcome`
    ///
    /// The other two axes decide per RECORD.
    ///
    /// DELIVERY. Claude Code's converter turns a `hook_success` attachment that carries a
    /// top-level `rendered` into text the model receives. `-t harness` therefore shows
    /// such a record although `harness.hook.outcome` is invisible. Claude Code's request
    /// assembler drops an `isVirtual` record and the `<synthetic>` API-error placeholder,
    /// and its converter drops a never-converted attachment payload. A bare face hides
    /// those records although their leaves are visible. `agent.message.structured` alone
    /// keeps its face, because the model wrote it.
    ///
    /// SURVIVAL. A record Claude Code's own conversation chain does not reach is not in
    /// the conversation either. That covers a recalled draft, a turn the operator rewound
    /// past, and anything under one. A bare role skips such a record. A glob or an exact
    /// leaf reaches it, marked `[abandoned]` or `[rewound]`.
    ///
    /// An invalid selector is a HARD error listing the valid set. With no `-t`, every leaf
    /// a default scan reaches is eligible, and a gated leaf still needs its own `-t` (the
    /// leaf tree's GATE words). There is no back-compat. The retired flat
    /// `thinking`/`tool`/`tool-response` spellings hard-error and name their successors.
    #[arg(
        short = 't',
        long = "label",
        value_name = "SELECTOR",
        value_parser = parse_label_selector,
        verbatim_doc_comment
    )]
    pub labels: Vec<String>,

    /// EXCLUDE labels matching this selector (repeatable). It takes the same grammar and
    /// validation as `-t`, the rg `-t`/`-T` duality, and the same record-level rule for a
    /// bare role. `-T harness` excludes a delivered `local_command` record and keeps an
    /// undelivered one. The effective set is the `-t` selectors (or ALL, with none) MINUS
    /// the `-T` selectors.
    ///
    ///   -T agent.thinking           everything except thinking
    ///   -t agent -T agent.tool      the agent role minus its tool traffic
    ///
    /// A multi-label record renders under its richest SURVIVING label. A combination that
    /// excludes everything it includes is a HARD error, since it could never match. `-T`
    /// filters HITS only, so `--siblings` still renders a turn's other records.
    #[arg(
        short = 'T',
        long = "label-not",
        value_name = "SELECTOR",
        value_parser = parse_label_selector,
        verbatim_doc_comment
    )]
    pub labels_not: Vec<String>,

    /// Case-insensitive match (overrides smart-case).
    #[arg(short = 'i', long, verbatim_doc_comment)]
    pub ignore_case: bool,

    /// Let `.` match newlines, for multiline patterns.
    #[arg(long, verbatim_doc_comment)]
    pub multiline: bool,

    /// Inclusive turn-index range, 0-BASED, in the shared grammar.
    ///
    ///   N       one turn
    ///   A..B    a closed range
    ///   N..     turn N to the end
    ///   ..N     the start to turn N
    ///   -k      the k-th turn from the end (`-3..` = the last 3)
    ///
    /// Turn 0 is the first opener's turn together with every record above it, which is
    /// the session's opening context. `1..N` therefore drops the first human turn as well
    /// as that context. A turn opens on a genuine typed message, an answered
    /// AskUserQuestion, a tool rejection carrying typed text, an inbound peer message, or
    /// a `<task-notification>` pulse. A slash command with arguments carries `user.message`
    /// yet opens no turn. Read turn indices off the `<id>·t<n>` exchange header in
    /// `csift search` text output, or off the `turn_index` field in any `--format json`
    /// record. `--turn` intersects (AND) with `--since` / `--until`.
    #[arg(
        long = "turn",
        value_name = "N|A..B|N..|-k",
        allow_hyphen_values = true,
        verbatim_doc_comment
    )]
    pub turn_range: Option<String>,

    // Both WHEN flags carry `allow_hyphen_values`: the grammar tolerates a leading `-`
    // (`--since -1y`), and without it clap reads that value as an unknown short flag.
    /// Lower time bound, in the WHEN grammar (system-local timezone).
    ///
    ///   `Ns` `Nm` `Nh` `Nd` `Nw` `Nmo` `Ny`   that many seconds, minutes, hours, days,
    ///                                         weeks, months or years AGO (`45s`, `90m`,
    ///                                         `2h`, `3d`, `1w`, `2mo`, `1y`). A month is
    ///                                         30 days and a year 365, and a leading `-`
    ///                                         is tolerated.
    ///   `2026-06-01T05:00:00Z`, `…+10:00`     an ISO8601 instant
    ///   `2026-06-01T05:00:00`                 a BARE datetime, read as that LOCAL
    ///                                         wall-clock time
    ///   `2026-06-01`                          a bare date, read as LOCAL MIDNIGHT that
    ///                                         day
    ///
    /// Intersects (AND) with `--turn`. Both filters apply.
    #[arg(
        long,
        value_name = "WHEN",
        allow_hyphen_values = true,
        verbatim_doc_comment
    )]
    pub since: Option<String>,

    /// Upper time bound, in the same WHEN grammar as `--since`. Intersects (AND) with
    /// `--turn`. Both filters apply.
    #[arg(
        long,
        value_name = "WHEN",
        allow_hyphen_values = true,
        verbatim_doc_comment
    )]
    pub until: Option<String>,

    /// Cap the emitted exchanges. The count is SIGNED.
    ///
    ///   N    keep the EARLIEST N of the chronological stream (`--max-count 1` = the
    ///        first occurrence ever)
    ///   -N   keep the LATEST N (`--max-count -1` = the most recent one)
    ///   0    uncapped (with no flag the default is unlimited as well)
    ///
    /// The kept exchanges always emit oldest-first among themselves. The sign only chooses
    /// which END of the stream survives, mirroring the shared range grammar's `-k` = from
    /// the end. Nothing is truncated silently. The head banner shows the emitted window,
    /// and the footer reports the drop.
    #[arg(
        long,
        value_name = "N",
        allow_negative_numbers = true,
        verbatim_doc_comment
    )]
    pub max_count: Option<i64>,

    /// Print ONLY the number of matching exchanges as one integer, the ripgrep `-c` idiom
    /// for "how many times X?". It honors every filter (`-t`, the time window, the session
    /// or path scope) and reports the TRUE total even when `--max-count` would cap the
    /// listing. With `--format json` it prints a header line and then a summary line whose
    /// `matched` key carries the total.
    ///
    /// You rarely need it. The normal output's footer ALWAYS carries the same match total
    /// beside the distinct-session total, and `--count-only` isolates that ONE integer for
    /// a pipe, hence "only".
    #[arg(long = "count-only", short = 'c', verbatim_doc_comment)]
    pub count_only: bool,

    /// Print ONLY the distinct OWNING sessions that matched (`parent_session_id`), one per
    /// line, the grep/rg `-l` idiom for "WHICH sessions?". The ids are in the scope-token
    /// domain. A subagent hit lists its parent uuid, which makes every line re-feedable.
    /// Per-transcript detail lives in the JSON summary's `transcript_ids` and in each hit's
    /// `refetch`. The list is sorted, deduplicated and UNCAPPED. When `--max-count` dropped
    /// exchanges, a stderr note says the listing may be incomplete. It pipes straight into
    /// `--sessions-from -` to scope the NEXT command to what matched.
    ///
    ///   csift search P -l | csift stats --sessions-from -
    #[arg(
        short = 'l',
        long = "sessions-with-matches",
        conflicts_with_all = ["count_only", "siblings"],
        verbatim_doc_comment
    )]
    pub sessions_with_matches: bool,

    /// Print ONLY a census of the matched records along ONE fixed axis, one `<count> <key>`
    /// line per key. Every axis counts RECORDS, so a record whose several sections match
    /// still counts once. The axes are a CLOSED set, not a query language.
    ///
    ///   label       per `role.category.leaf` leaf. A record counts under EVERY leaf it
    ///               carries, so a leaf's number is exactly how many records `-t <leaf>`
    ///               would surface. This is the exploration on-ramp. Run
    ///               `csift search "" <target> --count-by label` BEFORE you guess a `-t`.
    ///   tool        per tool name. These are RECORDS too, and a completed call is a
    ///               tool_use record PLUS a tool_result carrier record, so expect ≈2× the
    ///               per-CALL tallies `stats` prints. An answered AskUserQuestion re-homes
    ///               its carrier to user.answer, so AUQ stays ≈1×. The 2× gap is a unit
    ///               difference and no discrepancy.
    ///   turn        per turn, in ASCENDING turn order (a histogram)
    ///   session     per transcript
    ///   pairing     paired | pending | orphan, joined by tool_use_id. It covers every
    ///               record riding a tool_use or tool_result block, the SendMessage, spawn
    ///               and subagent-return communication views INCLUDED. "Any pending tools?"
    ///               is just `csift search "" <t> --count-by pairing`, and a frozen
    ///               SendMessage counts as pending under any selector.
    ///   model       per assistant model, the raw `message.model` value. Claude Code's own
    ///               `<synthetic>` placeholder, a stand-in assistant record Claude Code
    ///               fabricates (an API-error notice, for one), is reported verbatim.
    ///   attachment  per attachment payload type, the `type` inside a `type:"attachment"`
    ///               record's payload (`hook_additional_context`, `edited_text_file`,
    ///               `compact_file_reference`, …). This axis IMPLIES the `--attachments`
    ///               gate, and the census needs no separate flag.
    ///   version     per Claude Code version. Every record carries the writing Claude
    ///               Code's top-level `version` stamp, so this shows the version(s) a
    ///               session ran under and where an upgrade landed mid-flight.
    ///   result      per tool-result error state, `ok` | `error`. `pairing` answers "did a
    ///               result come back" and `result` answers "was it good", so "any failed
    ///               reads?" is `search "" T --count-by result`. An errored result also
    ///               renders an inline `[error]` and carries JSON `is_error`.
    ///   for-kind    per PROVENANCE relation, which `for` edges the scope carries (below)
    ///
    /// The for-kind axis is MULTI-KEY, like `label`. A record counts once under every
    /// distinct relation word its edges name, because one unit can carry several at once
    /// (an attachment names its parent AND the call it belongs to). A relation counts
    /// whether or not csift resolved its target. The census answers which relations EXIST,
    /// and a hit's own `for[]` says what each one resolved to. A record carrying no edge is
    /// excluded and counted.
    ///
    /// THE KEY SPACE is the closed vocabulary, twenty-seven words.
    ///
    ///   `armed_by` `attached_to` `banner` `blocked` `branches` `continued_in` `continues`
    ///   `fires` `for_tool_use` `forked_from` `hook_run` `interrupted` `leaf` `pairs`
    ///   `parent` `precedes` `preserves` `redoes` `refused` `repairs` `replays`
    ///   `replies_to` `reports` `result_of` `supersedes` `versions` `wraps`
    ///
    /// Six of them ride JSON alone and never render a label-zone tag, because each
    /// restates something the unit already shows. `parent` restates the record above and
    /// `result_of` what the pairing glyph says. The four survival words `supersedes` /
    /// `continues` / `replays` / `branches` restate the distance line and the
    /// `[abandoned]` / `[rewound]` / `[replay copy of L<n>]` markers. This census reads
    /// them anyway.
    ///
    /// THE WORDS ARE NOT THE WHOLE MAP. A relation whose target sits in ANOTHER transcript
    /// is a FIELD on the surface that owns the join, because an edge's target names a
    /// session without carrying the re-feedable id trio beside it.
    ///
    ///   list      `clone_of`, `cleared_from`, `continued_in`, `minted_elsewhere`
    ///   agents    `spawn_tool_use_id`, `parent_agent_id`
    ///   msg       the verdict
    ///
    /// A relation whose target is not a RECORD at all is a per-hit FACT with its own key.
    ///
    ///   `produced_by`                                   a command
    ///   `checkpoint_closes_at_utc` / `_local`           an instant
    ///   `peer_sender_task_id`                           a lane
    ///   `image_ids`                                     an image handle
    ///   `--resolve-persisted`                           an external file
    ///
    /// Some relations csift can NAME that nothing on disk states. `summarizes` has no key
    /// for the region a compaction summary replaced. The boundary names what it KEPT
    /// (`preserves`), and `csift verbatim` reconstructs the rest. `steers` has no key at
    /// all. `closes` resolves one hop through `hook_run` and infers the rest from position.
    /// `folds` is a csift RENDER decision and no record address. The csift skill's relation
    /// inventory is the same table, with one row each.
    ///
    /// Records outside an axis's domain (no tool name, no pairing, no model) are excluded,
    /// AND the excluded count is reported, never silently. The census honors `-t`/`-T`, the
    /// time bounds, `--turn` and the scope, and an empty pattern gives a whole-scope census.
    /// Under an active `-t`/`-T`, the `label` axis counts each surviving record ONLY under
    /// those of its labels that pass the same filter. A dual-labeled record never leaks its
    /// filtered-out twin into the census, so `-t user -T user.message --count-by label`
    /// shows user.* keys only. Drop `-t`/`-T` to census a record set's FULL label sets.
    /// Under `--format json` the census prints `census` rows (`axis`/`key`/`records`) and a
    /// summary.
    #[arg(
        long = "count-by",
        value_enum,
        value_name = "AXIS",
        conflicts_with_all = ["count_only", "sessions_with_matches", "siblings", "raw"],
        verbatim_doc_comment
    )]
    pub count_by: Option<CountAxis>,

    /// Print everything that is FOR one record, TRANSITIVELY. This is the reverse
    /// provenance query, and <ADDR> takes one of two forms.
    ///
    ///   L<line>   a line, which is per FILE, so the scope must resolve to exactly ONE
    ///             transcript (`@<uuid> --no-subagents`, `@<agent-id> --no-subagents` or
    ///             a `*.jsonl` path). A wider scope is refused, naming its transcript
    ///             count.
    ///   <uuid>    a record UUID, found in whichever scanned transcript carries it, in
    ///             any scope
    ///
    /// The closure is computed PER TRANSCRIPT and never crosses one. The answer is ordinary
    /// hits in file order with the addressed record at the head, and it grows. A record
    /// joins when it is for the address OR for something already in it. A call's result,
    /// the notification a background task it launched sent two turns later, and the
    /// attachments written for any of those therefore all come back from one query.
    ///
    /// THREE structural relations admit a row, and no others.
    ///
    ///   its `parentUuid` names a member     `parent`, and every family word built on the
    ///                                       parent link (`attached_to`, `blocked`,
    ///                                       `redoes`, `fires`, `wraps`)
    ///   it carries a `tool_result`          `result_of`
    ///   answering a member's call
    ///   it names a member's call by id in   `reports`, `for_tool_use`
    ///   its own text or payload
    ///
    /// The rest of the vocabulary is not reverse-walked. `--for` on a record that only a
    /// non-structural relation points AT comes back empty. Those relations are `versions`,
    /// `refused`, `interrupted`, `leaf`, `forked_from`, `continued_in`, `replies_to`,
    /// `armed_by`, `banner`, `hook_run`, the four survival words, `precedes` and
    /// `preserves`. The empty answer is a definitive absence about the three arms. It makes
    /// no claim that nothing points there.
    ///
    /// TWO relations are in neither list and come back anyway, because the walk follows
    /// the LINKS and not the words. `pairs` on a compaction summary and `repairs` on a
    /// paired resume placeholder both name the record ABOVE, which is their `parentUuid`.
    /// The parent arm therefore returns them from an address on that record, although
    /// their hit prints no `parent` edge. It prints none because of the
    /// ONE-RELATION-ONE-EDGE rule. `parent` stands down whenever ANY OTHER edge on the same
    /// unit already names that same TARGET. The test is on the target and never on who
    /// named it, so a family's word and the engine's own both trigger it. No FIELD is
    /// removed by that. The record keeps the parent it had, `for_root` still names its
    /// turn, and `--for` still walks it. The forward direction answers those, through the
    /// `for[]` on the hit or `--count-by for-kind`.
    ///
    /// The address may name a GATED line (a hook attachment, a bookkeeping line). The
    /// closure starts there either way, and the addressed record itself is emitted only
    /// under its own gate. `--for L12` gives you what L12 drew, and
    /// `--for L12 --attachments` gives you L12 as well. `--for` composes with `-t`/`-T`,
    /// the time bounds and the scope. The PATTERN still applies, so use an empty pattern
    /// for the whole set. Under JSON the summary gains `for_of` (`address`, `records`,
    /// `transcripts`).
    #[arg(long = "for", value_name = "ADDR", verbatim_doc_comment)]
    pub for_addr: Option<String>,

    /// Also render the SIBLING records of every matched turn, the rest of the
    /// back-and-forth around the match. A matched USER question then surfaces WITH the
    /// agent's reply, which answers "I said X, what did you say back?". The policy is
    /// FIXED and takes no argument. Message units always render (user.*, agent.message,
    /// agent.communication.*). Chattier machinery is capped per leaf.
    ///
    ///   agent.thinking ≤ 2    agent.thinking.narration ≤ 1    agent.tool.use ≤ 3
    ///   agent.tool.result ≤ 3    harness.* ≤ 2
    ///
    /// The caps apply to the NON-matching context records ONLY. Your actual pattern hits
    /// always render in full, however many share a leaf, so a `--siblings` block can
    /// legitimately show more same-leaf lines than the cap. Anything capped away is counted
    /// and surfaced as an explicit `(+N more · csift show @<id> --line A..B)` pointer. A
    /// record that itself matched is never repeated as a sibling. The flag has no effect
    /// under `--count-only`.
    #[arg(long, verbatim_doc_comment)]
    pub siblings: bool,

    /// Emit each matched record's VERBATIM raw jsonl line in place of the rendered
    /// exchange. This is the same escape hatch as `show --raw`, for the fields csift does
    /// not render (usage, stop_reason, model, any new field), FILTERED by the full search
    /// surface (PATTERN, `-t`/`-T`, time, turn, scope). stdout is a pure jsonl stream to
    /// pipe into `jq`. The scope, drop and malformed notes go to stderr, and so do the
    /// compaction crossing lines (`--cross-compactions`), because the scan's survival walk
    /// decides which records are hits. A raw stream has no envelope, so it carries none of
    /// the crossing JSON keys. The stream holds one line per matched RECORD, and a record
    /// hit under several labels emits once. A sidecar-merged record has no physical line
    /// and is omitted with a stderr note. `--resolve-persisted` still affects MATCHING
    /// only, and the emitted line is always the original.
    #[arg(
        long,
        conflicts_with_all = ["siblings", "count_only", "sessions_with_matches", "no_truncate"],
        verbatim_doc_comment
    )]
    pub raw: bool,

    /// Emit each matched (and `--siblings`) record's FULL text in place of the ~400-char
    /// excerpt, so you can READ a found message end to end (the question at the tail of a
    /// long reply, say) without dropping to the raw jsonl. Newlines are still collapsed to
    /// single spaces in the text output and in the JSON `excerpt`, one line per record.
    /// Under `--format json` this flag ALSO fills a `body` field per hit, carrying the same
    /// rendered text with its newlines intact, so a markdown table or a paragraph break
    /// survives. `body` is null without this flag. The default excerpt is a fragment
    /// anyway, so a wide scan is not doubled by a full copy of every record. The default
    /// excerpt stays centered on the match with an explicit `… (+N chars)` marker, and
    /// `--no-truncate` removes the cap entirely.
    #[arg(long, verbatim_doc_comment)]
    pub no_truncate: bool,

    /// Resolve `<persisted-output>` pointers to their `tool-results/<id>.txt` file.
    #[arg(long, verbatim_doc_comment)]
    pub resolve_persisted: bool,

    /// RETIRED. A TOMBSTONE: hidden from `--help`, accepted by the parser
    /// and then a hard error naming the successor, so a stale invocation fails loudly
    /// instead of silently scanning less than it asked for (the `turns` tombstone
    /// precedent). The leaf it gated has a name: select it with
    /// `-t harness.hook.context`.
    #[arg(long, hide = true)]
    pub additional_context: bool,

    /// Also scan EVERY `type:"attachment"` record. These are the harness sidecar payloads
    /// (hook context, edited-file snapshots, compact file references, file snapshots, …)
    /// that a default scan never parses, since they are the bulk of a transcript's bytes
    /// and echo files wholesale. The flag EQUALS naming every attachment-carried leaf with
    /// `.*`. The narrow ask needs no flag at all. A hook payload is
    /// `-t harness.hook.context`, an injected payload is `-t 'harness.context.*'`, and each
    /// renders its body by the rule ATTACHMENT LEAVES below states, which is also its
    /// matchable text. `--count-by attachment` implies this gate. An explicit
    /// `show --line`/`--uuid` address always renders an addressed attachment record, flag
    /// or not.
    #[arg(long, verbatim_doc_comment)]
    pub attachments: bool,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, verbatim_doc_comment)]
    pub format: OutputFormat,
}

mod gates;

pub(crate) use gates::*;

impl SearchArgs {
    /// Whether subagent transcripts are spanned (the default). `--no-subagents` restricts to
    /// the top-level session(s). Feeds [`crate::path::SubagentScope::from`].
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.subagents || !self.no_subagents
    }

    /// The project targets to scope to: the positional `[PATH]...`. Empty => the shared
    /// resolver scans every project.
    #[must_use]
    pub fn targets(&self) -> Vec<PathBuf> {
        self.paths.clone()
    }

    /// The effective `-t`/`-T` filter (include minus exclude).
    #[must_use]
    pub fn label_filter(&self) -> LabelFilter<'_> {
        LabelFilter::new(&self.labels, &self.labels_not)
    }
}
