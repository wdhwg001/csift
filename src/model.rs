//! JSON-line record + content-block data model.
//!
//! One JSON object per line. The shape below was verified + extended against real
//! `~/.claude/projects/**/*.jsonl` data (2026-06-07). Parsing discipline:
//!
//! - **Tolerate unknown fields.** Real records carry far more than the brief
//!   listed (`attachment`, `file-history-snapshot`, `queue-operation`, `isMeta`,
//!   `isSidechain`, `userType`, `toolUseResult`, `slug`, `entrypoint`, …). We
//!   deserialize only what we use and ignore the rest — never crash on a new field.
//! - **Tolerate missing `timestamp`.** Metadata-only records (`last-prompt`,
//!   `ai-title`, `permission-mode`, `file-history-snapshot`) have no timestamp;
//!   they are skipped in time logic, never panic.
//! - **`message.content` is string OR array.** Older / genuine-user text is a bare
//!   string; everything else is an array of typed blocks.
//!
//! ## Genuine-user vs tool-result-carrier (load-bearing)
//!
//! A `type:"user"` record is NOT always a human turn. In one real session: 332
//! genuine string-content users + 61 text-block users vs **1619** tool_result
//! carriers. A genuine user turn is: string content (and NOT `isCompactSummary`),
//! or content whose blocks are text (no `tool_result`). See [`Record::is_genuine_user`].
//!
//! ## Compaction
//!
//! A compaction summary is a `type:"user"` record with `isCompactSummary: true`
//! and `isVisibleInTranscriptOnly: true`, carrying string content — it must be
//! excluded from "genuine user". A separate `type:"system"`
//! `subtype:"compact_boundary"` record carries the metrics
//! (`trigger`, `preTokens`, `postTokens`, `durationMs`).

use serde::Deserialize;

/// The synthesized prefixes Claude Code writes into the `tool_result` answering an
/// `AskUserQuestion` (§4.4). CC has shipped (at least) TWO phrasings for the same
/// synthesized answer — verified across real `~/.claude/projects` data:
///
/// - `"User has answered your questions: \"<q>\"=\"<a>\". …"`
/// - `"Your questions have been answered: \"<q>\"=\"<a>\". …"`  (the dominant form
///   in current data; a single hardcoded marker missed it entirely)
///
/// Some sessions span a version transition and contain BOTH forms, so an AUQ
/// answer must be recognised if it carries EITHER prefix. Used to surface AUQ
/// answers under the `user` category without re-parsing `toolUseResult`.
pub const AUQ_ANSWER_MARKERS: &[&str] = &[
    "User has answered your questions",
    "Your questions have been answered",
];

/// True when `text` (a `tool_result`'s rendered content) is a synthesized
/// AskUserQuestion answer — i.e. it STARTS WITH a known AUQ-answer marker (§4.4).
///
/// START-anchored, not `contains`: a real AUQ answer is a CC-machine-synthesized
/// string that always LEADS with its marker (`"User has answered your questions: …"`
/// / `"Your questions have been answered: …"`). A `contains` check false-positives on
/// any tool_result that merely QUOTES the marker mid-content — e.g. csift's own dev
/// sessions Read/grep SPEC.md + fixtures that DOCUMENT these markers, which used to be
/// mislabeled `user.answer` and dumped whole files. `trim_start` tolerates leading
/// whitespace the renderer may prepend without admitting a mid-content quote.
#[must_use]
pub fn is_auq_answer_text(text: &str) -> bool {
    let head = text.trim_start();
    AUQ_ANSWER_MARKERS.iter().any(|m| head.starts_with(m))
}

/// True when `content` (a user record's string or joined-text body) is a
/// machine-synthesized marker the user never typed (§4.2.1–.3) and so must NOT count
/// as a genuine human turn: an exact interrupt marker, a `<local-command-stdout>…`
/// output, or a `<command-name>…` slash-command wrapper. CODEPOINT-SAFE — exact `==`
/// (interrupts, whole-token) or `starts_with` (the two ASCII-tag prefixes); never a
/// byte-offset slice.
#[must_use]
pub fn is_synthetic_user_marker(content: &str) -> bool {
    INTERRUPT_MARKERS.contains(&content)
        || content.starts_with(LOCAL_COMMAND_STDOUT_PREFIX)
        || is_slash_command_wrapper(content)
}

/// The two exact-content synthesized strings Claude Code writes when the user
/// interrupts (§4.2.1). They are a `type:"user"` `text`-block record whose content is
/// EXACTLY one of these — a machine-synthesized interrupt marker, NOT a human turn.
/// Verified across real `~/.claude/projects` data: 116 + 21 occurrences, all
/// non-`isMeta`, none carrying any extra prose (dropping them as turn boundaries loses
/// zero user content).
pub const INTERRUPT_MARKERS: &[&str] = &[
    "[Request interrupted by user]",
    "[Request interrupted by user for tool use]",
];

/// Prefix of a `<local-command-stdout>…` user record (§4.2.2) — local-command OUTPUT
/// (machine), not the user's prose. Non-`isMeta` string content (its sibling
/// `<local-command-caveat>` carries `isMeta` and is already excluded). Must NOT open a
/// turn.
pub const LOCAL_COMMAND_STDOUT_PREFIX: &str = "<local-command-stdout>";

/// Prefix of a `<command-name>/x…</command-name>` slash-command invocation record
/// (§4.2.3) — the machine-templated EXPANSION of a slash command, non-`isMeta`. The
/// templated wrapper must NOT open a turn; any genuine prose the user typed after the
/// command lives in the `<command-args>…</command-args>` body and is recovered
/// separately (see [`Record::slash_command_args`]).
pub const COMMAND_NAME_PREFIX: &str = "<command-name>";

/// The SAME slash-command wrapper in the NEWER tag order: current CC emits
/// `<command-message>…</command-message>\n<command-name>/x</command-name>\n<command-args>…`
/// — the message tag FIRST (verified live 2026-07: 14 sessions new-order vs 35 old-order
/// in one real corpus; both orders coexist). Detection must accept EITHER leading tag:
/// anchoring on `<command-name>` alone silently reclassified every new-order record as
/// GENUINE user prose — raw wrapper XML surfaced as `user.message`, and a no-args
/// wrapper opened a turn.
pub const COMMAND_MESSAGE_PREFIX: &str = "<command-message>";

/// True when `content` is a slash-command wrapper (§4.2.3) in EITHER tag order.
#[must_use]
pub fn is_slash_command_wrapper(content: &str) -> bool {
    content.starts_with(COMMAND_NAME_PREFIX) || content.starts_with(COMMAND_MESSAGE_PREFIX)
}

/// Prefix of a `<task-notification>…</task-notification>` user record — a MACHINE-INJECTED
/// automation trigger (a background-command / workflow / spawned-task completion notice CC
/// inserts as a `type:"user"`, non-`isMeta`, STRING-content record). It LOOKS like a human
/// turn to [`Record::is_genuine_user`] (it passes every gate), so it DOES open a turn — but
/// it is an automation pulse, not the operator's prose. [`Record::automation_trigger`]
/// classifies it so surfaces can LABEL the segment (`[workflow <id> completed] <summary>`)
/// instead of dumping the raw `<task-id>`/`<output-file>`/`<status>` XML wrapper.
pub const TASK_NOTIFICATION_PREFIX: &str = "<task-notification>";

/// The synthesized marker Claude Code writes into the `tool_result` when the user
/// REJECTS a tool use (§4.2.4) — fires for ANY rejected tool_use (ExitPlanMode plan
/// kick-backs AND rejected AskUserQuestion / Edit / etc.). On its own it is NOT a user
/// turn; it becomes one only when followed by the [`PLAN_REJECTION_USER_PREFIX`] tail
/// (a real typed user instruction).
pub const PLAN_REJECTION_MARKER: &str = "The user doesn't want to proceed with this tool use";

/// The fixed ASCII delimiter that precedes the user's typed instruction in a
/// rejection-with-message (§4.2.4): everything AFTER it is the genuine user message.
/// A rejection WITHOUT this delimiter (the `STOP what you are doing and wait…` form)
/// carries no typed message and must NOT open a turn.
pub const PLAN_REJECTION_USER_PREFIX: &str = "To tell you how to proceed, the user said:\n";

/// The opening tag of an inbound `<teammate-message …>` peer-agent message (GOLD §5). A
/// teammate (another Claude session on the same team / FleetView) sends prose or a control
/// signal that Claude Code delivers as a `type:"user"`, `role:"user"`, STRING-content record
/// — so it LOOKS like a human turn ([`Record::is_genuine_user`] used to return `true` for it,
/// counting 106 peer messages as the human user in one real session). It is a peer message,
/// never the operator: classified `agent.communication.{inbox,signal}`, never `user`.
pub const TEAMMATE_MESSAGE_OPEN: &str = "<teammate-message";

/// The preamble Claude Code prepends to a peer message it relays into a session (GOLD §5):
/// `Another Claude session sent a message:\n<teammate-message …>`. A peer tag IMMEDIATELY after
/// this preamble is at a section BOUNDARY (FINDING-1, [`is_section_boundary`]) — so a real relayed
/// peer message is recognized while a tag merely QUOTED mid-prose is not.
pub const PEER_MESSAGE_PREAMBLE: &str = "Another Claude session sent a message:";

/// The opening tag of an inbound `<agent-message from="…">` peer form (P1c M1) — a DISTINCT
/// inbound peer message from [`TEAMMATE_MESSAGE_OPEN`], seen `isMeta` in real data (e.g. an OMC
/// agent replying to a peer, relayed into this session: `<agent-message
/// from="oh-my-claudecode:architect">…`). Classifies `agent.communication.inbox` (the
/// `from="…"` attribute ⇨ self), never `user.message`.
pub const AGENT_MESSAGE_OPEN: &str = "<agent-message";

/// Section CLOSE tags (FINDING-1). A peer / `<task-notification>` open tag that sits right after
/// one of these (modulo whitespace) is at a section BOUNDARY ([`is_section_boundary`]), so a
/// BATCHED record's later sections are still recognized — while a tag QUOTED mid-prose (a genuine
/// user message that merely mentions the literal tag, common in csift's OWN dev sessions) is NOT a
/// boundary and never starts a section. Kept beside their open-tag constants so the pair never drift.
const TASK_NOTIFICATION_CLOSE: &str = "</task-notification>";
const TEAMMATE_MESSAGE_CLOSE: &str = "</teammate-message>";
const AGENT_MESSAGE_CLOSE: &str = "</agent-message>";

/// The leading sentence of an ASYNC/background `Agent` spawn's launch-confirmation tool_result
/// (`"Async agent launched successfully.\nagentId: …"`). This is a launch ACK, NOT the child's
/// report — the report arrives LATER via the `<task-notification>` `<result>` pulse (G1 → inbox).
/// On disk the ack also carries the structured `toolUseResult.{isAsync:true, status:"async_launched"}`
/// shape ([`Record::is_async_launch_ack`] prefers the structured signal, falls back to this prefix).
pub const ASYNC_LAUNCH_ACK_PREFIX: &str = "Async agent launched successfully";

/// The fixed harness-injected continuation marker (GOLD §5) — `harness.schedule.continuation`.
/// A `type:"user"` (`isMeta`) record CC injects to resume a session from where it left off.
/// Verified across real `~/.claude/projects` data (522 occurrences), exact content.
pub const SCHEDULE_CONTINUATION_MARKER: &str = "Continue from where you left off.";

/// The `ScheduleWakeup` TIMER's fired-prompt sentinel (GOLD §5) — `harness.schedule.wakeup`.
/// When a `ScheduleWakeup` tool fires, the harness injects its `prompt`; this fixed sentinel is
/// that injected prompt. This is the SCHEDULER timer firing — DISTINCT from the autonomous-loop
/// DRIVER ticks (`# Autonomous loop tick` / `Run the autonomous check`), which are
/// `harness.meta.loop` ([`AUTONOMOUS_LOOP_TICK_PREFIX`] / [`AUTONOMOUS_CHECK_MARKER`]); the two
/// must not be conflated. Together with [`SCHEDULE_WAKEUP_LOOP_CHECK_PREFIX`] /
/// [`SCHEDULE_WAKEUP_TIMER_MARKER`] these are the fixed wakeup-tick markers — a generic
/// cron/monitor tick's injected prompt is still operator-authored free text with no universal
/// marker (the `ScheduleWakeup` *tool_use* that ARMS a wakeup is the agent's action, classified
/// `agent.tool.use`, not the fired tick). See the GOLD-gap note in the module docs.
pub const SCHEDULE_WAKEUP_MARKER: &str = "<<autonomous-loop-dynamic>>";

/// The header of the harness-injected FIRED autonomous-loop / `ScheduleWakeup` timer tick (P1c
/// M2a / oracle D12) — `harness.schedule.wakeup`. When the timer FIRES, the harness injects an
/// `isMeta` `type:"user"` record whose content opens `# Autonomous loop check\n\nYou're being
/// invoked on a timer …`. DISTINCT from the `meta.loop` DRIVER ticks
/// ([`AUTONOMOUS_LOOP_TICK_PREFIX`] = `# Autonomous loop tick` / [`AUTONOMOUS_CHECK_MARKER`]):
/// `check` ≠ `tick`, so the two prefixes never collide. The wakeup arm is matched BEFORE the
/// meta.loop arm in [`Record::classify`], so the fired tick routes to `schedule.wakeup`.
pub const SCHEDULE_WAKEUP_LOOP_CHECK_PREFIX: &str = "# Autonomous loop check";

/// See [`SCHEDULE_WAKEUP_LOOP_CHECK_PREFIX`] — the fired-timer body sentence (matched anywhere,
/// as it follows the `# Autonomous loop check` header after a blank line). Verified verbatim
/// against real `~/.claude/projects` data (straight ASCII apostrophe).
pub const SCHEDULE_WAKEUP_TIMER_MARKER: &str = "You're being invoked on a timer";

/// `harness.meta.hook` markers (GOLD §2, edge-fixtures G2) — hook-injected feedback, NOT the
/// operator: a stop-hook feedback message, a `<local-command-caveat>` wrapper, or the
/// edit-failed-retry notice CC injects when an Edit's target changed under it. (These are
/// `isMeta` user records that would otherwise fall through to `user.message`.)
pub const STOP_HOOK_FEEDBACK_PREFIX: &str = "Stop hook feedback:";
/// See [`STOP_HOOK_FEEDBACK_PREFIX`] — the `<local-command-caveat>…` hook wrapper.
pub const LOCAL_COMMAND_CAVEAT_PREFIX: &str = "<local-command-caveat>";
/// See [`STOP_HOOK_FEEDBACK_PREFIX`] — the edit-failed-retry notice (matched anywhere, as it
/// also rides inside a `Stop hook feedback:` body).
pub const EDIT_RETRY_MARKER: &str = "The last Edit failed because the target file was modified";

/// `harness.meta.loop` markers (GOLD §2, edge-fixtures G2) — autonomous-loop drivers (distinct
/// from the [`SCHEDULE_WAKEUP_MARKER`] sentinel, which stays `harness.schedule.wakeup`).
pub const AUTONOMOUS_LOOP_TICK_PREFIX: &str = "# Autonomous loop tick";
/// See [`AUTONOMOUS_LOOP_TICK_PREFIX`] — matched anywhere (it can sit mid-prompt).
pub const AUTONOMOUS_CHECK_MARKER: &str = "Run the autonomous check";

/// An `isMeta` `[Image: source:…]` pseudo-record (GOLD §2, edge-fixtures G2) — EXCLUDED from
/// the taxonomy entirely (classify yields no label), so it is never mislabeled `user.message`.
pub const IMAGE_SOURCE_PREFIX: &str = "[Image: source:";

/// A single parsed jsonl line. Unknown top-level fields are ignored by serde.
///
/// Several fields below are deserialized for completeness of the documented record
/// model (SPEC §3.2) and to keep parsing tolerant, but are not (yet) read by any
/// handler — e.g. `parent_uuid` (the §6.4 round-trip reconstruction keys on file
/// order + genuine-user delimiting, not the uuid tree), `is_sidechain`,
/// `is_visible_in_transcript_only`, `subtype`, `content`. They are part of the
/// data contract, intentionally retained, hence the targeted allow rather than
/// deleting SPEC-mandated shape.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct Record {
    /// Record discriminator: "user", "assistant", "system", "summary",
    /// "last-prompt", "attachment", … (open set — keep as String, never enum-panic).
    #[serde(default)]
    pub r#type: Option<String>,

    #[serde(default)]
    pub uuid: Option<String>,

    #[serde(default, rename = "parentUuid")]
    pub parent_uuid: Option<String>,

    /// ISO8601 UTC, e.g. `2026-06-07T05:43:00.000Z`. Absent on metadata-only records.
    #[serde(default)]
    pub timestamp: Option<String>,

    #[serde(default, rename = "sessionId")]
    pub session_id: Option<String>,

    #[serde(default)]
    pub cwd: Option<String>,

    /// Claude Code version string.
    #[serde(default)]
    pub version: Option<String>,

    #[serde(default, rename = "gitBranch")]
    pub git_branch: Option<String>,

    #[serde(default, rename = "isSidechain")]
    pub is_sidechain: Option<bool>,

    /// Compaction summary marker — when true, this user record is NOT a human turn.
    #[serde(default, rename = "isCompactSummary")]
    pub is_compact_summary: Option<bool>,

    /// System-injected pseudo-turn marker (§4.2). `true` ⇒ a `type:"user"` record
    /// whose string/text content LOOKS human ("Continue from where you left off.",
    /// loop ticks, stop-hook feedback, `<local-command-caveat>…`) but is machine-
    /// generated — must be excluded from genuine-user and from turn-delimiting.
    #[serde(default, rename = "isMeta")]
    pub is_meta: Option<bool>,

    /// Co-set with `isCompactSummary` on compaction-summary records (§4.7).
    #[serde(default, rename = "isVisibleInTranscriptOnly")]
    pub is_visible_in_transcript_only: Option<bool>,

    /// `system` record subtype: stop_hook_summary | turn_duration | away_summary
    /// | compact_boundary | …
    #[serde(default)]
    pub subtype: Option<String>,

    /// `system` record inline content (e.g. away_summary text, or the `compact_boundary`
    /// `"Conversation compacted …"` line). Read by `search` as the message-less fallback text (D7).
    #[serde(default)]
    pub content: Option<serde_json::Value>,

    /// `compact_boundary` metrics (§3.5 / D7): `{trigger, preTokens, postTokens, durationMs}` on a
    /// `type:"system"`/`subtype:"compact_boundary"` record. Kept RAW; `search` renders it as a
    /// readable excerpt (`record_raw_text`) so `-t harness.compaction.boundary` can enumerate
    /// compaction points and inspect what each clipped. Absent on every other record. Tolerant.
    #[serde(default, rename = "compactMetadata")]
    pub compact_metadata: Option<serde_json::Value>,

    /// The role-bearing message payload (present on user/assistant records).
    #[serde(default)]
    pub message: Option<Message>,

    /// Structured echo on tool-result carriers (§4.6). Kept as UNPARSED raw JSON text
    /// (`Box<RawValue>`) rather than a built `Value` tree: this blob routinely carries
    /// the full file/output content a tool returned (≈20-25% of a candidate line's
    /// bytes), and eagerly tree-building it for EVERY carrier dominated the parse cost
    /// of every scanning subcommand. The hot paths consult only a handful of small
    /// fields — read them via the [`Record::tur_probe`] typed probe (skips the huge
    /// values without allocating); the one deep consumer (`recover`) parses the full
    /// tree on demand via [`Record::tool_use_result_value`].
    #[serde(default, rename = "toolUseResult")]
    pub tool_use_result: Option<Box<serde_json::value::RawValue>>,

    /// Top-level `attachment` payload (a sibling of `message`, not a content block).
    /// Real records carry attachments for hook output, `edited_text_file` external
    /// edits, `file` snapshots, etc. Kept as UNPARSED raw JSON text (same rationale as
    /// `tool_use_result` — attachments embed whole file snapshots) and read only by
    /// `recover`/`plan` via [`Record::attachment_value`]; additive + tolerant, so no
    /// other subcommand changes behaviour.
    #[serde(default)]
    pub attachment: Option<Box<serde_json::value::RawValue>>,

    /// `file-history-snapshot` payload (a top-level sibling). Carries
    /// `{messageId, trackedFileBackups: {<path>: {backupFileName, version, backupTime}}}`.
    /// Read only by `recover` to know a disk backup EXISTED for a path at a time
    /// (a coverage annotation); the on-disk blob name is not derivable from it (the
    /// real `backupFileName` is frequently `null`), so it is never used to fabricate
    /// content. Additive + tolerant.
    #[serde(default)]
    pub snapshot: Option<serde_json::Value>,

    /// csift's own provenance marker on an ELICITATION SIDECAR record (§3.10). A
    /// hook-written `elicitations.jsonl` line carries `csift:"elicitation-marker-v1"`
    /// on every record so a merged sidecar record is distinguishable from a native CC
    /// record. `None` on every native transcript record. Additive + tolerant.
    #[serde(default)]
    pub csift: Option<String>,

    /// The sidecar record's pairing PHASE — `"pending"` (an unanswered elicitation,
    /// missing from the native transcript) or `"resolved"` (a lightweight close marker
    /// used only for pairing). Read by [`crate::elicitation`]. `None` on a native record.
    #[serde(default, rename = "csiftPhase")]
    pub csift_phase: Option<String>,

    /// The sidecar elicitation KIND — `"AskUserQuestion"` / `"ExitPlanMode"` /
    /// `"mcp-elicitation"`. `None` on a native record.
    #[serde(default, rename = "csiftKind")]
    pub csift_kind: Option<String>,

    /// The sidecar pairing KEY (tool_use_id / elicitation_id / MCP server) — groups a
    /// `pending` with its later `resolved`. `None` on a native record.
    #[serde(default, rename = "csiftKey")]
    pub csift_key: Option<String>,

    /// The MCP server name on an `mcp-elicitation` sidecar record (for the rendered
    /// detail). `None` otherwise. Additive + tolerant.
    #[serde(default, rename = "csiftMcpServer")]
    pub csift_mcp_server: Option<String>,
}

/// The `message` object on user / assistant records.
#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub role: Option<String>,

    /// Either a bare string (genuine text) or an array of typed blocks.
    #[serde(default)]
    pub content: Option<Content>,

    /// The model id on assistant records (`claude-…`). Kept as a raw `Value` so an
    /// unexpected shape can never fail the whole record (tolerance discipline); read
    /// via `as_str`. Consumed by `stats`.
    #[serde(default)]
    pub model: Option<serde_json::Value>,

    /// The token-usage echo on assistant records. Kept UNPARSED (same rationale as
    /// `Record::tool_use_result`): only `stats` reads it, via [`Message::token_usage`].
    #[serde(default)]
    pub usage: Option<Box<serde_json::value::RawValue>>,
}

/// The token-usage fields `stats` sums (each optional + tolerant — a missing/odd
/// field reads as absent, never an error).
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
}

impl Message {
    /// Parse the raw `usage` blob into the typed probe on demand (`None` when absent
    /// or shaped unexpectedly — tolerant, never an error).
    #[must_use]
    pub fn token_usage(&self) -> Option<TokenUsage> {
        let raw = self.usage.as_ref()?;
        serde_json::from_str(raw.get()).ok()
    }

    /// The model id string, when present and a string.
    #[must_use]
    pub fn model_id(&self) -> Option<&str> {
        self.model.as_ref().and_then(serde_json::Value::as_str)
    }
}

/// Typed probe of the SMALL `toolUseResult` fields the hot paths consult (see
/// [`Record::tur_probe`]). Every field is an `Option<Value>` — a tiny scalar/map tree
/// that accepts ANY JSON type, so one oddly-typed field can never fail the whole probe
/// (each accessor then applies the same `as_str`/`as_bool`/`as_object` coercion the
/// former `.get(…)` chains did — byte-identical semantics). Crucially, the blob's HUGE
/// unlisted values (file bodies, stdout echoes, structured patches) are skipped by
/// serde's ignore path without ever being allocated.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TurProbe {
    r#type: Option<serde_json::Value>,
    #[serde(rename = "filePath")]
    file_path: Option<serde_json::Value>,
    #[serde(rename = "persistedOutputPath")]
    persisted_output_path: Option<serde_json::Value>,
    status: Option<serde_json::Value>,
    #[serde(rename = "isAsync")]
    is_async: Option<serde_json::Value>,
    answers: Option<serde_json::Value>,
}

/// `message.content` is polymorphic: a plain string or a list of blocks.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Content {
    /// Bare-string content (older format / genuine user text / compaction summary).
    Text(String),
    /// Array of typed content blocks.
    Blocks(Vec<Block>),
}

/// A typed content block within `message.content`. `#[serde(other)]` Unknown
/// catches any future block type without a parse error.
///
/// Some block fields are deserialized for the documented model (SPEC §3.5) but not
/// read by current logic (`signature`, the `ToolUse.id`, the `ToolResult`
/// `tool_use_id`/`is_error`, `Image.source`). The round-trip reconstruction returns
/// the whole turn, so `tool_use`↔`tool_result` pairing-by-id is not needed (both sit
/// in the same emitted exchange); the fields stay for shape-completeness + future use.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        #[serde(default)]
        text: String,
    },
    Thinking {
        #[serde(default)]
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },
    /// A `redacted_thinking` block — encrypted/opaque reasoning CC emits in place of a visible
    /// `thinking` block (no readable text, only an opaque `data` payload). Classified
    /// `agent.thinking` exactly like a normal thinking block (GOLD §2 / oracle B3); the opaque
    /// `data` is captured for shape-completeness but never rendered.
    RedactedThinking {
        #[serde(default)]
        data: String,
    },
    ToolUse {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        input: Option<serde_json::Value>,
    },
    ToolResult {
        #[serde(default, rename = "tool_use_id")]
        tool_use_id: Option<String>,
        /// String OR array of {type:text,text}/{type:image} — keep raw.
        #[serde(default)]
        content: Option<serde_json::Value>,
        #[serde(default)]
        is_error: Option<bool>,
    },
    Image {
        #[serde(default)]
        source: Option<serde_json::Value>,
    },
    /// Any block type not modeled above — never a parse failure.
    #[serde(other)]
    Unknown,
}

/// The provenance value every csift elicitation-sidecar record carries in its `csift`
/// field (§3.10). A line lacking this is a native CC record / foreign line.
pub const ELICITATION_MARKER: &str = "elicitation-marker-v1";

impl Record {
    /// True when this record is `type == "<t>"`.
    #[must_use]
    pub fn is_type(&self, t: &str) -> bool {
        self.r#type.as_deref() == Some(t)
    }

    /// True when this record is a csift ELICITATION-SIDECAR marker (§3.10) — it carries
    /// `csift:"elicitation-marker-v1"`. Distinguishes a hook-backfilled record (merged
    /// into search/turns/list) from a native CC transcript record.
    #[must_use]
    pub fn is_elicitation_marker(&self) -> bool {
        self.csift.as_deref() == Some(ELICITATION_MARKER)
    }

    /// True when this record is a GENUINE human turn (or a user answer to
    /// AskUserQuestion), NOT a `tool_result`-carrier and NOT a compaction summary.
    ///
    /// Rule (§4.1, ALL must hold):
    /// - must be `type:"user"` with `message.role == "user"`;
    /// - `isCompactSummary` must be falsey (excludes compaction summaries, §4.7);
    /// - `isMeta` must be falsey (excludes system-injected pseudo-turns, §4.2);
    /// - the content must NOT be a machine-synthesized marker the user never typed
    ///   (§4.2.1–.3): an interrupt marker (`[Request interrupted by user]` etc.), a
    ///   `<local-command-stdout>…` output, or a `<command-name>…` slash-command wrapper;
    /// - string content => genuine; block content => genuine iff it contains a
    ///   text block and NO `tool_result` block.
    ///
    /// CODEPOINT-SAFE: every synthetic-marker test is exact `==` or `starts_with`
    /// (whole-token / prefix), never a byte-offset slice, so a CJK body can never be
    /// split mid-codepoint.
    #[must_use]
    pub fn is_genuine_user(&self) -> bool {
        if !self.is_type("user") {
            return false;
        }
        if self.is_compact_summary.unwrap_or(false) {
            return false;
        }
        // §4.2 TRAP: isMeta:true user records look human but are system-injected.
        if self.is_meta.unwrap_or(false) {
            return false;
        }
        // NOTE on `isSidechain`: a subagent transcript's FIRST record is an
        // `isSidechain:true` user seed. It is NOT gated out here on purpose — `list`'s
        // per-subagent preview legitimately treats that seed as the subagent's "first
        // user message", and in TOP-LEVEL transcripts a sidechain seed does not occur in
        // any real corpus. Gating it would silently blank the subagent preview for zero
        // real benefit; the per-surface scan owns subagent-vs-parent context instead.
        let Some(msg) = &self.message else {
            return false;
        };
        if msg.role.as_deref() != Some("user") {
            return false;
        }
        // GOLD §1 BUG FIX + FINDING-2: an inbound PEER message — `<teammate-message>` OR
        // `<agent-message from="…">` — is `type:user`/`role:user`/string content and matches no
        // synthetic marker, so it used to slip through as a GENUINE HUMAN turn (106 peer messages
        // mislabeled as the user in one real session). Both are PEER-AGENT messages — never the
        // operator. Excluded here via [`is_peer_message`] (each still OPENS a turn via
        // [`Record::opens_turn`], but classifies `agent.communication.inbox`, not `user`). The check
        // is on the borrowed content (no allocation on the common path) and is BOUNDARY-anchored
        // (FINDING-1), so a genuine message merely QUOTING the tag stays `user.message`.
        match &msg.content {
            Some(Content::Text(s)) => !is_synthetic_user_marker(s) && !is_peer_message(s),
            Some(Content::Blocks(blocks)) => {
                let has_tool_result = blocks.iter().any(|b| matches!(b, Block::ToolResult { .. }));
                let has_text = blocks.iter().any(|b| matches!(b, Block::Text { .. }));
                if !has_text || has_tool_result {
                    return false;
                }
                // §4.2.1: an interrupt marker arrives as a single `text` block whose text
                // is EXACTLY the marker — exclude it (exact match, codepoint-safe).
                let joined = flatten_content_text(msg.content.as_ref().unwrap());
                !is_synthetic_user_marker(&joined) && !is_peer_message(&joined)
            }
            None => false,
        }
    }

    /// The genuine prose a user typed after a slash command (§4.2.3), recovered from the
    /// `<command-args>…</command-args>` body of a `<command-name>…` record. Returns the
    /// trimmed args text, or `None` when the record is not a slash-command wrapper or the
    /// args are empty. Codepoint-safe: uses `str::find` on the ASCII tag bounds and slices
    /// only on those ASCII byte offsets (never inside the args text), so a CJK args body
    /// is never split mid-codepoint.
    #[must_use]
    pub fn slash_command_args(&self) -> Option<String> {
        let content = self.message.as_ref()?.content.as_ref()?;
        let Content::Text(s) = content else {
            return None;
        };
        if !is_slash_command_wrapper(s) {
            return None;
        }
        const OPEN: &str = "<command-args>";
        const CLOSE: &str = "</command-args>";
        let start = s.find(OPEN)? + OPEN.len();
        let end = s[start..].find(CLOSE).map_or(s.len(), |rel| start + rel);
        let args = s[start..end].trim();
        if args.is_empty() {
            None
        } else {
            Some(normalize_line(args))
        }
    }

    /// The invoked slash command's name (e.g. `/csift`) from the wrapper's
    /// `<command-name>…</command-name>` tag — EITHER tag order. `None` when the record
    /// is not a slash-command wrapper or the tag is absent/empty. Same codepoint-safety
    /// as [`Record::slash_command_args`] (slices only on ASCII tag offsets).
    #[must_use]
    pub fn slash_command_name(&self) -> Option<String> {
        let content = self.message.as_ref()?.content.as_ref()?;
        let Content::Text(s) = content else {
            return None;
        };
        if !is_slash_command_wrapper(s) {
            return None;
        }
        const OPEN: &str = "<command-name>";
        const CLOSE: &str = "</command-name>";
        let start = s.find(OPEN)? + OPEN.len();
        let end = s[start..].find(CLOSE).map_or(s.len(), |rel| start + rel);
        let name = s[start..end].trim();
        (!name.is_empty()).then(|| name.to_string())
    }

    /// Classify this record as a MACHINE-INJECTED automation trigger, if it is one.
    ///
    /// A `<task-notification>` record is a `type:"user"`, non-`isMeta`, STRING-content
    /// record CC inserts when a background command / spawned task / workflow completes. It
    /// passes every [`Record::is_genuine_user`] gate (so it opens a turn like a human
    /// message), but it is an automation pulse — surfacing its raw `<task-id>` /
    /// `<output-file>` / `<status>` XML as "user prose" is noise. This parser extracts the
    /// stable inner tags so a surface can render `[workflow <task-id> completed] <summary>`
    /// instead. Returns `None` for any non-`<task-notification>` record.
    ///
    /// CODEPOINT-SAFE: uses `str::find` on the ASCII tag bounds and slices only on those
    /// ASCII offsets, so a CJK summary body is never split mid-codepoint.
    #[must_use]
    pub fn automation_trigger(&self) -> Option<AutomationTrigger> {
        let content = self.message.as_ref()?.content.as_ref()?;
        let Content::Text(s) = content else {
            return None;
        };
        if !s.starts_with(TASK_NOTIFICATION_PREFIX) {
            return None;
        }
        let task_id = extract_xml_tag(s, "task-id");
        let status = extract_xml_tag(s, "status");
        let summary = extract_xml_tag(s, "summary");
        // Monitor-class pulses carry their real outcome in `<event>` (e.g.
        // `STAGE2_OUTPUT_READY`, `[Monitor timed out — re-arm if needed.]`) and frequently have
        // NO `<status>` tag, so the label must read the event rather than defaulting status to
        // a fabricated `completed`.
        let event = extract_xml_tag(s, "event");
        let kind = AutomationKind::from_summary(summary.as_deref());
        Some(AutomationTrigger {
            kind,
            task_id,
            status,
            summary,
            event,
        })
    }

    /// The one-line ATTRIBUTION label for an automation-trigger opener, or `None` when this
    /// record is not one: `[<kind> <task-id> <status>] <summary>` where `<kind>` is the TRUE
    /// trigger class parsed from the summary's leading classifier (`background-command` /
    /// `workflow` / `agent` / fallback `task`) — NOT the hardcoded literal `workflow` that
    /// mislabeled 81% of triggers on a captured session (85 background-command + 2 agent). A missing
    /// field is elided gracefully. This is what `turns` / `search` render as the segment
    /// opener in place of the raw `<task-notification>` XML blob.
    #[must_use]
    pub fn automation_label(&self) -> Option<String> {
        let content = self.message.as_ref()?.content.as_ref()?;
        let Content::Text(s) = content else {
            return None;
        };
        if !s.starts_with(TASK_NOTIFICATION_PREFIX) {
            return None;
        }
        Some(automation_label_for_section(s))
    }

    /// Plain-text rendering of a GENUINE user message for the `list`/`search`
    /// excerpt: the raw string, or the concatenation of all `text` blocks. Returns
    /// `None` for non-user / non-genuine records. Whitespace-normalized to a single
    /// line (callers truncate explicitly — never silently).
    #[must_use]
    pub fn genuine_user_text(&self) -> Option<String> {
        if !self.is_genuine_user() {
            return None;
        }
        let content = self.message.as_ref()?.content.as_ref()?;
        Some(flatten_content_text(content))
    }

    /// The content blocks of this record's message, if it has an array body.
    /// Returns `None` for string content or no message.
    #[must_use]
    pub fn blocks(&self) -> Option<&[Block]> {
        match self.message.as_ref()?.content.as_ref()? {
            Content::Blocks(blocks) => Some(blocks),
            Content::Text(_) => None,
        }
    }

    /// True when this is an AUQ-answer carrier: a `type:"user"` record carrying a
    /// `tool_result` block whose textual content is a synthesized AUQ-answer string
    /// (any known marker, §4.4 — both `"User has answered your questions: …"` and
    /// `"Your questions have been answered: …"`). Such a record is surfaced under the
    /// `user` category even though it rides on a carrier.
    #[must_use]
    pub fn is_auq_answer(&self) -> bool {
        if !self.is_type("user") {
            return false;
        }
        let Some(blocks) = self.blocks() else {
            return false;
        };
        blocks.iter().any(|b| match b {
            Block::ToolResult { content, .. } => content
                .as_ref()
                .map(tool_result_content_text)
                .is_some_and(|t| is_auq_answer_text(&t)),
            _ => false,
        })
    }

    /// True when this record is an ANSWERED AskUserQuestion carrier that should open a
    /// turn (§4.4 / §6.4): a `type:"user"` record carrying a `tool_result` block whose
    /// `is_error` is not true, AND it is a real answer — signalled by a non-empty
    /// `toolUseResult.answers` object (the clean, structured source) OR, as a fallback
    /// for an older record without `toolUseResult`, the synthesized AUQ-answer marker in
    /// the tool_result content. The answer is a genuine USER message (the user's
    /// selection + prose reasoning), so it is a turn boundary.
    ///
    /// A CANCELLED / rejected / validation-errored AUQ (no `answers`, `is_error:true`,
    /// or a `Cancelled…` / `<tool_use_error>…` body) is NOT a boundary — those carry no
    /// typed user message. Verified on real data: all 81 answered carriers have
    /// non-empty `toolUseResult.answers`, the marker string, and `is_error` false; the
    /// rejection/cancel carriers have none of the three.
    #[must_use]
    pub fn is_auq_answer_boundary(&self) -> bool {
        if !self.is_type("user") {
            return false;
        }
        // The carrier must ride on a non-errored tool_result block.
        let Some(blocks) = self.blocks() else {
            return false;
        };
        let mut has_non_errored_tool_result = false;
        for b in blocks {
            if let Block::ToolResult { is_error, .. } = b {
                if is_error.unwrap_or(false) {
                    return false; // an errored AUQ result (cancel/reject) is never a boundary
                }
                has_non_errored_tool_result = true;
            }
        }
        if !has_non_errored_tool_result {
            return false;
        }
        // Primary signal: structured, non-empty `toolUseResult.answers`.
        if self.has_auq_answers() {
            return true;
        }
        // Fallback (older records without `toolUseResult`): the synthesized marker.
        self.is_auq_answer()
    }

    /// Parse the raw `toolUseResult` blob into a full `Value` tree ON DEMAND — for the
    /// few DEEP consumers (`recover`'s event extraction, the AUQ exchange
    /// reconstruction). Each call re-parses, so a caller needing several reads parses
    /// once and shares the local value. `None` when absent or unparseable (the raw text
    /// was validated as part of the line's JSON, so unparseable never happens in
    /// practice — the guard is tolerance, not control flow).
    #[must_use]
    pub fn tool_use_result_value(&self) -> Option<serde_json::Value> {
        let raw = self.tool_use_result.as_ref()?;
        serde_json::from_str(raw.get()).ok()
    }

    /// Parse the raw `attachment` blob into a full `Value` tree ON DEMAND (`recover`'s
    /// external-edit/file-snapshot reader, `plan`'s `plan_mode` binding reader).
    #[must_use]
    pub fn attachment_value(&self) -> Option<serde_json::Value> {
        let raw = self.attachment.as_ref()?;
        serde_json::from_str(raw.get()).ok()
    }

    /// Hook-injected `additionalContext` text: a `type:"attachment"` record whose payload is
    /// `{"type":"hook_additional_context","content":[…],…}` — the context a SessionStart /
    /// UserPromptSubmit / … hook injected into the turn. `content` is a string ARRAY in real
    /// data (one element per injected block; joined with `\n`), tolerated as a bare string.
    /// `None` for every other record shape. Cheap for non-attachment records (one type
    /// compare before any parse).
    #[must_use]
    pub fn hook_additional_context_text(&self) -> Option<String> {
        if !self.is_type("attachment") {
            return None;
        }
        let v = self.attachment_value()?;
        let att = v.as_object()?;
        if att.get("type").and_then(serde_json::Value::as_str) != Some("hook_additional_context") {
            return None;
        }
        match att.get("content")? {
            serde_json::Value::String(s) => {
                let s = s.trim();
                (!s.is_empty()).then(|| s.to_string())
            }
            serde_json::Value::Array(parts) => {
                let texts: Vec<&str> = parts.iter().filter_map(serde_json::Value::as_str).collect();
                (!texts.is_empty()).then(|| texts.join("\n"))
            }
            _ => None,
        }
    }

    /// Cheap typed probe of the SMALL `toolUseResult` fields the hot paths consult —
    /// deserializing it skips the huge content values (file bodies, stdout echoes)
    /// without allocating them. `None` when there is no `toolUseResult` or it is not a
    /// JSON object (e.g. a subagent's bare-string echo) — exactly the cases where every
    /// former `.get(…)` probe answered `None` too.
    fn tur_probe(&self) -> Option<TurProbe> {
        let raw = self.tool_use_result.as_ref()?;
        serde_json::from_str(raw.get()).ok()
    }

    /// The structured `toolUseResult.answers` object test (§4.4): present AND non-empty.
    /// `false` for a cancelled/rejected AUQ (no answers) or a non-AUQ carrier.
    fn has_auq_answers(&self) -> bool {
        self.tur_probe()
            .as_ref()
            .and_then(|p| p.answers.as_ref())
            .and_then(serde_json::Value::as_object)
            .is_some_and(|m| !m.is_empty())
    }

    /// Reconstruct the COMPLETE AskUserQuestion exchange (§4.4) as one genuine-user unit:
    /// `[AskUserQuestion · N questions]` followed by, per question, the header, the
    /// question, each option WITH its description (supplementary note), the user's answer, and any
    /// free-text `annotations.notes` attached to that answer (the `"(notes only)"` path,
    /// where the user's real message lives — never dropped). Built from the structured
    /// `toolUseResult.questions[]` zipped with `toolUseResult.answers{}` + `.annotations{}`;
    /// falls back to the synthesized `tool_result` string (parsed for `"<q>"="<a>"`) when
    /// `toolUseResult` is absent. Returns `None` when this is not an answered AUQ carrier.
    ///
    /// CODEPOINT-SAFE: works entirely on owned `String`/`&str` values pulled structurally
    /// from JSON; the only excerpting is whitespace normalization, never a byte-offset
    /// slice into a (possibly CJK) question/answer body.
    #[must_use]
    pub fn auq_exchange(&self) -> Option<String> {
        if !self.is_auq_answer_boundary() {
            return None;
        }
        // Structured path: questions[] (ordered) zipped with answers{question -> answer}.
        // Parse the raw blob ONCE here (this runs only on an actual answered-AUQ carrier)
        // and read answers/annotations/questions from the shared local tree.
        let tur = self.tool_use_result_value();
        let answers = tur
            .as_ref()
            .and_then(|t| t.get("answers"))
            .and_then(serde_json::Value::as_object)
            .filter(|m| !m.is_empty());
        if let Some(answers) = answers {
            let questions = tur
                .as_ref()
                .and_then(|t| t.get("questions"))
                .and_then(serde_json::Value::as_array);
            // `annotations` map (§4.4) — per-question `{notes?, preview?}`; when the answer
            // is the `"(notes only)"` placeholder the user's ENTIRE real message lives here.
            let annotations = tur
                .as_ref()
                .and_then(|t| t.get("annotations"))
                .and_then(serde_json::Value::as_object)
                .filter(|m| !m.is_empty());
            let mut out = String::new();
            let n = questions.map_or(answers.len(), Vec::len);
            out.push_str(&format!("[AskUserQuestion · {n} question{}]", plural(n)));
            if let Some(qs) = questions {
                for (i, q) in qs.iter().enumerate() {
                    let header = q.get("header").and_then(serde_json::Value::as_str);
                    let question = q
                        .get("question")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    // Each option is a (label, description) pair — BOTH surfaced; the
                    // description (supplementary note) is the per-option detail the user wants kept,
                    // and is what was being dropped (only the label survived).
                    let opts: Vec<(String, Option<String>)> = q
                        .get("options")
                        .and_then(serde_json::Value::as_array)
                        .map(|os| {
                            os.iter()
                                .filter_map(|o| {
                                    let label =
                                        o.get("label").and_then(serde_json::Value::as_str)?;
                                    let desc = o
                                        .get("description")
                                        .and_then(serde_json::Value::as_str)
                                        .filter(|s| !s.is_empty())
                                        .map(str::to_string);
                                    Some((label.to_string(), desc))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    // The answer is keyed by the (verbatim) question string in `answers`.
                    let answer = answers
                        .get(question)
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    // Free-text notes the user attached to THIS answer. When the answer is
                    // the `"(notes only)"` placeholder, the notes ARE the user's message —
                    // dropping them silently swallowed the whole turn (the common path,
                    // since the user routinely answers AUQs with typed prose, not a click).
                    let note = annotations
                        .and_then(|a| a.get(question))
                        .and_then(|v| v.get("notes"))
                        .and_then(serde_json::Value::as_str)
                        .filter(|s| !s.is_empty());
                    out.push_str(&format!("\nQ{} ", i + 1));
                    if let Some(h) = header {
                        out.push_str(&format!("({}): ", normalize_line(h)));
                    }
                    out.push_str(&normalize_line(question));
                    for (label, desc) in &opts {
                        out.push_str(&format!("\n  - {}", normalize_line(label)));
                        if let Some(d) = desc {
                            out.push_str(&format!(": {}", normalize_line(d)));
                        }
                    }
                    out.push_str(&format!("\nA{}: {}", i + 1, normalize_line(answer)));
                    if let Some(n) = note {
                        out.push_str(&format!("\n   note: {}", normalize_line(n)));
                    }
                }
            } else {
                // No questions[] array (rare): list the answers map directly, still
                // surfacing any notes attached to each answer.
                for (i, (q, a)) in answers.iter().enumerate() {
                    out.push_str(&format!(
                        "\nQ{}: {}\nA{}: {}",
                        i + 1,
                        normalize_line(q),
                        i + 1,
                        normalize_line(a.as_str().unwrap_or_default())
                    ));
                    if let Some(n) = annotations
                        .and_then(|an| an.get(q))
                        .and_then(|v| v.get("notes"))
                        .and_then(serde_json::Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        out.push_str(&format!("\n   note: {}", normalize_line(n)));
                    }
                }
            }
            return Some(out);
        }
        // Fallback path: the synthesized marker string is the whole exchange.
        self.auq_answer_marker_text()
            .map(|t| format!("[AskUserQuestion] {}", normalize_line(&t)))
    }

    /// The synthesized AUQ-answer string from this carrier's `tool_result` (§4.4) — the
    /// fallback content when `toolUseResult.answers` is absent. `None` if no AUQ marker.
    fn auq_answer_marker_text(&self) -> Option<String> {
        let blocks = self.blocks()?;
        for b in blocks {
            if let Block::ToolResult {
                content: Some(c), ..
            } = b
            {
                let t = tool_result_content_text(c);
                if is_auq_answer_text(&t) {
                    return Some(t);
                }
            }
        }
        None
    }

    /// When this record is a tool-use REJECTION carrying a typed user instruction
    /// (§4.2.4), return `(rejected_tool_use_id, user_message)`. The genuine user message
    /// is everything AFTER the fixed [`PLAN_REJECTION_USER_PREFIX`] delimiter.
    ///
    /// `None` when this is not a rejection, or it is a rejection WITHOUT a typed message
    /// (the `STOP what you are doing and wait…` form — the user clicked reject but typed
    /// nothing, so there is no user turn).
    ///
    /// CODEPOINT-SAFE: the tail is taken with `str::split_once` on the ASCII delimiter
    /// (UTF-8-safe, never a byte-offset slice); the tail (often CJK) is returned whole.
    #[must_use]
    pub fn plan_rejection_message(&self) -> Option<(Option<String>, String)> {
        if !self.is_type("user") {
            return None;
        }
        let blocks = self.blocks()?;
        for b in blocks {
            if let Block::ToolResult {
                tool_use_id,
                content: Some(c),
                is_error,
            } = b
            {
                if !is_error.unwrap_or(false) {
                    continue;
                }
                let text = tool_result_content_text(c);
                if !text.contains(PLAN_REJECTION_MARKER) {
                    continue;
                }
                // The typed instruction is everything after the fixed ASCII delimiter.
                if let Some((_, tail)) = text.split_once(PLAN_REJECTION_USER_PREFIX) {
                    let msg = tail.trim();
                    if !msg.is_empty() {
                        return Some((tool_use_id.clone(), msg.to_string()));
                    }
                }
            }
        }
        None
    }

    /// True when this record is a tool-use rejection carrying a typed user instruction
    /// (§4.2.4) and so should open a turn. A rejection without a typed message is NOT a
    /// boundary (see [`Record::plan_rejection_message`]).
    #[must_use]
    pub fn is_plan_rejection_boundary(&self) -> bool {
        self.plan_rejection_message().is_some()
    }

    /// The single boundary predicate (§6.4): this record opens a new turn iff it is a
    /// genuine human message, an ANSWERED AskUserQuestion (the answer is the user's
    /// message), a tool-use rejection carrying a typed user instruction, OR an inbound
    /// teammate/peer message. Every surface (turns / search / recover / files) keys turn
    /// delimiting on THIS predicate so they never drift.
    ///
    /// GOLD §1 + FINDING-2: an inbound PEER message (`<teammate-message>` OR `<agent-message>`) is
    /// no longer [`Record::is_genuine_user`] (it is a peer, not the operator), but it MUST still
    /// delimit a turn — so the dedicated [`Record::is_peer_message_record`] clause keeps `opens_turn`
    /// firing for peer records (true before and after the fix for the non-isMeta teammate/agent
    /// forms), leaving turn grouping byte-identical where peers already opened turns while the `user`
    /// mislabel is removed.
    #[must_use]
    pub fn opens_turn(&self) -> bool {
        self.is_genuine_user()
            || self.is_auq_answer_boundary()
            || self.is_plan_rejection_boundary()
            || self.is_peer_message_record()
    }

    /// The rendered genuine-user text for any boundary-opening record, normalized to a
    /// single line — the unified opener body used by `turns` / `search` / `list` /
    /// `recover`:
    /// - a plain genuine user → its text (same as [`Record::genuine_user_text`]);
    /// - an answered AskUserQuestion → the full Q+options+answer unit
    ///   ([`Record::auq_exchange`]);
    /// - a tool-use rejection-with-message → the user's typed instruction, optionally
    ///   suffixed with a `[plan: <path>]` pointer when `plan_index` resolves the rejected
    ///   `tool_use_id` to an ExitPlanMode plan (§4.2.4). `plan_index` may be `None` (no
    ///   plan resolution attempted), in which case the rejection text is returned alone.
    ///
    /// Returns `None` when this record does not open a turn. CODEPOINT-SAFE throughout
    /// (delegates to the codepoint-safe accessors).
    #[must_use]
    pub fn reconstructed_user_text(&self, plan_index: Option<&PlanIndex>) -> Option<String> {
        if let Some(text) = self.genuine_user_text() {
            return Some(text);
        }
        if let Some(unit) = self.auq_exchange() {
            return Some(normalize_line(&unit));
        }
        if let Some((rejected_id, msg)) = self.plan_rejection_message() {
            let mut out = normalize_line(&msg);
            if let (Some(idx), Some(id)) = (plan_index, rejected_id.as_deref()) {
                if let Some(path) = idx.plan_path(id) {
                    out.push_str(&format!(" [plan: {path}]"));
                }
            }
            return Some(out);
        }
        // A slash-command wrapper is NOT a turn boundary (§4.2.3), but when the user
        // typed prose after the command (`/compact <prose>`) that prose IS genuine user
        // input — surface it as `/name args` so `search -t user` still finds it within
        // its turn and the wrapper XML never masquerades as prose. Prefilter/gate note:
        // both the name and the args are VERBATIM raw-line substrings, and the seam
        // between them is a space (a whitespace-bearing pattern is never
        // prefilter-eligible), so no synth needle is needed for this render.
        if let Some(args) = self.slash_command_args() {
            return Some(match self.slash_command_name() {
                Some(name) => format!("{name} {args}"),
                None => args,
            });
        }
        // GOLD §1: an inbound TEAMMATE message opens a turn but is NOT genuine-user, so the
        // genuine-user arm above no longer yields its body. Render the message text here so a
        // teammate-opened turn is not BLANK — preserving the exact text `turns`/`search`/`list`
        // produced before the `is_genuine_user` fix. This stays TEAMMATE-specific on purpose: the
        // `<agent-message>` peer form (FINDING-2) opens a turn too, but every surface that renders an
        // opener body catches it FIRST via [`Record::inbound_comm_preview`] (`turns`/`list`) or
        // `record_text_sections` (`search`), and `list` deliberately keeps an `<agent-message>`
        // INELIGIBLE to front a preview (session.rs `preview_text`) — so widening this arm would only
        // change that decision, never prevent a blank.
        if self.is_teammate_message_record() {
            if let Some(content) = self.message.as_ref().and_then(|m| m.content.as_ref()) {
                return Some(flatten_content_text(content));
            }
        }
        None
    }

    /// The ExitPlanMode tool_use blocks carried by this (assistant) record, as
    /// `(tool_use_id, plan_file_path)` pairs — the raw material a [`PlanIndex`] is built
    /// from. `plan_file_path` prefers `input.planFilePath`; a block with no path yields
    /// an empty string (still indexed so the id is known to be an ExitPlanMode). Empty
    /// for any record carrying no ExitPlanMode tool_use.
    #[must_use]
    pub fn exit_plan_pointers(&self) -> Vec<(String, String)> {
        let Some(blocks) = self.blocks() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for b in blocks {
            if let Block::ToolUse {
                id: Some(id),
                name: Some(name),
                input,
            } = b
            {
                if name == "ExitPlanMode" {
                    let path = input
                        .as_ref()
                        .and_then(|v| v.get("planFilePath"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    out.push((id.clone(), path));
                }
            }
        }
        out
    }

    /// The persisted-output file path for this carrier (§4.6), preferring the
    /// structured `toolUseResult.persistedOutputPath` (exact — no regex) and falling
    /// back to scraping the inline `Full output saved to: <path>` marker from a
    /// `tool_result` block. Returns `None` when there is no persisted pointer.
    #[must_use]
    pub fn persisted_output_path(&self) -> Option<String> {
        // Structured field first (SPEC §4.6 resolution rule).
        if let Some(probe) = self.tur_probe() {
            if let Some(p) = probe
                .persisted_output_path
                .as_ref()
                .and_then(serde_json::Value::as_str)
            {
                if !p.is_empty() {
                    return Some(p.to_string());
                }
            }
        }
        // Inline fallback: scan tool_result content for the marker.
        let blocks = self.blocks()?;
        for b in blocks {
            if let Block::ToolResult {
                content: Some(c), ..
            } = b
            {
                let text = tool_result_content_text(c);
                if let Some(p) = scrape_persisted_path(&text) {
                    return Some(p);
                }
            }
        }
        None
    }

    /// Plain-text rendering of the assistant's VISIBLE end-of-turn message — the
    /// concatenation of its `text` blocks (`thinking`/`tool_use` excluded). Returns
    /// `None` unless this is an `assistant` record carrying at least one non-empty
    /// `text` block. This is the "last agent message" target for `list`.
    #[must_use]
    pub fn agent_text(&self) -> Option<String> {
        if !self.is_type("assistant") {
            return None;
        }
        let content = self.message.as_ref()?.content.as_ref()?;
        let Content::Blocks(blocks) = content else {
            // Assistant content is always a block array in CC 2.1.x; a bare string
            // would be a genuine surprise — surface it rather than silently drop.
            if let Content::Text(s) = content {
                let t = normalize_line(s);
                return if t.is_empty() { None } else { Some(t) };
            }
            return None;
        };
        let mut parts: Vec<&str> = Vec::new();
        for b in blocks {
            if let Block::Text { text } = b {
                if !text.trim().is_empty() {
                    parts.push(text);
                }
            }
        }
        if parts.is_empty() {
            return None;
        }
        Some(normalize_line(&parts.join(" ")))
    }
}

/// A file-mutating operation kind, keyed off the tool that performed it. The
/// structured tools (`Write`/`Edit`/`MultiEdit`/`NotebookEdit`) are AUTHORITATIVE —
/// they name an exact `file_path`/`notebook_path`. `BashMutation` is HEURISTIC: it is
/// parsed lexically from a Bash command string (see [`crate::bash_mutations`]), which
/// cannot be a true shell parse, so it is labelled heuristic everywhere it surfaces.
///
/// Write/Edit/NotebookEdit/MultiEdit are kept DISTINCT (not collapsed to "mutation")
/// because the acid-test question — "how many files did it create vs edit" — needs
/// create-vs-edit discrimination, and the per-op counts are a stated output of
/// `csift files --by-file`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOp {
    /// `Write` tool — writes a file whole (a create when the path was new).
    Write,
    /// `Edit` tool — a single in-place string replacement in an existing file.
    Edit,
    /// `NotebookEdit` tool — edits a Jupyter notebook cell (`notebook_path`).
    NotebookEdit,
    /// `MultiEdit` tool — multiple edits to one file in a single call.
    MultiEdit,
    /// A file mutation inferred HEURISTICALLY from a Bash command string.
    BashMutation,
}

impl FileOp {
    /// Stable lowercase label used in CLI output + JSON (mirrors `SubagentKind::label`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            FileOp::Write => "write",
            FileOp::Edit => "edit",
            FileOp::NotebookEdit => "notebook-edit",
            FileOp::MultiEdit => "multi-edit",
            FileOp::BashMutation => "bash",
        }
    }

    /// The JSON-idiomatic token (UNDERSCORE-delimited) for the `--timeline` `op` field, so
    /// the per-mutation `op` value spells a multi-word op the SAME way the grouped
    /// (`--by-file`/`--by-dir`/`--summary`) per-op COUNT keys do (`notebook_edit`,
    /// `multi_edit`). [`label`] keeps the hyphenated form for human-readable TEXT output;
    /// this method is the on-wire spelling so a script normalizing across the two `files`
    /// JSON modes never special-cases the delimiter. Single-word ops coincide either way.
    #[must_use]
    pub fn json_key(self) -> &'static str {
        match self {
            FileOp::Write => "write",
            FileOp::Edit => "edit",
            FileOp::NotebookEdit => "notebook_edit",
            FileOp::MultiEdit => "multi_edit",
            FileOp::BashMutation => "bash",
        }
    }

    /// True only for [`FileOp::BashMutation`] — drives the explicit "heuristic"
    /// labelling in `files` output (Bash mutations are a best-effort lexical parse,
    /// never authoritative).
    #[must_use]
    pub fn is_heuristic(self) -> bool {
        matches!(self, FileOp::BashMutation)
    }
}

/// One extracted file-mutation fact, pure per-record (the turn index is assigned by
/// the `files` module during turn reconstruction, NOT stored here). The `path` is the
/// absolute path exactly as written in the record — never re-encoded or absolutized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMutation {
    /// The path as written in the record (NOT re-encoded / absolutized).
    pub path: String,
    pub op: FileOp,
    /// Raw ISO8601 UTC timestamp from the tool_use record, if present.
    pub timestamp_utc: Option<String>,
    /// `true` when the paired carrier reported `toolUseResult.type == "create"` (a new
    /// file). On a bare tool_use record the carrier field is usually absent, so this
    /// defaults `false` ("unknown / treat as edit"); the joiner in `files` enriches it.
    pub is_create: bool,
}

impl Record {
    /// The NON-HEURISTIC file mutations carried by this record's structured tool_use
    /// blocks (`Write`/`Edit`/`MultiEdit` → `input.file_path`; `NotebookEdit` →
    /// `input.notebook_path`). One [`FileMutation`] per qualifying block.
    ///
    /// MODELLING NOTE: in real data the `file_path` lives on the **tool_use** record
    /// while `toolUseResult.type` (`create`/`update`) lives on the **paired
    /// tool_result carrier**. This function extracts only what is locally present, so
    /// `is_create` here is consulted from THIS record's own `toolUseResult` first (it
    /// is usually absent on a tool_use record, defaulting `is_create` to `false` —
    /// honestly "unknown / treat as edit"); the `files` module (Section 3) joins the
    /// two sides by `tool_use_id` within a turn via [`Record::carrier_create_paths`]
    /// so `is_create` becomes accurate. Keeping this per-record-pure mirrors how
    /// `search` treats a turn as the join unit.
    ///
    /// Blocks whose path is absent/empty are skipped (a defensive arm, tested).
    #[must_use]
    pub fn structured_tool_mutations(&self) -> Vec<FileMutation> {
        let Some(blocks) = self.blocks() else {
            return Vec::new();
        };
        // This record's own carrier `type` (usually absent on a tool_use record).
        let self_is_create = self
            .tur_probe()
            .as_ref()
            .and_then(|p| p.r#type.as_ref())
            .and_then(serde_json::Value::as_str)
            == Some("create");

        let mut out = Vec::new();
        for block in blocks {
            let Block::ToolUse { name, input, .. } = block else {
                continue;
            };
            let Some(name) = name.as_deref() else {
                continue;
            };
            let (op, key) = match name {
                "Write" => (FileOp::Write, "file_path"),
                "Edit" => (FileOp::Edit, "file_path"),
                "MultiEdit" => (FileOp::MultiEdit, "file_path"),
                "NotebookEdit" => (FileOp::NotebookEdit, "notebook_path"),
                _ => continue,
            };
            let path = input
                .as_ref()
                .and_then(|v| v.get(key))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if path.is_empty() {
                continue; // defensive: a structured tool_use with no/empty path.
            }
            out.push(FileMutation {
                path: path.to_string(),
                op,
                timestamp_utc: self.timestamp.clone(),
                is_create: self_is_create,
            });
        }
        out
    }

    /// The carrier side of a file mutation: when this record's `toolUseResult` is an
    /// object carrying a `filePath`, return `(tool_use_id, filePath, is_create)` for
    /// each `tool_result` block, so the `files` joiner can set `is_create` on the
    /// matching structured mutation (and fall back to this `filePath` if the
    /// tool_use's own path was somehow absent).
    ///
    /// `toolUseResult.type` ∈ {`create`, `update`, `file_unchanged`, `text`, `image`};
    /// only `create` ⇒ `is_create = true`, everything else ⇒ `false`. When there is no
    /// `toolUseResult`, it is not an object, or it has no `filePath`, the result is
    /// empty (defensive arms, each tested).
    #[must_use]
    pub fn carrier_create_paths(&self) -> Vec<(String, String, bool)> {
        let Some(probe) = self.tur_probe() else {
            return Vec::new();
        };
        let Some(file_path) = probe.file_path.as_ref().and_then(serde_json::Value::as_str) else {
            return Vec::new();
        };
        if file_path.is_empty() {
            return Vec::new();
        }
        let is_create = probe.r#type.as_ref().and_then(serde_json::Value::as_str) == Some("create");

        // The carrier rides on a `tool_result` block whose `tool_use_id` joins it back
        // to the structured tool_use. Emit one tuple per tool_result block id found.
        let mut out = Vec::new();
        if let Some(blocks) = self.blocks() {
            for block in blocks {
                if let Block::ToolResult {
                    tool_use_id: Some(id),
                    ..
                } = block
                {
                    out.push((id.clone(), file_path.to_string(), is_create));
                }
            }
        }
        out
    }

    /// The Bash command string for a `Block::ToolUse { name: "Bash", .. }`
    /// (`input.command`). Returns `None` for any other record / a Bash tool_use with
    /// no command. Feeds the heuristic parser in [`crate::bash_mutations`].
    #[must_use]
    pub fn bash_command(&self) -> Option<&str> {
        let blocks = self.blocks()?;
        for block in blocks {
            if let Block::ToolUse { name, input, .. } = block {
                if name.as_deref() == Some("Bash") {
                    if let Some(cmd) = input
                        .as_ref()
                        .and_then(|v| v.get("command"))
                        .and_then(serde_json::Value::as_str)
                    {
                        return Some(cmd);
                    }
                }
            }
        }
        None
    }
}

/// An index of ExitPlanMode `tool_use_id → planFilePath` built from a session's records
/// (§4.2.4). A tool-use rejection-with-message ([`Record::plan_rejection_message`])
/// resolves the rejected `tool_use_id` through this index to surface a `[plan: <path>]`
/// pointer so a consuming LLM can go Read the plan. Built once per session via
/// [`PlanIndex::from_records`]; cheap (one `BTreeMap` of the few ExitPlanMode calls).
#[derive(Debug, Clone, Default)]
pub struct PlanIndex {
    by_id: std::collections::BTreeMap<String, String>,
}

impl PlanIndex {
    /// Build the index from a session's records: every ExitPlanMode tool_use's
    /// `id → planFilePath` (see [`Record::exit_plan_pointers`]). A block with no
    /// `planFilePath` is skipped (an empty path is not a useful pointer).
    #[must_use]
    pub fn from_records<'a, I>(records: I) -> Self
    where
        I: IntoIterator<Item = &'a Record>,
    {
        let mut by_id = std::collections::BTreeMap::new();
        for rec in records {
            for (id, path) in rec.exit_plan_pointers() {
                if !path.is_empty() {
                    by_id.insert(id, path);
                }
            }
        }
        Self { by_id }
    }

    /// The plan file path an ExitPlanMode tool_use with `id` pointed to, if known.
    #[must_use]
    pub fn plan_path(&self, id: &str) -> Option<&str> {
        self.by_id.get(id).map(String::as_str)
    }
}

/// `"s"` for plural counts, `""` for exactly one — for the `N question(s)` label.
fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Group records (in file order) into TURNS, returning one `Vec<usize>` of record
/// indices per turn — the outer index IS the 0-based turn index (genuine-user order).
///
/// The single source of truth for turn delimiting (§6.4), shared by `search`'s
/// exchange reconstruction and `files`'s mutation attribution so the two never drift:
///
/// - A turn opens on a boundary record (`is_genuine`); every record after it, up to the
///   next boundary, belongs to that turn (a non-boundary `tool_result`-carrier, an
///   `isMeta` pseudo-turn, and a compaction summary are turn MEMBERS, never delimiters).
/// - Records before the first boundary (rare: leading tool noise) seed turn 0 so they
///   are never lost. When such a synthetic lead exists AND a real user turn follows, the
///   lead is folded into the first real turn so indices stay 0-based on boundary
///   openers. With NO boundary at all, the orphans are a standalone turn 0.
///
/// `is_genuine` is a closure (rather than calling [`Record::opens_turn`] directly) only
/// so callers can test the grouping over lightweight bool fixtures; in production it is
/// always [`Record::opens_turn`] — which opens on a genuine human message, an answered
/// AskUserQuestion (the answer is the user's message, §4.4), OR a tool-use
/// rejection-with-message (§4.2.4). An AUQ answer / plan rejection becoming a turn
/// boundary is the sanctioned correct behavior change (a previously-MISSED genuine user
/// message); interrupts / `<local-command-stdout>` / `<command-name>` wrappers, formerly
/// spurious boundaries, are excluded by `is_genuine_user` (regression fixes).
///
/// NOTE: this raw grouper trusts file order and does NOT suppress superseded drafts. The
/// production surfaces use [`group_turn_indices_deduped`], which additionally drops the
/// abandoned-draft openers an esc-cancel / edit-resend leaves behind (§6.4.1). This bare
/// form stays for the lightweight bool-fixture tests and any caller that has no `Record`.
// Production now routes through `group_turn_indices_deduped`, so in the bin build this bare
// generic is reached only from `#[cfg(test)]` — kept as the documented base primitive +
// bool-fixture test entry (same retained-shape rationale as the `#[allow(dead_code)]` on
// `Record`).
#[allow(dead_code)]
#[must_use]
pub fn group_turn_indices<T>(records: &[T], is_genuine: impl Fn(&T) -> bool) -> Vec<Vec<usize>> {
    group_turn_indices_core(records, is_genuine, &std::collections::HashSet::new())
}

/// Indices of turn-opening records that are SUPERSEDED DRAFTS — an earlier sibling of a
/// later turn-opener sharing the SAME non-null `parentUuid` (§6.4.1). This is the on-disk
/// shape of the "type a message, ESC-cancel / edit, resend" loop (and any rewind that
/// re-opens a turn from the same point): Claude Code appends every draft as its own
/// `type:"user"` record, yet only ONE — the last in file order — was actually delivered to
/// the model. The earlier siblings are abandoned drafts.
///
/// WHY last-in-file is the survivor (verified on real `~/.claude/projects` data): distinct
/// real turns never share a `parentUuid` (each user turn is parented to the assistant
/// message that preceded it), so same-parent openers are ALWAYS alternative versions of one
/// logical turn; and across the corpus the last sibling's subtree is the one that reaches
/// furthest toward the leaf (the live branch). A content-similarity heuristic would miss the
/// common case where the user *prepended/inserted* text on the edit (`look…` → `take a closer look…`),
/// so the parent-uuid identity — not text — is the load-bearing signal.
///
/// `rec` projects each element to its `Record` (works for `&Record`, `Record`, and the
/// search `Kept` wrapper alike). Records with a null/empty `parentUuid` are NEVER grouped
/// (grouping on "no parent" would merge unrelated first-message drafts); in real data a
/// genuine user always carries a parent, so this costs nothing.
///
/// HONEST BOUND: only the superseded OPENER is reported, not the downstream of a branch
/// abandoned AFTER it already drew replies (rewind-after-response). Those rare descendants
/// (≤2% of turns on the measured corpus) keep their own distinct parents and survive; fully
/// pruning them needs an active-leaf walk, which a compaction boundary severs — so we do not
/// risk silently dropping a live turn to chase them.
#[must_use]
pub fn superseded_draft_indices<T>(
    records: &[T],
    rec: impl Fn(&T) -> &Record,
) -> std::collections::HashSet<usize> {
    let mut latest: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut superseded: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for (i, item) in records.iter().enumerate() {
        let r = rec(item);
        if !r.opens_turn() {
            continue;
        }
        let Some(parent) = r.parent_uuid.as_deref() else {
            continue; // null parent: never grouped (would merge unrelated records)
        };
        if parent.is_empty() {
            continue;
        }
        // Keep the LAST opener per parent: when a new sibling appears, the previously-seen
        // one for that parent becomes a superseded draft.
        if let Some(prev) = latest.insert(parent, i) {
            superseded.insert(prev);
        }
    }
    superseded
}

/// [`group_turn_indices`] with esc-cancel / edit-resend DRAFT SUPPRESSION (§6.4.1): a
/// superseded draft ([`superseded_draft_indices`]) is dropped ENTIRELY — it neither opens a
/// turn nor folds in as a member — so a message the user edited away before sending can
/// never resurface as a phantom turn (nor leak its abandoned text into a neighbour). This is
/// the delimiter every session-operating surface (`turns` / `search` / `files` / `recover`)
/// uses, so they stay byte-consistent on what counts as a turn.
#[must_use]
pub fn group_turn_indices_deduped<T>(
    records: &[T],
    rec: impl Fn(&T) -> &Record,
) -> Vec<Vec<usize>> {
    let skip = superseded_draft_indices(records, |x| rec(x));
    group_turn_indices_core(records, |x| rec(x).opens_turn(), &skip)
}

/// Shared engine for [`group_turn_indices`] and [`group_turn_indices_deduped`]. Every index
/// in `skip` is omitted entirely (`continue`) — neither a turn boundary nor a member — which
/// is how superseded drafts are dropped. With an empty `skip` the behaviour is identical to
/// the original file-order grouper.
fn group_turn_indices_core<T>(
    records: &[T],
    is_genuine: impl Fn(&T) -> bool,
    skip: &std::collections::HashSet<usize>,
) -> Vec<Vec<usize>> {
    let mut turns: Vec<Vec<usize>> = Vec::new();
    let mut first_emitted: Option<usize> = None;
    for (i, rec) in records.iter().enumerate() {
        if skip.contains(&i) {
            continue; // superseded draft: invisible to turn reconstruction
        }
        if first_emitted.is_none() {
            first_emitted = Some(i);
        }
        if is_genuine(rec) {
            turns.push(vec![i]);
        } else if let Some(last) = turns.last_mut() {
            last.push(i);
        } else {
            // Pre-first-user records seed turn 0 (a standalone turn 0 if no genuine
            // user ever opens).
            turns.push(vec![i]);
        }
    }
    // If the first EMITTED (non-skipped) record is a synthetic pre-user lead AND a real user
    // turn follows, fold the lead into the first real turn so indices align with genuine-user
    // order. Basing this on the first non-skipped record keeps behaviour identical when no
    // draft is skipped (`first_emitted` is then index 0, matching `records.first()`).
    let synthetic_lead = first_emitted.is_some_and(|i| !is_genuine(&records[i]));
    if synthetic_lead && turns.len() > 1 {
        let lead = turns.remove(0);
        if let Some(first_real) = turns.first_mut() {
            let mut merged = lead;
            merged.extend(first_real.iter().copied());
            *first_real = merged;
        }
    }
    turns
}

/// Flatten a `Content` to a single normalized line of its textual parts.
/// `string` → itself; `blocks` → all `text` blocks joined (other block types,
/// which never co-occur with a genuine user `text` block, are ignored).
fn flatten_content_text(content: &Content) -> String {
    match content {
        Content::Text(s) => normalize_line(s),
        Content::Blocks(blocks) => {
            let joined = blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            normalize_line(&joined)
        }
    }
}

/// Extract the textual payload of a `tool_result` block's `content` (§4.5). The
/// content is raw `serde_json::Value`: a bare string, OR an array of
/// `{type:"text",text}` / `{type:"image"}` / `{type:"tool_reference",tool_name}`
/// objects. We concatenate every `text` field found and, for `tool_reference`,
/// surface the `tool_name` (so a regex like `ToolSearch` still matches). Anything
/// else (images, unknown shapes) contributes nothing. Whitespace is NOT normalized
/// here — callers that excerpt do their own normalization; matchers want the raw
/// text. Returns an owned `String` (possibly empty).
#[must_use]
pub fn tool_result_content_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => {
            let mut parts: Vec<String> = Vec::new();
            for item in items {
                if let Some(t) = item.get("text").and_then(serde_json::Value::as_str) {
                    parts.push(t.to_string());
                } else if let Some(name) = item.get("tool_name").and_then(serde_json::Value::as_str)
                {
                    parts.push(name.to_string());
                }
            }
            parts.join("\n")
        }
        // Object/number/bool/null: render compactly so a regex can still match
        // structured payloads that aren't the common string/array shapes.
        other => other.to_string(),
    }
}

/// Scrape the inline persisted-output pointer (§4.6 fallback): the line
/// `Full output saved to: <ABSOLUTE_PATH>` inside a `<persisted-output>` block.
/// Returns the trimmed path, or `None` if the marker is absent.
fn scrape_persisted_path(text: &str) -> Option<String> {
    const MARKER: &str = "Full output saved to:";
    let idx = text.find(MARKER)?;
    let rest = &text[idx + MARKER.len()..];
    // The path runs to end-of-line.
    let line_end = rest.find('\n').unwrap_or(rest.len());
    let path = rest[..line_end].trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// The TRUE class of a `<task-notification>` automation trigger, parsed from the leading
/// classifier of its `<summary>` (verified against real sessions: the summary opens with
/// `Background command "…"`, `Dynamic workflow "…"`, or `Agent …`). This is the attribution
/// the P2 turn-segmentation lens demands — the old code hardcoded the literal `workflow` for
/// EVERY trigger, mislabeling background-command + agent pulses (81% on a captured session).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationKind {
    /// A `Background command "…"` completion pulse (a `&`-detached shell command CC ran).
    BackgroundCommand,
    /// A `Dynamic workflow "…"` completion pulse (an OMC / dynamic workflow run).
    Workflow,
    /// An `Agent …` completion pulse (a spawned subagent).
    Agent,
    /// A monitor / cron cadence COMPLETION pulse. Matches a `<task-notification>` whose summary
    /// EITHER opens `Monitor`/`scheduled`/`cron` (the captured-monitor shape: `Monitor event: …`)
    /// OR opens `Background command "…"` with a monitor-cadence token in the quoted command NAME
    /// (the captured-monitor shape: `Relaunch monitor timer (cycle N)`, `Re-arm corrected monitor …`,
    /// `nightly monitor tick (25min)`). The captured session's monitor loop is implemented as `&`-detached
    /// background commands, so without the quoted-name scan it ALL read as `background-command`
    /// and this class matched zero of it. NOTE: this still matches only `<task-notification>`
    /// pulses — the `ScheduleWakeup` wakeup-tick PROMPTS that drive a monitor/cron cadence are
    /// `isMeta:true` user records (not `<task-notification>`s) and are NOT segmented here (they
    /// bypass [`Record::automation_trigger`] entirely via the isMeta gate in
    /// [`Record::is_genuine_user`]); attributing those is a deferred enhancement.
    Monitor,
    /// Any other / unrecognized classifier — the safe fallback (renders `task`).
    Task,
}

impl AutomationKind {
    /// Classify from the `<summary>`. Case-insensitive on the known leading prefixes; anything
    /// else (or a missing summary) is [`AutomationKind::Task`]. The `monitor`/`scheduled`/`cron`
    /// LEADING prefixes route a monitor-COMPLETION `<task-notification>` to
    /// [`AutomationKind::Monitor`] (the captured-monitor `Monitor event:` shape). ADDITIONALLY, a
    /// `Background command "…"` pulse whose QUOTED NAME carries a monitor-cadence token
    /// (`monitor`/`re-arm`/`relaunch monitor`/`liveness`) routes to `Monitor` too — the
    /// captured-monitor shape, where the monitor loop is a `&`-detached background command (a pure
    /// leading-prefix check disguised ALL of it as `background-command`). This does NOT cover
    /// `ScheduleWakeup` wakeup-tick prompts (isMeta records that never reach this classifier).
    #[must_use]
    pub fn from_summary(summary: Option<&str>) -> Self {
        let s = summary.unwrap_or("").trim_start();
        // The classifiers are a fixed leading phrase; match the longest-distinguishing
        // prefix case-insensitively so a `Background command "…"` is not mistaken for `task`.
        let lower = s.to_ascii_lowercase();
        if lower.starts_with("background command") {
            // A monitor/cron cadence is FREQUENTLY implemented as a `&`-detached background
            // command whose QUOTED NAME is the monitor mechanism (`Background command
            // "Relaunch monitor timer (cycle 2)"` / `"Re-arm corrected monitor …"` /
            // `"nightly monitor tick (25min)"`). The leading classifier is `Background command`, so
            // a pure prefix check buried EVERY such pulse under `background-command` and the
            // `Monitor` class matched zero of them on a captured session. Route to `Monitor` when
            // the quoted command NAME carries a monitor-cadence token, so the dominant monitor
            // activity is attributed to its own class instead of disguised as generic bg-cmd.
            if quoted_name_is_monitor_cadence(s) {
                AutomationKind::Monitor
            } else {
                AutomationKind::BackgroundCommand
            }
        } else if lower.starts_with("dynamic workflow") || lower.starts_with("workflow") {
            AutomationKind::Workflow
        } else if lower.starts_with("monitor")
            || lower.starts_with("scheduled")
            || lower.starts_with("cron")
        {
            AutomationKind::Monitor
        } else if lower.starts_with("agent") {
            AutomationKind::Agent
        } else {
            AutomationKind::Task
        }
    }

    /// The stable lowercase slug rendered in the `[<kind> <id> <status>]` label.
    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            AutomationKind::BackgroundCommand => "background-command",
            AutomationKind::Workflow => "workflow",
            AutomationKind::Agent => "agent",
            AutomationKind::Monitor => "monitor",
            AutomationKind::Task => "task",
        }
    }
}

/// True when a `Background command "<name>" …` summary's QUOTED command name names a
/// monitor / cron cadence — so the pulse is attributed to [`AutomationKind::Monitor`] rather
/// than the generic [`AutomationKind::BackgroundCommand`]. Extracts the substring between the
/// FIRST pair of double quotes (the command name) and matches a conservative set of
/// monitor-cadence tokens against it (case-insensitive): the standalone word `monitor`, or
/// `re-arm`, `relaunch monitor`, `liveness`. The match is restricted to the quoted NAME (never
/// the whole summary) so a background command that merely mentions "monitor" in trailing prose
/// is not over-captured; absent quotes, nothing matches (stays `BackgroundCommand`). Tokens
/// chosen to be strongly monitor-specific — `tick`/`cadence` alone are too broad and excluded.
fn quoted_name_is_monitor_cadence(summary: &str) -> bool {
    let Some(open) = summary.find('"') else {
        return false;
    };
    let rest = &summary[open + 1..];
    let Some(close) = rest.find('"') else {
        return false;
    };
    let name = rest[..close].to_ascii_lowercase();
    // The standalone word `monitor` (not a substring of a larger word) is the dominant signal;
    // `re-arm` / `relaunch monitor` / `liveness` cover the re-arming-loop names.
    name.split(|c: char| !c.is_alphanumeric())
        .any(|w| w == "monitor" || w == "liveness")
        || name.contains("re-arm")
        || name.contains("relaunch monitor")
}

/// A parsed `<task-notification>` automation trigger — the stable inner tags of a
/// machine-injected background-command / workflow / spawned-task completion notice. Every
/// field is `Option` because a malformed / partial notification must degrade gracefully
/// (the label still renders with `?`/`completed` fallbacks) rather than be dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationTrigger {
    /// The TRUE trigger class (parsed from the `<summary>` classifier) — the attribution the
    /// label renders, replacing the prior hardcoded `workflow`.
    pub kind: AutomationKind,
    /// The `<task-id>` (the workflow / background-command id), if present.
    pub task_id: Option<String>,
    /// The `<status>` (`completed` / `failed` / …), if present.
    pub status: Option<String>,
    /// The `<summary>` (the human-readable "what completed" line), if present.
    pub summary: Option<String>,
    /// The `<event>` payload, if present — where a Monitor / ScheduleWakeup pulse carries its
    /// real outcome (`STAGE2_OUTPUT_READY`, `[Monitor timed out — re-arm if needed.]`). Often
    /// the only outcome signal on a Monitor pulse (which usually has no `<status>`), so the
    /// label falls back to it instead of fabricating `completed`.
    pub event: Option<String>,
}

/// Extract the text between `<tag>` and `</tag>` in `s`, trimmed, or `None` when the tag
/// is absent or empty. Codepoint-safe: `str::find` returns ASCII byte offsets of the
/// (ASCII) tag delimiters, and the slice is taken on those offsets only — never inside the
/// (possibly CJK) body. A missing close tag yields `None` (never a runaway slice).
fn extract_xml_tag(s: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = s.find(&open)? + open.len();
    let end_rel = s[start..].find(&close)?;
    let inner = s[start..start + end_rel].trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
    }
}

/// Collapse all runs of ASCII whitespace (incl. newlines/tabs) to single spaces
/// and trim the ends, so an excerpt renders on one line. Does NOT truncate —
/// length capping with an explicit `… (+N chars)` marker is the caller's job.
pub(crate) fn normalize_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_ws && !out.is_empty() {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    // Trim a possible trailing space from the run-collapse above.
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

// ============================================================================
// role.class.sub classification engine (GOLD plan §2–§6) — ADDITIVE, P1.
//
// This is the NEW taxonomy core, testable in isolation. It is NOT yet wired into any
// consumer (the legacy `cli::Category` + `-t` selector still drive output); P2 cuts the
// surfaces over to [`Record::classify`] and removes the old enum. Until then the new
// items carry a targeted `#[allow(dead_code)]` (the binary never calls them yet).
//
// GOLD GAPS surfaced during P1 (reported upstream, not silently absorbed):
//   - `harness.schedule.wakeup`: the FIRED autonomous-loop / `ScheduleWakeup` timer tick is
//     detected via its fixed markers ([`SCHEDULE_WAKEUP_MARKER`] sentinel +
//     [`SCHEDULE_WAKEUP_LOOP_CHECK_PREFIX`] / [`SCHEDULE_WAKEUP_TIMER_MARKER`], P1c M2a). A
//     GENERIC cron/monitor tick's injected prompt is still operator-authored free text with no
//     universal marker; such an isMeta tick that matches no marker is EXCLUDED (P1c M2b: an
//     isMeta record is never `user.message`), not mislabeled. The `ScheduleWakeup` *tool_use*
//     (the agent ARMING a wakeup) is classified `agent.tool.use`, not the harness tick.
//   - `agent.thinking` covers BOTH [`Block::Thinking`] and [`Block::RedactedThinking`] (the
//     encrypted/opaque thinking form). The latter is UNATTESTED in the current corpus (oracle
//     B3/G7) so it is exercised by a SYNTHETIC fixture; it carries no readable text, so the
//     render surfaces a `[redacted thinking]` placeholder while still classifying `agent.thinking`.
// ============================================================================

/// The top-level ROLE of a classified record (GOLD §2). The first dot-segment of every
/// [`Class::path`]. A multi-label record can span roles (e.g. an AUQ answer is both
/// [`Role::User`] and [`Role::Agent`]).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// The human operator.
    User,
    /// The assistant (incl. its tool I/O and peer communication).
    Agent,
    /// Claude Code machinery (notifications, compaction, slash wrappers, interrupts, schedule).
    Harness,
}

#[allow(dead_code)]
impl Role {
    /// The stable lowercase slug (the first dot-segment of a [`Class::path`]).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Agent => "agent",
            Role::Harness => "harness",
        }
    }
}

/// A LEAF class in the role.class.sub taxonomy (GOLD §2). One variant per leaf; the dotted
/// [`Class::path`] is the canonical wire/selector form and [`Class::role`] its top-level role.
/// A record carries a `Vec<Class>` (multi-label, GOLD §3) via [`Record::classify`].
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    /// `user.message` — genuine human prose (incl. slash-command `<command-args>`).
    UserMessage,
    /// `user.answer` — an AskUserQuestion answer (the Q+options+answer unit). Dual-labeled
    /// with [`Class::AgentToolResult`] (it rides on the answering tool_result carrier).
    UserAnswer,
    /// `user.rejection` — a plan/tool rejection carrying a typed instruction. Dual-labeled
    /// with [`Class::AgentToolResult`].
    UserRejection,
    /// `agent.message` — the assistant's visible end-of-turn text block(s).
    AgentMessage,
    /// `agent.thinking` — a thinking block (see the GOLD-gap note re `redacted_thinking`).
    AgentThinking,
    /// `agent.tool.use` — a tool_use block (incl. a pending elicitation sidecar marker).
    AgentToolUse,
    /// `agent.tool.result` — a tool_result block (incl. errored).
    AgentToolResult,
    /// `agent.communication.inbox` — a received peer message / spawn prompt / subagent return.
    CommInbox,
    /// `agent.communication.sent` — a sent peer message (`SendMessage`) or a spawn.
    CommSent,
    /// `agent.communication.signal` — a control/status comm (idle_notification, shutdown_*).
    CommSignal,
    /// `harness.notification.workflow` — a `<task-notification>` for a dynamic/OMC workflow.
    NotificationWorkflow,
    /// `harness.notification.monitor` — a monitor/cron cadence completion pulse.
    NotificationMonitor,
    /// `harness.notification.subagent` — a spawned-subagent completion pulse (renamed from
    /// [`AutomationKind::Agent`] so it never collides with the `agent` role).
    NotificationSubagent,
    /// `harness.notification.background-command` — a `&`-detached shell command pulse.
    NotificationBackgroundCommand,
    /// `harness.notification.task` — any other / unclassified `<task-notification>`.
    NotificationTask,
    /// `harness.compaction.summary` — the `isCompactSummary` summary record.
    CompactionSummary,
    /// `harness.compaction.boundary` — the `system`/`compact_boundary` metrics record.
    CompactionBoundary,
    /// `harness.command.invocation` — a `<command-name>…` slash-command wrapper.
    CommandInvocation,
    /// `harness.command.stdout` — a `<local-command-stdout>…` local-command output.
    CommandStdout,
    /// `harness.interrupt.user` — `[Request interrupted by user]`.
    InterruptUser,
    /// `harness.interrupt.tool` — `[Request interrupted by user for tool use]`.
    InterruptTool,
    /// `harness.schedule.wakeup` — a fired `ScheduleWakeup` TIMER tick (its injected
    /// [`SCHEDULE_WAKEUP_MARKER`] prompt). Distinct from [`Class::MetaLoop`] (the
    /// autonomous-loop driver prose); the timer is the harness scheduler firing.
    ScheduleWakeup,
    /// `harness.schedule.continuation` — a `Continue from where you left off.` resume tick.
    ScheduleContinuation,
    /// `harness.meta.hook` — hook-injected feedback (stop-hook / `<local-command-caveat>` /
    /// edit-failed-retry), not the operator.
    MetaHook,
    /// `harness.meta.loop` — an autonomous-loop driver tick (`# Autonomous loop tick` /
    /// `Run the autonomous check`).
    MetaLoop,
}

#[allow(dead_code)]
impl Class {
    /// Every leaf [`Class`] in taxonomy order (GOLD §2). The single source of truth for
    /// enumerating the class space — P2 builds the `-t` selector table from it, and tests
    /// assert `path()`/`role()` exhaustively over it (a new variant added to the enum but not
    /// here is caught by the `all_classes_cover_the_enum` test). Order: user, agent (+comm),
    /// harness (notification, compaction, command, interrupt, schedule, meta).
    pub const ALL: &'static [Class] = &[
        Class::UserMessage,
        Class::UserAnswer,
        Class::UserRejection,
        Class::AgentMessage,
        Class::AgentThinking,
        Class::AgentToolUse,
        Class::AgentToolResult,
        Class::CommInbox,
        Class::CommSent,
        Class::CommSignal,
        Class::NotificationWorkflow,
        Class::NotificationMonitor,
        Class::NotificationSubagent,
        Class::NotificationBackgroundCommand,
        Class::NotificationTask,
        Class::CompactionSummary,
        Class::CompactionBoundary,
        Class::CommandInvocation,
        Class::CommandStdout,
        Class::InterruptUser,
        Class::InterruptTool,
        Class::ScheduleWakeup,
        Class::ScheduleContinuation,
        Class::MetaHook,
        Class::MetaLoop,
    ];

    /// The canonical dotted path (GOLD §2) — the `-t` selector form (P2) and render label.
    #[must_use]
    pub fn path(self) -> &'static str {
        match self {
            Class::UserMessage => "user.message",
            Class::UserAnswer => "user.answer",
            Class::UserRejection => "user.rejection",
            Class::AgentMessage => "agent.message",
            Class::AgentThinking => "agent.thinking",
            Class::AgentToolUse => "agent.tool.use",
            Class::AgentToolResult => "agent.tool.result",
            Class::CommInbox => "agent.communication.inbox",
            Class::CommSent => "agent.communication.sent",
            Class::CommSignal => "agent.communication.signal",
            Class::NotificationWorkflow => "harness.notification.workflow",
            Class::NotificationMonitor => "harness.notification.monitor",
            Class::NotificationSubagent => "harness.notification.subagent",
            Class::NotificationBackgroundCommand => "harness.notification.background-command",
            Class::NotificationTask => "harness.notification.task",
            Class::CompactionSummary => "harness.compaction.summary",
            Class::CompactionBoundary => "harness.compaction.boundary",
            Class::CommandInvocation => "harness.command.invocation",
            Class::CommandStdout => "harness.command.stdout",
            Class::InterruptUser => "harness.interrupt.user",
            Class::InterruptTool => "harness.interrupt.tool",
            Class::ScheduleWakeup => "harness.schedule.wakeup",
            Class::ScheduleContinuation => "harness.schedule.continuation",
            Class::MetaHook => "harness.meta.hook",
            Class::MetaLoop => "harness.meta.loop",
        }
    }

    /// The top-level role (the first dot-segment of [`Class::path`]). Exhaustive (no
    /// wildcard) so a future leaf forces an explicit role decision at compile time.
    #[must_use]
    pub fn role(self) -> Role {
        match self {
            Class::UserMessage | Class::UserAnswer | Class::UserRejection => Role::User,
            Class::AgentMessage
            | Class::AgentThinking
            | Class::AgentToolUse
            | Class::AgentToolResult
            | Class::CommInbox
            | Class::CommSent
            | Class::CommSignal => Role::Agent,
            Class::NotificationWorkflow
            | Class::NotificationMonitor
            | Class::NotificationSubagent
            | Class::NotificationBackgroundCommand
            | Class::NotificationTask
            | Class::CompactionSummary
            | Class::CompactionBoundary
            | Class::CommandInvocation
            | Class::CommandStdout
            | Class::InterruptUser
            | Class::InterruptTool
            | Class::ScheduleWakeup
            | Class::ScheduleContinuation
            | Class::MetaHook
            | Class::MetaLoop => Role::Harness,
        }
    }
}

/// A parsed inbound `<teammate-message …>` (GOLD §4/§5): the `teammate_id` attribute (the
/// comm FROM) and, when the body is a `{"type":"<sig>"}` JSON payload, the control-signal
/// type (e.g. `idle_notification`). A prose message has `signal_type == None`.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeammateMessage {
    /// The `teammate_id` attribute = the SENDER (the FROM of the comm direction).
    pub teammate_id: Option<String>,
    /// The control-signal type when the body is `{"type":"<sig>"}` (idle_notification /
    /// shutdown_request / teammate_terminated / shutdown_approved / …); `None` for prose.
    pub signal_type: Option<String>,
}

#[allow(dead_code)]
impl TeammateMessage {
    /// True when this is a control SIGNAL (a `{"type":…}` payload), not a prose message →
    /// classifies `agent.communication.signal` rather than `…inbox`.
    #[must_use]
    pub fn is_signal(&self) -> bool {
        self.signal_type.is_some()
    }
}

/// True when `content` carries a `open` section tag at a valid section BOUNDARY (FINDING-1) — the
/// non-allocating predicate behind [`is_teammate_message`] / [`is_agent_message`]. Returns on the
/// FIRST [`is_section_boundary`] occurrence; a tag that only ever appears MID-PROSE yields `false`.
/// The common no-tag case costs exactly one `memmem` (the `find` returns `None` immediately), so
/// the hot path (`is_genuine_user` on every user record) is not regressed.
fn has_boundary_section(content: &str, open: &str) -> bool {
    let mut idx = 0;
    while let Some(rel) = content[idx..].find(open) {
        let start = idx + rel;
        if is_section_boundary(&content[..start]) {
            return true;
        }
        idx = start + open.len();
    }
    false
}

/// True when `content` is an inbound teammate/peer message (GOLD §5) — it carries a
/// [`TEAMMATE_MESSAGE_OPEN`] tag at a section BOUNDARY (FINDING-1). Real data (edge-fixtures scout):
/// the real shape is ALWAYS the relayed wrapper `Another Claude session sent a message:\n
/// <teammate-message …>\n<BODY>\n</teammate-message>\n\n<security footer>` (126 of 126), so the
/// boundary is the content start, just after the relay preamble, or right after a prior section's
/// close tag. A tag merely QUOTED mid-prose (a genuine user message that mentions the literal tag —
/// common in csift's OWN dev sessions) is NOT a boundary, so the record stays `user.message` rather
/// than being mislabeled `agent.communication.inbox` (the FINDING-1 fix).
#[allow(dead_code)]
#[must_use]
pub fn is_teammate_message(content: &str) -> bool {
    has_boundary_section(content, TEAMMATE_MESSAGE_OPEN)
}

/// True when `content` is an inbound `<agent-message from="…">` peer message (P1c M1 / FINDING-2) at
/// a section BOUNDARY — the DISTINCT peer form from [`is_teammate_message`]. Like a teammate message
/// it classifies `agent.communication.inbox`, is excluded from [`Record::is_genuine_user`], yet
/// still opens a turn. Boundary-anchored (FINDING-1) for the same reason — a quoted tag is not it.
#[allow(dead_code)]
#[must_use]
pub fn is_agent_message(content: &str) -> bool {
    has_boundary_section(content, AGENT_MESSAGE_OPEN)
}

/// True when `content` is ANY inbound peer message — a `<teammate-message>` OR an `<agent-message>`
/// at a section boundary (GOLD §1 + P1c M1 + FINDING-2). Both are PEER-agent messages, never the
/// operator: excluded from [`Record::is_genuine_user`] yet still turn-opening ([`Record::opens_turn`]).
#[must_use]
pub fn is_peer_message(content: &str) -> bool {
    is_teammate_message(content) || is_agent_message(content)
}

/// Parse an inbound teammate/peer message (GOLD §5) into its `teammate_id` + optional signal
/// type. `None` when `content` is not a teammate message. CODEPOINT-SAFE: every slice is
/// taken on ASCII byte offsets returned by `str::find` (the tag/attribute delimiters), never
/// inside a (possibly CJK) message body.
#[allow(dead_code)]
#[must_use]
pub fn parse_teammate_message(content: &str) -> Option<TeammateMessage> {
    // The FIRST boundary-anchored `<teammate-message>` section (FINDING-1): reuses the
    // boundary-aware scan so a tag quoted mid-prose is never parsed as a teammate message.
    parse_all_teammate_messages(content).into_iter().next()
}

/// Parse ALL `<teammate-message …>` sections in `content` (edge-fixtures G4/G5 batching): one
/// `type:"user"` record can carry SEVERAL sections of MIXED kind (prose + idle_notification +
/// teammate_terminated + …). Returns one [`TeammateMessage`] per section, in file order — the
/// caller unions their labels. Each section's body/signal is scoped to its own close tag (so a
/// later section, the trailing security footer, and inter-section text never bleed in). Empty
/// when `content` has no tag. CODEPOINT-SAFE: ASCII-offset slicing only.
#[allow(dead_code)]
#[must_use]
pub fn parse_all_teammate_messages(content: &str) -> Vec<TeammateMessage> {
    let mut out = Vec::new();
    scan_tag_sections(
        content,
        TEAMMATE_MESSAGE_OPEN,
        TEAMMATE_MESSAGE_CLOSE,
        |_, section| {
            out.push(TeammateMessage {
                teammate_id: extract_xml_attr(section, "teammate_id"),
                signal_type: teammate_signal_type(section),
            });
        },
    );
    out
}

/// True when an open tag whose PREFIX (the text before it) is `prefix` sits at a valid SECTION
/// BOUNDARY (FINDING-1): the content start (only whitespace precedes), immediately after the
/// relayed peer preamble ([`PEER_MESSAGE_PREAMBLE`]), or right after a prior section's CLOSE tag
/// (`</task-notification>` / `</teammate-message>` / `</agent-message>`, modulo trailing
/// whitespace). A tag that appears MID-PROSE — a genuine user message merely QUOTING the literal
/// tag, common in csift's own dev sessions — is NOT a boundary, so it does not start a section and
/// the record stays `user.message`. Codepoint-safe: pure suffix tests on `trim_end`, no slicing.
fn is_section_boundary(prefix: &str) -> bool {
    let t = prefix.trim_end();
    t.is_empty()
        || t.ends_with(PEER_MESSAGE_PREAMBLE)
        || t.ends_with(TASK_NOTIFICATION_CLOSE)
        || t.ends_with(TEAMMATE_MESSAGE_CLOSE)
        || t.ends_with(AGENT_MESSAGE_CLOSE)
}

/// Invoke `emit(offset, section)` for each BOUNDARY-anchored `<open …>…</close>` section in
/// `content`, in file order. `offset` is the byte offset of the section's open tag; `section` is the
/// slice from the open tag through (inclusive) its close tag — or to end-of-string if the close tag
/// is absent (malformed). Only an open tag at an [`is_section_boundary`] starts a section
/// (FINDING-1); a tag quoted mid-prose is skipped (advance past it and keep scanning for a later
/// boundary-anchored one). The scan advances past each section's close tag (or, if absent, to end so
/// it always terminates). Shared by the teammate / agent-message / task-notification section scans so
/// they never drift. CODEPOINT-SAFE: ASCII-offset slicing only (`str::find` on the tags).
fn scan_tag_sections<F: FnMut(usize, &str)>(content: &str, open: &str, close: &str, mut emit: F) {
    let mut idx = 0;
    while let Some(rel) = content[idx..].find(open) {
        let start = idx + rel;
        if !is_section_boundary(&content[..start]) {
            // A tag QUOTED mid-prose — not a section start; step past it and keep scanning.
            idx = start + open.len();
            continue;
        }
        let after = &content[start..];
        let end_rel = after.find(close).map_or(after.len(), |c| c + close.len());
        emit(start, &after[..end_rel]);
        idx = start + end_rel;
    }
}

/// One inbound peer-message section located in a `type:"user"` record's text (GOLD §5 + P1c M1):
/// a `<teammate-message …>` OR the distinct `<agent-message from="…">` peer form. Carries the
/// SENDER id (the comm FROM), whether the body is a control SIGNAL (a teammate `{"type":…}`
/// payload — an `<agent-message>` is always prose → inbox), and the byte OFFSET of its open tag
/// (so a batched scan can MASK a peer tag quoted inside a `<task-notification>` span).
#[derive(Debug, Clone, PartialEq, Eq)]
struct PeerSection {
    from: Option<String>,
    is_signal: bool,
    offset: usize,
    /// The raw section slice (open tag through close tag) — the per-section render body (GOLD
    /// §3 G4/G5). Excludes the relay preamble and the trailing security footer (both fall
    /// OUTSIDE the tag span), so a batched record renders each peer message on its own.
    text: String,
}

/// Scan ALL inbound peer-message sections (`<teammate-message>` AND `<agent-message>`) in
/// `content`, returned in file (offset) order (P1c M1 + GOLD §5 batching). Empty when `content`
/// has no peer tag. CODEPOINT-SAFE: ASCII-offset slicing only.
fn parse_all_peer_sections(content: &str) -> Vec<PeerSection> {
    let mut out: Vec<PeerSection> = Vec::new();
    scan_tag_sections(
        content,
        TEAMMATE_MESSAGE_OPEN,
        TEAMMATE_MESSAGE_CLOSE,
        |offset, section| {
            out.push(PeerSection {
                from: extract_xml_attr(section, "teammate_id"),
                is_signal: teammate_signal_type(section).is_some(),
                offset,
                text: section.to_string(),
            });
        },
    );
    scan_tag_sections(
        content,
        AGENT_MESSAGE_OPEN,
        AGENT_MESSAGE_CLOSE,
        |offset, section| {
            out.push(PeerSection {
                from: extract_xml_attr(section, "from"),
                is_signal: false,
                offset,
                text: section.to_string(),
            });
        },
    );
    out.sort_by_key(|p| p.offset);
    out
}

/// The inner BODY of a single peer-message section slice (`<teammate-message …>BODY</teammate-message>`
/// or `<agent-message …>BODY</agent-message>`): the prose between the open tag's `>` and the close
/// tag, trimmed — the wrapper tags stripped so a render shows only the peer's own words (the trailing
/// harness security footer already sits OUTSIDE the section slice). Falls back to the whole slice when
/// the tag bounds are absent (malformed). Codepoint-safe: ASCII-offset slicing only.
fn peer_section_body(section: &str) -> &str {
    let body = match section.find('>') {
        Some(i) => &section[i + 1..],
        None => section,
    };
    body.strip_suffix("</teammate-message>")
        .or_else(|| body.strip_suffix("</agent-message>"))
        .unwrap_or(body)
        .trim()
}

/// Extract a `name="value"` attribute's value from the start of an XML-ish tag, trimmed
/// (empty → `None`). Codepoint-safe: ASCII-offset `find` only.
fn extract_xml_attr(s: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = s.find(&needle)? + needle.len();
    let rel = s[start..].find('"')?;
    let val = s[start..start + rel].trim();
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

/// The control-signal `type` of a `<teammate-message …>{"type":"<sig>"}</teammate-message>`
/// body (GOLD §5), or `None` for a prose body. Extracts the body between the opening tag's
/// `>` and `</teammate-message>`, and — only when it is a JSON object — reads its `type`.
fn teammate_signal_type(after_open: &str) -> Option<String> {
    let gt = after_open.find('>')?;
    let body_start = gt + 1;
    const CLOSE: &str = "</teammate-message>";
    let body_end = after_open[body_start..]
        .find(CLOSE)
        .map_or(after_open.len(), |rel| body_start + rel);
    let body = after_open[body_start..body_end].trim();
    if !body.starts_with('{') {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("type")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// True for a spawn tool (GOLD §5): `Task` / `Agent` / `Workflow` — a tool_use that spawns a
/// subagent (the `self ⇨ child` comm). Kept local so `model.rs` stays dependency-free.
fn is_spawn_tool_name(name: &str) -> bool {
    matches!(name, "Task" | "Agent" | "Workflow")
}

/// True when a `SendMessage` `input` is a control SIGNAL rather than a prose message (GOLD
/// §3): the top-level `type` (or a nested `message.type`) is present and is NOT `message`/
/// `direct` (e.g. `shutdown_request`/`shutdown_response`/…). Absent type ⇒ a plain message.
fn send_message_is_signal(input: Option<&serde_json::Value>) -> bool {
    let Some(input) = input else {
        return false;
    };
    let type_at = |v: &serde_json::Value| {
        v.get("type")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    if let Some(t) = type_at(input) {
        return !matches!(t.as_str(), "message" | "direct");
    }
    if let Some(t) = input.get("message").and_then(type_at) {
        return !matches!(t.as_str(), "message" | "direct");
    }
    false
}

/// The recipient id of a `SendMessage` (the comm TO) — `input.to` preferred, else
/// `input.recipient`. `None` when neither is a non-empty string.
fn send_message_recipient(input: Option<&serde_json::Value>) -> Option<String> {
    let input = input?;
    for key in ["to", "recipient"] {
        if let Some(v) = input.get(key).and_then(serde_json::Value::as_str) {
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// The spawn target NAME of a spawn tool_use (`input.name` for a teammate/named spawn, else
/// `input.subagent_type`) — used to resolve the spawned child id (the comm TO). `None` when
/// neither is present.
fn spawn_target_name(input: Option<&serde_json::Value>) -> Option<String> {
    let input = input?;
    for key in ["name", "subagent_type"] {
        if let Some(v) = input.get(key).and_then(serde_json::Value::as_str) {
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Map a parsed `<task-notification>` [`AutomationKind`] to its `harness.notification.*`
/// [`Class`] (GOLD §2 — `Agent` becomes `subagent` to avoid the `agent` role collision).
fn notification_class(kind: AutomationKind) -> Class {
    match kind {
        AutomationKind::BackgroundCommand => Class::NotificationBackgroundCommand,
        AutomationKind::Workflow => Class::NotificationWorkflow,
        AutomationKind::Agent => Class::NotificationSubagent,
        AutomationKind::Monitor => Class::NotificationMonitor,
        AutomationKind::Task => Class::NotificationTask,
    }
}

/// The token a `<task-notification>` carrying the background agent's REAL report embeds
/// (edge-fixtures G1): a `<result>` tag. A notification WITHOUT it is a bare launch-ack pulse.
const NOTIFICATION_RESULT_TAG: &str = "<result>";

/// Build the `[<kind> <id> <status>] <summary>` attribution label for ONE
/// `<task-notification>…</task-notification>` section string. Shared by
/// [`Record::automation_label`] (whole-record = the single section) and the batched per-section
/// render ([`Record::record_text_sections`]) so the two never drift. The status slot prefers the
/// explicit `<status>`; absent (the common Monitor/ScheduleWakeup case), the real outcome lives in
/// `<event>` so render THAT rather than fabricating `completed`; only when BOTH are missing do we
/// fall back to `completed`. A missing field is elided gracefully.
fn automation_label_for_section(section: &str) -> String {
    let task_id = extract_xml_tag(section, "task-id");
    let status = extract_xml_tag(section, "status");
    let summary = extract_xml_tag(section, "summary");
    let event = extract_xml_tag(section, "event");
    let kind = AutomationKind::from_summary(summary.as_deref());
    let id = task_id.as_deref().unwrap_or("?");
    let event_norm = event
        .as_deref()
        .filter(|e| !e.is_empty())
        .map(normalize_line);
    let status = status
        .as_deref()
        .map(str::to_string)
        .or(event_norm)
        .unwrap_or_else(|| "completed".to_string());
    let head = format!("[{} {id} {status}]", kind.slug());
    match summary.as_deref() {
        Some(sum) if !sum.is_empty() => format!("{head} {}", normalize_line(sum)),
        _ => head,
    }
}

/// Classify ALL batched sections of a `type:"user"` record's raw text (edge-fixtures G4/G5 +
/// P1c M1/M3): scan for BOTH `<task-notification>` automation pulse(s) AND inbound peer
/// message(s) (`<teammate-message>` / `<agent-message>`), unioning every section's labels
/// (deduped, first-seen order). Each notification contributes its `harness.notification.<kind>`,
/// plus `agent.communication.inbox` when it carries a `<result>` (the G1 child ⇨ parent
/// dual-label); each peer section contributes `agent.communication.{inbox,signal}`.
///
/// PRECEDENCE (M3a): notification spans are matched FIRST and a peer tag whose open falls INSIDE
/// any notification span (e.g. a `<result>` body that merely QUOTES "<teammate-message") is
/// IGNORED — so a notification never leaks a spurious comm label. CROSS-FAMILY (M3b): a record
/// carrying a real notification section AND a real peer section (outside any notification span)
/// unions both families' labels.
///
/// Returns `true` iff ≥1 section matched (the caller's classification is then complete); `false`
/// leaves the record to the plain marker/prose classifier.
fn classify_batched_sections(raw: &str, out: &mut Vec<Class>) -> bool {
    let mut matched = false;
    // (a) <task-notification> sections — classify each, recording its byte span to mask the
    //     peer scan against tags quoted inside it.
    let mut notif_spans: Vec<(usize, usize)> = Vec::new();
    scan_tag_sections(
        raw,
        TASK_NOTIFICATION_PREFIX,
        TASK_NOTIFICATION_CLOSE,
        |offset, section| {
            let kind = AutomationKind::from_summary(extract_xml_tag(section, "summary").as_deref());
            push_unique(out, notification_class(kind));
            if section.contains(NOTIFICATION_RESULT_TAG) {
                push_unique(out, Class::CommInbox);
            }
            notif_spans.push((offset, offset + section.len()));
            matched = true;
        },
    );
    // (b) inbound peer sections OUTSIDE every notification span (precedence + cross-family).
    for peer in parse_all_peer_sections(raw) {
        if notif_spans
            .iter()
            .any(|&(s, e)| peer.offset >= s && peer.offset < e)
        {
            continue;
        }
        push_unique(
            out,
            if peer.is_signal {
                Class::CommSignal
            } else {
                Class::CommInbox
            },
        );
        matched = true;
    }
    matched
}

/// One renderable record-level text SECTION of a (possibly batched) `type:"user"` record (GOLD
/// §3 G4/G5 per-section render): its leaf [`Class`], the display text to excerpt, and the comm
/// `from ⇨ to` direction for a communication leaf. Built by [`Record::record_text_sections`] so a
/// record batching several `<task-notification>` / inbound-peer sections of MIXED kind renders ONE
/// hit PER section (each with its own label + direction) rather than collapsing to one.
#[derive(Debug, Clone)]
pub struct RecordTextSection {
    /// The leaf class for THIS section (a `harness.notification.*` / `agent.communication.*`).
    pub class: Class,
    /// The display text to match + excerpt (a per-section automation label, the `<result>` report
    /// body, or the raw peer-message section slice).
    pub text: String,
    /// `from ⇨ to` for a communication leaf (GOLD §4); `None` for a `harness.notification.*`.
    pub direction: Option<(String, String)>,
}

/// A CLEAN inbound-communication preview of a peer/teammate turn-opener, for the `turns` / `list`
/// render surfaces (the GOLD §1 inbound-comm presentation). RENDER-ONLY: it does NOT affect
/// [`Record::classify`] / [`Record::opens_turn`] — a peer opener still opens a turn and classifies
/// `agent.communication.{inbox,signal}` through the engine; this is only the human-facing render of
/// that opener so the previews no longer dump the raw `<teammate-message …>` XML blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundComm {
    /// [`Class::CommInbox`] (a prose message) or [`Class::CommSignal`] (a control payload).
    pub class: Class,
    /// The sender id (the comm FROM); the comm TO is always the transcript owner (`self`).
    pub from: String,
    /// The peer's own message body — the `<teammate-message …>` / `<agent-message …>` wrapper tags
    /// AND the trailing harness security footer stripped, normalized to one line (only the prose).
    pub body: String,
}

/// Push `c` into `out` only if not already present (multi-label dedup, GOLD §3) — preserves
/// first-seen order so the richest/most-salient label leads.
fn push_unique(out: &mut Vec<Class>, c: Class) {
    if !out.contains(&c) {
        out.push(c);
    }
}

/// A read-only lookup for spawn pairing (GOLD §4/§7), supplied via [`ClassifyCtx`]. Backed in
/// P2 by the global spawn index (`subagent::ParentSpawnIndex` / `build_global_spawn_index`),
/// behind a trait so `model.rs` does not depend on `subagent.rs`. Both queries key on the
/// SAME join as the topology builder, so they resolve the spawn `self ⇨ child` direction AND
/// detect a `child ⇨ self` subagent return (the Task tool_result whose id was a spawn).
#[allow(dead_code)]
pub trait SpawnLookup {
    /// The spawned child's agent id for a spawn `tool_use_id` (the `id` of a Task/Agent/
    /// Workflow tool_use; equivalently the `tool_use_id` of its returning tool_result).
    /// `Some` ⇒ that id spawned a subagent — used for the spawn TO and the return FROM.
    fn child_for_spawn_tool_use_id(&self, tool_use_id: &str) -> Option<String>;
    /// The spawned child's agent id for a spawn by NAME / `subagent_type` (the teammate
    /// name-join, where the meta carries no `toolUseId`). The fallback when the id-join misses.
    fn child_for_spawn_name(&self, name: &str) -> Option<String>;
}

/// Cross-record context [`Record::classify`] / [`Record::direction`] need that a single record
/// cannot supply (GOLD §6). Construct with [`ClassifyCtx::top_level`] and set the relevant
/// fields. **What P2 must populate per record:**
/// - `owner_id`: the transcript owner's re-feedable id — the session uuid for a top-level
///   transcript, or the bare agent id for a subagent (the `self` of every comm direction).
/// - `owner_name`: the owner's teammate/agent NAME when known (display only; optional).
/// - `is_subagent`: whether THIS transcript lives under `subagents/`
///   (`subagent::is_subagent_path`).
/// - `parent_id`: the owning/parent session-or-agent id (the FROM of a subagent opener) —
///   `subagent::parent_session_id_from_path` / the topology `parent_agent_id`.
/// - `is_transcript_opener`: `true` ONLY for the positional FIRST turn-opener of a subagent
///   transcript (the spawn-prompt seed) — flips that genuine-user-shaped record from
///   `user.message` to `agent.communication.inbox` (parent ⇨ self). P2 sets it positionally.
/// - `spawn`: the [`SpawnLookup`] (the global spawn index) for comm direction + subagent-return
///   detection. `None` ⇒ direction degrades gracefully (spawn TO / return falls back to the
///   raw name or `?`), so the engine is fully testable without a real index.
#[allow(dead_code)]
pub struct ClassifyCtx<'a> {
    /// The transcript owner's re-feedable id (session uuid / bare agent id) = comm `self`.
    pub owner_id: Option<&'a str>,
    /// The owner's teammate/agent name, when known (display only).
    pub owner_name: Option<&'a str>,
    /// Whether THIS transcript is a subagent transcript (under `subagents/`).
    pub is_subagent: bool,
    /// The owning/parent session-or-agent id (the FROM of a subagent opener).
    pub parent_id: Option<&'a str>,
    /// `true` only for the positional first turn-opener of a subagent transcript (the seed).
    pub is_transcript_opener: bool,
    /// Spawn pairing lookup for comm direction + subagent-return detection.
    pub spawn: Option<&'a dyn SpawnLookup>,
}

#[allow(dead_code)]
impl<'a> ClassifyCtx<'a> {
    /// A bare top-level context: no owner identity, not a subagent, no spawn lookup. The
    /// neutral base for tests and for classifying a top-level transcript before P2 enriches it.
    #[must_use]
    pub fn top_level() -> Self {
        ClassifyCtx {
            owner_id: None,
            owner_name: None,
            is_subagent: false,
            parent_id: None,
            is_transcript_opener: false,
            spawn: None,
        }
    }
}

// `ClassifyCtx` holds a `&dyn SpawnLookup` (not `Debug`), so derive is impossible; render the
// lookup as a presence flag to satisfy `missing_debug_implementations`.
impl std::fmt::Debug for ClassifyCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClassifyCtx")
            .field("owner_id", &self.owner_id)
            .field("owner_name", &self.owner_name)
            .field("is_subagent", &self.is_subagent)
            .field("parent_id", &self.parent_id)
            .field("is_transcript_opener", &self.is_transcript_opener)
            .field("has_spawn_lookup", &self.spawn.is_some())
            .finish()
    }
}

#[allow(dead_code)]
impl Record {
    /// True when this is the `system`/`compact_boundary` metrics record (GOLD §5) —
    /// `harness.compaction.boundary`.
    #[must_use]
    pub fn is_compact_boundary(&self) -> bool {
        self.is_type("system") && self.subtype.as_deref() == Some("compact_boundary")
    }

    /// The parsed inbound teammate/peer message (GOLD §5) carried by this `type:"user"`
    /// record, or `None`. Reads the raw (un-normalized) message text so the peer preamble's
    /// `\n` survives. Gated to `type:"user"` (the only place a teammate message arrives).
    #[must_use]
    pub fn teammate_message(&self) -> Option<TeammateMessage> {
        if !self.is_type("user") {
            return None;
        }
        let text = self.raw_message_text()?;
        parse_teammate_message(&text)
    }

    /// True when this record is an inbound TEAMMATE message specifically (GOLD §1) — a
    /// `<teammate-message>` at a section boundary. Used by the `list`/`turns` clean-preview gate.
    #[must_use]
    pub fn is_teammate_message_record(&self) -> bool {
        self.teammate_message().is_some()
    }

    /// True when this record is ANY inbound PEER message (GOLD §1 + FINDING-2) — a
    /// `<teammate-message>` OR `<agent-message>` at a section boundary. The predicate
    /// [`Record::is_genuine_user`] EXCLUDES and [`Record::opens_turn`] INCLUDES (a peer message is
    /// not the operator, but it still delimits a turn). Reads the raw (un-normalized) message text so
    /// the relay preamble's `\n` survives; gated to `type:"user"` (the only place a peer message
    /// arrives). The body render for a peer-opened turn comes from
    /// [`Record::inbound_comm_preview`] (`turns`/`list`) / `record_text_sections` (`search`).
    #[must_use]
    pub fn is_peer_message_record(&self) -> bool {
        if !self.is_type("user") {
            return false;
        }
        match self.raw_message_text() {
            Some(text) => is_peer_message(&text),
            None => false,
        }
    }

    /// The raw textual body of this message for MARKER detection — the bare string, or text
    /// blocks joined with `\n` (NOT whitespace-normalized, so `\n`-bearing markers survive).
    /// `None` when there is no message / no text. (Distinct from [`flatten_content_text`],
    /// which normalizes whitespace for display.)
    fn raw_message_text(&self) -> Option<String> {
        let content = self.message.as_ref()?.content.as_ref()?;
        match content {
            Content::Text(s) => Some(s.clone()),
            Content::Blocks(blocks) => {
                let parts: Vec<&str> = blocks
                    .iter()
                    .filter_map(|b| match b {
                        Block::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join("\n"))
                }
            }
        }
    }

    /// True when this carrier is a TEAMMATE-SPAWN ACK, not a child return (edge-fixtures
    /// correction): a persistent teammate `Agent` spawn's tool_result is an immediate
    /// `toolUseResult.status == "teammate_spawned"` acknowledgement — the teammate's actual
    /// work returns LATER as inbound `<teammate-message>`s, never via this tool_result. So an
    /// ACK is `agent.tool.result` ONLY, never `…inbox` (unlike a one-shot Task return).
    fn is_teammate_spawn_ack(&self) -> bool {
        self.tur_probe()
            .as_ref()
            .and_then(|p| p.status.as_ref())
            .and_then(serde_json::Value::as_str)
            == Some("teammate_spawned")
    }

    /// True when this carrier is an ASYNC-LAUNCH ACK, not a child return (smoke-found bug): an
    /// ASYNC/background `Agent` spawn's tool_result is the immediate launch confirmation
    /// (`"Async agent launched successfully.\nagentId: …"`), shaped on disk as
    /// `toolUseResult.{isAsync:true, status:"async_launched"}`. It shares the spawn
    /// `tool_use_id`, so the [`SpawnLookup`] WOULD resolve it — but it is the LAUNCH ack, not
    /// the work product. The async child's real report arrives LATER via the
    /// `<task-notification>` `<result>` pulse (G1 → `agent.communication.inbox`), never via this
    /// tool_result. So a launch ack is `agent.tool.result` ONLY (unlike a SYNC one-shot Task
    /// return, which IS the child's reply → `…inbox`). Robust dual detection: the structured
    /// `toolUseResult` shape first, then the content prefix ([`ASYNC_LAUNCH_ACK_PREFIX`]) for a
    /// record lacking the structured field.
    fn is_async_launch_ack(&self) -> bool {
        if let Some(probe) = self.tur_probe() {
            if probe.status.as_ref().and_then(serde_json::Value::as_str) == Some("async_launched")
                || probe.is_async.as_ref().and_then(serde_json::Value::as_bool) == Some(true)
            {
                return true;
            }
        }
        self.blocks().is_some_and(|blocks| {
            blocks.iter().any(|b| match b {
                Block::ToolResult {
                    content: Some(c), ..
                } => tool_result_content_text(c).starts_with(ASYNC_LAUNCH_ACK_PREFIX),
                _ => false,
            })
        })
    }

    /// True when this carrier is a spawn LAUNCH ACK rather than a child RETURN — either a
    /// persistent teammate spawn ([`Record::is_teammate_spawn_ack`]) or an async/background
    /// `Agent` launch ([`Record::is_async_launch_ack`]). Both share the spawn `tool_use_id`
    /// (so the [`SpawnLookup`] would resolve them) yet are the launch confirmation, not the
    /// work product → `agent.tool.result` ONLY, never `…inbox`/a child ⇨ self direction.
    fn is_spawn_launch_ack(&self) -> bool {
        self.is_teammate_spawn_ack() || self.is_async_launch_ack()
    }

    /// True when this record is a SUBAGENT RETURN (GOLD §3) — a tool_result whose
    /// `tool_use_id` the spawn lookup resolves to a spawned child (the Task tool_result of a
    /// ONE-SHOT spawn = the child's return, child ⇨ self). `false` without a [`SpawnLookup`]
    /// in `ctx`, AND `false` for a spawn LAUNCH ACK ([`Record::is_spawn_launch_ack`] — teammate
    /// or async) — the ACK shares the spawn `tool_use_id` so the lookup WOULD resolve it, but it
    /// is not a return.
    fn is_subagent_return(&self, ctx: &ClassifyCtx) -> bool {
        let Some(spawn) = ctx.spawn else {
            return false;
        };
        if self.is_spawn_launch_ack() {
            return false;
        }
        let Some(blocks) = self.blocks() else {
            return false;
        };
        blocks.iter().any(|b| match b {
            Block::ToolResult {
                tool_use_id: Some(id),
                ..
            } => spawn.child_for_spawn_tool_use_id(id).is_some(),
            _ => false,
        })
    }

    /// Classify this record into ALL applicable leaf [`Class`]es (GOLD §3, multi-label,
    /// deduped, richest-first order). Pure + tolerant + no `unwrap` — an unmodeled record
    /// yields an empty `Vec`. Cross-record facts come from `ctx` (see [`ClassifyCtx`]).
    #[must_use]
    pub fn classify(&self, ctx: &ClassifyCtx) -> Vec<Class> {
        let mut out: Vec<Class> = Vec::new();

        // Elicitation sidecar markers (§3.10): a PENDING marker stands in for the native
        // tool_use CC has not yet written → agent.tool.use (covers the MCP `system` form too,
        // which carries no tool_use block). A RESOLVED marker is a pairing artifact → no label.
        if self.is_elicitation_marker() {
            if self.csift_phase.as_deref() == Some("pending") {
                push_unique(&mut out, Class::AgentToolUse);
            }
            return out;
        }

        // Hook-injected additionalContext (a `type:"attachment"` record): harness machinery,
        // not a message — labeled `harness.meta.hook`. Only `search --additional-context`
        // (or an explicit `show --line`/`--uuid` address) ever parses these lines, so the
        // label is unreachable elsewhere; the record never opens a turn.
        if self.hook_additional_context_text().is_some() {
            push_unique(&mut out, Class::MetaHook);
            return out;
        }

        match self.r#type.as_deref() {
            Some("system") => {
                if self.is_compact_boundary() {
                    push_unique(&mut out, Class::CompactionBoundary);
                }
            }
            Some("assistant") => self.classify_assistant(&mut out),
            Some("user") => self.classify_user(ctx, &mut out),
            _ => {}
        }
        out
    }

    /// Classify an `assistant` record's blocks (GOLD §2/§3): visible text → `agent.message`;
    /// thinking → `agent.thinking`; tool_use → `agent.tool.use` (+ `…sent`/`…signal` for a
    /// `SendMessage`/spawn); tool_result (rare on assistant) → `agent.tool.result`.
    fn classify_assistant(&self, out: &mut Vec<Class>) {
        let Some(blocks) = self.blocks() else {
            // A bare-string assistant body (rare) is a visible message (§ `agent_text`).
            if self.agent_text().is_some() {
                push_unique(out, Class::AgentMessage);
            }
            return;
        };
        for b in blocks {
            match b {
                Block::Text { text } if !text.trim().is_empty() => {
                    push_unique(out, Class::AgentMessage);
                }
                Block::Text { .. } => {}
                Block::Thinking { .. } | Block::RedactedThinking { .. } => {
                    push_unique(out, Class::AgentThinking);
                }
                Block::ToolUse { name, input, .. } => {
                    push_unique(out, Class::AgentToolUse);
                    match name.as_deref() {
                        Some("SendMessage") => {
                            if send_message_is_signal(input.as_ref()) {
                                push_unique(out, Class::CommSignal);
                            } else {
                                push_unique(out, Class::CommSent);
                            }
                        }
                        Some(n) if is_spawn_tool_name(n) => push_unique(out, Class::CommSent),
                        _ => {}
                    }
                }
                Block::ToolResult { .. } => push_unique(out, Class::AgentToolResult),
                _ => {}
            }
        }
    }

    /// Classify a `user` record (GOLD §2/§3): compaction summary (by the `isCompactSummary`
    /// FLAG, not text — G9); BATCHED mixed-family sections — `<task-notification>` pulse(s) and/or
    /// inbound peer message(s) `<teammate-message>` / `<agent-message>` (the §1 fix, the G4/G5
    /// union, and P1c M1/M3 cross-family/precedence, via [`classify_batched_sections`]); then the
    /// string-content vs block-content sub-cases.
    fn classify_user(&self, ctx: &ClassifyCtx, out: &mut Vec<Class>) {
        if self.is_compact_summary.unwrap_or(false) {
            push_unique(out, Class::CompactionSummary);
            return;
        }
        // BATCHED mixed-family sections: a `<task-notification>` automation pulse and/or an
        // inbound peer message can be concatenated in ONE record. Scan ALL sections and UNION
        // their labels, with notification precedence over a peer tag quoted inside a
        // notification span (P1c M3). When ≥1 section matches, that fully classifies the record.
        if let Some(raw) = self.raw_message_text() {
            if classify_batched_sections(&raw, out) {
                return;
            }
        }
        match self.message.as_ref().and_then(|m| m.content.as_ref()) {
            Some(Content::Text(s)) => self.classify_user_string(ctx, s, out),
            Some(Content::Blocks(blocks)) => self.classify_user_blocks(ctx, blocks, out),
            None => {}
        }
    }

    /// Classify the string body of a `user` record (also reused for the joined text of a
    /// no-tool_result block record): the harness markers (interrupts, `<local-command-stdout>`,
    /// `<command-name>`, schedule ticks, meta hook/loop), else genuine prose — or
    /// `agent.communication.inbox` when this is a subagent transcript opener (parent ⇨ self).
    /// (Batched `<task-notification>` / peer-message sections are handled UPSTREAM by
    /// [`classify_batched_sections`], so they never reach here.)
    fn classify_user_string(&self, ctx: &ClassifyCtx, s: &str, out: &mut Vec<Class>) {
        if s == INTERRUPT_MARKERS[0] {
            push_unique(out, Class::InterruptUser);
            return;
        }
        if s == INTERRUPT_MARKERS[1] {
            push_unique(out, Class::InterruptTool);
            return;
        }
        if s.starts_with(LOCAL_COMMAND_STDOUT_PREFIX) {
            push_unique(out, Class::CommandStdout);
            return;
        }
        if is_slash_command_wrapper(s) {
            // Prose typed after the slash command (`<command-args>`) IS genuine user
            // input — and the RICHER view (richest-view law: the prose beats the
            // wrapper), so it is pushed FIRST: the unfiltered record-text emission
            // renders `/name args`, never the wrapper XML. An explicit
            // `-t harness.command.invocation` still reaches the wrapper form.
            if self.slash_command_args().is_some() {
                push_unique(out, Class::UserMessage);
            }
            push_unique(out, Class::CommandInvocation);
            return;
        }
        if s.trim_start().starts_with(SCHEDULE_CONTINUATION_MARKER) {
            push_unique(out, Class::ScheduleContinuation);
            return;
        }
        // harness.schedule.wakeup: the FIRED autonomous-loop / ScheduleWakeup timer tick. Three
        // fixed markers (P1c M2a): the `<<autonomous-loop-dynamic>>` sentinel, the `# Autonomous
        // loop check` header, and the `You're being invoked on a timer` body sentence. Matched
        // BEFORE the meta.loop arm — `check` ≠ `tick`, so the loop-DRIVER prefix never collides.
        if s.contains(SCHEDULE_WAKEUP_MARKER)
            || s.starts_with(SCHEDULE_WAKEUP_LOOP_CHECK_PREFIX)
            || s.contains(SCHEDULE_WAKEUP_TIMER_MARKER)
        {
            push_unique(out, Class::ScheduleWakeup);
            return;
        }
        // harness.meta.hook (G2): hook-injected feedback — stop-hook, <local-command-caveat>,
        // or the edit-failed-retry notice. (These are isMeta records that would otherwise fall
        // through to user.message.)
        if s.starts_with(STOP_HOOK_FEEDBACK_PREFIX)
            || s.starts_with(LOCAL_COMMAND_CAVEAT_PREFIX)
            || s.contains(EDIT_RETRY_MARKER)
        {
            push_unique(out, Class::MetaHook);
            return;
        }
        // harness.meta.loop (G2): autonomous-loop DRIVER ticks (`# Autonomous loop tick` /
        // `Run the autonomous check`), distinct from the schedule.wakeup fired tick above.
        if s.starts_with(AUTONOMOUS_LOOP_TICK_PREFIX) || s.contains(AUTONOMOUS_CHECK_MARKER) {
            push_unique(out, Class::MetaLoop);
            return;
        }
        // isMeta "[Image: source:…]" pseudo-record (G2): EXCLUDED — emit no label rather than
        // mislabel it user.message.
        if s.starts_with(IMAGE_SOURCE_PREFIX) {
            return;
        }
        // The spawn-prompt seed of a subagent transcript is an inbound comm (parent ⇨ self),
        // not the operator (GOLD §3) — unchanged, regardless of isMeta.
        if ctx.is_subagent && ctx.is_transcript_opener {
            push_unique(out, Class::CommInbox);
            return;
        }
        // M2b ROOT FIX: a genuine `user.message` is NEVER isMeta. An isMeta record that matched
        // no marker above is a harness-injected pseudo-turn (a generic cron/monitor tick, a
        // novel hook wrapper), NOT the operator — emit NOTHING rather than mislabel it
        // `user.message` (the role-level isMeta gate `is_genuine_user` already applies). Only
        // genuine, non-isMeta unmarked prose is `user.message`.
        if !self.is_meta.unwrap_or(false) {
            push_unique(out, Class::UserMessage);
        }
    }

    /// Classify a block-content `user` record (GOLD §3). A tool_result carrier →
    /// `agent.tool.result`, plus the dual labels (`user.answer` for an AUQ answer,
    /// `user.rejection` for a typed rejection, `agent.communication.inbox` for a subagent
    /// return). A no-tool_result block record is routed through the string classifier on its
    /// joined text (interrupt markers / genuine prose can ride on a text block).
    fn classify_user_blocks(&self, ctx: &ClassifyCtx, blocks: &[Block], out: &mut Vec<Class>) {
        let has_tool_result = blocks.iter().any(|b| matches!(b, Block::ToolResult { .. }));
        if has_tool_result {
            // Order follows the GOLD §3 table (label A then label B): the user-facing dual
            // label (`user.answer`/`user.rejection`) leads its carrier; a subagent return
            // leads with its `agent.tool.result` base then the `…inbox` comm view. These
            // dual-label shapes are mutually exclusive, so an if/else chain is deterministic.
            if self.is_auq_answer_boundary() {
                push_unique(out, Class::UserAnswer);
                push_unique(out, Class::AgentToolResult);
            } else if self.is_plan_rejection_boundary() {
                push_unique(out, Class::UserRejection);
                push_unique(out, Class::AgentToolResult);
            } else if self.is_subagent_return(ctx) {
                push_unique(out, Class::AgentToolResult);
                push_unique(out, Class::CommInbox);
            } else {
                push_unique(out, Class::AgentToolResult);
            }
            return;
        }
        let joined = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if joined.trim().is_empty() {
            return; // image-only / empty → nothing in the taxonomy
        }
        self.classify_user_string(ctx, &joined, out);
    }

    /// The FROM id of the FIRST inbound peer section (a `<teammate-message>` or `<agent-message>`)
    /// in this `type:"user"` record — the comm FROM for [`Record::direction`] (GOLD §4 + P1c M1).
    /// `None` when this is not a peer record; a section with no sender attribute degrades to the
    /// literal `"peer"`. Reads the raw (un-normalized) text so the relay preamble's `\n` survives.
    fn first_peer_from(&self) -> Option<String> {
        if !self.is_type("user") {
            return None;
        }
        let text = self.raw_message_text()?;
        let first = parse_all_peer_sections(&text).into_iter().next()?;
        Some(first.from.unwrap_or_else(|| "peer".to_string()))
    }

    /// The CLEAN inbound-comm preview of this record when it is (or leads with) an inbound peer
    /// message — a `<teammate-message …>` or `<agent-message from="…">` (GOLD §1/§5). Returns the
    /// FIRST inbound peer section's class + sender + tag/footer-stripped body, so `turns` / `list`
    /// render `agent.communication.inbox  <from> ⇨ self  <body>` instead of the raw `<teammate-message
    /// …>` XML blob a peer opener used to show. `None` for a non-peer record. RENDER-ONLY (does not
    /// affect [`Record::classify`] / [`Record::opens_turn`]). Pure + tolerant + codepoint-safe
    /// (delegates to the ASCII-offset peer-section scan).
    #[must_use]
    pub fn inbound_comm_preview(&self) -> Option<InboundComm> {
        let text = self.raw_message_text()?;
        let first = parse_all_peer_sections(&text).into_iter().next()?;
        let class = if first.is_signal {
            Class::CommSignal
        } else {
            Class::CommInbox
        };
        Some(InboundComm {
            class,
            from: first.from.unwrap_or_else(|| "peer".to_string()),
            body: normalize_line(peer_section_body(&first.text)),
        })
    }

    /// The per-section record-level text emissions of a BATCHED `type:"user"` record (≥1
    /// `<task-notification>` and/or inbound peer `<teammate-message>` / `<agent-message>` section)
    /// — GOLD §3 G4/G5 per-section render. One [`RecordTextSection`] per section's label, MIRRORING
    /// [`classify_batched_sections`] EXACTLY (same notification-span precedence/masking) so the text
    /// render never drifts from the classification: each `<task-notification>` yields its
    /// `harness.notification.<kind>` (text = the per-section automation label) PLUS, when it carries
    /// a `<result>` (G1), an `agent.communication.inbox` section (child ⇨ self via the embedded
    /// `<tool-use-id>`, degrading to `?` without a [`SpawnLookup`]); each inbound peer section
    /// outside every notification span yields `agent.communication.{inbox,signal}` (sender ⇨ self).
    /// EMPTY when the record carries no such section — the caller then falls back to the single
    /// richest-label record-text emission.
    #[must_use]
    pub fn record_text_sections(&self, ctx: &ClassifyCtx) -> Vec<RecordTextSection> {
        let mut out: Vec<RecordTextSection> = Vec::new();
        let Some(raw) = self.raw_message_text() else {
            return out;
        };
        let owner = || ctx.owner_id.unwrap_or("self").to_string();
        // (a) <task-notification> sections (+ the G1 inbox view of a <result>-bearing pulse),
        //     recording each span to mask a peer tag quoted inside it.
        let mut notif_spans: Vec<(usize, usize)> = Vec::new();
        scan_tag_sections(
            &raw,
            TASK_NOTIFICATION_PREFIX,
            TASK_NOTIFICATION_CLOSE,
            |offset, section| {
                let kind =
                    AutomationKind::from_summary(extract_xml_tag(section, "summary").as_deref());
                let label = automation_label_for_section(section);
                out.push(RecordTextSection {
                    class: notification_class(kind),
                    text: label.clone(),
                    direction: None,
                });
                if section.contains(NOTIFICATION_RESULT_TAG) {
                    let child = extract_xml_tag(section, "tool-use-id")
                        .and_then(|id| ctx.spawn.and_then(|sp| sp.child_for_spawn_tool_use_id(&id)))
                        .unwrap_or_else(|| "?".to_string());
                    // The inbox view excerpts the child's REPORT (the <result> body); fall back to
                    // the attribution label when the body is absent/empty.
                    let report = extract_xml_tag(section, "result")
                        .map(|r| normalize_line(&r))
                        .filter(|r| !r.is_empty())
                        .unwrap_or(label);
                    out.push(RecordTextSection {
                        class: Class::CommInbox,
                        text: report,
                        direction: Some((child, owner())),
                    });
                }
                notif_spans.push((offset, offset + section.len()));
            },
        );
        // (b) inbound peer sections OUTSIDE every notification span (precedence + cross-family).
        for peer in parse_all_peer_sections(&raw) {
            if notif_spans
                .iter()
                .any(|&(s, e)| peer.offset >= s && peer.offset < e)
            {
                continue;
            }
            let from = peer.from.clone().unwrap_or_else(|| "peer".to_string());
            out.push(RecordTextSection {
                class: if peer.is_signal {
                    Class::CommSignal
                } else {
                    Class::CommInbox
                },
                text: normalize_line(&peer.text),
                direction: Some((from, owner())),
            });
        }
        out
    }

    /// The comm direction `(from, to)` for a communication record (GOLD §4), or `None` for a
    /// non-comm record. The `self` side is `ctx.owner_id` (falls back to the literal `"self"`
    /// so direction is testable without a real id); record-supplied ids (teammate_id, the
    /// SendMessage recipient) are used verbatim; a spawn child / subagent-return FROM is
    /// resolved via the [`SpawnLookup`], degrading to the raw spawn name or `"?"`. A record
    /// carrying multiple comm blocks returns the FIRST (most salient) direction.
    #[must_use]
    pub fn direction(&self, ctx: &ClassifyCtx) -> Option<(String, String)> {
        let owner = || ctx.owner_id.unwrap_or("self").to_string();

        // M3 precedence: a <task-notification> record is resolved FIRST — BEFORE the peer scan —
        // so a notification whose <result> merely QUOTES a "<teammate-message" tag never takes
        // the peer direction. A G1 <result>-bearing pulse is the bg-agent's report (child ⇨
        // self), the child resolved via the embedded <tool-use-id> spawn id (degrading to "?"
        // without a lookup); a bare launch-ack pulse (no <result>) carries no comm direction.
        if let Some(Content::Text(s)) = self.message.as_ref().and_then(|m| m.content.as_ref()) {
            if s.starts_with(TASK_NOTIFICATION_PREFIX) {
                if s.contains(NOTIFICATION_RESULT_TAG) {
                    let child = extract_xml_tag(s, "tool-use-id")
                        .and_then(|id| ctx.spawn.and_then(|sp| sp.child_for_spawn_tool_use_id(&id)))
                        .unwrap_or_else(|| "?".to_string());
                    return Some((child, owner()));
                }
                return None;
            }
        }

        // Inbound peer message (teammate-message / agent-message): from ⇨ self. The FIRST
        // section's sender (most-salient), per the multi-section rule (P1c M1 folds the
        // <agent-message> peer form in alongside <teammate-message>).
        if let Some(from) = self.first_peer_from() {
            return Some((from, owner()));
        }

        if let Some(blocks) = self.blocks() {
            for b in blocks {
                match b {
                    Block::ToolUse { id, name, input } => {
                        let Some(name) = name.as_deref() else {
                            continue;
                        };
                        if name == "SendMessage" {
                            let to = send_message_recipient(input.as_ref())
                                .unwrap_or_else(|| "?".to_string());
                            return Some((owner(), to));
                        }
                        if is_spawn_tool_name(name) {
                            // TO = the spawned child: id-join first, then the name-join, else
                            // the raw spawn name, else `?`.
                            let to = id
                                .as_deref()
                                .and_then(|i| {
                                    ctx.spawn.and_then(|s| s.child_for_spawn_tool_use_id(i))
                                })
                                .or_else(|| {
                                    spawn_target_name(input.as_ref()).map(|n| {
                                        ctx.spawn
                                            .and_then(|s| s.child_for_spawn_name(&n))
                                            .unwrap_or(n)
                                    })
                                })
                                .unwrap_or_else(|| "?".to_string());
                            return Some((owner(), to));
                        }
                    }
                    // Subagent return: child ⇨ self (the Task tool_result of a one-shot spawn).
                    // A spawn LAUNCH ACK (teammate OR async/background Agent) shares the spawn id
                    // but is NOT a return → no direction (the real reply comes later — a teammate
                    // via a teammate-message, an async agent via the <task-notification> result).
                    Block::ToolResult {
                        tool_use_id: Some(tid),
                        ..
                    } if !self.is_spawn_launch_ack() => {
                        if let Some(child) =
                            ctx.spawn.and_then(|s| s.child_for_spawn_tool_use_id(tid))
                        {
                            return Some((child, owner()));
                        }
                    }
                    _ => {}
                }
            }
        }

        // Subagent transcript opener (the spawn-prompt seed): parent ⇨ self.
        if ctx.is_subagent && ctx.is_transcript_opener && self.opens_turn() {
            let from = ctx.parent_id.unwrap_or("parent").to_string();
            return Some((from, owner()));
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Record {
        serde_json::from_str(line).expect("valid record")
    }

    #[test]
    fn genuine_user_string_content() {
        let r = parse(r#"{"type":"user","message":{"role":"user","content":"hello"}}"#);
        assert!(r.is_genuine_user());
    }

    #[test]
    fn tool_result_carrier_is_not_genuine_user() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok"}]}}"#,
        );
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn compaction_summary_is_not_genuine_user() {
        let r = parse(
            r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"summary..."}}"#,
        );
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn unknown_top_level_fields_are_ignored() {
        // Real records carry attachment/isMeta/slug/etc. — must not break parsing.
        let r = parse(
            r#"{"type":"assistant","attachment":{"x":1},"isMeta":true,"slug":"s","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
        );
        assert!(r.is_type("assistant"));
    }

    #[test]
    fn unknown_block_type_does_not_fail() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"future_block","data":1}]}}"#,
        );
        assert!(r.is_type("assistant"));
    }

    #[test]
    fn metadata_record_without_timestamp_parses() {
        let r = parse(r#"{"type":"last-prompt","leafUuid":"abc","sessionId":"s"}"#);
        assert!(r.timestamp.is_none());
        assert!(r.is_type("last-prompt"));
    }

    #[test]
    fn is_meta_user_is_not_genuine_user() {
        // §4.2 TRAP: looks human, is system-injected — must be excluded.
        let r = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off."}}"#,
        );
        assert!(!r.is_genuine_user());
        assert!(r.genuine_user_text().is_none());
    }

    #[test]
    fn is_meta_local_command_caveat_excluded() {
        // The real-data shape: <local-command-caveat> on an isMeta user record.
        let r = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>Caveat: ...</local-command-caveat>"}}"#,
        );
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn genuine_user_text_string_content() {
        let r =
            parse(r#"{"type":"user","message":{"role":"user","content":"  hello\n  world  "}}"#);
        assert_eq!(r.genuine_user_text().as_deref(), Some("hello world"));
    }

    #[test]
    fn genuine_user_text_joins_text_blocks() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"line one"},{"type":"text","text":"line two"}]}}"#,
        );
        assert_eq!(r.genuine_user_text().as_deref(), Some("line one line two"));
    }

    #[test]
    fn agent_text_extracts_visible_text_only() {
        // thinking + tool_use are NOT the visible message; only `text` counts.
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"},{"type":"tool_use","id":"t","name":"Bash","input":{}},{"type":"text","text":"Done — built."}]}}"#,
        );
        assert_eq!(r.agent_text().as_deref(), Some("Done — built."));
    }

    #[test]
    fn agent_text_none_when_no_visible_text() {
        // A pure tool-call assistant turn has no end-of-turn message.
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#,
        );
        assert!(r.agent_text().is_none());
    }

    #[test]
    fn agent_text_none_for_user_record() {
        let r = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
        assert!(r.agent_text().is_none());
    }

    #[test]
    fn auq_answer_carrier_detected() {
        // Real shape: a user-carrier whose tool_result content is the synthesized
        // "User has answered your questions: …" string (§4.4).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"User has answered your questions: \"Q\"=\"A\". You can now continue."}]}}"#,
        );
        assert!(r.is_auq_answer());
        // It is NOT a genuine user (it's a carrier) but IS an AUQ answer — and, per the
        // §6.4 behavior change, it DOES open a turn (the answer is the user's message).
        assert!(!r.is_genuine_user());
        assert!(r.is_auq_answer_boundary());
        assert!(r.opens_turn());
    }

    #[test]
    fn auq_answer_alternate_phrasing_detected() {
        // The DOMINANT real-data phrasing the single hardcoded marker used to miss:
        // "Your questions have been answered: …" (verified across real sessions —
        // 16 sessions use this form vs 13 the other, 4 contain BOTH). Must be
        // recognised under the `user` category exactly like the other phrasing.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"Your questions have been answered: \"Q\"=\"A\". You can now continue with these answers in mind."}]}}"#,
        );
        assert!(r.is_auq_answer(), "alternate AUQ phrasing must be detected");
        assert!(!r.is_genuine_user());
        // The dominant phrasing is also a turn boundary (§6.4).
        assert!(r.is_auq_answer_boundary());
        assert!(r.opens_turn());
    }

    #[test]
    fn is_auq_answer_text_recognises_both_phrasings() {
        assert!(is_auq_answer_text(
            "User has answered your questions: \"q\"=\"a\"."
        ));
        assert!(is_auq_answer_text(
            "Your questions have been answered: \"q\"=\"a\"."
        ));
        assert!(!is_auq_answer_text("a normal tool output"));
    }

    #[test]
    fn is_auq_answer_text_is_start_anchored_not_contains() {
        // The reported bug: a tool_result (e.g. a Read of SPEC.md / a fixture) that merely
        // QUOTES the marker mid-content must NOT be taken for a synthesized AUQ answer. The
        // real machine answer LEADS with the marker; a quote does not.
        assert!(!is_auq_answer_text(
            "# SPEC.md\nThe synthesized answer string is \"User has answered your questions:\" which leads the body."
        ));
        assert!(!is_auq_answer_text(
            "see the marker \"Your questions have been answered\" documented above"
        ));
        // Leading whitespace the renderer may prepend is tolerated (still anchored).
        assert!(is_auq_answer_text(
            "  User has answered your questions: \"q\"=\"a\"."
        ));
    }

    #[test]
    fn auq_answer_no_false_positive_on_file_quoting_marker() {
        // A Read/grep tool_result whose content QUOTES the marker mid-text (the csift
        // dev-session failure): NOT an AUQ answer, NOT a boundary, and classify yields a
        // plain `agent.tool.result` — never `user.answer` dumping the whole file.
        // NB: `r##"…"##` delimiter — the JSON `"content":"# SPEC.md` has `"#`, which would
        // close a plain `r#"…"#` raw string early.
        let r = parse(
            r##"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"# SPEC.md (10000 chars)\n...the marker is \"User has answered your questions:\" which CC emits to synthesize the answer record. Lots more file content follows here."}]}}"##,
        );
        assert!(!r.is_auq_answer());
        assert!(!r.is_auq_answer_boundary());
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentToolResult]
        );
    }

    #[test]
    fn auq_answer_genuine_marker_lead_still_classifies_as_user_answer() {
        // A genuine synthesized answer (content STARTS with the marker, NO structured
        // toolUseResult.answers) stays detected via the fallback arm: a boundary + the
        // [user.answer, agent.tool.result] dual label.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"User has answered your questions: \"Pick one\"=\"A\". You can now continue with the user's answers in mind."}]}}"#,
        );
        assert!(r.is_auq_answer());
        assert!(r.is_auq_answer_boundary());
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::UserAnswer, Class::AgentToolResult]
        );
    }

    #[test]
    fn auq_answer_structured_path_independent_of_marker_text() {
        // The PRIMARY (modern) path — a non-empty structured `toolUseResult.answers` —
        // is start-anchor-independent: it classifies `user.answer` even when the carrier's
        // content does NOT lead with the marker.
        let r = parse(
            r#"{"type":"user","toolUseResult":{"answers":{"Pick one":"A"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"(answer recorded)"}]}}"#,
        );
        assert!(r.is_auq_answer_boundary());
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::UserAnswer, Class::AgentToolResult]
        );
    }

    #[test]
    fn plain_tool_result_is_not_auq_answer() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"just a normal tool output"}]}}"#,
        );
        assert!(!r.is_auq_answer());
    }

    #[test]
    fn persisted_output_path_structured_field_preferred() {
        let r = parse(
            r#"{"type":"user","toolUseResult":{"persistedOutputPath":"/tmp/x/tool-results/abc.txt","persistedOutputSize":123},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"<persisted-output>\nOutput too large (1 KB). Full output saved to: /wrong/inline/path.txt\n</persisted-output>"}]}}"#,
        );
        // Structured field wins over the inline marker.
        assert_eq!(
            r.persisted_output_path().as_deref(),
            Some("/tmp/x/tool-results/abc.txt")
        );
    }

    #[test]
    fn persisted_output_path_inline_fallback() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"<persisted-output>\nOutput too large (200 KB). Full output saved to: /tmp/sess/tool-results/b070yh2rb.txt\n\nPreview (first 2KB):\n…\n</persisted-output>"}]}}"#,
        );
        assert_eq!(
            r.persisted_output_path().as_deref(),
            Some("/tmp/sess/tool-results/b070yh2rb.txt")
        );
    }

    #[test]
    fn persisted_output_path_none_when_absent() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"a normal small tool output"}]}}"#,
        );
        assert!(r.persisted_output_path().is_none());
    }

    #[test]
    fn scrape_persisted_path_handles_marker() {
        assert_eq!(
            scrape_persisted_path("Full output saved to: /a/b/c.txt\nmore"),
            Some("/a/b/c.txt".to_string())
        );
        assert!(scrape_persisted_path("no marker here").is_none());
    }

    #[test]
    fn tool_result_content_text_string_and_array() {
        let s = serde_json::json!("hello world");
        assert_eq!(tool_result_content_text(&s), "hello world");
        let arr = serde_json::json!([
            {"type":"text","text":"first"},
            {"type":"image","source":{}},
            {"type":"text","text":"second"},
            {"type":"tool_reference","tool_name":"WebSearch"}
        ]);
        assert_eq!(tool_result_content_text(&arr), "first\nsecond\nWebSearch");
    }

    // ── Branch-completeness: the negative / fallback arms ──

    #[test]
    fn is_genuine_user_false_for_non_user_type() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
        );
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn is_genuine_user_false_when_message_absent() {
        // A `type:"user"` record with NO `message` object at all (the `let Some(msg)
        // else` arm).
        let r = parse(r#"{"type":"user","uuid":"x"}"#);
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn is_genuine_user_false_when_role_not_user() {
        // role mismatch inside an otherwise user-typed record.
        let r = parse(r#"{"type":"user","message":{"role":"assistant","content":"hi"}}"#);
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn is_genuine_user_false_when_content_absent() {
        // message present, role user, but NO content (the `None => false` arm).
        let r = parse(r#"{"type":"user","message":{"role":"user"}}"#);
        assert!(!r.is_genuine_user());
        assert!(r.genuine_user_text().is_none());
    }

    #[test]
    fn is_genuine_user_false_for_blocks_without_text() {
        // Block content that has NO text block (only an image) → not genuine.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{}}]}}"#,
        );
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn blocks_none_for_string_content() {
        // `blocks()` returns None when content is a bare string (Content::Text arm).
        let r = parse(r#"{"type":"user","message":{"role":"user","content":"plain"}}"#);
        assert!(r.blocks().is_none());
    }

    #[test]
    fn blocks_none_when_no_message() {
        let r = parse(r#"{"type":"system","subtype":"compact_boundary"}"#);
        assert!(r.blocks().is_none());
    }

    #[test]
    fn is_auq_answer_false_for_non_user() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"x"}]}}"#,
        );
        assert!(!r.is_auq_answer());
    }

    #[test]
    fn is_auq_answer_false_when_no_blocks() {
        // user record with string content → no blocks → not an AUQ answer.
        let r = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
        assert!(!r.is_auq_answer());
    }

    #[test]
    fn persisted_output_path_empty_structured_falls_through_to_inline() {
        // An empty structured persistedOutputPath must NOT win — the inline marker
        // is used instead (the `!p.is_empty()` false arm).
        let r = parse(
            r#"{"type":"user","toolUseResult":{"persistedOutputPath":""},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"<persisted-output>\nFull output saved to: /tmp/real/inline.txt\n</persisted-output>"}]}}"#,
        );
        assert_eq!(
            r.persisted_output_path().as_deref(),
            Some("/tmp/real/inline.txt")
        );
    }

    #[test]
    fn persisted_output_path_none_when_no_blocks() {
        // user record with string content → blocks() is None → `?` short-circuits.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"no persisted pointer here"}}"#,
        );
        assert!(r.persisted_output_path().is_none());
    }

    #[test]
    fn persisted_output_path_structured_present_but_missing_key_falls_to_inline() {
        // `toolUseResult` exists but lacks `persistedOutputPath` (the
        // `tur.get(...)` None arm) → fall through to the inline marker scan.
        let r = parse(
            r#"{"type":"user","toolUseResult":{"somethingElse":1},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"Full output saved to: /tmp/fallback.txt\n"}]}}"#,
        );
        assert_eq!(
            r.persisted_output_path().as_deref(),
            Some("/tmp/fallback.txt")
        );
    }

    #[test]
    fn persisted_output_path_skips_non_tool_result_blocks_in_inline_scan() {
        // The inline fallback loop must skip a non-ToolResult block (the `if let
        // Block::ToolResult` FALSE arm) and find the marker in a later tool_result.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"not a tool result"},{"type":"tool_result","tool_use_id":"x","content":"Full output saved to: /tmp/later.txt\n"}]}}"#,
        );
        assert_eq!(r.persisted_output_path().as_deref(), Some("/tmp/later.txt"));
    }

    #[test]
    fn is_auq_answer_skips_non_tool_result_blocks() {
        // is_auq_answer's `.any()` must return false for a block that is NOT a
        // tool_result (the match's `_ => false` arm) when no AUQ marker is present.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"plain user text, no AUQ marker"}]}}"#,
        );
        assert!(!r.is_auq_answer());
    }

    #[test]
    fn agent_text_none_for_non_assistant() {
        let r = parse(r#"{"type":"system","subtype":"away_summary"}"#);
        assert!(r.agent_text().is_none());
    }

    #[test]
    fn agent_text_handles_bare_string_assistant_content() {
        // CC normally sends assistant content as a block array; a bare-string body is
        // a surprise we surface rather than drop (the Content::Text fallback arm).
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":"  surprise bare string  "}}"#,
        );
        assert_eq!(r.agent_text().as_deref(), Some("surprise bare string"));
    }

    #[test]
    fn agent_text_none_for_empty_bare_string() {
        // A bare-string assistant body that is all whitespace normalizes to empty → None.
        let r = parse(r#"{"type":"assistant","message":{"role":"assistant","content":"   "}}"#);
        assert!(r.agent_text().is_none());
    }

    #[test]
    fn agent_text_none_when_no_content() {
        let r = parse(r#"{"type":"assistant","message":{"role":"assistant"}}"#);
        assert!(r.agent_text().is_none());
    }

    #[test]
    fn agent_text_skips_blank_text_blocks() {
        // A text block that is all whitespace is skipped; only the real one counts.
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"   "},{"type":"text","text":"real"}]}}"#,
        );
        assert_eq!(r.agent_text().as_deref(), Some("real"));
    }

    #[test]
    fn flatten_blocks_ignores_non_text_blocks() {
        // genuine_user_text over blocks where a non-text block is interleaved: the
        // tool_use/image are filtered out, only text survives. (Exercised via a
        // genuine-user record whose blocks mix text with an image.)
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"keep me"},{"type":"image","source":{}}]}}"#,
        );
        assert_eq!(r.genuine_user_text().as_deref(), Some("keep me"));
    }

    #[test]
    fn tool_result_content_text_object_fallback() {
        // A non-string/non-array content (e.g. an object) renders compactly so a
        // regex can still match structured payloads (the `other => to_string` arm).
        let obj = serde_json::json!({"k": "v", "n": 1});
        let out = tool_result_content_text(&obj);
        assert!(out.contains("\"k\""), "compact object render: {out}");
        // A bare number/bool/null also goes through the same arm.
        assert_eq!(tool_result_content_text(&serde_json::json!(42)), "42");
        assert_eq!(tool_result_content_text(&serde_json::json!(null)), "null");
    }

    #[test]
    fn scrape_persisted_path_empty_after_marker_is_none() {
        // The marker is present but the path is blank → None (the `path.is_empty()`
        // true arm).
        assert!(scrape_persisted_path("Full output saved to:   \nnext").is_none());
        // Marker with the path running to EOF (no trailing newline) → the `unwrap_or`
        // line_end == rest.len() branch.
        assert_eq!(
            scrape_persisted_path("Full output saved to: /a/b.txt"),
            Some("/a/b.txt".to_string())
        );
    }

    #[test]
    fn normalize_line_collapses_and_trims() {
        // Leading/trailing/internal whitespace runs collapse to single spaces; the
        // trailing-space pop loop runs.
        assert_eq!(normalize_line("  a\t\tb \n c   "), "a b c");
        assert_eq!(normalize_line(""), "");
        assert_eq!(normalize_line("   "), "");
    }

    // ── File-mutation extraction (FileOp / FileMutation / Record helpers) ──

    #[test]
    fn file_op_label_all_variants() {
        assert_eq!(FileOp::Write.label(), "write");
        assert_eq!(FileOp::Edit.label(), "edit");
        assert_eq!(FileOp::NotebookEdit.label(), "notebook-edit");
        assert_eq!(FileOp::MultiEdit.label(), "multi-edit");
        assert_eq!(FileOp::BashMutation.label(), "bash");
    }

    #[test]
    fn file_op_json_key_uses_underscores_for_multiword() {
        // The on-wire JSON spelling is UNDERSCORE-delimited so the timeline `op` field
        // matches the grouped per-op COUNT keys; single-word ops coincide with `label`.
        assert_eq!(FileOp::Write.json_key(), "write");
        assert_eq!(FileOp::Edit.json_key(), "edit");
        assert_eq!(FileOp::NotebookEdit.json_key(), "notebook_edit");
        assert_eq!(FileOp::MultiEdit.json_key(), "multi_edit");
        assert_eq!(FileOp::BashMutation.json_key(), "bash");
        // The two multi-word ops are the only ones that DIFFER from `label` (hyphen vs `_`).
        assert_ne!(
            FileOp::NotebookEdit.json_key(),
            FileOp::NotebookEdit.label()
        );
        assert_ne!(FileOp::MultiEdit.json_key(), FileOp::MultiEdit.label());
    }

    #[test]
    fn file_op_is_heuristic_only_for_bash() {
        assert!(FileOp::BashMutation.is_heuristic());
        assert!(!FileOp::Write.is_heuristic());
        assert!(!FileOp::Edit.is_heuristic());
        assert!(!FileOp::NotebookEdit.is_heuristic());
        assert!(!FileOp::MultiEdit.is_heuristic());
    }

    #[test]
    fn structured_tool_mutations_write_edit_multiedit() {
        let r = parse(
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/tmp/a.md","content":"x"}},
                {"type":"tool_use","id":"t2","name":"Edit","input":{"file_path":"/p/spec/gaps.md","old_string":"a","new_string":"b","replace_all":false}},
                {"type":"tool_use","id":"t3","name":"MultiEdit","input":{"file_path":"/p/multi.rs","edits":[]}}
            ]}}"#,
        );
        let muts = r.structured_tool_mutations();
        assert_eq!(muts.len(), 3);
        assert_eq!(muts[0].op, FileOp::Write);
        assert_eq!(muts[0].path, "/tmp/a.md");
        assert_eq!(
            muts[0].timestamp_utc.as_deref(),
            Some("2026-06-07T05:00:00.000Z")
        );
        assert!(!muts[0].is_create, "no carrier on tool_use → unknown=false");
        assert_eq!(muts[1].op, FileOp::Edit);
        assert_eq!(muts[1].path, "/p/spec/gaps.md");
        assert_eq!(muts[2].op, FileOp::MultiEdit);
        assert_eq!(muts[2].path, "/p/multi.rs");
    }

    #[test]
    fn structured_tool_mutations_notebook_uses_notebook_path() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","name":"NotebookEdit","input":{"notebook_path":"/p/nb.ipynb","new_source":"code"}}
            ]}}"#,
        );
        let muts = r.structured_tool_mutations();
        assert_eq!(muts.len(), 1);
        assert_eq!(muts[0].op, FileOp::NotebookEdit);
        assert_eq!(muts[0].path, "/p/nb.ipynb");
    }

    #[test]
    fn structured_tool_mutations_reads_create_from_own_carrier() {
        // The rare case where the tool_use record ALSO carries the toolUseResult with
        // type:"create" (the `self_is_create` true arm).
        let r = parse(
            r#"{"type":"assistant","toolUseResult":{"type":"create","filePath":"/tmp/new.md"},"message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/tmp/new.md","content":"x"}}
            ]}}"#,
        );
        let muts = r.structured_tool_mutations();
        assert_eq!(muts.len(), 1);
        assert!(
            muts[0].is_create,
            "own carrier type:create → is_create true"
        );
    }

    #[test]
    fn structured_tool_mutations_skips_missing_and_empty_path() {
        // A Write with NO file_path and an Edit with an EMPTY file_path → both skipped
        // (the defensive `path.is_empty()` arm). A non-mutating tool is ignored too.
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","name":"Write","input":{"content":"x"}},
                {"type":"tool_use","id":"t2","name":"Edit","input":{"file_path":"","old_string":"a"}},
                {"type":"tool_use","id":"t3","name":"Read","input":{"file_path":"/p/read.rs"}}
            ]}}"#,
        );
        assert!(r.structured_tool_mutations().is_empty());
    }

    #[test]
    fn structured_tool_mutations_skips_tool_use_with_no_name() {
        // A tool_use block missing its `name` → the `name.as_deref()` None arm.
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","input":{"file_path":"/p/x.md"}}
            ]}}"#,
        );
        assert!(r.structured_tool_mutations().is_empty());
    }

    #[test]
    fn structured_tool_mutations_none_for_string_content() {
        // A record with string content → blocks() None → empty (the early-return arm).
        let r = parse(r#"{"type":"user","message":{"role":"user","content":"plain"}}"#);
        assert!(r.structured_tool_mutations().is_empty());
    }

    #[test]
    fn carrier_create_paths_create_update_and_missing() {
        // type:"create" → is_create true, joined to the tool_result block's id.
        let create = parse(
            r#"{"type":"user","toolUseResult":{"type":"create","filePath":"/tmp/new.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
        );
        assert_eq!(
            create.carrier_create_paths(),
            vec![("t1".to_string(), "/tmp/new.md".to_string(), true)]
        );
        // type:"update" → is_create false.
        let update = parse(
            r#"{"type":"user","toolUseResult":{"type":"update","filePath":"/p/gaps.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"ok"}]}}"#,
        );
        assert_eq!(
            update.carrier_create_paths(),
            vec![("t2".to_string(), "/p/gaps.md".to_string(), false)]
        );
        // No toolUseResult at all → empty (the `let Some(tur) else` arm).
        let none = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t3","content":"ok"}]}}"#,
        );
        assert!(none.carrier_create_paths().is_empty());
    }

    #[test]
    fn carrier_create_paths_file_unchanged_is_not_create() {
        // type:"file_unchanged" (the edit-touched-but-no-change case) → is_create false.
        let r = parse(
            r#"{"type":"user","toolUseResult":{"type":"file_unchanged","filePath":"/p/same.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
        );
        assert_eq!(
            r.carrier_create_paths(),
            vec![("t1".to_string(), "/p/same.md".to_string(), false)]
        );
    }

    #[test]
    fn carrier_create_paths_no_file_path_or_not_object() {
        // toolUseResult present but NO filePath → empty (the filePath None arm).
        let no_fp = parse(
            r#"{"type":"user","toolUseResult":{"type":"text","stdout":"x"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"x"}]}}"#,
        );
        assert!(no_fp.carrier_create_paths().is_empty());
        // toolUseResult is NOT an object (a bare string) → `.get("filePath")` is None.
        let not_obj = parse(
            r#"{"type":"user","toolUseResult":"just a string","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"x"}]}}"#,
        );
        assert!(not_obj.carrier_create_paths().is_empty());
        // filePath present but EMPTY → empty (the `file_path.is_empty()` arm).
        let empty_fp = parse(
            r#"{"type":"user","toolUseResult":{"type":"create","filePath":""},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"x"}]}}"#,
        );
        assert!(empty_fp.carrier_create_paths().is_empty());
    }

    #[test]
    fn carrier_create_paths_skips_non_tool_result_blocks_and_no_id() {
        // A carrier with a filePath but whose only block is NOT a tool_result, OR a
        // tool_result with no tool_use_id → no joinable id, so empty.
        let no_id = parse(
            r#"{"type":"user","toolUseResult":{"type":"create","filePath":"/tmp/x.md"},"message":{"role":"user","content":[{"type":"text","text":"not a tool result"},{"type":"tool_result","content":"ok"}]}}"#,
        );
        assert!(no_id.carrier_create_paths().is_empty());
    }

    #[test]
    fn carrier_create_paths_empty_when_no_blocks() {
        // filePath present but the record has string content (no blocks) → empty.
        let r = parse(
            r#"{"type":"user","toolUseResult":{"type":"create","filePath":"/tmp/x.md"},"message":{"role":"user","content":"string body"}}"#,
        );
        assert!(r.carrier_create_paths().is_empty());
    }

    // ── group_turn_indices (the shared §6.4 turn delimiter) ──

    #[test]
    fn group_turn_indices_basic_two_turns() {
        // bools = is_genuine_user; [user, member, member, user, member].
        let flags = [true, false, false, true, false];
        let turns = group_turn_indices(&flags, |b| *b);
        assert_eq!(turns, vec![vec![0, 1, 2], vec![3, 4]]);
    }

    #[test]
    fn group_turn_indices_synthetic_lead_folds_into_first_turn() {
        // Leading non-genuine records (a synthetic lead) fold into turn 0 when a real
        // user follows, so indices stay 0-based on genuine users.
        let flags = [false, false, true, false, true];
        let turns = group_turn_indices(&flags, |b| *b);
        // The two lead members (0,1) join the first real turn (opening at idx 2).
        assert_eq!(turns, vec![vec![0, 1, 2, 3], vec![4]]);
    }

    #[test]
    fn group_turn_indices_only_synthetic_lead_no_genuine_user() {
        // No genuine user ever → a single standalone turn 0 holding the orphans (the
        // `turns.len() > 1` false guard means no fold).
        let flags = [false, false, false];
        let turns = group_turn_indices(&flags, |b| *b);
        assert_eq!(turns, vec![vec![0, 1, 2]]);
    }

    #[test]
    fn group_turn_indices_empty_is_empty() {
        let flags: [bool; 0] = [];
        assert!(group_turn_indices(&flags, |b| *b).is_empty());
    }

    #[test]
    fn group_turn_indices_first_record_genuine_no_fold() {
        // The first record IS genuine → no synthetic lead → no fold.
        let flags = [true, false, true];
        let turns = group_turn_indices(&flags, |b| *b);
        assert_eq!(turns, vec![vec![0, 1], vec![2]]);
    }

    #[test]
    fn opens_turn_grouping_splits_on_auq_answer_and_skips_interrupt() {
        // A realistic record sequence over the PRODUCTION predicate (`opens_turn`):
        //   0 genuine user      → turn 0 opener
        //   1 assistant AUQ tool_use (member of turn 0)
        //   2 AUQ answer carrier → turn 1 opener (the behavior change)
        //   3 assistant reply   (member of turn 1)
        //   4 interrupt marker  → NOT a boundary (member of turn 1)
        //   5 genuine user      → turn 2 opener
        let records: Vec<Record> = [
            r#"{"type":"user","message":{"role":"user","content":"pick one"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
            r#"{"type":"user","toolUseResult":{"answers":{"which?":"the bold one"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"the bold one\"."}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":"next question"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
        let turns = group_turn_indices(&records, |r| r.opens_turn());
        assert_eq!(
            turns,
            vec![vec![0, 1], vec![2, 3, 4], vec![5]],
            "AUQ answer opens turn 1; interrupt is a member, not a boundary"
        );
    }

    // ── §6.4.1 esc-cancel / edit-resend DRAFT SUPPRESSION ──
    // Real shape (verified on ~/.claude/projects): the user submits, ESC-cancels or edits,
    // and resends; CC appends EACH draft as its own genuine `type:"user"` record sharing the
    // SAME `parentUuid`. Only the last in file order reached the model. `superseded_draft_indices`
    // marks the earlier siblings; `group_turn_indices_deduped` drops them so they never become
    // phantom turns. None of these patterns are reachable through bool fixtures — they need the
    // real uuid/parentUuid tree, so they parse genuine record JSON.

    #[test]
    fn superseded_drafts_collapse_same_parent_edit_resend() {
        // u0 (parent root) → assistant a0 → THREE drafts of one turn under parent a0:
        // "draft v1" → edited "draft v2" → "draft v2, with a tail" (the one that continued) → assistant a1.
        // Only the last sibling survives; the two earlier ones are superseded drafts.
        let records: Vec<Record> = [
            r#"{"type":"user","uuid":"u0","parentUuid":"root","message":{"role":"user","content":"start"}}"#,
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"user","uuid":"d1","parentUuid":"a0","message":{"role":"user","content":"draft v1"}}"#,
            r#"{"type":"user","uuid":"d2","parentUuid":"a0","message":{"role":"user","content":"draft v2"}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","message":{"role":"user","content":"draft v2, with a tail"}}"#,
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","message":{"role":"assistant","content":[{"type":"text","text":"noted"}]}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();

        let sup = superseded_draft_indices(&records, |r| r);
        assert_eq!(
            sup.len(),
            2,
            "the two earlier same-parent drafts are superseded"
        );
        assert!(
            sup.contains(&2) && sup.contains(&3),
            "drafts d1,d2 superseded; u1 (last in file order) survives"
        );
        assert!(!sup.contains(&4));

        let turns = group_turn_indices_deduped(&records, |r| r);
        assert_eq!(
            turns,
            vec![vec![0, 1], vec![4, 5]],
            "two real turns; abandoned drafts vanish entirely (neither boundary nor member)"
        );
    }

    #[test]
    fn superseded_drafts_exact_duplicate_collapses_to_one() {
        // The same message appears 3× verbatim under one parent (ESC-cancel re-submits) →
        // exactly one turn, not three.
        let records: Vec<Record> = [
            r#"{"type":"assistant","uuid":"a0","parentUuid":"root","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
            r#"{"type":"user","uuid":"d1","parentUuid":"a0","message":{"role":"user","content":"the same resent line"}}"#,
            r#"{"type":"user","uuid":"d2","parentUuid":"a0","message":{"role":"user","content":"the same resent line"}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","message":{"role":"user","content":"the same resent line"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
        // Leading assistant (idx0) is a synthetic lead that folds into the first real turn.
        assert_eq!(
            group_turn_indices_deduped(&records, |r| r),
            vec![vec![0, 3]],
            "3 identical drafts → 1 turn (opener idx3)"
        );
    }

    #[test]
    fn superseded_drafts_distinct_parents_not_merged() {
        // Two identical-content user records with DIFFERENT parents are two real turns —
        // distinct turns legitimately share content but never a parentUuid (each is parented
        // to the assistant message that preceded it).
        let records: Vec<Record> = [
            r#"{"type":"user","uuid":"u0","parentUuid":"a0","message":{"role":"user","content":"continue"}}"#,
            r#"{"type":"assistant","uuid":"x","parentUuid":"u0","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"x","message":{"role":"user","content":"continue"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
        assert!(superseded_draft_indices(&records, |r| r).is_empty());
        assert_eq!(
            group_turn_indices_deduped(&records, |r| r),
            vec![vec![0, 1], vec![2]]
        );
    }

    #[test]
    fn superseded_drafts_null_parent_never_grouped() {
        // No parentUuid → never grouped (grouping on "no parent" would merge unrelated
        // first-message records). In real data a genuine user always carries a parent.
        let records: Vec<Record> = [
            r#"{"type":"user","uuid":"u0","message":{"role":"user","content":"a"}}"#,
            r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"b"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
        assert!(superseded_draft_indices(&records, |r| r).is_empty());
        assert_eq!(
            group_turn_indices_deduped(&records, |r| r),
            vec![vec![0], vec![1]]
        );
    }

    #[test]
    fn deduped_grouping_matches_plain_when_no_drafts() {
        // With no same-parent draft siblings, deduped grouping is identical to the plain
        // delimiter — a regression guard on the shared core.
        let records: Vec<Record> = [
            r#"{"type":"user","uuid":"u0","parentUuid":"r","message":{"role":"user","content":"q1"}}"#,
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","message":{"role":"assistant","content":[{"type":"text","text":"r1"}]}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","message":{"role":"user","content":"q2"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
        let plain = group_turn_indices(&records, |r| r.opens_turn());
        let deduped = group_turn_indices_deduped(&records, |r| r);
        assert_eq!(plain, deduped);
        assert_eq!(deduped, vec![vec![0, 1], vec![2]]);
    }

    // ── §4.2.1 interrupts: synthesized markers are NOT genuine-user (spurious-boundary
    //    removal). Real shape: a text-block user record whose text is EXACTLY the marker
    //    (116 + 21 occurrences in the corpus, none isMeta, none carrying extra prose). ──

    #[test]
    fn interrupt_marker_plain_is_not_genuine_user() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#,
        );
        assert!(
            !r.is_genuine_user(),
            "interrupt marker must not open a turn"
        );
        assert!(!r.opens_turn());
    }

    #[test]
    fn interrupt_marker_for_tool_use_is_not_genuine_user() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user for tool use]"}]}}"#,
        );
        assert!(!r.is_genuine_user());
        assert!(!r.opens_turn());
    }

    #[test]
    fn interrupt_marker_as_string_content_is_not_genuine_user() {
        // The same marker can arrive as bare-string content too — still excluded.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"[Request interrupted by user]"}}"#,
        );
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn text_that_merely_contains_interrupt_phrase_is_still_genuine() {
        // Exact-match only: a real message that QUOTES the phrase must stay genuine
        // (codepoint-safe `==`, never a substring/slice).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"why did i see [Request interrupted by user] earlier?"}}"#,
        );
        assert!(
            r.is_genuine_user(),
            "a message merely containing the phrase is still a human turn"
        );
    }

    // ── §4.2.2 / §4.2.3: <local-command-stdout> output + <command-name> slash wrapper
    //    are machine-templated, NOT genuine-user (string content, non-isMeta). ──

    #[test]
    fn local_command_stdout_is_not_genuine_user() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<local-command-stdout>✓ Updated oh-my-claudecode.</local-command-stdout>"}}"#,
        );
        assert!(!r.is_genuine_user());
        assert!(!r.opens_turn());
    }

    #[test]
    fn command_name_wrapper_is_not_genuine_user() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args></command-args>"}}"#,
        );
        assert!(
            !r.is_genuine_user(),
            "slash-command wrapper must not open a turn"
        );
        assert!(!r.opens_turn());
        // Empty args → no recoverable prose.
        assert!(r.slash_command_args().is_none());
    }

    #[test]
    fn command_name_wrapper_with_args_recovers_prose() {
        // Real shape: `/compact Just shipped spec-batch-14 …` — the typed prose lives in
        // <command-args>; it is recovered (and is the reconstructed user text), but the
        // wrapper itself is still not a standalone genuine-user record.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args> Just shipped spec-batch-14, summarize</command-args>"}}"#,
        );
        assert!(!r.is_genuine_user());
        assert_eq!(
            r.slash_command_args().as_deref(),
            Some("Just shipped spec-batch-14, summarize")
        );
        // v0.5: rendered as `/name args` — the prose keeps its command context, and the
        // wrapper XML never masquerades as the body.
        assert_eq!(
            r.reconstructed_user_text(None).as_deref(),
            Some("/compact Just shipped spec-batch-14, summarize")
        );
    }

    #[test]
    fn command_message_first_wrapper_detected_and_recovered() {
        // The NEWER CC tag order (`<command-message>` FIRST — both orders coexist in real
        // corpora). Detection anchored on `<command-name>` alone used to misclassify this
        // as GENUINE user prose (raw XML as `user.message`, and it opened a turn).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<command-message>csift</command-message>\n<command-name>/csift</command-name>\n<command-args>what changed in v5?</command-args>"}}"#,
        );
        assert!(!r.is_genuine_user(), "wrapper is never the human");
        assert_eq!(r.slash_command_name().as_deref(), Some("/csift"));
        assert_eq!(
            r.slash_command_args().as_deref(),
            Some("what changed in v5?")
        );
        assert_eq!(
            r.reconstructed_user_text(None).as_deref(),
            Some("/csift what changed in v5?")
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::UserMessage, Class::CommandInvocation]
        );
        // A no-args NEW-order wrapper is pure machinery: never genuine, never user.message.
        let bare = parse(
            r#"{"type":"user","message":{"role":"user","content":"<command-message>compact</command-message>\n<command-name>/compact</command-name>\n<command-args></command-args>"}}"#,
        );
        assert!(!bare.is_genuine_user());
        assert_eq!(
            bare.classify(&ClassifyCtx::top_level()),
            vec![Class::CommandInvocation]
        );
    }

    #[test]
    fn command_name_wrapper_with_multibyte_args_is_codepoint_safe() {
        // A multi-byte args body must be recovered whole (codepoint-safe slice on the ASCII
        // tags only) — the live panic class.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-args>🤖 just shipped the batch, summarize 🎉</command-args>"}}"#,
        );
        assert_eq!(
            r.slash_command_args().as_deref(),
            Some("🤖 just shipped the batch, summarize 🎉")
        );
    }

    // ── §4.4 AskUserQuestion answer = a genuine-user TURN BOUNDARY (the sanctioned
    //    behavior change). Real shape verified on a captured sample: a user tool_result
    //    carrier with toolUseResult.answers + the synthesized marker, is_error absent. ──

    #[test]
    fn auq_answer_with_structured_answers_is_a_boundary() {
        let r = parse(
            r#"{"type":"user","toolUseResult":{"questions":[{"question":"which?","header":"FIX","options":[{"label":"opt A"},{"label":"opt B"}]}],"answers":{"which?":"go with opt A and also fix the prod gap"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"go with opt A and also fix the prod gap\". You can now continue."}]}}"#,
        );
        // It is still NOT a "genuine user" (it rides on a carrier) but IS a boundary.
        assert!(!r.is_genuine_user());
        assert!(r.is_auq_answer_boundary());
        assert!(r.opens_turn());
        // The reconstructed exchange carries Q + options + the answer prose.
        let unit = r.auq_exchange().expect("auq exchange");
        assert!(unit.contains("AskUserQuestion · 1 question"));
        assert!(unit.contains("which?"));
        assert!(unit.contains("FIX"));
        // Options render one-per-line as `- <label>` (description appended when present).
        assert!(unit.contains("- opt A"), "option A rendered: {unit}");
        assert!(unit.contains("- opt B"), "option B rendered: {unit}");
        assert!(unit.contains("go with opt A and also fix the prod gap"));
        // reconstructed_user_text routes to the same unit.
        assert!(r
            .reconstructed_user_text(None)
            .unwrap()
            .contains("go with opt A"));
    }

    #[test]
    fn auq_answer_multibyte_is_codepoint_safe_boundary() {
        // A multi-byte answer prose — must reconstruct whole, no mid-codepoint slice.
        let r = parse(
            r#"{"type":"user","toolUseResult":{"questions":[{"question":"which option for step two? 🤖","header":"STEP TWO","options":[{"label":"option A (recommended)"}]}],"answers":{"which option for step two? 🤖":"🤖 option A is fine, the scope is broader than stated"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"Your questions have been answered: \"which option for step two? 🤖\"=\"🤖 option A is fine\"."}]}}"#,
        );
        assert!(r.is_auq_answer_boundary());
        let unit = r.auq_exchange().expect("multibyte auq exchange");
        assert!(unit.contains("🤖 option A is fine, the scope is broader than stated"));
        assert!(unit.contains("which option for step two? 🤖"));
        assert!(unit.contains("option A (recommended)"));
    }

    #[test]
    fn auq_exchange_surfaces_each_option_description() {
        // Real-captured shape: every option carries a `description` (supplementary note)
        // alongside its `label`. BOTH must survive into the reconstructed unit — the
        // description was previously dropped (only labels rendered).
        let r = parse(
            r#"{"type":"user","toolUseResult":{"questions":[{"header":"EXIF tool","multiSelect":false,"options":[{"description":"standard route, ~10MB download, one-liner","label":"brew install exiftool (Recommended)"},{"description":"pure python, pip install piexif","label":"pip install piexif"}],"question":"which tool re-attaches EXIF?"}],"answers":{"which tool re-attaches EXIF?":"brew install exiftool (Recommended)"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which tool re-attaches EXIF?\"=\"brew install exiftool (Recommended)\". You can now continue."}]}}"#,
        );
        let unit = r.auq_exchange().expect("auq exchange");
        // Both labels AND both descriptions present, verbatim.
        assert!(unit.contains("brew install exiftool (Recommended)"));
        assert!(
            unit.contains("standard route, ~10MB download, one-liner"),
            "option description must survive: {unit}"
        );
        assert!(unit.contains("pip install piexif"));
        assert!(
            unit.contains("pure python, pip install piexif"),
            "second option description must survive: {unit}"
        );
    }

    #[test]
    fn auq_exchange_surfaces_notes_when_answer_is_notes_only() {
        // Real-captured shape: the user answered by typing prose into the
        // notes field; the answer value is the literal "(notes only)" placeholder and the
        // ACTUAL message lives in `annotations[question].notes`. It must be surfaced —
        // previously the whole user message was silently dropped.
        let r = parse(
            r#"{"type":"user","toolUseResult":{"questions":[{"header":"Routing","multiSelect":false,"options":[{"description":"the inbound path","label":"Route A"},{"description":"the outbound path","label":"Route B"}],"question":"which route for the queue?"}],"answers":{"which route for the queue?":"(notes only)"},"annotations":{"which route for the queue?":{"notes":"never conflate the two — Route A is inbound only, Route B is outbound only"}}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which route for the queue?\"=\"(notes only)\". You can now continue."}]}}"#,
        );
        let unit = r.auq_exchange().expect("auq exchange");
        // The placeholder answer is shown, but the real message (the notes) is what the
        // user actually said — it MUST be present and searchable.
        assert!(
            unit.contains("never conflate the two"),
            "notes (the user's real message) must surface: {unit}"
        );
        assert!(
            unit.contains("Route B is outbound only"),
            "full notes verbatim: {unit}"
        );
        // Options + descriptions still present alongside.
        assert!(unit.contains("Route A"));
        assert!(unit.contains("the outbound path"));
    }

    #[test]
    fn auq_answer_marker_only_fallback_is_a_boundary() {
        // Older-shape carrier: the synthesized marker is present but there is no
        // toolUseResult.answers → the marker fallback still classifies it a boundary.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"q\"=\"chosen label\". You can now continue."}]}}"#,
        );
        assert!(r.is_auq_answer_boundary());
        assert!(r.opens_turn());
        let unit = r.auq_exchange().expect("fallback exchange");
        assert!(unit.contains("AskUserQuestion"));
        assert!(unit.contains("chosen label"));
    }

    #[test]
    fn cancelled_auq_is_not_a_boundary() {
        // A rejected/cancelled AUQ: is_error:true, no answers, the generic rejection
        // marker WITHOUT a "the user said" tail → must NOT open a turn (no user message).
        let r = parse(
            r#"{"type":"user","toolUseResult":"User rejected tool use","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed."}]}}"#,
        );
        assert!(
            !r.is_auq_answer_boundary(),
            "cancelled AUQ is not a boundary"
        );
        assert!(
            !r.is_plan_rejection_boundary(),
            "no typed message → not a rejection boundary"
        );
        assert!(!r.opens_turn());
    }

    #[test]
    fn auq_validation_error_is_not_a_boundary() {
        // An InputValidationError AUQ result (is_error:true, <tool_use_error>…) is not an
        // answer → not a boundary.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","is_error":true,"content":"<tool_use_error>InputValidationError: AskUserQuestion failed…</tool_use_error>"}]}}"#,
        );
        assert!(!r.is_auq_answer_boundary());
        assert!(!r.opens_turn());
    }

    // ── §4.2.4 ExitPlanMode / tool-use rejection WITH a typed message = a genuine-user
    //    boundary + a plan pointer. Real-captured shape (the typed tail is the message). ──

    #[test]
    fn plan_rejection_with_typed_message_is_a_boundary() {
        // The user rejects the plan and types a follow-up instruction.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_PLANREJECT01","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). To tell you how to proceed, the user said:\nplease run the smoke tests once and diff the output before calling it done."}]}}"#,
        );
        assert!(r.is_plan_rejection_boundary());
        assert!(r.opens_turn());
        let (id, msg) = r.plan_rejection_message().expect("rejection message");
        assert_eq!(id.as_deref(), Some("toolu_PLANREJECT01"));
        // The genuine message is ONLY the typed tail (whole), not the synthesized prefix.
        assert_eq!(
            msg,
            "please run the smoke tests once and diff the output before calling it done."
        );
        assert!(!msg.contains("doesn't want to proceed"));
    }

    #[test]
    fn plan_rejection_with_english_message_is_a_boundary() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01YKnvDN43RnTQMGcw18HShW","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). To tell you how to proceed, the user said:\nRound-4 sign-off: run the e2e once more before declaring done."}]}}"#,
        );
        let (_, msg) = r.plan_rejection_message().expect("english rejection");
        assert_eq!(
            msg,
            "Round-4 sign-off: run the e2e once more before declaring done."
        );
    }

    #[test]
    fn plan_rejection_without_message_is_not_a_boundary() {
        // The `STOP what you are doing and wait…` form carries NO typed message → no
        // boundary (36 such records in the corpus).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed."}]}}"#,
        );
        assert!(r.plan_rejection_message().is_none());
        assert!(!r.is_plan_rejection_boundary());
        assert!(!r.opens_turn());
    }

    #[test]
    fn plan_approval_is_not_a_boundary() {
        // The approval path is the harness greenlight (no typed message, no is_error) —
        // must NOT become a turn boundary.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"User has approved your plan. You can now start coding. Your plan has been saved to: /Users/testuser/.claude/plans/elegant-scribbling-dream.md"}]}}"#,
        );
        assert!(!r.is_plan_rejection_boundary());
        assert!(!r.is_auq_answer_boundary());
        assert!(!r.opens_turn());
    }

    #[test]
    fn plan_rejection_surfaces_plan_pointer_via_index() {
        // The ExitPlanMode tool_use carries planFilePath; the rejection resolves it via
        // the PlanIndex → the reconstructed user text carries a `[plan: …]` pointer.
        let tool_use = parse(
            r##"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_PLAN","name":"ExitPlanMode","input":{"plan":"# the plan body","planFilePath":"/Users/testuser/.claude/plans/elegant-scribbling-dream.md"}}]}}"##,
        );
        let rejection = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_PLAN","is_error":true,"content":"The user doesn't want to proceed with this tool use. To tell you how to proceed, the user said:\nadd a screenshot check first"}]}}"#,
        );
        let index = PlanIndex::from_records([&tool_use]);
        assert_eq!(
            index.plan_path("toolu_PLAN"),
            Some("/Users/testuser/.claude/plans/elegant-scribbling-dream.md")
        );
        let text = rejection
            .reconstructed_user_text(Some(&index))
            .expect("reconstructed");
        assert!(text.starts_with("add a screenshot check first"));
        assert!(
            text.contains("[plan: /Users/testuser/.claude/plans/elegant-scribbling-dream.md]"),
            "plan pointer surfaced: {text}"
        );
    }

    #[test]
    fn plan_rejection_of_non_exit_plan_tool_has_no_pointer() {
        // A rejection-with-message whose tool_use_id is NOT an ExitPlanMode → still a
        // genuine-user boundary, but no plan pointer (the index does not resolve it).
        let rejection = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_EDIT","is_error":true,"content":"The user doesn't want to proceed with this tool use. To tell you how to proceed, the user said:\nuse a different file path"}]}}"#,
        );
        let index = PlanIndex::default(); // no ExitPlanMode indexed
        assert!(rejection.is_plan_rejection_boundary());
        let text = rejection.reconstructed_user_text(Some(&index)).unwrap();
        assert_eq!(text, "use a different file path");
        assert!(!text.contains("[plan:"));
    }

    #[test]
    fn exit_plan_pointers_extracts_id_and_path() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"p1","name":"ExitPlanMode","input":{"plan":"x","planFilePath":"/plans/a.md"}}]}}"#,
        );
        assert_eq!(
            r.exit_plan_pointers(),
            vec![("p1".to_string(), "/plans/a.md".to_string())]
        );
        // A non-ExitPlanMode record yields nothing.
        let other = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Read","input":{"file_path":"/x"}}]}}"#,
        );
        assert!(other.exit_plan_pointers().is_empty());
    }

    #[test]
    fn plan_index_skips_pointer_without_path() {
        // An ExitPlanMode with NO planFilePath is not indexed (empty path → no pointer).
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"p1","name":"ExitPlanMode","input":{"plan":"x"}}]}}"#,
        );
        let index = PlanIndex::from_records([&r]);
        assert!(index.plan_path("p1").is_none());
    }

    #[test]
    fn opens_turn_matches_genuine_user() {
        // A plain genuine user opens a turn; a plain tool_result carrier does not.
        let genuine = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
        assert!(genuine.opens_turn());
        let carrier = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok"}]}}"#,
        );
        assert!(!carrier.opens_turn());
    }

    // ── Automation-trigger classification (`<task-notification>`) ──

    #[test]
    fn automation_trigger_parses_task_notification() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>wh1it9jlj</task-id>\n<tool-use-id>toolu_x</tool-use-id>\n<output-file>/tmp/x.output</output-file>\n<status>completed</status>\n<summary>Dynamic workflow \"READ-ONLY: verify csift files\" completed</summary>\n</task-notification>"}}"#,
        );
        // It STILL opens a turn (it is a real boundary) — but is now classified.
        assert!(
            r.is_genuine_user(),
            "a task-notification passes the genuine-user gate"
        );
        assert!(r.opens_turn());
        let t = r.automation_trigger().expect("classified as automation");
        assert_eq!(t.task_id.as_deref(), Some("wh1it9jlj"));
        assert_eq!(t.status.as_deref(), Some("completed"));
        assert_eq!(
            t.kind,
            AutomationKind::Workflow,
            "Dynamic workflow → workflow"
        );
        assert_eq!(
            t.summary.as_deref(),
            Some("Dynamic workflow \"READ-ONLY: verify csift files\" completed")
        );
        // The rendered ATTRIBUTION label replaces the raw XML blob; a `Dynamic workflow`
        // summary keeps the `workflow` kind.
        let label = r.automation_label().unwrap();
        assert!(
            label.starts_with("[workflow wh1it9jlj completed]"),
            "got: {label}"
        );
        assert!(
            label.contains("Dynamic workflow"),
            "summary in label: {label}"
        );
        assert!(
            !label.contains("<task-id>"),
            "raw XML must not leak: {label}"
        );
    }

    #[test]
    fn automation_trigger_none_for_human_and_partial_graceful() {
        // A plain human message is NOT an automation trigger.
        let human =
            parse(r#"{"type":"user","message":{"role":"user","content":"please fix the bug"}}"#);
        assert!(human.automation_trigger().is_none());
        assert!(human.automation_label().is_none());
        // A partial notification (no summary/status) still labels gracefully with fallbacks.
        let partial = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>abc</task-id>\n</task-notification>"}}"#,
        );
        // No summary → kind falls back to `task` (NOT the old hardcoded `workflow`).
        let label = partial.automation_label().unwrap();
        assert_eq!(label, "[task abc completed]");
    }

    #[test]
    fn automation_kind_classifies_background_command_and_agent() {
        // The mislabel fix: a `Background command "…"` summary renders `background-command`,
        // an `Agent …` summary renders `agent` — NOT the old blanket `workflow`.
        let bg = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>b497m4ncp</task-id>\n<status>completed</status>\n<summary>Background command \"build venvs\" completed (exit code 0)</summary>\n</task-notification>"}}"#,
        );
        let t = bg.automation_trigger().unwrap();
        assert_eq!(t.kind, AutomationKind::BackgroundCommand);
        assert!(
            bg.automation_label()
                .unwrap()
                .starts_with("[background-command b497m4ncp completed]"),
            "got: {:?}",
            bg.automation_label()
        );
        let ag = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>ag1</task-id>\n<status>completed</status>\n<summary>Agent executor finished its task</summary>\n</task-notification>"}}"#,
        );
        assert_eq!(ag.automation_trigger().unwrap().kind, AutomationKind::Agent);
        assert!(ag
            .automation_label()
            .unwrap()
            .starts_with("[agent ag1 completed]"));
        // A failed background command keeps its kind + status.
        let bgf = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>b0855naiz</task-id>\n<status>failed</status>\n<summary>Background command \"Launch the overnight guard\" failed with exit code 1</summary>\n</task-notification>"}}"#,
        );
        assert!(bgf
            .automation_label()
            .unwrap()
            .starts_with("[background-command b0855naiz failed]"));
    }

    #[test]
    fn monitor_cadence_event_replaces_fabricated_completed_status() {
        // A real-captured monitor shape: a Monitor pulse with NO <status> but a real
        // <event> outcome. The label must surface the EVENT (STAGE2_OUTPUT_READY), not fabricate
        // `completed` — which would invert a timed-out monitor's attribution.
        let mon = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>b718g3gqq</task-id>\n<summary>Monitor event: \"full test suite re-run completion\"</summary>\n<event>STAGE2_OUTPUT_READY</event>\n</task-notification>"}}"#,
        );
        let t = mon.automation_trigger().expect("a monitor trigger");
        assert_eq!(t.kind, AutomationKind::Monitor);
        assert_eq!(t.status, None, "this monitor pulse carries no <status>");
        assert_eq!(t.event.as_deref(), Some("STAGE2_OUTPUT_READY"));
        let label = mon.automation_label().unwrap();
        assert!(
            label.starts_with("[monitor b718g3gqq STAGE2_OUTPUT_READY]"),
            "event must replace fabricated `completed`: {label}"
        );
        assert!(
            !label.contains("completed"),
            "no fabricated `completed` when an event is present: {label}"
        );

        // A timed-out monitor carries the timeout notice in <event> — also surfaced, never
        // inverted to `completed`.
        let timeout = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>q9</task-id>\n<summary>Monitor tick</summary>\n<event>[Monitor timed out — re-arm if needed.]</event>\n</task-notification>"}}"#,
        );
        let label2 = timeout.automation_label().unwrap();
        assert!(label2.contains("Monitor timed out"), "got: {label2}");
        assert!(!label2.contains("completed"), "got: {label2}");

        // When BOTH status and event are absent, the label still falls back to `completed`.
        let bare = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>z</task-id>\n<summary>Monitor tick</summary>\n</task-notification>"}}"#,
        );
        assert_eq!(
            bare.automation_label().unwrap(),
            "[monitor z completed] Monitor tick"
        );

        // An explicit <status> still wins over <event> (status is the more authoritative slot).
        let both = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>w</task-id>\n<status>failed</status>\n<summary>Monitor tick</summary>\n<event>SOME_EVENT</event>\n</task-notification>"}}"#,
        );
        assert!(both
            .automation_label()
            .unwrap()
            .starts_with("[monitor w failed]"));
    }

    #[test]
    fn automation_kind_from_summary_direct() {
        use AutomationKind::*;
        assert_eq!(
            AutomationKind::from_summary(Some("Background command \"x\"")),
            BackgroundCommand
        );
        assert_eq!(
            AutomationKind::from_summary(Some("Dynamic workflow \"x\"")),
            Workflow
        );
        assert_eq!(
            AutomationKind::from_summary(Some("workflow run done")),
            Workflow
        );
        assert_eq!(AutomationKind::from_summary(Some("Agent x done")), Agent);
        assert_eq!(
            AutomationKind::from_summary(Some("  background command y")),
            BackgroundCommand
        );
        // A monitor-COMPLETION `<task-notification>` (summary opens Monitor/Scheduled/cron)
        // is its own labeled class — the real `Monitor event: "…"` pulse (seen many times
        // across captures) must NOT fall to `task`. (This is NOT the isMeta ScheduleWakeup tick
        // PROMPT, which never reaches this summary classifier; see AutomationKind::Monitor docs.)
        assert_eq!(
            AutomationKind::from_summary(Some("Monitor event: \"full test suite re-run\"")),
            Monitor
        );
        assert_eq!(AutomationKind::from_summary(Some("Monitor tick")), Monitor);
        assert_eq!(
            AutomationKind::from_summary(Some("Scheduled wakeup fired")),
            Monitor
        );
        assert_eq!(AutomationKind::from_summary(Some("cron run")), Monitor);
        // The captured-monitor shape: a monitor/cron cadence implemented as a `&`-detached
        // `Background command "<monitor-named>"`. The quoted command NAME carrying a
        // monitor-cadence token routes to Monitor (not the generic BackgroundCommand), so the
        // dominant monitor activity is not disguised. (Verified against a captured session, where
        // the monitor loop is `Relaunch monitor timer` / `Re-arm corrected monitor` bg-cmds.)
        assert_eq!(
            AutomationKind::from_summary(Some(
                "Background command \"Relaunch monitor timer (cycle 2)\" completed"
            )),
            Monitor
        );
        assert_eq!(
            AutomationKind::from_summary(Some(
                "Background command \"Re-arm corrected monitor (full-tree liveness)\" completed"
            )),
            Monitor
        );
        assert_eq!(
            AutomationKind::from_summary(Some(
                "Background command \"nightly monitor tick (25min)\""
            )),
            Monitor
        );
        // PRECISION: a background command that merely mentions monitoring in PROSE (outside the
        // quoted name) or names an unrelated command stays BackgroundCommand — no over-capture.
        assert_eq!(
            AutomationKind::from_summary(Some(
                "Background command \"Run pre-commit gate\" completed (monitor it for failures)"
            )),
            BackgroundCommand
        );
        assert_eq!(
            AutomationKind::from_summary(Some("Background command \"Baseline release build\"")),
            BackgroundCommand
        );
        // The standalone-word guard: `monitoring`/`demonitor` are NOT the word `monitor`.
        assert_eq!(
            AutomationKind::from_summary(Some("Background command \"resource monitoring agent\"")),
            BackgroundCommand
        );
        assert_eq!(AutomationKind::from_summary(Some("something else")), Task);
        assert_eq!(AutomationKind::from_summary(None), Task);
        assert_eq!(AutomationKind::from_summary(Some("")), Task);
        // Slugs round-trip.
        assert_eq!(BackgroundCommand.slug(), "background-command");
        assert_eq!(Workflow.slug(), "workflow");
        assert_eq!(Agent.slug(), "agent");
        assert_eq!(Monitor.slug(), "monitor");
        assert_eq!(Task.slug(), "task");
    }

    #[test]
    fn automation_trigger_multibyte_summary_codepoint_safe() {
        // A multi-byte summary body must not be split mid-codepoint by the tag extractor.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>zh1</task-id>\n<status>completed</status>\n<summary>🤖 batch shipped, please summarize 🎉</summary>\n</task-notification>"}}"#,
        );
        let t = r.automation_trigger().unwrap();
        assert_eq!(
            t.summary.as_deref(),
            Some("🤖 batch shipped, please summarize 🎉")
        );
    }

    #[test]
    fn extract_xml_tag_handles_missing_and_empty() {
        assert_eq!(extract_xml_tag("<a>x</a>", "a").as_deref(), Some("x"));
        assert_eq!(extract_xml_tag("<a></a>", "a"), None); // empty inner → None
        assert_eq!(extract_xml_tag("<a>x", "a"), None); // missing close → None
        assert_eq!(extract_xml_tag("no tags here", "a"), None);
    }

    #[test]
    fn plural_word_pick_pinned() {
        // Mutation pin: `"s"` for anything but exactly one.
        assert_eq!(plural(0), "s");
        assert_eq!(plural(1), "");
        assert_eq!(plural(2), "s");
    }

    #[test]
    fn monitor_cadence_tokens_route_each_disjunct() {
        // Mutation pin: each cadence token ALONE routes a `Background command "…"` pulse to
        // Monitor (the disjuncts must stay independent), and a plain name stays bg-command.
        for s in [
            r#"Background command "nightly monitor tick (25min)" completed"#,
            r#"Background command "liveness probe" completed"#,
            r#"Background command "Re-arm corrected watchdog" completed"#,
            r#"Background command "Relaunch monitor timer (cycle 2)" completed"#,
        ] {
            assert_eq!(
                AutomationKind::from_summary(Some(s)),
                AutomationKind::Monitor,
                "{s}"
            );
        }
        assert_eq!(
            AutomationKind::from_summary(Some(r#"Background command "build project" completed"#)),
            AutomationKind::BackgroundCommand,
            "a plain quoted name stays background-command"
        );
        // The word must be STANDALONE — a substring inside a larger word is not the signal.
        assert_eq!(
            AutomationKind::from_summary(Some(
                r#"Background command "monitoring-dashboard build" completed"#
            )),
            AutomationKind::BackgroundCommand,
            "substring 'monitor' inside a larger word must not route"
        );
    }

    #[test]
    fn automation_label_failed_status_and_no_summary() {
        // A non-`completed` status is rendered verbatim (the status arm is not hardcoded).
        let failed = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>bad7</task-id>\n<status>failed</status>\n<summary>The build broke</summary>\n</task-notification>"}}"#,
        );
        // "The build broke" carries no kind classifier → `task` fallback.
        assert_eq!(
            failed.automation_label().as_deref(),
            Some("[task bad7 failed] The build broke")
        );
        // A trigger with a status but EMPTY summary → the head-only arm (no trailing text).
        let no_sum = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>q9</task-id>\n<status>running</status>\n<summary></summary>\n</task-notification>"}}"#,
        );
        assert_eq!(
            no_sum.automation_label().as_deref(),
            Some("[task q9 running]")
        );
        // A trigger with NO task-id and NO status → both `?`/`completed` fallbacks fire.
        let bare = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<summary>just a note</summary>\n</task-notification>"}}"#,
        );
        assert_eq!(
            bare.automation_label().as_deref(),
            Some("[task ? completed] just a note")
        );
    }

    #[test]
    fn is_synthetic_user_marker_matches_each_form() {
        assert!(is_synthetic_user_marker("[Request interrupted by user]"));
        assert!(is_synthetic_user_marker(
            "[Request interrupted by user for tool use]"
        ));
        assert!(is_synthetic_user_marker("<local-command-stdout>anything"));
        assert!(is_synthetic_user_marker("<command-name>/x</command-name>"));
        assert!(!is_synthetic_user_marker("a normal human message"));
        // Exact-match for interrupts: a longer string merely starting with the marker is
        // NOT excluded as an interrupt.
        assert!(!is_synthetic_user_marker(
            "[Request interrupted by user] and then I said more"
        ));
    }

    #[test]
    fn bash_command_some_and_none() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"rm -rf /tmp/x","description":"clean"}}]}}"#,
        );
        assert_eq!(r.bash_command(), Some("rm -rf /tmp/x"));
        // A Bash tool_use with NO command → None.
        let no_cmd = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"description":"x"}}]}}"#,
        );
        assert!(no_cmd.bash_command().is_none());
        // A non-Bash tool_use → None.
        let other = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/p/x"}}]}}"#,
        );
        assert!(other.bash_command().is_none());
        // A record with no blocks → None.
        let no_blocks = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
        assert!(no_blocks.bash_command().is_none());
    }

    // ============================================================================
    // role.class.sub classification engine (GOLD §2–§6) — P1 unit tests.
    // ============================================================================

    /// A spawn lookup stub: `toolu_spawn` spawned `child-abc`; the teammate name `VSRepro`
    /// resolves to its name-embedded id. Everything else is unknown (graceful degrade path).
    struct FakeSpawn;
    impl SpawnLookup for FakeSpawn {
        fn child_for_spawn_tool_use_id(&self, id: &str) -> Option<String> {
            (id == "toolu_spawn").then(|| "child-abc".to_string())
        }
        fn child_for_spawn_name(&self, name: &str) -> Option<String> {
            (name == "VSRepro").then(|| "aVSRepro-deadbeef".to_string())
        }
    }

    // ── Class::path() + role() for EVERY variant (the canonical wire forms) ──

    #[test]
    fn class_path_for_every_variant() {
        let table = [
            (Class::UserMessage, "user.message"),
            (Class::UserAnswer, "user.answer"),
            (Class::UserRejection, "user.rejection"),
            (Class::AgentMessage, "agent.message"),
            (Class::AgentThinking, "agent.thinking"),
            (Class::AgentToolUse, "agent.tool.use"),
            (Class::AgentToolResult, "agent.tool.result"),
            (Class::CommInbox, "agent.communication.inbox"),
            (Class::CommSent, "agent.communication.sent"),
            (Class::CommSignal, "agent.communication.signal"),
            (Class::NotificationWorkflow, "harness.notification.workflow"),
            (Class::NotificationMonitor, "harness.notification.monitor"),
            (Class::NotificationSubagent, "harness.notification.subagent"),
            (
                Class::NotificationBackgroundCommand,
                "harness.notification.background-command",
            ),
            (Class::NotificationTask, "harness.notification.task"),
            (Class::CompactionSummary, "harness.compaction.summary"),
            (Class::CompactionBoundary, "harness.compaction.boundary"),
            (Class::CommandInvocation, "harness.command.invocation"),
            (Class::CommandStdout, "harness.command.stdout"),
            (Class::InterruptUser, "harness.interrupt.user"),
            (Class::InterruptTool, "harness.interrupt.tool"),
            (Class::ScheduleWakeup, "harness.schedule.wakeup"),
            (Class::ScheduleContinuation, "harness.schedule.continuation"),
            (Class::MetaHook, "harness.meta.hook"),
            (Class::MetaLoop, "harness.meta.loop"),
        ];
        for (c, p) in table {
            assert_eq!(c.path(), p, "path mismatch for {c:?}");
            // The role is always the first dot-segment of the path.
            let head = p.split('.').next().unwrap();
            assert_eq!(c.role().as_str(), head, "role/path head mismatch for {c:?}");
        }
        // No two leaves share a path (the selector space is unambiguous).
        let mut paths: Vec<&str> = table.iter().map(|(c, _)| c.path()).collect();
        paths.sort_unstable();
        let n = paths.len();
        paths.dedup();
        assert_eq!(paths.len(), n, "duplicate Class path");
    }

    #[test]
    fn all_classes_cover_the_enum() {
        // Class::ALL must list EVERY variant (the local table here is the independent oracle);
        // a variant added to the enum but missing from ALL is caught by the path/role coverage.
        for &c in Class::ALL {
            // path() is total + role()'s as_str() is the path head — exercised for every leaf.
            let head = c.path().split('.').next().unwrap();
            assert_eq!(c.role().as_str(), head, "role/path head mismatch for {c:?}");
        }
        // ALL has no duplicates and matches the verified table size (25 leaves).
        let mut seen: Vec<&str> = Class::ALL.iter().map(|c| c.path()).collect();
        seen.sort_unstable();
        let n = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), n, "duplicate in Class::ALL");
        assert_eq!(n, 25, "Class::ALL leaf count drifted");
    }

    #[test]
    fn role_as_str_and_class_role_partition() {
        assert_eq!(Role::User.as_str(), "user");
        assert_eq!(Role::Agent.as_str(), "agent");
        assert_eq!(Role::Harness.as_str(), "harness");
        // Spot-check the role partition.
        assert_eq!(Class::UserAnswer.role(), Role::User);
        assert_eq!(Class::CommSignal.role(), Role::Agent);
        assert_eq!(Class::AgentToolResult.role(), Role::Agent);
        assert_eq!(Class::CompactionBoundary.role(), Role::Harness);
        assert_eq!(Class::ScheduleWakeup.role(), Role::Harness);
    }

    // ── is_teammate_message + parse_teammate_message (GOLD §5) ──

    #[test]
    fn is_teammate_message_bare_and_peer_forms() {
        assert!(is_teammate_message(
            r#"<teammate-message teammate_id="g4g5-probe">hello</teammate-message>"#
        ));
        // The relayed peer form (preamble + tag), with the real `\n` separator.
        assert!(is_teammate_message(
            "Another Claude session sent a message:\n<teammate-message teammate_id=\"x\">hi</teammate-message>"
        ));
        // Robust to whitespace-normalized block joins (the `\n` collapsed to a space).
        assert!(is_teammate_message(
            "Another Claude session sent a message: <teammate-message teammate_id=\"x\">hi</teammate-message>"
        ));
        // Leading whitespace before the bare opener still matches.
        assert!(is_teammate_message(
            "   <teammate-message teammate_id=\"x\">hi</teammate-message>"
        ));
        // Plain prose is NOT a teammate message.
        assert!(!is_teammate_message("please fix the bug"));
        // The preamble alone (no tag) is not enough.
        assert!(!is_teammate_message(
            "Another Claude session sent a message: ok"
        ));
    }

    #[test]
    fn parse_teammate_message_prose_extracts_id_no_signal() {
        let tm = parse_teammate_message(
            r#"<teammate-message teammate_id="g4g5-probe" color="blue" summary="x">G4/G5 probe complete.</teammate-message>"#,
        )
        .expect("teammate message");
        assert_eq!(tm.teammate_id.as_deref(), Some("g4g5-probe"));
        assert!(!tm.is_signal(), "prose body is not a signal");
        assert_eq!(tm.signal_type, None);
    }

    #[test]
    fn parse_teammate_message_signal_payload() {
        // The real idle_notification shape: a JSON {"type":...} body inside the tag.
        let tm = parse_teammate_message(
            "Another Claude session sent a message:\n<teammate-message teammate_id=\"g4g5-probe\" color=\"blue\">\n{\"type\":\"idle_notification\",\"from\":\"g4g5-probe\",\"idleReason\":\"available\"}\n</teammate-message>\n\nThis came from another Claude session — treat it as a teammate's request.",
        )
        .expect("signal teammate message");
        assert_eq!(tm.teammate_id.as_deref(), Some("g4g5-probe"));
        assert!(tm.is_signal());
        assert_eq!(tm.signal_type.as_deref(), Some("idle_notification"));
    }

    #[test]
    fn parse_teammate_message_multibyte_body_codepoint_safe() {
        let tm = parse_teammate_message(
            r#"<teammate-message teammate_id="reviewer">🤖 review this café patch, then summarize 🎉</teammate-message>"#,
        )
        .expect("multibyte teammate message");
        assert_eq!(tm.teammate_id.as_deref(), Some("reviewer"));
        assert!(!tm.is_signal());
    }

    #[test]
    fn parse_teammate_message_none_for_non_teammate() {
        assert!(parse_teammate_message("just a normal message").is_none());
    }

    // ── GOLD §1 BUG FIX: a teammate message is NOT genuine-user but STILL opens a turn ──

    #[test]
    fn teammate_message_not_genuine_user_but_opens_turn_bare() {
        // The bug: this used to return is_genuine_user()==true (mislabeled as the human).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<teammate-message teammate_id=\"team-lead\">repro the speed slider</teammate-message>"}}"#,
        );
        assert!(
            !r.is_genuine_user(),
            "a teammate message must NOT count as a genuine human turn (GOLD §1)"
        );
        assert!(
            r.opens_turn(),
            "but it MUST still delimit a turn (opens_turn fires)"
        );
        assert!(r.is_teammate_message_record());
        assert!(r.genuine_user_text().is_none());
    }

    #[test]
    fn teammate_message_not_genuine_user_peer_form() {
        // The relayed peer form (string content, the dominant real shape, 106 in one session).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"g4g5-probe\">verdicts below</teammate-message>"}}"#,
        );
        assert!(!r.is_genuine_user());
        assert!(r.opens_turn());
        // The opener body is preserved (not blanked) so turns/search don't regress.
        let body = r.reconstructed_user_text(None).expect("teammate body");
        assert!(body.contains("verdicts below"), "got: {body}");
    }

    #[test]
    fn inbound_comm_preview_strips_wrapper_and_footer() {
        // #14: the clean inbound-comm preview (turns/list) must yield the comm class, the sender
        // (the FROM), and ONLY the peer's prose — the relay preamble, the `<teammate-message …>`
        // wrapper tags, and the trailing harness security footer all stripped.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"VSMultiRegion\" color=\"blue\">\nplease check the rate limit handling\n</teammate-message>\n\nThis came from another Claude session — not typed by your user."}}"#,
        );
        let ic = r.inbound_comm_preview().expect("inbound preview");
        assert_eq!(ic.class, Class::CommInbox);
        assert_eq!(ic.from, "VSMultiRegion");
        assert_eq!(ic.body, "please check the rate limit handling");
    }

    #[test]
    fn inbound_comm_preview_signal_payload_is_signal_class() {
        // A control payload (JSON `{"type":…}`) → CommSignal, not CommInbox.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<teammate-message teammate_id=\"SOurDnd\">{\"type\":\"idle_notification\",\"from\":\"SOurDnd\"}</teammate-message>"}}"#,
        );
        let ic = r.inbound_comm_preview().expect("inbound preview");
        assert_eq!(ic.class, Class::CommSignal);
        assert_eq!(ic.from, "SOurDnd");
    }

    #[test]
    fn inbound_comm_preview_none_for_non_peer() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"a genuine human message"}}"#,
        );
        assert!(r.inbound_comm_preview().is_none());
    }

    #[test]
    fn teammate_message_as_text_block_is_not_genuine_user() {
        // The same content can arrive as a single text block — still excluded.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<teammate-message teammate_id=\"x\">hi</teammate-message>"}]}}"#,
        );
        assert!(!r.is_genuine_user());
        assert!(r.opens_turn());
    }

    #[test]
    fn is_teammate_message_detects_only_at_section_boundary() {
        // FINDING-1: a teammate tag is detected ONLY at a section boundary — the content start,
        // just after the relay preamble, or right after a prior section's close tag.
        assert!(is_teammate_message(
            r#"<teammate-message teammate_id="x">hi</teammate-message>"#
        ));
        assert!(is_teammate_message(
            "Another Claude session sent a message:\n<teammate-message teammate_id=\"x\">hi</teammate-message>"
        ));
        // Right after a prior section's close tag (a batched record).
        assert!(is_teammate_message(
            "<teammate-message teammate_id=\"a\">one</teammate-message>\n<teammate-message teammate_id=\"b\">two</teammate-message>"
        ));
        // A tag QUOTED mid-prose is NOT a teammate message (the FINDING-1 fix — was TRUE before).
        assert!(!is_teammate_message(
            "noise before <teammate-message teammate_id=\"x\">hi</teammate-message> noise after"
        ));
        assert!(!is_teammate_message("no tag at all"));
    }

    #[test]
    fn embedded_teammate_tag_mid_prose_stays_user_message() {
        // FINDING-1 (FLIPPED from the former accepted-tradeoff): a genuine user message that merely
        // QUOTES the tag mid-prose is NOT a peer message — it stays `user.message` (this bites
        // csift's OWN dev sessions, which quote the tag constantly).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"why does a <teammate-message ...> show up in my logs?"}}"#,
        );
        assert!(
            r.is_genuine_user(),
            "a quoted tag mid-prose is still genuine user"
        );
        assert!(r.opens_turn());
        assert!(!r.is_peer_message_record());
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::UserMessage]
        );
    }

    #[test]
    fn embedded_both_tags_mid_prose_stays_user_message() {
        // FINDING-1 acceptance: a user.message quoting BOTH `<task-notification>` AND
        // `<teammate-message>` mid-text classifies `user.message` ONLY — not harness.notification,
        // not agent.communication.inbox.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"In csift, the <task-notification> pulse and the <teammate-message peer form both route through classify()."}}"#,
        );
        assert!(r.is_genuine_user());
        assert!(!r.is_peer_message_record());
        assert!(r.automation_label().is_none());
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::UserMessage]
        );
    }

    #[test]
    fn agent_message_non_meta_excluded_opens_turn_inbox() {
        // FINDING-2: an `<agent-message from="…">` peer form (even non-isMeta) is NOT genuine-user,
        // STILL opens a turn, and classifies `agent.communication.inbox` (symmetry with teammate).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<agent-message from=\"oh-my-claudecode:architect\">use the shared resolver.</agent-message>"}}"#,
        );
        assert!(
            !r.is_genuine_user(),
            "an agent-message peer form must not count as a genuine human turn (FINDING-2)"
        );
        assert!(r.opens_turn(), "but it MUST still delimit a turn");
        assert!(r.is_peer_message_record());
        assert!(
            !r.is_teammate_message_record(),
            "it is the agent-message peer form, not teammate"
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::CommInbox]
        );
    }

    // ── classify(): single-label record shapes ──

    #[test]
    fn classify_genuine_user_message() {
        let r = parse(r#"{"type":"user","message":{"role":"user","content":"please fix it"}}"#);
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::UserMessage]
        );
    }

    #[test]
    fn classify_assistant_text_is_message() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Done."}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentMessage]
        );
    }

    #[test]
    fn classify_assistant_thinking_only() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm","signature":"s"}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentThinking]
        );
    }

    #[test]
    fn classify_assistant_redacted_thinking_only() {
        // #12 / oracle B3: a `redacted_thinking` block (opaque encrypted reasoning, no readable
        // text) classifies `agent.thinking` exactly like a normal thinking block. UNATTESTED in
        // the corpus → exercised by this SYNTHETIC fixture.
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"redacted_thinking","data":"EncryptedOpaqueBlob=="}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentThinking]
        );
    }

    #[test]
    fn classify_assistant_thinking_then_tool_use_multilabel() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"plan"},{"type":"tool_use","id":"t","name":"Bash","input":{"command":"ls"}}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentThinking, Class::AgentToolUse]
        );
    }

    #[test]
    fn classify_assistant_text_and_tool_use_multilabel() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"running it"},{"type":"tool_use","id":"t","name":"Read","input":{"file_path":"/x"}}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentMessage, Class::AgentToolUse]
        );
    }

    #[test]
    fn classify_assistant_bare_string_message() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":"surprise bare string"}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentMessage]
        );
    }

    // ── classify(): communication (GOLD §3 + §4) ──

    #[test]
    fn classify_sendmessage_message_is_tool_use_plus_sent() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"to":"ab9018739543b1df0","type":"message","message":"do the thing"}}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentToolUse, Class::CommSent]
        );
    }

    #[test]
    fn classify_sendmessage_direct_is_sent_not_signal() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"to":"x","type":"direct","message":"hi"}}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentToolUse, Class::CommSent]
        );
    }

    #[test]
    fn classify_sendmessage_no_type_defaults_to_sent() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"to":"x","message":"hi"}}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentToolUse, Class::CommSent]
        );
    }

    #[test]
    fn classify_sendmessage_shutdown_request_is_signal() {
        // Top-level type:"shutdown_request" (the real shape).
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"to":"S-Sync","type":"shutdown_request","recipient":"S-Sync","reason":"done"}}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentToolUse, Class::CommSignal]
        );
    }

    #[test]
    fn classify_sendmessage_nested_shutdown_payload_is_signal() {
        // The nested message:{type:shutdown_request} form (also real).
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"to":"x","message":{"type":"shutdown_request","reason":"done"}}}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentToolUse, Class::CommSignal]
        );
    }

    #[test]
    fn classify_spawn_tool_use_is_tool_use_plus_sent() {
        for tool in ["Task", "Agent", "Workflow"] {
            let r = parse(&format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t","name":"{tool}","input":{{"subagent_type":"executor","description":"go"}}}}]}}}}"#
            ));
            assert_eq!(
                r.classify(&ClassifyCtx::top_level()),
                vec![Class::AgentToolUse, Class::CommSent],
                "spawn tool {tool}"
            );
        }
    }

    #[test]
    fn classify_teammate_prose_is_inbox() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"reviewer\">verdict: LGTM</teammate-message>"}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::CommInbox]
        );
    }

    #[test]
    fn classify_teammate_signal_is_signal() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"peer\">\n{\"type\":\"idle_notification\",\"from\":\"peer\"}\n</teammate-message>"}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::CommSignal]
        );
    }

    // ── classify(): dual-label user carriers (GOLD §3) ──

    #[test]
    fn classify_auq_answer_is_user_answer_plus_tool_result() {
        let r = parse(
            r#"{"type":"user","toolUseResult":{"questions":[{"question":"which?","options":[{"label":"A"}]}],"answers":{"which?":"go with A"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"go with A\"."}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::UserAnswer, Class::AgentToolResult]
        );
    }

    #[test]
    fn classify_plan_rejection_is_user_rejection_plus_tool_result() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"p","is_error":true,"content":"The user doesn't want to proceed with this tool use. To tell you how to proceed, the user said:\nadd tests first"}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::UserRejection, Class::AgentToolResult]
        );
    }

    #[test]
    fn classify_plain_tool_result_carrier_is_tool_result_only() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok, ran fine"}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentToolResult]
        );
    }

    #[test]
    fn classify_subagent_return_is_tool_result_plus_inbox() {
        // A tool_result whose tool_use_id was a spawn → the subagent return (child ⇨ self).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_spawn","content":"subagent done: 3 files changed"}]}}"#,
        );
        let ctx = ClassifyCtx {
            spawn: Some(&FakeSpawn),
            ..ClassifyCtx::top_level()
        };
        assert_eq!(
            r.classify(&ctx),
            vec![Class::AgentToolResult, Class::CommInbox]
        );
        // Without a spawn lookup it is just a tool_result (no return detection).
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentToolResult]
        );
    }

    #[test]
    fn classify_teammate_spawn_ack_is_tool_result_only_not_inbox() {
        // Edge-fixtures correction #3: a persistent teammate `Agent` spawn's tool_result is an
        // immediate {status:"teammate_spawned"} ACK that shares the spawn tool_use_id (so the
        // lookup WOULD resolve it), but it is NOT the child's return → agent.tool.result ONLY.
        let r = parse(
            r#"{"type":"user","toolUseResult":{"status":"teammate_spawned","name":"P1-engine","agent_id":"aP1-engine-9cf2","agent_type":"executor"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_spawn","content":"teammate spawned"}]}}"#,
        );
        let ctx = ClassifyCtx {
            owner_id: Some("parent"),
            spawn: Some(&FakeSpawn),
            ..ClassifyCtx::top_level()
        };
        assert!(r.is_teammate_spawn_ack());
        assert_eq!(
            r.classify(&ctx),
            vec![Class::AgentToolResult],
            "the ACK must NOT carry agent.communication.inbox"
        );
        // And it produces NO child ⇨ self direction (the real reply arrives later).
        assert!(r.direction(&ctx).is_none());
    }

    #[test]
    fn classify_async_launch_ack_is_tool_result_only_not_inbox() {
        // Smoke-found bug: an ASYNC/background `Agent` spawn's tool_result is the LAUNCH ack
        // (`toolUseResult.{isAsync:true,status:"async_launched"}`, content begins "Async agent
        // launched successfully…"). It shares the spawn tool_use_id (so the lookup WOULD
        // resolve it), but it is NOT the child's return — the report arrives LATER via the
        // <task-notification> <result> (G1 → inbox). So agent.tool.result ONLY, no …inbox.
        let r = parse(
            r#"{"type":"user","toolUseResult":{"isAsync":true,"status":"async_launched","agentId":"ad8012462a52f5c25","description":"draft the fold"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_spawn","content":[{"type":"text","text":"Async agent launched successfully.\nagentId: ad8012462a52f5c25 (internal ID - do not mention to user.)"}]}]}}"#,
        );
        let ctx = ClassifyCtx {
            owner_id: Some("parent"),
            spawn: Some(&FakeSpawn),
            ..ClassifyCtx::top_level()
        };
        assert!(r.is_async_launch_ack());
        assert!(r.is_spawn_launch_ack());
        assert_eq!(
            r.classify(&ctx),
            vec![Class::AgentToolResult],
            "the async-launch ACK must NOT carry agent.communication.inbox"
        );
        // And it produces NO child ⇨ self direction (the real report arrives later).
        assert!(
            r.direction(&ctx).is_none(),
            "a launch ack carries no child ⇨ self direction"
        );
    }

    #[test]
    fn classify_async_launch_ack_detected_by_content_prefix_fallback() {
        // A record lacking the structured `toolUseResult` still detects the ack from the
        // tool_result content prefix alone — and still resolves to tool.result-only.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_spawn","content":"Async agent launched successfully.\nagentId: ad80 (internal ID)"}]}}"#,
        );
        let ctx = ClassifyCtx {
            spawn: Some(&FakeSpawn),
            ..ClassifyCtx::top_level()
        };
        assert!(r.is_async_launch_ack());
        assert_eq!(r.classify(&ctx), vec![Class::AgentToolResult]);
        assert!(r.direction(&ctx).is_none());
    }

    #[test]
    fn classify_sync_task_return_still_tool_result_plus_inbox_vs_async_ack() {
        // Contrast guard: a SYNC one-shot Task tool_result IS the child's reply (no ack shape,
        // no launch-ack prefix) → [agent.tool.result, agent.communication.inbox] with a child ⇨
        // self direction — the async-launch ACK fix must NOT regress this.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_spawn","content":"subagent done: 3 files changed"}]}}"#,
        );
        let ctx = ClassifyCtx {
            owner_id: Some("parent"),
            spawn: Some(&FakeSpawn),
            ..ClassifyCtx::top_level()
        };
        assert!(!r.is_spawn_launch_ack());
        assert_eq!(
            r.classify(&ctx),
            vec![Class::AgentToolResult, Class::CommInbox]
        );
        assert_eq!(
            r.direction(&ctx),
            Some(("child-abc".to_string(), "parent".to_string()))
        );
    }

    #[test]
    fn classify_teammate_terminated_signal_id_is_system() {
        // Edge-fixtures correction #5: a teammate_terminated payload has teammate_id="system"
        // on the attr; the dead agent is named in the BODY, not the attr. Direction FROM is
        // "system", never the dead agent.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<teammate-message teammate_id=\"system\">\n{\"type\":\"teammate_terminated\",\"message\":\"B38F1Check has shut down.\"}\n</teammate-message>"}}"#,
        );
        let tm = r.teammate_message().expect("teammate message");
        assert_eq!(tm.teammate_id.as_deref(), Some("system"));
        assert_eq!(tm.signal_type.as_deref(), Some("teammate_terminated"));
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::CommSignal]
        );
        let ctx = ClassifyCtx {
            owner_id: Some("me"),
            ..ClassifyCtx::top_level()
        };
        assert_eq!(
            r.direction(&ctx),
            Some(("system".to_string(), "me".to_string())),
            "FROM is system, NOT the dead agent named in the body"
        );
    }

    #[test]
    fn classify_shutdown_approved_signal() {
        // The real f-shutdown_approved shape (JSON body with requestId etc.).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<teammate-message teammate_id=\"B38F1Check\" color=\"yellow\">\n{\"type\":\"shutdown_approved\",\"requestId\":\"shutdown-1@B38F1Check\",\"from\":\"B38F1Check\",\"backendType\":\"in-process\"}\n</teammate-message>"}}"#,
        );
        let tm = r.teammate_message().unwrap();
        assert_eq!(tm.signal_type.as_deref(), Some("shutdown_approved"));
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::CommSignal]
        );
    }

    #[test]
    fn classify_sendmessage_real_shutdown_request_dict_message_shape() {
        // The exact real f-shutdown_request shape: top-level type=shutdown_request, message is
        // a DICT {type,reason} (polymorphic vs the string form), to==recipient = a NAME.
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"to":"GraftBoard","message":{"type":"shutdown_request","reason":"done"},"summary":"shut down","type":"shutdown_request","recipient":"GraftBoard","content":"done"}}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentToolUse, Class::CommSignal]
        );
        // Direction TO is the teammate NAME (not always a re-feedable id).
        assert_eq!(
            r.direction(&ClassifyCtx::top_level()),
            Some(("self".to_string(), "GraftBoard".to_string()))
        );
    }

    #[test]
    fn classify_subagent_opener_is_inbox_via_ctx() {
        // The spawn-prompt seed of a subagent transcript: a genuine-user-shaped record that
        // ctx reclassifies as parent ⇨ self inbox.
        let r = parse(
            r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"go repro the bug"}}"#,
        );
        let ctx = ClassifyCtx {
            is_subagent: true,
            is_transcript_opener: true,
            ..ClassifyCtx::top_level()
        };
        assert_eq!(r.classify(&ctx), vec![Class::CommInbox]);
        // The SAME record on a top-level transcript is a plain user message.
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::UserMessage]
        );
    }

    // ── classify(): harness records (GOLD §2/§5) ──

    #[test]
    fn classify_task_notification_each_kind() {
        // Summaries kept quote-free so they embed cleanly in the JSON content string; the
        // kind classifier keys on the leading word, which is unaffected.
        let cases = [
            ("Dynamic workflow x completed", Class::NotificationWorkflow),
            (
                "Background command build completed (exit code 0)",
                Class::NotificationBackgroundCommand,
            ),
            ("Agent executor finished", Class::NotificationSubagent),
            ("Monitor event tick", Class::NotificationMonitor),
            ("something unclassified", Class::NotificationTask),
        ];
        for (summary, want) in cases {
            let r = parse(&format!(
                r#"{{"type":"user","message":{{"role":"user","content":"<task-notification>\n<task-id>id1</task-id>\n<status>completed</status>\n<summary>{summary}</summary>\n</task-notification>"}}}}"#
            ));
            assert_eq!(
                r.classify(&ClassifyCtx::top_level()),
                vec![want],
                "summary: {summary}"
            );
        }
    }

    #[test]
    fn classify_compaction_summary_and_boundary() {
        let summary = parse(
            r#"{"type":"user","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"message":{"role":"user","content":"This session is being continued..."}}"#,
        );
        assert_eq!(
            summary.classify(&ClassifyCtx::top_level()),
            vec![Class::CompactionSummary]
        );
        let boundary = parse(
            r#"{"type":"system","subtype":"compact_boundary","compactMetadata":{"trigger":"auto","preTokens":1}}"#,
        );
        assert!(boundary.is_compact_boundary());
        assert_eq!(
            boundary.classify(&ClassifyCtx::top_level()),
            vec![Class::CompactionBoundary]
        );
    }

    #[test]
    fn classify_command_invocation_with_and_without_args() {
        let no_args = parse(
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-args></command-args>"}}"#,
        );
        assert_eq!(
            no_args.classify(&ClassifyCtx::top_level()),
            vec![Class::CommandInvocation]
        );
        let with_args = parse(
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-args>just shipped, summarize</command-args>"}}"#,
        );
        // v0.5: the prose is the RICHER view (richest-view law) — pushed FIRST so the
        // unfiltered record-text emission renders `/name args`, not the wrapper XML.
        assert_eq!(
            with_args.classify(&ClassifyCtx::top_level()),
            vec![Class::UserMessage, Class::CommandInvocation]
        );
    }

    #[test]
    fn classify_local_command_stdout() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<local-command-stdout>✓ done</local-command-stdout>"}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::CommandStdout]
        );
    }

    #[test]
    fn classify_interrupts_user_and_tool() {
        let u = parse(
            r#"{"type":"user","message":{"role":"user","content":"[Request interrupted by user]"}}"#,
        );
        assert_eq!(
            u.classify(&ClassifyCtx::top_level()),
            vec![Class::InterruptUser]
        );
        let t = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user for tool use]"}]}}"#,
        );
        assert_eq!(
            t.classify(&ClassifyCtx::top_level()),
            vec![Class::InterruptTool]
        );
    }

    #[test]
    fn classify_schedule_continuation_and_wakeup() {
        let cont = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":[{"type":"text","text":"Continue from where you left off."}]}}"#,
        );
        assert_eq!(
            cont.classify(&ClassifyCtx::top_level()),
            vec![Class::ScheduleContinuation]
        );
        let wake = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<<autonomous-loop-dynamic>>"}}"#,
        );
        assert_eq!(
            wake.classify(&ClassifyCtx::top_level()),
            vec![Class::ScheduleWakeup]
        );
    }

    // ── G2: harness.meta.{hook, loop} + isMeta image exclusion ──

    #[test]
    fn classify_meta_hook_variants() {
        // Stop-hook feedback (the dominant shape: feedback + the edit-failed-retry body).
        let stop = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Stop hook feedback:\nThe last Edit failed because the target file was modified."}}"#,
        );
        assert_eq!(
            stop.classify(&ClassifyCtx::top_level()),
            vec![Class::MetaHook]
        );
        // <local-command-caveat> wrapper (isMeta) — previously fell through to user.message.
        let caveat = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>Caveat: the messages below were generated by a command.</local-command-caveat>"}}"#,
        );
        assert_eq!(
            caveat.classify(&ClassifyCtx::top_level()),
            vec![Class::MetaHook]
        );
        // The bare edit-failed-retry notice (matched anywhere).
        let edit = parse(
            r#"{"type":"user","message":{"role":"user","content":"The last Edit failed because the target file was modified since it was read."}}"#,
        );
        assert_eq!(
            edit.classify(&ClassifyCtx::top_level()),
            vec![Class::MetaHook]
        );
    }

    #[test]
    fn classify_meta_loop_variants() {
        // NB: `r##"…"##` delimiter — the JSON content has `:"# ` whose `"#` would close a
        // plain `r#"…"#` raw string early.
        let tick = parse(
            r##"{"type":"user","isMeta":true,"message":{"role":"user","content":"# Autonomous loop tick\nproceed with the next step."}}"##,
        );
        assert_eq!(
            tick.classify(&ClassifyCtx::top_level()),
            vec![Class::MetaLoop]
        );
        let check = parse(
            r#"{"type":"user","message":{"role":"user","content":"Run the autonomous check and continue."}}"#,
        );
        assert_eq!(
            check.classify(&ClassifyCtx::top_level()),
            vec![Class::MetaLoop]
        );
        // The schedule.wakeup sentinel stays its OWN class (not folded into meta.loop).
        let wake = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<<autonomous-loop-dynamic>>"}}"#,
        );
        assert_eq!(
            wake.classify(&ClassifyCtx::top_level()),
            vec![Class::ScheduleWakeup]
        );
    }

    #[test]
    fn classify_ismeta_image_record_is_excluded() {
        // G2: an isMeta "[Image: source:…]" pseudo-record is EXCLUDED (no label), never
        // mislabeled user.message.
        let r = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"[Image: source: /Users/x/.claude/image-cache/abc.png]"}}"#,
        );
        assert!(r.classify(&ClassifyCtx::top_level()).is_empty());
    }

    // ── G1: a notification carrying a <result> is ALSO an inbound report (child ⇨ parent) ──

    #[test]
    fn classify_notification_with_result_dual_labels_inbox() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>w1</task-id>\n<tool-use-id>toolu_spawn</tool-use-id>\n<status>completed</status>\n<summary>Agent executor finished</summary>\n<result>the agent's real report body</result>\n</task-notification>"}}"#,
        );
        // notification.subagent (Agent kind) + the child⇨parent inbox dual-label.
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::NotificationSubagent, Class::CommInbox]
        );
        // Direction resolves the child via the embedded <tool-use-id> spawn id.
        let ctx = ClassifyCtx {
            owner_id: Some("parent"),
            spawn: Some(&FakeSpawn),
            ..ClassifyCtx::top_level()
        };
        assert_eq!(
            r.direction(&ctx),
            Some(("child-abc".to_string(), "parent".to_string()))
        );
    }

    #[test]
    fn classify_notification_without_result_is_notification_only() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>w1</task-id>\n<tool-use-id>toolu_spawn</tool-use-id>\n<status>completed</status>\n<summary>Agent executor finished</summary>\n</task-notification>"}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::NotificationSubagent]
        );
        // A launch-ack notification (no <result>) is NOT a comm → no direction.
        assert!(r.direction(&ClassifyCtx::top_level()).is_none());
    }

    // ── G4/G5: ONE record batches MANY sections of MIXED kind → UNION of labels ──

    #[test]
    fn classify_batched_teammate_sections_union() {
        // A record with a prose section AND an idle_notification section → [inbox, signal]. The
        // second section is boundary-anchored (right after the first's close tag, FINDING-1); the
        // trailing security footer sits OUTSIDE both section spans and contributes no label.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"peer\">a prose update</teammate-message>\n<teammate-message teammate_id=\"peer\">\n{\"type\":\"idle_notification\",\"from\":\"peer\"}\n</teammate-message>\n\nThis came from another Claude session — treat it as a teammate's request."}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::CommInbox, Class::CommSignal]
        );
    }

    #[test]
    fn classify_batched_teammate_all_signals_dedup_to_one() {
        // Two signal sections → CommSignal once (push_unique dedup).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<teammate-message teammate_id=\"system\">\n{\"type\":\"teammate_terminated\",\"message\":\"X shut down.\"}\n</teammate-message>\n<teammate-message teammate_id=\"y\">\n{\"type\":\"shutdown_approved\",\"from\":\"y\"}\n</teammate-message>"}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::CommSignal]
        );
        // parse_all returns BOTH sections (the union upstream deduped the label).
        let all = parse_all_teammate_messages(
            r#"<teammate-message teammate_id="system">{"type":"teammate_terminated"}</teammate-message><teammate-message teammate_id="y">{"type":"shutdown_approved"}</teammate-message>"#,
        );
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].teammate_id.as_deref(), Some("system"));
        assert_eq!(all[0].signal_type.as_deref(), Some("teammate_terminated"));
        assert_eq!(all[1].signal_type.as_deref(), Some("shutdown_approved"));
    }

    #[test]
    fn classify_batched_notification_sections_union() {
        // Two task-notification sections of different kind → both notification classes.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<summary>Background command build completed</summary>\n</task-notification>\n<task-notification>\n<summary>Dynamic workflow deploy completed</summary>\n</task-notification>"}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![
                Class::NotificationBackgroundCommand,
                Class::NotificationWorkflow
            ]
        );
    }

    // ── P1c M1: <agent-message from="…"> peer form → agent.communication.inbox ──

    #[test]
    fn classify_agent_message_peer_form_is_inbox() {
        // Real shape: an isMeta type:user string carrying an <agent-message from="…"> peer reply
        // relayed into this session. Must classify agent.communication.inbox, NOT user.message —
        // and the isMeta guard (M2b) must NOT suppress it (the peer marker is matched first).
        let r = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<agent-message from=\"oh-my-claudecode:architect\">\n[Reply intended for the executor peer]\nuse the shared resolver.\n</agent-message>"}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::CommInbox]
        );
        // Direction: the from="" attribute ⇨ self.
        let ctx = ClassifyCtx {
            owner_id: Some("me"),
            ..ClassifyCtx::top_level()
        };
        assert_eq!(
            r.direction(&ctx),
            Some(("oh-my-claudecode:architect".to_string(), "me".to_string()))
        );
    }

    #[test]
    fn classify_agent_message_no_from_degrades_to_peer_direction() {
        let r = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<agent-message>no sender attr</agent-message>"}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::CommInbox]
        );
        assert_eq!(
            r.direction(&ClassifyCtx::top_level()),
            Some(("peer".to_string(), "self".to_string()))
        );
    }

    // ── P1c M2a: fired autonomous-loop / ScheduleWakeup timer tick → harness.schedule.wakeup ──

    #[test]
    fn classify_schedule_wakeup_fired_timer_markers() {
        // The real oracle-D12 record: isMeta, header "# Autonomous loop check", body "You're
        // being invoked on a timer …". Used to fall through to user.message (the M2 mislabel).
        let loop_check = parse(
            r##"{"type":"user","isMeta":true,"message":{"role":"user","content":"# Autonomous loop check\n\nYou're being invoked on a timer while the user is away."}}"##,
        );
        assert_eq!(
            loop_check.classify(&ClassifyCtx::top_level()),
            vec![Class::ScheduleWakeup]
        );
        // The body sentence alone (no header) also routes to schedule.wakeup.
        let timer_only = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"You're being invoked on a timer to keep work moving."}}"#,
        );
        assert_eq!(
            timer_only.classify(&ClassifyCtx::top_level()),
            vec![Class::ScheduleWakeup]
        );
    }

    #[test]
    fn classify_wakeup_check_vs_loop_tick_no_collision() {
        // "# Autonomous loop check" → schedule.wakeup; "# Autonomous loop tick" → meta.loop. The
        // two share the "# Autonomous loop " prefix but diverge at check/tick — must NOT collide.
        let check = parse(
            r##"{"type":"user","isMeta":true,"message":{"role":"user","content":"# Autonomous loop check\nproceed."}}"##,
        );
        assert_eq!(
            check.classify(&ClassifyCtx::top_level()),
            vec![Class::ScheduleWakeup]
        );
        let tick = parse(
            r##"{"type":"user","isMeta":true,"message":{"role":"user","content":"# Autonomous loop tick\nproceed."}}"##,
        );
        assert_eq!(
            tick.classify(&ClassifyCtx::top_level()),
            vec![Class::MetaLoop]
        );
        // The sentinel stays schedule.wakeup; "Run the autonomous check" stays meta.loop.
        let sentinel = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<<autonomous-loop-dynamic>>"}}"#,
        );
        assert_eq!(
            sentinel.classify(&ClassifyCtx::top_level()),
            vec![Class::ScheduleWakeup]
        );
        let run_check = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Run the autonomous check now."}}"#,
        );
        assert_eq!(
            run_check.classify(&ClassifyCtx::top_level()),
            vec![Class::MetaLoop]
        );
    }

    // ── P1c M2b: an isMeta record matching no marker is EXCLUDED (never user.message) ──

    #[test]
    fn classify_ismeta_unmarked_record_is_excluded() {
        // A genuine user.message is NEVER isMeta. An isMeta record matching no marker (a novel
        // harness pseudo-turn / generic cron tick) must emit NOTHING, not fall to user.message.
        let r = parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"some unrecognized harness-injected pseudo prose"}}"#,
        );
        let labels = r.classify(&ClassifyCtx::top_level());
        assert!(
            labels.is_empty(),
            "isMeta unmarked must be excluded: {labels:?}"
        );
        // The role-level gate agrees (it is not a genuine user either).
        assert!(!r.is_genuine_user());
    }

    #[test]
    fn classify_nonmeta_unmarked_prose_is_user_message() {
        // The complement: non-isMeta unmarked prose is STILL user.message (M2b is isMeta-scoped).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"please refactor the parser"}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::UserMessage]
        );
    }

    #[test]
    fn classify_ismeta_subagent_opener_still_inbox() {
        // The subagent-opener inbox case is unchanged by M2b — even an isMeta seed → CommInbox
        // (the opener check precedes the isMeta guard).
        let r = parse(
            r#"{"type":"user","isMeta":true,"isSidechain":true,"message":{"role":"user","content":"go map the bridge"}}"#,
        );
        let ctx = ClassifyCtx {
            is_subagent: true,
            is_transcript_opener: true,
            ..ClassifyCtx::top_level()
        };
        assert_eq!(r.classify(&ctx), vec![Class::CommInbox]);
    }

    // ── P1c M3: task-notification precedence over a quoted teammate tag + cross-family union ──

    #[test]
    fn classify_notification_quoting_teammate_tag_stays_notification() {
        // M3a: a <task-notification> whose <result> body merely QUOTES "<teammate-message" must
        // stay harness.notification.* — the quoted tag (inside the notification span) is masked,
        // so no spurious teammate comm label/direction leaks. The CommInbox here is the G1
        // <result> dual-label (child ⇨ self), NOT a teammate-derived label.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>w1</task-id>\n<tool-use-id>toolu_spawn</tool-use-id>\n<summary>Agent executor finished</summary>\n<result>I sent a <teammate-message teammate_id=\"peer\"> earlier</result>\n</task-notification>"}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::NotificationSubagent, Class::CommInbox]
        );
        // Direction is child ⇨ self (G1 via <tool-use-id>), NOT the quoted teammate "peer".
        let ctx = ClassifyCtx {
            owner_id: Some("parent"),
            spawn: Some(&FakeSpawn),
            ..ClassifyCtx::top_level()
        };
        assert_eq!(
            r.direction(&ctx),
            Some(("child-abc".to_string(), "parent".to_string()))
        );
    }

    #[test]
    fn classify_notification_no_result_quoting_teammate_has_no_comm() {
        // M3a without G1: a launch-ack notification (no <result>) that quotes the tag →
        // notification ONLY, NO comm label and NO direction (the quoted tag is masked).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>w1</task-id>\n<summary>Background command grep <teammate-message done</summary>\n</task-notification>"}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::NotificationBackgroundCommand]
        );
        assert!(r.direction(&ClassifyCtx::top_level()).is_none());
    }

    #[test]
    fn classify_cross_family_notification_and_teammate_union() {
        // M3b: a record carrying a REAL <task-notification> section AND a REAL <teammate-message>
        // section (outside the notification span) → UNION both families' labels.
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<summary>Background command build completed</summary>\n</task-notification>\nAnother Claude session sent a message:\n<teammate-message teammate_id=\"peer\">heads up, merged</teammate-message>"}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::NotificationBackgroundCommand, Class::CommInbox]
        );
    }

    #[test]
    fn classify_cross_family_notification_result_plus_teammate_signal() {
        // M3b: notification-with-<result> (→ notification + G1 inbox) AND a teammate idle-signal
        // section after it → [notification, inbox, signal] (inbox deduped to one).
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<summary>Dynamic workflow deploy completed</summary>\n<result>done</result>\n</task-notification>\n<teammate-message teammate_id=\"peer\">\n{\"type\":\"idle_notification\",\"from\":\"peer\"}\n</teammate-message>"}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![
                Class::NotificationWorkflow,
                Class::CommInbox,
                Class::CommSignal
            ]
        );
    }

    #[test]
    fn classify_elicitation_pending_and_resolved() {
        let pending = parse(
            r#"{"type":"assistant","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"k","message":{"role":"assistant","content":[{"type":"tool_use","id":"q","name":"AskUserQuestion","input":{}}]}}"#,
        );
        assert_eq!(
            pending.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentToolUse]
        );
        // An MCP pending marker is a system record with NO tool_use block — still tool.use.
        let mcp = parse(
            r#"{"type":"system","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"mcp-elicitation","csiftKey":"srv","content":"elicitation"}"#,
        );
        assert_eq!(
            mcp.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentToolUse]
        );
        let resolved = parse(
            r#"{"type":"csift-elicitation-resolved","csift":"elicitation-marker-v1","csiftPhase":"resolved","csiftKey":"k"}"#,
        );
        assert!(
            resolved.classify(&ClassifyCtx::top_level()).is_empty(),
            "a resolved marker is a pairing artifact, no label"
        );
    }

    #[test]
    fn classify_unmodeled_records_are_empty() {
        // A system away_summary is outside the taxonomy → no labels (never crash).
        let sys = parse(r#"{"type":"system","subtype":"away_summary","content":"gone 5m"}"#);
        assert!(sys.classify(&ClassifyCtx::top_level()).is_empty());
        // A metadata-only record likewise.
        let meta = parse(r#"{"type":"last-prompt","leafUuid":"x"}"#);
        assert!(meta.classify(&ClassifyCtx::top_level()).is_empty());
        // An image-only user record (no text, no tool_result) → empty.
        let img = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{}}]}}"#,
        );
        assert!(img.classify(&ClassifyCtx::top_level()).is_empty());
    }

    #[test]
    fn classify_dedups_repeated_labels() {
        // Two SendMessage blocks in one record → each label appears ONCE.
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"a","name":"SendMessage","input":{"to":"x","message":"1"}},{"type":"tool_use","id":"b","name":"SendMessage","input":{"to":"y","message":"2"}}]}}"#,
        );
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentToolUse, Class::CommSent]
        );
    }

    // ── direction() (GOLD §4) ──

    #[test]
    fn direction_teammate_inbox_from_peer_to_self() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"g4g5-probe\">verdicts</teammate-message>"}}"#,
        );
        let ctx = ClassifyCtx {
            owner_id: Some("session-uuid-1"),
            ..ClassifyCtx::top_level()
        };
        assert_eq!(
            r.direction(&ctx),
            Some(("g4g5-probe".to_string(), "session-uuid-1".to_string()))
        );
        // Without an owner id, the self side falls back to the literal "self".
        assert_eq!(
            r.direction(&ClassifyCtx::top_level()),
            Some(("g4g5-probe".to_string(), "self".to_string()))
        );
    }

    #[test]
    fn direction_sendmessage_self_to_recipient() {
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"to":"ab9018739543b1df0","message":"hi"}}]}}"#,
        );
        let ctx = ClassifyCtx {
            owner_id: Some("me"),
            ..ClassifyCtx::top_level()
        };
        assert_eq!(
            r.direction(&ctx),
            Some(("me".to_string(), "ab9018739543b1df0".to_string()))
        );
    }

    #[test]
    fn direction_sendmessage_recipient_fallback_field() {
        // No `to`, only `recipient`.
        let r = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"recipient":"team-lead","type":"shutdown_response","approve":true}}]}}"#,
        );
        assert_eq!(
            r.direction(&ClassifyCtx::top_level()),
            Some(("self".to_string(), "team-lead".to_string()))
        );
    }

    #[test]
    fn direction_spawn_resolves_child_via_lookup_then_degrades() {
        // id-join hit.
        let by_id = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_spawn","name":"Task","input":{"subagent_type":"executor"}}]}}"#,
        );
        let ctx = ClassifyCtx {
            owner_id: Some("parent"),
            spawn: Some(&FakeSpawn),
            ..ClassifyCtx::top_level()
        };
        assert_eq!(
            by_id.direction(&ctx),
            Some(("parent".to_string(), "child-abc".to_string()))
        );
        // name-join hit (the teammate spawn: meta has no toolUseId, joins by input.name).
        let by_name = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"other","name":"Agent","input":{"name":"VSRepro","subagent_type":"qa-tester"}}]}}"#,
        );
        assert_eq!(
            by_name.direction(&ctx),
            Some(("parent".to_string(), "aVSRepro-deadbeef".to_string()))
        );
        // No lookup at all → degrade to the raw spawn name.
        assert_eq!(
            by_name.direction(&ClassifyCtx::top_level()),
            Some(("self".to_string(), "VSRepro".to_string()))
        );
    }

    #[test]
    fn direction_subagent_return_child_to_self() {
        let r = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_spawn","content":"done"}]}}"#,
        );
        let ctx = ClassifyCtx {
            owner_id: Some("parent"),
            spawn: Some(&FakeSpawn),
            ..ClassifyCtx::top_level()
        };
        assert_eq!(
            r.direction(&ctx),
            Some(("child-abc".to_string(), "parent".to_string()))
        );
    }

    #[test]
    fn direction_subagent_opener_parent_to_self() {
        let r = parse(
            r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"go do the thing"}}"#,
        );
        let ctx = ClassifyCtx {
            owner_id: Some("child-id"),
            parent_id: Some("parent-id"),
            is_subagent: true,
            is_transcript_opener: true,
            ..ClassifyCtx::top_level()
        };
        assert_eq!(
            r.direction(&ctx),
            Some(("parent-id".to_string(), "child-id".to_string()))
        );
    }

    #[test]
    fn direction_none_for_non_comm_records() {
        let user = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
        assert!(user.direction(&ClassifyCtx::top_level()).is_none());
        let agent = parse(
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#,
        );
        assert!(agent.direction(&ClassifyCtx::top_level()).is_none());
        // A plain (non-spawn) tool_result is not a comm without a spawn match.
        let tr = parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok"}]}}"#,
        );
        assert!(tr.direction(&ClassifyCtx::top_level()).is_none());
    }

    // ── send_message_is_signal helper edge arms ──

    #[test]
    fn send_message_is_signal_arms() {
        use serde_json::json;
        assert!(!send_message_is_signal(Some(&json!({"type":"message"}))));
        assert!(!send_message_is_signal(Some(&json!({"type":"direct"}))));
        assert!(send_message_is_signal(Some(
            &json!({"type":"shutdown_request"})
        )));
        assert!(send_message_is_signal(Some(
            &json!({"message":{"type":"shutdown_response"}})
        )));
        assert!(!send_message_is_signal(Some(&json!({"to":"x"})))); // no type → message
        assert!(!send_message_is_signal(None));
    }

    #[test]
    fn classify_ctx_debug_renders_spawn_presence() {
        let ctx = ClassifyCtx {
            spawn: Some(&FakeSpawn),
            ..ClassifyCtx::top_level()
        };
        let dbg = format!("{ctx:?}");
        assert!(dbg.contains("has_spawn_lookup: true"), "got: {dbg}");
    }

    #[test]
    fn hook_additional_context_text_shapes_and_classify() {
        // Array content (the real on-disk shape) joins with `\n`; classify → MetaHook only.
        let arr: Record = serde_json::from_str(
            r#"{"type":"attachment","uuid":"x1","attachment":{"type":"hook_additional_context","content":["alpha block","beta block"],"hookEvent":"SessionStart"}}"#,
        )
        .unwrap();
        assert_eq!(
            arr.hook_additional_context_text().as_deref(),
            Some("alpha block\nbeta block")
        );
        let ctx = ClassifyCtx::top_level();
        assert_eq!(arr.classify(&ctx), vec![Class::MetaHook]);
        assert!(!arr.opens_turn());

        // Bare-string content is tolerated (trimmed); a different attachment type is None.
        let s: Record = serde_json::from_str(
            r#"{"type":"attachment","attachment":{"type":"hook_additional_context","content":" solo "}}"#,
        )
        .unwrap();
        assert_eq!(s.hook_additional_context_text().as_deref(), Some("solo"));
        let other: Record = serde_json::from_str(
            r#"{"type":"attachment","attachment":{"type":"file_snapshot","content":"zz"}}"#,
        )
        .unwrap();
        assert_eq!(other.hook_additional_context_text(), None);
        assert!(other.classify(&ctx).is_empty());
    }
}
