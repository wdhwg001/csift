//! The serde record model: Record / Message / Content / Block (+ probes).

use super::*;

/// A single parsed jsonl line. Unknown top-level fields are ignored by serde.
///
/// Several fields below are deserialized for completeness of the documented record
/// model (SPEC §3.2) and to keep parsing tolerant, but are not (yet) read by any
/// handler - e.g. `parent_uuid` (the §6.4 round-trip reconstruction keys on file
/// order + genuine-user delimiting, not the uuid tree), `is_sidechain`,
/// `is_visible_in_transcript_only`, `subtype`, `content`. They are part of the
/// data contract, intentionally retained, hence the targeted allow rather than
/// deleting SPEC-mandated shape.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct Record {
    /// Record discriminator: "user", "assistant", "system", "summary",
    /// "last-prompt", "attachment", … (open set - keep as String, never enum-panic).
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

    /// The session's plan slug (one stable value per session, minted BEFORE Plan Mode
    /// is entered - the first slug-carrying record precedes the plan_mode line in every
    /// measured session - and absent from every record before the mint and from
    /// metadata-only records). The harness derives the plan file name from it.
    #[serde(default)]
    pub slug: Option<String>,

    /// Fork provenance (line 1 of a `/fork` child transcript, `type:"fork-context-ref"`):
    /// the PARENT session's last record uuid at fork time - the fork point, feedable to
    /// `csift show @<parent> --uuid <it>`. Absent everywhere else.
    #[serde(default, rename = "parentLastUuid")]
    pub parent_last_uuid: Option<String>,

    /// The context length (in messages) carried into the fork, from the same
    /// `fork-context-ref` record.
    #[serde(default, rename = "contextLength")]
    pub context_length: Option<u64>,

    /// The pasted-image ids of a user prompt, in the ORDER OF ITS IMAGE BLOCKS (the
    /// submit path builds the block array and this array together in one ascending-id
    /// pass, while the `[Image #N]` markers keep the operator's text order; ledger
    /// IMG-005). Absent on every other record. Tolerant.
    #[serde(default, rename = "imagePasteIds")]
    pub image_paste_ids: Option<Vec<u64>>,

    #[serde(default, rename = "isSidechain")]
    pub is_sidechain: Option<bool>,

    /// Compaction summary marker - when true, this user record is NOT a human turn.
    #[serde(default, rename = "isCompactSummary")]
    pub is_compact_summary: Option<bool>,

    /// System-injected pseudo-turn marker (§4.2). `true` ⇒ a `type:"user"` record
    /// whose string/text content LOOKS human ("Continue from where you left off.",
    /// loop ticks, stop-hook feedback, `<local-command-caveat>…`) but is machine-
    /// generated - must be excluded from genuine-user and from turn-delimiting.
    #[serde(default, rename = "isMeta")]
    pub is_meta: Option<bool>,

    /// Co-set with `isCompactSummary` on compaction-summary records (§4.7).
    #[serde(default, rename = "isVisibleInTranscriptOnly")]
    pub is_visible_in_transcript_only: Option<bool>,

    /// `system` record subtype: stop_hook_summary | turn_duration | away_summary
    /// | compact_boundary | informational | api_error | model_refusal_* | agents_killed
    /// | local_command | scheduled_task_fire | …
    #[serde(default)]
    pub subtype: Option<String>,

    /// `system` record severity (`"warning"` on the Remote Control disconnect notice,
    /// `"info"` elsewhere); rides the `harness.meta.system` excerpt head (v0.10.1).
    #[serde(default)]
    pub level: Option<String>,

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

    /// `logicalParentUuid` (top-level on `compact_boundary` system records): the TRUE
    /// predecessor record the compaction re-links to (`parentUuid` is null on a
    /// boundary) - harness ground truth for the post-compaction chain. Additive +
    /// tolerant.
    #[serde(default, rename = "logicalParentUuid")]
    pub logical_parent_uuid: Option<String>,

    /// The role-bearing message payload (present on user/assistant records).
    #[serde(default)]
    pub message: Option<Message>,

    /// Structured echo on tool-result carriers (§4.6). Kept as UNPARSED raw JSON text
    /// (`Box<RawValue>`) rather than a built `Value` tree: this blob routinely carries
    /// the full file/output content a tool returned (≈20-25% of a candidate line's
    /// bytes), and eagerly tree-building it for EVERY carrier dominated the parse cost
    /// of every scanning subcommand. The hot paths consult only a handful of small
    /// fields - read them via the [`Record::tur_probe`] typed probe (skips the huge
    /// values without allocating); the one deep consumer (`recover`) parses the full
    /// tree on demand via [`Record::tool_use_result_value`].
    #[serde(default, rename = "toolUseResult")]
    pub tool_use_result: Option<Box<serde_json::value::RawValue>>,

    /// Top-level `attachment` payload (a sibling of `message`, not a content block).
    /// Real records carry attachments for hook output, `edited_text_file` external
    /// edits, `file` snapshots, etc. Kept as UNPARSED raw JSON text (same rationale as
    /// `tool_use_result` - attachments embed whole file snapshots) and read only by
    /// `recover`/`plan` via [`Record::attachment_value`]; additive + tolerant, so no
    /// other subcommand changes behaviour.
    #[serde(default)]
    pub attachment: Option<Box<serde_json::value::RawValue>>,

    /// `file-history-snapshot` payload (a top-level sibling). Carries
    /// `{messageId, trackedFileBackups: {<path>: {backupFileName, version, backupTime}}}`.
    /// Read only by `recover` to know a disk backup EXISTED for a path at a time
    /// (a coverage annotation). `backupFileName` is usually present (measured 83-98%
    /// across real corpora), but the store it names is PRUNED and its content has no
    /// transcript anchor, so it is never used to fabricate content; `recover
    /// --list-backups` lists the store itself. Additive + tolerant.
    #[serde(default)]
    pub snapshot: Option<serde_json::Value>,

    /// `file-history-delta` (v0.10.0): the ONE path this delta line tracks, beside the
    /// `backup` object below. Absent everywhere else. Additive + tolerant.
    #[serde(default, rename = "trackingPath")]
    pub tracking_path: Option<String>,

    /// `file-history-delta` `backup` object: `{backupFileName, version, backupTime,
    /// realParentDir?}`. Kept as a `Value` (tiny); read by the promoted-leaf render.
    #[serde(default)]
    pub backup: Option<serde_json::Value>,

    /// `queue-operation` (v0.10.0): the queue event - `enqueue` | `dequeue` | `remove` |
    /// `popAll` (open set; measured those four). The human-typed (or automation) text
    /// rides top-level `content` on every operation except `dequeue`.
    #[serde(default)]
    pub operation: Option<String>,

    /// `queue-operation` `remove` reason - measured values `absorbed_mid_turn` and
    /// `delivered_to_agent`, both STRUCTURAL evidence that the queued text was
    /// consumed. Absent on every other line. Open set, kept verbatim.
    #[serde(default)]
    pub reason: Option<String>,

    /// `system`/`turn_duration` (v0.10.0): wall-clock ms of the turn. Kept as a raw
    /// `Value` so an odd shape can never fail the record (tolerance discipline, the
    /// `Message.model` precedent); read via [`Record::u64_field`].
    #[serde(default, rename = "durationMs")]
    pub duration_ms: Option<serde_json::Value>,

    /// `turn_duration`: the running message count at turn end.
    #[serde(default, rename = "messageCount")]
    pub message_count: Option<serde_json::Value>,

    /// `turn_duration`: background agents still running at turn end (the REPL's
    /// "Waiting for N agents" line). Optional; measured on ~3% of records.
    #[serde(default, rename = "pendingBackgroundAgentCount")]
    pub pending_background_agent_count: Option<serde_json::Value>,

    /// `turn_duration`: workflows still running at turn end. Optional (~5%).
    #[serde(default, rename = "pendingWorkflowCount")]
    pub pending_workflow_count: Option<serde_json::Value>,

    /// `system`/`stop_hook_summary` (v0.10.0): how many Stop hooks ran.
    #[serde(default, rename = "hookCount")]
    pub hook_count: Option<serde_json::Value>,

    /// `stop_hook_summary`: `[{command, durationMs}, …]` - the hook command lines are
    /// the record's only text.
    #[serde(default, rename = "hookInfos")]
    pub hook_infos: Option<serde_json::Value>,

    /// `stop_hook_summary`: the hooks that errored (an array; empty on most records).
    #[serde(default, rename = "hookErrors")]
    pub hook_errors: Option<serde_json::Value>,

    /// `stop_hook_summary`: true when a Stop hook blocked the turn from ending.
    #[serde(default, rename = "preventedContinuation")]
    pub prevented_continuation: Option<bool>,

    /// csift's own provenance marker on an ELICITATION SIDECAR record (§3.10). A
    /// hook-written `elicitations.jsonl` line carries `csift:"elicitation-marker-v1"`
    /// on every record so a merged sidecar record is distinguishable from a native CC
    /// record. `None` on every native transcript record. Additive + tolerant.
    #[serde(default)]
    pub csift: Option<String>,

    /// The sidecar record's pairing PHASE - `"pending"` (an unanswered elicitation,
    /// missing from the native transcript) or `"resolved"` (a lightweight close marker
    /// used only for pairing). Read by [`crate::elicitation`]. `None` on a native record.
    #[serde(default, rename = "csiftPhase")]
    pub csift_phase: Option<String>,

    /// The sidecar elicitation KIND - `"AskUserQuestion"` / `"ExitPlanMode"` /
    /// `"mcp-elicitation"`. `None` on a native record.
    #[serde(default, rename = "csiftKind")]
    pub csift_kind: Option<String>,

    /// The sidecar pairing KEY (tool_use_id / elicitation_id / MCP server) - groups a
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
    /// `stop_reason` on assistant messages (`tool_use` / `end_turn` / `stop_sequence` /
    /// `max_tokens` / `refusal` / null). Persisted per record; trustworthy on the MAIN
    /// lane (measured 0.0-0.3% null), NORMALLY null mid-message on subagent lanes (which
    /// flush per content block). Read by the live-truth surfaces. Additive + tolerant.
    #[serde(default, rename = "stop_reason")]
    pub stop_reason: Option<String>,

    /// The API message id (`msg_...`). One API message spans MULTIPLE records (CC
    /// writes one line per content block and repeats the message envelope on each),
    /// so this is the usage-dedupe key for `stats`. Additive + tolerant.
    #[serde(default)]
    pub id: Option<String>,

    /// The token-usage echo on assistant records. Kept UNPARSED (same rationale as
    /// `Record::tool_use_result`): only `stats` reads it, via [`Message::token_usage`].
    #[serde(default)]
    pub usage: Option<Box<serde_json::value::RawValue>>,
}

/// The token-usage fields `stats` sums (each optional + tolerant - a missing/odd
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
    /// or shaped unexpectedly - tolerant, never an error).
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
/// [`Record::tur_probe`]). Every field is an `Option<Value>` - a tiny scalar/map tree
/// that accepts ANY JSON type, so one oddly-typed field can never fail the whole probe
/// (each accessor then applies the same `as_str`/`as_bool`/`as_object` coercion the
/// former `.get(…)` chains did - byte-identical semantics). Crucially, the blob's HUGE
/// unlisted values (file bodies, stdout echoes, structured patches) are skipped by
/// serde's ignore path without ever being allocated.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct TurProbe {
    pub(crate) r#type: Option<serde_json::Value>,
    #[serde(rename = "filePath")]
    pub(crate) file_path: Option<serde_json::Value>,
    #[serde(rename = "persistedOutputPath")]
    pub(crate) persisted_output_path: Option<serde_json::Value>,
    pub(crate) status: Option<serde_json::Value>,
    #[serde(rename = "isAsync")]
    pub(crate) is_async: Option<serde_json::Value>,
    pub(crate) answers: Option<serde_json::Value>,
    /// AskUserQuestion's freeform answer (v0.10.3): the text an answerer typed instead
    /// of a structured option. Written beside an EMPTY `answers` map, synthesized as
    /// `The user responded: <text>`; the TUI dialog never writes it (its Other entry
    /// lands in `answers`), so it comes from an answerer outside the dialog.
    pub(crate) response: Option<serde_json::Value>,
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
    /// A `redacted_thinking` block - encrypted/opaque reasoning CC emits in place of a visible
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
        /// String OR array of {type:text,text}/{type:image} - keep raw.
        #[serde(default)]
        content: Option<serde_json::Value>,
        #[serde(default)]
        is_error: Option<bool>,
    },
    Image {
        #[serde(default)]
        source: Option<serde_json::Value>,
    },
    /// Any block type not modeled above - never a parse failure.
    #[serde(other)]
    Unknown,
}

/// The provenance value every csift elicitation-sidecar record carries in its `csift`
/// field (§3.10). A line lacking this is a native CC record / foreign line.
pub const ELICITATION_MARKER: &str = "elicitation-marker-v1";
