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
        unless it contains an uppercase letter; `-i` forces case-insensitive and \
        always wins. `--multiline` lets `.` cross newlines. An EMPTY pattern is a \
        pure filter; it matches every label-eligible record, so combine it with \
        `--label` / `--since` / `--turn` (a bare empty pattern with no other \
        filter warns that it will emit a lot).\n\n\
        CATEGORIES (`-t`, repeatable): a dotted `role.class.sub` SELECTOR. A selector matches a \
        record label iff it is a dot-SEGMENT prefix of the label's path, so `-t agent` covers the \
        whole agent role while `-t agent.tool` covers use+result. The leaf labels: \
        user.message | user.answer | user.rejection | user.unsent | user.rewound | user.queued | \
        agent.message | \
        agent.thinking | agent.thinking.narration | agent.tool.use | agent.tool.result | \
        agent.communication.{inbox,sent,signal,channel} | \
        harness.notification.{workflow,monitor,subagent,background-command,task} | \
        harness.compaction.{summary,boundary} | harness.command.{invocation,stdout} | \
        harness.interrupt.{user,tool} | harness.schedule.{wakeup,fire} | \
        harness.resume.{prompt,placeholder} | \
        harness.meta.{hook,loop,attachment,turn-duration,away-summary,stop-hooks,snapshot,system}. \
        With none given, EVERY label is eligible. `-T`/`--label-not` \
        EXCLUDES with the same selector grammar (the rg -t/-T duality): the effective set is \
        (-t selectors, or ALL) minus (-T selectors); a combination that excludes everything it \
        includes is a hard error. The human turn is \
        `user.message`; an AskUserQuestion answer is `user.answer` (the full Q+options+answer \
        unit); a plan-rejection-with-message is `user.rejection` (+ a [plan: …] pointer). An \
        inbound peer/teammate message is `agent.communication.inbox` (NOT `user`); a \
        `<task-notification>` automation pulse is `harness.notification.*` (NOT `user`). A \
        tool named after harness machinery still classifies by ROLE: a `ScheduleWakeup` CALL \
        (arming a timer) is `agent.tool.use` like any other tool: `harness.schedule.wakeup` \
        is only the FIRED tick, the harness-injected marker-carrying wakeup prompt. A \
        custom-prompt tick carries no marker at all: it lands under \
        `harness.schedule.fire`, the prompt a scheduled task fires, keyed on the \
        record's own isMeta + promptSource system stamp rather than on its text.\n\n\
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
    after_help = SEARCH_AFTER_HELP
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
    /// sibling subcommand uses (`csift search PATTERN .` now works, matching
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
    /// (`-t user`) selects the leaves the model RECEIVED (excluding, on the first of THREE
    /// axes below, `user.unsent` and `user.rewound`, which the surviving conversation no
    /// longer reaches, `harness.compaction.boundary`, which carries no message at all, and
    /// the six gated leaves); a GLOB (`-t 'user.*'`) selects every
    /// leaf under the prefix, visibility ignored; an intermediate prefix or exact leaf
    /// (`-t agent.tool`, `-t user.unsent`) selects its full set. Axis 1 is that leaf default;
    /// the other two decide per RECORD. DELIVERY: Claude Code's request assembler re-mints a
    /// `system`/`local_command` record (a slash command's own echo and stdout) as a user
    /// message, so `-t harness` shows it although its leaf is invisible, and it drops an
    /// `isVirtual` record and the `<synthetic>` API-error placeholder, so `-t agent` hides
    /// those although `agent.message` is visible. SURVIVAL: a record Claude Code's own
    /// conversation chain no longer reaches - a recalled draft, a turn the operator rewound
    /// past, anything under one - is not in the conversation either, so a bare role skips it
    /// while a glob or an exact leaf reaches it, marked `[abandoned]`/`[rewound]`. An invalid
    /// selector is a HARD error listing the valid set; with none given, every label is
    /// eligible. (0 back-compat: the old flat `thinking`/`tool`/`tool-response` now error.)
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

    /// Lower time bound. WHEN grammar (system-local tz): a relative `Ns`/`Nm`/`Nh`/`Nd`/`Nw`
    /// = that many seconds/minutes/hours/days/weeks/months/years AGO (`45s`, `90m`, `2h`,
    /// `3d`, `1w`, `2mo`, `1y`; `mo` = 30 days, `y` = 365; a leading `-` is tolerated);
    /// an ISO8601 instant (`2026-06-01T05:00:00Z` / `…+10:00`); a BARE datetime
    /// (`2026-06-01T05:00:00`) = that LOCAL wall-clock time; or a bare date (`2026-06-01`)
    /// = LOCAL MIDNIGHT that day. Intersects (AND) with --turn (both filters apply).
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as --since). Intersects (AND) with --turn (both filters apply).
    #[arg(long, value_name = "WHEN")]
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
    /// and carry JSON `is_error`). Records
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

    /// Also scan hook-injected `additionalContext`: the `attachment` records a SessionStart /
    /// UserPromptSubmit / … hook writes into the transcript. Off by default: injected context
    /// is harness machinery and often echoes prompts and files wholesale, drowning genuine
    /// hits. When enabled, these records surface under `harness.meta.hook`; an explicit
    /// `show --line`/`--uuid` address always renders one, flag or not. ONE hook payload is
    /// exempt and always scanned: a csift-channel delivery (`agent.communication.channel`),
    /// because a message another lane sent this one is not machinery.
    #[arg(long)]
    pub additional_context: bool,

    /// Also scan EVERY `type:"attachment"` record: the harness sidecar payloads (hook
    /// context, edited-file snapshots, compact file references, file snapshots, …) a
    /// default scan never parses (they are the bulk of a transcript's bytes and echo files
    /// wholesale). A SUPERSET of --additional-context: a hook payload still surfaces under
    /// `harness.meta.hook`; every other payload surfaces under `harness.meta.attachment`
    /// with its VERBATIM payload JSON as the matchable text. `--count-by attachment`
    /// implies this gate; an explicit `show --line`/`--uuid` address always renders an
    /// addressed attachment record, flag or not.
    #[arg(long)]
    pub attachments: bool,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

/// The promoted non-record leaves (v0.10.0, plus the v0.10.1 catch-all) - scanned only
/// under an explicit selector ([`SearchArgs::reaches_gated`]) or a `show` address.
pub const GATED_LEAVES: [Class; 6] = [
    Class::UserQueued,
    Class::MetaTurnDuration,
    Class::MetaAwaySummary,
    Class::MetaStopHooks,
    Class::MetaSnapshot,
    Class::MetaSystem,
];

impl SearchArgs {
    /// Whether subagent transcripts are spanned (the default). `--no-subagents` restricts to
    /// the top-level session(s). Feeds [`crate::path::SubagentScope::from`].
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.subagents || !self.no_subagents
    }

    /// The project targets to scope to: the positional `[PATH]...`. Empty ⇒ the shared
    /// resolver scans every project.
    #[must_use]
    pub fn targets(&self) -> Vec<PathBuf> {
        self.paths.clone()
    }

    /// True when the scan should keep `type:"attachment"` lines: the explicit
    /// `--attachments` flag, or the `--count-by attachment` axis (unanswerable without
    /// the gate - the D7 implied-widening precedent).
    #[must_use]
    pub fn scan_attachments(&self) -> bool {
        self.attachments || matches!(self.count_by, Some(CountAxis::Attachment))
    }

    /// The v0.10.0 gated-leaf rule: a promoted non-record leaf is scanned ONLY when an
    /// EXPLICIT `-t` selector reaches it (the full path, a glob, or the `harness.meta`
    /// prefix). A bare scan with no `-t` admits every label elsewhere but never parses
    /// these lines: most queued content is a duplicate automation pulse and every
    /// promoted line is a non-message line, so the default surface stays the
    /// conversation. `-T`-only filters do not reach them either.
    #[must_use]
    pub fn reaches_gated(&self, c: Class) -> bool {
        !self.labels.is_empty() && self.label_filter().selected(c.path())
    }

    /// True when a BARE `harness` role selector is active. Such a query asks for what
    /// the model received under that role, and one gated leaf carries delivered
    /// records: a `system`/`local_command` line, which Claude Code's request assembler
    /// re-mints as a user message. The scan therefore admits the `"subtype"` candidate
    /// lines under this selector too - a key-only memmem on a line shape that is rare
    /// (585 in the whole measured corpus) - and the per-record override then keeps only
    /// the `local_command` ones. Every OTHER gated leaf stays exactly as gated.
    #[must_use]
    pub fn bare_harness_role(&self) -> bool {
        self.labels.iter().any(|l| l == "harness")
    }

    /// True when the scan parses `type:"system"` candidate lines at all: an explicit
    /// selector reaching `harness.meta.system`, or a bare `harness` role selector. THE
    /// one predicate for that admission - the candidate gate and the §7f whole-file
    /// gate's synthesized-marker set must agree, or a pattern that can only match the
    /// FABRICATED `[<subtype> <level>]` head would be gated away on a file that holds
    /// it (a silent drop, the §7 landmine).
    #[must_use]
    pub fn scans_system_lines(&self) -> bool {
        self.reaches_gated(Class::MetaSystem) || self.bare_harness_role()
    }

    /// True when the scan actually PARSES a gated leaf's lines: an explicit selector
    /// reaches it, or - for `harness.meta.system` alone - a bare `harness` role admits
    /// them ([`SearchArgs::scans_system_lines`]). THE predicate the candidate gate and
    /// the zero-match diagnosis both key on: `reaches_gated` answers "did a selector
    /// name it", which is no longer the same question.
    #[must_use]
    pub fn scans_gated(&self, c: Class) -> bool {
        if c == Class::MetaSystem {
            return self.scans_system_lines();
        }
        self.reaches_gated(c)
    }

    /// True when NO gated leaf's lines were parsed (the zero-match diagnosis names them
    /// so an absence is never mistaken for a scan of them). Keys on
    /// [`SearchArgs::scans_gated`]: under a bare `harness` role the system lines WERE
    /// parsed, and telling a consumer otherwise is a false statement on the
    /// honest-empties keystone.
    #[must_use]
    pub fn gated_unreached(&self) -> bool {
        !GATED_LEAVES.iter().any(|&c| self.scans_gated(c))
    }

    /// The effective `-t`/`-T` filter (include minus exclude).
    #[must_use]
    pub fn label_filter(&self) -> LabelFilter<'_> {
        LabelFilter::new(&self.labels, &self.labels_not)
    }
}
