//! SearchArgs (pattern + filters + terminal modes + span pair).

use super::*;

#[derive(Debug, Clone, Args, Default)]
#[command(
    about = "Regex-search sessions, returning complete request/response round-trip exchanges",
    long_about = "Regex-search transcripts, returning the COMPLETE round-trip exchange \
        containing each hit; never a bare fragment. A turn is delimited by a genuine \
        user message; the emitted exchange is the whole turn (the opening user record \
        plus every assistant/thinking/tool_use/tool_result record chained under it \
        until the next genuine user). So a matched tool_use comes WITH its \
        tool_result, a matched user turn WITH the agent's response, etc.\n\n\
        The PATTERN is ripgrep-like and defaults to SMART-CASE: case-insensitive \
        unless it spells an uppercase letter - a literal, or a character in a bracket \
        class such as `[A-Z]`, while an escape like `\\S`, `\\W`, `\\D` or `\\B` is no \
        letter; `-i` forces case-insensitive and always wins. `--multiline` lets `.` cross newlines. An EMPTY pattern is a \
        pure filter; it matches every label-eligible record, so combine it with \
        `--label` / `--since` / `--turn` (a bare empty pattern with no other \
        filter warns that it will emit a lot).\n\n\
        LABELS (`-t`, repeatable): a dotted `role.category.leaf` SELECTOR - 3 roles, 17 \
        categories, 73 leaves. A selector matches a record label iff it is a dot-SEGMENT prefix \
        of the label's path, so `-t agent.tool` covers use+result and `-t agent` the whole role. \
        The categories: user | agent.{message,thinking,tool,communication} | \
        harness.{notification,compaction,command,interrupt,schedule,resume,hook,context,enforce,\
        turn,system,bookkeeping}; `csift search --help` lists every leaf under each. A bare ROLE \
        or CATEGORY selects only the leaves the model RECEIVES; a GLOB (`-t 'user.*'`) or an \
        exact LEAF reaches every record. With none given, every leaf a default scan reaches is \
        eligible; a gated leaf still needs its own `-t` (see the leaf tree). `-T`/\
        `--label-not` EXCLUDES with the same grammar (the rg -t/-T duality): the effective set \
        is (-t selectors, or ALL) minus (-T selectors), and ONLY a -T that cancels your -t is a \
        hard error. `harness.bookkeeping` is the one category whose whole face is invisible: \
        the bare form is accepted, matches nothing by the law, and says so with the remedy. \
        Every retired spelling is a hard error naming its successors - `harness.meta` and every \
        `harness.meta.*` path, the flat `thinking`/`tool`/`tool-response`, \
        `harness.schedule.continuation`. The human turn is \
        `user.message`; an AskUserQuestion answer is `user.answer` (the full Q+options+answer \
        unit); a plan-rejection-with-message is `user.rejection` (+ a [plan: …] pointer). An \
        inbound peer/teammate message is `agent.communication.inbox` (NOT `user`); a \
        `<task-notification>` automation pulse is `harness.notification.*` (NOT `user`). A \
        tool named after harness machinery still classifies by ROLE: a `ScheduleWakeup` CALL \
        (arming a timer) is `agent.tool.use` like any other tool: `harness.schedule.wakeup` \
        is only the FIRED tick, the harness-injected marker-carrying wakeup prompt. A \
        custom-prompt tick carries no marker at all: it lands under `harness.schedule.fire`, \
        the prompt a scheduled task fires, recognised by its parent link to the scheduler's \
        own fire record rather than by its text.\n\n\
        AUTOMATION TRIGGERS: a `<task-notification>` (a background-command / workflow / \
        spawned-agent / monitor COMPLETION pulse Claude Code injects as a `type:\"user\"` record) \
        OPENS a turn like a human message but classifies under `harness.notification.<kind>` \
        (kind = background-command | workflow | subagent | monitor | task, read from the \
        summary). It renders as the parsed `[<kind> <task-id>[, <task-id>…] <status>] <summary>` \
        attribution label - every task the pulse closes, never the raw `<task-id>`/`<output-file>` \
        XML, and never the internal scan marker, which becomes a trailing \
        `(orphan reconciliation: <kind>)` instead (JSON `task_ids` / `orphan_kind`). Match it like \
        any other text (e.g. `search 'background-command' -t harness.notification.background-command`).\n\n\
        WINDOWING: `--turn` takes the shared range grammar: `N` (one turn) · `A..B` \
        (closed) · `N..` (turn N → the end) · `..N` (start → N) · `-k` = k-th FROM THE END \
        (`-3..` = the last 3 turns), 0-based on turn-boundary order, and INTERSECTS with \
        `--since`/`--until` (both filters AND). Time bounds accept ISO8601 (`2026-06-01` = local \
        midnight · `2026-06-01T05:00:00` BARE = that LOCAL wall-clock time · \
        `2026-06-01T05:00:00Z`/`…+10:00` = explicit zone) or a relative form (`2h`, `3d`, `90m`, `45s`, `1w`, `2mo`, `1y`; a leading `-` is tolerated) meaning \
        \"that long ago\" in the system local timezone.\n\n\
        ZERO MATCHES IS A DEFINITIVE ANSWER, NOT A FAILURE: a no-match search prints a stderr \
        diagnosis: \"DEFINITIVE absence (exit 0), NOT an error\", the active filters, and (when a \
        `-t`/`-T` filter was on) an active probe that NAMES the label(s) the pattern DOES occur \
        under (e.g. it was excluded by your `-t user.message` but occurs under `agent.tool.use`). \
        Read the diagnosis and adjust the filter; do NOT assume a syntax error. To SEE a scope's \
        record-types before you filter, run `--count-by label` (a per-leaf census; with an empty \
        pattern it censuses the whole scope).\n\n\
        `--max-count` caps emitted exchanges but reports the dropped count (default: \
        unlimited; no cap); there is NO silent truncation anywhere.",
    after_help = SEARCH_AFTER_HELP.as_str()
)]
pub struct SearchArgs {
    /// Regex pattern (ripgrep-like, default smart-case). MAY be empty for a
    /// pure filter (use `--label` / time / turn filters alone).
    ///
    /// search's FIRST positional is PATTERN (unlike files/verbatim/list/agents/recover, whose
    /// first positional is the PATH target), so a bare uuid here is a LITERAL pattern, searched
    /// verbatim. To SCOPE to a session, pass it as an `@<uuid>` POSITIONAL (a PATH target),
    /// exactly like every sibling: `csift search PATTERN @<uuid>`.
    #[arg(value_name = "PATTERN", default_value = "")]
    pub pattern: String,

    /// Project target(s) as a POSITIONAL `[PATH]...`: the SAME scope-target surface every
    /// sibling subcommand uses (`csift search PATTERN .` is the same gesture as
    /// `csift files .`). An actual cwd or an encoded `-Users-…` dir token; repeatable.
    /// With none, every project is scanned. A target may ALSO be an `@<uuid>` (8-4-4-4-12 hex)
    /// session token (so `csift search PATTERN @<uuid>` scopes to that one session), an
    /// `@<agent-id>` (one subagent + its subtree; bare hex ≥12 or teammate `aName-<hex>`, the
    /// ids `csift agents` prints), `@main`/`@trap:<marker>` (the calling session), or a
    /// `*.jsonl` file.
    ///
    /// `allow_hyphen_values` is REQUIRED: every encoded dir starts with `-`; the
    /// `normalize_argv` pre-pass routes declared flags (LONG and the short `-t`/`-i`)
    /// away from the positional, so a trailing flag (`… <path> --format json` or
    /// `… <path> -t user`) is never swallowed.
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

    /// Exclude subagent transcripts: search only the top-level `<uuid>.jsonl` sessions.
    /// Subagent transcripts are searched by default; this is the only span flag. Workflow
    /// `journal.jsonl` event logs are not transcripts and are never searched.
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Span subagent transcripts: the DEFAULT here; the explicit flag exists so every
    /// span command answers the same two switches (`--subagents` / `--no-subagents`).
    #[arg(long = "subagents", conflicts_with = "no_subagents")]
    pub subagents: bool,

    /// Filter to one or more `-t`/`--label` SELECTORS (dotted `role.class.sub`, repeatable). A
    /// selector matches a record label by dot-SEGMENT prefix, in three forms: a bare ROLE
    /// (`-t user`) or CATEGORY selects the leaves the model RECEIVES - on the first of THREE
    /// axes below, every leaf but the model-invisible ones, which are the scan-layer
    /// `user.unsent` and `user.rewound` (the surviving conversation does not reach them),
    /// `user.queued` and the whole `harness.bookkeeping` category (lines with no message),
    /// the textless `agent.thinking.omitted` and `agent.thinking.redacted`, the
    /// message-less system records (`harness.compaction.boundary`,
    /// `harness.schedule.event`, the three `harness.turn` closers and every
    /// `harness.system` kind but `.command`) and `harness.hook.outcome`; a GLOB (`-t
    /// 'user.*'`) selects every leaf under the prefix, visibility ignored; an intermediate
    /// prefix or exact leaf (`-t agent.tool`, `-t user.unsent`) selects its full set. Axis 1
    /// is that leaf default; the other two decide per RECORD. DELIVERY: Claude Code's
    /// converter turns a `hook_success` attachment that carries a top-level `rendered` into
    /// text the model receives, so `-t harness` shows it although `harness.hook.outcome` is
    /// invisible; its request assembler drops an `isVirtual` record and the `<synthetic>`
    /// API-error placeholder, and its converter drops a never-converted attachment payload,
    /// so a bare face hides those although their leaves are visible (`agent.message.structured`
    /// alone keeps its face - the model wrote it). SURVIVAL: a record Claude Code's own
    /// conversation chain does not reach - a recalled draft, a turn the operator rewound
    /// past, anything under one - is not in the conversation either, so a bare role skips it
    /// while a glob or an exact leaf reaches it, marked `[abandoned]`/`[rewound]`. An invalid
    /// selector is a HARD error listing the valid set; with none given, every leaf a default
    /// scan reaches is eligible and a gated leaf still needs its own `-t` (the leaf tree's
    /// GATE words). (No back-compat: the retired flat `thinking`/`tool`/`tool-response`
    /// spellings hard-error and name their successors.)
    #[arg(
        short = 't',
        long = "label",
        value_name = "SELECTOR",
        value_parser = parse_label_selector
    )]
    pub labels: Vec<String>,

    /// EXCLUDE labels matching this selector (same grammar + validation as `-t`; repeatable);
    /// the rg `-t`/`-T` duality, the record-level rule for a bare role included (`-T harness`
    /// excludes a delivered `local_command` record and keeps an undelivered one). Effective
    /// set = (`-t` selectors, or ALL with none) MINUS
    /// `-T` selectors: `-T agent.thinking` = everything except thinking; `-t agent -T
    /// agent.tool` = the agent role minus its tool traffic. A multi-label record renders under
    /// its richest SURVIVING label. A combination that excludes everything it includes is a
    /// HARD error (it could never match). Filters HITS only: `--siblings` still renders a
    /// turn's other records.
    #[arg(
        short = 'T',
        long = "label-not",
        value_name = "SELECTOR",
        value_parser = parse_label_selector
    )]
    pub labels_not: Vec<String>,

    /// Case-insensitive match (overrides smart-case).
    #[arg(short = 'i', long)]
    pub ignore_case: bool,

    /// Allow `.` to match newlines / multiline patterns.
    #[arg(long)]
    pub multiline: bool,

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

    // Both WHEN flags carry `allow_hyphen_values`: the grammar tolerates a leading `-`
    // (`--since -1y`), and without it clap reads that value as an unknown short flag.
    /// Lower time bound. WHEN grammar (system-local tz): a relative `Ns`/`Nm`/`Nh`/`Nd`/`Nw`
    /// = that many seconds/minutes/hours/days/weeks/months/years AGO (`45s`, `90m`, `2h`,
    /// `3d`, `1w`, `2mo`, `1y`; `mo` = 30 days, `y` = 365; a leading `-` is tolerated);
    /// an ISO8601 instant (`2026-06-01T05:00:00Z` / `…+10:00`); a BARE datetime
    /// (`2026-06-01T05:00:00`) = that LOCAL wall-clock time; or a bare date (`2026-06-01`)
    /// = LOCAL MIDNIGHT that day. Intersects (AND) with --turn (both filters apply).
    #[arg(long, value_name = "WHEN", allow_hyphen_values = true)]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as --since). Intersects (AND) with --turn (both filters apply).
    #[arg(long, value_name = "WHEN", allow_hyphen_values = true)]
    pub until: Option<String>,

    /// Cap emitted exchanges. SIGNED: `N` keeps the EARLIEST N of the chronological stream,
    /// `-N` keeps the LATEST N (so `--max-count 1` = the first occurrence ever, `--max-count -1`
    /// = the most recent one), `0` = uncapped (default: unlimited). The kept exchanges always
    /// emit oldest-first among themselves: the sign only selects which END of the stream
    /// survives, mirroring the shared range grammar's `-k = from the end`. NO silent
    /// truncation: the head banner shows the emitted window, the footer reports the drop.
    #[arg(long, value_name = "N", allow_negative_numbers = true)]
    pub max_count: Option<i64>,

    /// Print ONLY the total number of matching exchanges (one integer): the ripgrep
    /// `-c` idiom for "how many times X?". Honors every filter (`-t`, time window,
    /// session/path scope) and reports the TRUE total even if `--max-count` would cap
    /// the listing. With `--format json`, prints `{"matched":N}` instead. (You rarely need
    /// it: the normal output's footer ALWAYS carries this same match total plus the
    /// distinct-session total: `--count-only` just isolates that ONE integer for a pipe,
    /// hence "only".)
    #[arg(long = "count-only", short = 'c')]
    pub count_only: bool,

    /// Print ONLY the distinct matching OWNING sessions (`parent_session_id`), one per line
    ///: the grep/rg `-l` idiom for "WHICH sessions?". Scope-token domain: a subagent hit
    /// lists its parent uuid, so every line re-feeds (per-transcript detail lives in the
    /// JSON summary's `transcript_ids` and each hit's `refetch`). Sorted, deduplicated,
    /// UNCAPPED; when `--max-count` dropped exchanges a stderr note says the listing may be
    /// incomplete. Pipes straight into `--sessions-from -` to scope the NEXT command to
    /// what matched: `csift search P -l | csift stats --sessions-from -`.
    #[arg(
        short = 'l',
        long = "sessions-with-matches",
        conflicts_with_all = ["count_only", "siblings"]
    )]
    pub sessions_with_matches: bool,

    /// Print ONLY a census of the matched records along ONE fixed axis: `<count> <key>`,
    /// one key per line. Every axis counts RECORDS (a record whose several sections match
    /// still counts once). Axes (a CLOSED set, not a query language): `label` (per
    /// role.class.sub leaf; a record counts under EVERY leaf it carries, so a leaf's number
    /// is exactly how many records `-t <leaf>` would surface: the exploration on-ramp:
    /// run `csift search "" <target> --count-by label` BEFORE you guess a `-t`) · `tool`
    /// (per tool name; RECORDS again: a completed call is a tool_use record PLUS a
    /// tool_result carrier record, so expect ≈2× the per-CALL tallies `stats` prints; an
    /// answered AskUserQuestion re-homes its carrier to user.answer, so AUQ stays ≈1×.
    /// The 2× gap is a unit difference, not a discrepancy) · `turn` (per turn, ASCENDING
    /// turn order: a histogram) · `session`
    /// (per transcript) · `pairing` (paired | pending | orphan, joined by tool_use_id;
    /// covers every record riding a tool_use/tool_result block INCLUDING the
    /// SendMessage/spawn/subagent-return communication views, so "any pending tools?" is
    /// just `csift search "" <t> --count-by pairing`: a frozen SendMessage counts as
    /// pending under any selector) · `model` (per assistant model, the raw `message.model`
    /// value; Claude Code's own `<synthetic>` placeholder, a CC-fabricated stand-in
    /// assistant record such as an API-error notice, is reported verbatim) · `attachment`
    /// (per attachment payload type - the `type` inside a `type:"attachment"` record's
    /// payload, e.g. `hook_additional_context` / `edited_text_file` /
    /// `compact_file_reference`; this axis IMPLIES the --attachments gate, so the census
    /// is answerable without the separate flag) · `version` (per Claude Code version -
    /// every record carries the writing CC's top-level `version` stamp, so this shows the
    /// version(s) a session ran under and where an upgrade landed mid-flight) · `result`
    /// (per tool-result error state, `ok` | `error` - `pairing` answers "did a result
    /// come back", `result` answers "was it good": "any failed reads?" is
    /// `search "" T --count-by result`; errored results also render an inline `[error]`
    /// and carry JSON `is_error`) · `for-kind` (per PROVENANCE relation - which `for`
    /// edges the scope carries. MULTI-KEY like `label`: a record counts once under every
    /// distinct relation word its edges name, because one unit can carry several at once
    /// (an attachment names its parent AND the call it belongs to). A relation counts
    /// whether or not csift resolved its target - the census answers which relations
    /// EXIST, and a hit's own `for[]` says what each one resolved to. A record carrying no
    /// edge is excluded and counted. THE KEY SPACE is the closed vocabulary, twenty-seven
    /// words: `armed_by` `attached_to` `banner` `blocked` `branches` `continued_in`
    /// `continues` `fires` `for_tool_use` `forked_from` `hook_run` `interrupted` `leaf`
    /// `pairs` `parent` `precedes` `preserves` `redoes` `refused` `repairs` `replays`
    /// `replies_to` `reports` `result_of` `supersedes` `versions` `wraps`. Six of them
    /// ride JSON alone and never render a label-zone tag, because each restates something
    /// the unit already shows - `parent` the record above, `result_of` what the pairing
    /// glyph says, and the four survival words `supersedes` / `continues` / `replays` /
    /// `branches` the distance line and the `[abandoned]` / `[rewound]` /
    /// `[replay copy of L<n>]` markers. This census reads them anyway. THE WORDS ARE NOT
    /// THE WHOLE MAP: a relation whose target sits in ANOTHER transcript is a FIELD on the
    /// surface that owns the join - `clone_of` / `cleared_from` / `continued_in` /
    /// `minted_elsewhere` on `list`, `spawn_tool_use_id` / `parent_agent_id` on `agents`,
    /// the verdict on `msg` - because an edge's target names a session without carrying the
    /// re-feedable id trio beside it; and a relation whose target is not a RECORD at all is
    /// a per-hit FACT with its own key - `produced_by` (a command), `checkpoint_closes_at`
    /// (an instant), `peer_sender_task_id` (a lane), `image_ids` (an image handle),
    /// `--resolve-persisted` (an external file). Some relations csift can NAME and nothing
    /// on disk states: `summarizes` has no key for the region a compaction summary replaced
    /// (the boundary names what it KEPT, `preserves`, and `csift verbatim` reconstructs the
    /// rest), `steers` has none at all, `closes` resolves one hop through `hook_run` and
    /// infers the rest from position, and `folds` is a csift RENDER decision rather than a
    /// record address. SKILL's relation inventory is the same table with one row each).
    /// Records
    /// outside an axis's domain (no tool name / no pairing / no model) are excluded AND
    /// the excluded count is reported; never silently. Honors `-t`/`-T`/time/turn/scope;
    /// empty pattern = whole-scope census. Under an active `-t`/`-T`, the `label` axis
    /// counts each surviving record ONLY under its labels that pass the same filter; a
    /// dual-labeled record never leaks its filtered-out twin into the census (so
    /// `-t user -T user.message --count-by label` shows user.* keys only; drop `-t`/`-T`
    /// to census a record set's FULL label sets). JSON: `census` rows
    /// (`axis`/`key`/`records`) + a summary.
    #[arg(
        long = "count-by",
        value_enum,
        value_name = "AXIS",
        conflicts_with_all = ["count_only", "sessions_with_matches", "siblings", "raw"]
    )]
    pub count_by: Option<CountAxis>,

    /// Print everything that is FOR one record, TRANSITIVELY: the reverse provenance
    /// query. `<ADDR>` is `L<line>` (per FILE, so the scope must resolve to exactly ONE
    /// transcript - `@<uuid> --no-subagents`, `@<agent-id> --no-subagents` or a `*.jsonl` path;
    /// a wider scope is refused, naming its transcript count) or a record UUID (which finds
    /// its record in whichever scanned transcript carries it, in any scope).
    /// The closure itself is computed PER TRANSCRIPT and never crosses one. The answer is
    /// ordinary hits in file order, the addressed record at the
    /// head, and it grows: a record is in it when it is for the address OR for something
    /// already in it - so a call's result, the notification a background task it launched
    /// sent two turns later, and the attachments written for any of those all come back
    /// from one query. THREE structural relations admit a row and no others: its
    /// `parentUuid` names a member (which carries `parent` and every family word built on
    /// the parent link - `attached_to`, `blocked`, `redoes`, `fires`, `wraps`), it carries
    /// a `tool_result` answering a member's call (`result_of`), or it names a member's
    /// call by id in its own text or payload (`reports`, `for_tool_use`). The rest of the
    /// vocabulary is not reverse-walked, so `--for` on a record that only a
    /// non-structural relation points AT comes back empty - `versions`, `refused`,
    /// `interrupted`, `leaf`, `forked_from`, `continued_in`, `replies_to`, `armed_by`,
    /// `banner`, `hook_run`, the four survival words, `precedes` and `preserves`.
    /// That is a definitive absence about the three arms, not a claim
    /// that nothing points there.
    /// TWO relations are in neither list and come back anyway, because the walk is over the
    /// LINKS and not over the words: `pairs` on a compaction summary and `repairs` on a
    /// paired resume placeholder both name the record ABOVE, which is their `parentUuid`,
    /// so the parent arm returns them from an address on that record even though their hit
    /// prints no `parent` edge. It prints none because of the ONE-RELATION-ONE-EDGE rule:
    /// `parent` stands down whenever ANY OTHER edge on the same unit already names that
    /// same TARGET - the test is on the target and never on who named it, so a family's
    /// word and the engine's own both do it. No FIELD is removed by that; the record keeps
    /// the parent it had, `for_root` still names its turn, and `--for` still walks it.
    /// The forward direction answers those: the `for[]` on the hit, or
    /// `--count-by for-kind`. The address may name a GATED line (a hook attachment, a bookkeeping
    /// line): the closure starts there either way, and the addressed record itself is
    /// emitted only under its own gate - `--for L12` gives you what L12 drew, `--for L12
    /// --attachments` gives you L12 as well. Composes with `-t`/`-T`/time/scope; the
    /// PATTERN still applies, so use an empty pattern for the whole set. JSON: the summary
    /// gains `for_of` (`address`, `records`, `transcripts`).
    #[arg(long = "for", value_name = "ADDR")]
    pub for_addr: Option<String>,

    /// Also render the SIBLING records of every matched turn: the rest of the
    /// back-and-forth, not only the matched line, so a matched USER question surfaces
    /// WITH the agent's reply (answers "I said X, what did you say back?"). FIXED policy,
    /// zero arguments: message units always render (user.*, agent.message,
    /// agent.communication.*); chattier machinery is capped per leaf (agent.thinking ≤ 2,
    /// agent.thinking.narration ≤ 1, agent.tool.use ≤ 3, agent.tool.result ≤ 3,
    /// harness.* ≤ 2): the caps apply to the
    /// NON-matching context records ONLY; your actual pattern hits always render in full,
    /// however many share a leaf (so a `--siblings` block can legitimately show more than
    /// the cap count of same-leaf lines). Anything capped away is
    /// counted and surfaced as an explicit `(+N more · csift show @<id> --line A..B)`
    /// pointer. A record that itself matched is never repeated as a sibling. No effect
    /// under `--count-only`.
    #[arg(long)]
    pub siblings: bool,

    /// Emit each matched record's VERBATIM raw jsonl line instead of the rendered exchange
    ///: the same escape hatch as `show --raw` (fields csift does not render: usage,
    /// stop_reason, model, any new field), but FILTERED by the full search surface
    /// (PATTERN, `-t`/`-T`, time, turn, scope). stdout is a pure jsonl stream (pipe
    /// into `jq`); scope/drop/malformed notes go to stderr. One line per matched RECORD (a
    /// record hit under several labels emits once); a sidecar-merged record has no physical
    /// line and is omitted with a stderr note. `--resolve-persisted` still affects MATCHING
    /// only: the emitted line is always the original.
    #[arg(
        long,
        conflicts_with_all = ["siblings", "count_only", "sessions_with_matches", "no_truncate"]
    )]
    pub raw: bool,

    /// Emit each matched (and `--siblings`) record's FULL text instead of the ~400-char
    /// excerpt, so you can READ a found message end-to-end (e.g. the question at the tail
    /// of a long reply) without dropping to the raw jsonl. Newlines are still collapsed to
    /// single spaces (one line per record) in the text output and in the JSON `excerpt`;
    /// under `--format json` this flag ALSO fills a `body` field per hit, carrying the same
    /// rendered text with its newlines intact (so a markdown table or a paragraph break
    /// survives). `body` is null without this flag - the default excerpt is a fragment
    /// anyway, so a wide scan is not doubled by a full copy of every record. The default
    /// excerpt stays centered on the match with an explicit `… (+N chars)` marker;
    /// `--no-truncate` removes the cap entirely.
    #[arg(long)]
    pub no_truncate: bool,

    /// Resolve `<persisted-output>` pointers to their `tool-results/<id>.txt` file.
    #[arg(long)]
    pub resolve_persisted: bool,

    /// RETIRED. A TOMBSTONE: hidden from `--help`, accepted by the parser
    /// and then a hard error naming the successor, so a stale invocation fails loudly
    /// instead of silently scanning less than it asked for (the `turns` tombstone
    /// precedent). The leaf it gated has a name: select it with
    /// `-t harness.hook.context`.
    #[arg(long, hide = true)]
    pub additional_context: bool,

    /// Also scan EVERY `type:"attachment"` record: the harness sidecar payloads (hook
    /// context, edited-file snapshots, compact file references, file snapshots, …) a
    /// default scan never parses (they are the bulk of a transcript's bytes and echo files
    /// wholesale). It EQUALS naming every attachment-carried leaf with `.*`, and the
    /// narrow ask needs no flag at all - a hook payload is `-t harness.hook.context`,
    /// an injected payload is `-t 'harness.context.*'`, and each renders with its
    /// VERBATIM payload JSON as the matchable text. `--count-by attachment`
    /// implies this gate; an explicit `show --line`/`--uuid` address always renders an
    /// addressed attachment record, flag or not.
    #[arg(long)]
    pub attachments: bool,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
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
