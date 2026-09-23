//! The serde record model: `Record` itself, and the two children that carry the rest of
//! the shapes a transcript line holds.
//!
//! A PAIRED ROOT since the root crossed the 600-line structure gate: `Record` and its
//! accessors stay here, the `message` object goes to `record/message.rs`, the content
//! BLOCK enum to `record/blocks.rs`, and the EXPOSED KEYS a record states about the call
//! behind it - their shapes, their accessors and the width reasoning - to
//! `record/keys.rs`. All three are re-exported, so every `crate::model::<item>` path
//! resolves exactly as before.

mod blocks;
mod keys;
mod lenient;
mod message;

pub use blocks::*;
pub use keys::{ApiFailure, Attribution, Exposed};
pub use message::*;

use super::*;

/// A single parsed jsonl line. Unknown top-level fields are ignored by serde.
///
/// Every field below is read by some handler EXCEPT two, and each of the two is kept for
/// a stated reason rather than for completeness. `is_visible_in_transcript_only` is the
/// compaction summary's either/or SECOND key, and claim CMP-001 rests on it: the writer
/// sets it or `summarizeMetadata`, never both and never neither, so a reader that dropped
/// it would have no way to tell an ordinary compaction from a `/rewind` summarize whose
/// metadata a later build stopped writing. `prompt_source` is Claude Code's own
/// human-entry test (`typed` / `queued` / `suggestion_accepted` against the `system` it
/// stamps on every isMeta submission), and claim CLS-031 is the record of why csift steers
/// on none of it: the key has a version floor, so counting it names about a third of the
/// prompts it would have to decide. The targeted allow is for those two, not a licence to
/// add a third.
#[allow(dead_code)]
#[derive(Debug, Clone, Default, Deserialize)]
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

    /// Background-handoff provenance, written into the PARENT transcript as a
    /// `type:"continued-in"` line: the id of the CHILD session the parent was handed
    /// to. The line carries four keys and no `uuid`, so it is not a chain node and its
    /// only address is its jsonl line. Absent on every other line type.
    #[serde(default, rename = "continuedInSessionId")]
    pub continued_in_session_id: Option<String>,

    /// The LANE KIND the writing process reported: `bg` on every record a background lane
    /// writes, absent from a foreground lane's records. It is a per-PROCESS fact, not a
    /// per-record one - one stamp built from one environment variable, whose value is
    /// undefined in a foreground process, and the serializer drops an undefined property -
    /// so the key stops appearing in the SAME transcript once a foreground process appends
    /// to it. Rides the four record-bearing line types only.
    #[serde(default, rename = "sessionKind")]
    pub session_kind: Option<String>,

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

    /// Claude Code's own display-only marker: a record rendered in the UI but filtered
    /// out of the API request. The shipped schema describes it in exactly those words,
    /// and the request assembler's drop predicate reads it on user and assistant
    /// records ([`Record::delivery_override`]). A conditional strip on the persistence
    /// path normally removes the key before the write, so the corpus specimen count is
    /// 0 - the field is modeled because the strip has two carry-through branches, not
    /// because the value is common. Additive + tolerant.
    #[serde(default, rename = "isVirtual")]
    pub is_virtual: Option<bool>,

    /// Why a tool call did not run, stamped top-level beside `toolUseResult` on the
    /// user record the harness synthesises for a call it refused. Four values are
    /// written: `user-rejected` (a human answered no at the prompt), `interrupted`
    /// (the turn was aborted), `cancelled` (the call was moved to the background) and
    /// `permission-rule` (a hook denied it). The classifier's own REASON - the sentence
    /// naming which removal arm fired - is rendered into the prompt only and never
    /// reaches disk, so a transcript can show THAT a call was refused and never which
    /// reason tail fired. Open set, kept as a String; additive and tolerant.
    #[serde(default, rename = "toolDenialKind")]
    pub tool_denial_kind: Option<String>,

    /// True on the assistant record Claude Code fabricates in place of a failed API
    /// call. Paired with `message.model == "<synthetic>"` it is the request
    /// assembler's third drop arm ([`Record::delivery_override`]): the placeholder is
    /// on disk and rendered to the human, never sent back to the model. Additive +
    /// tolerant.
    #[serde(default, rename = "isApiErrorMessage")]
    pub is_api_error_message: Option<bool>,

    /// Where a `type:"user"` record's prompt entered the submit path: `typed` / `queued` /
    /// `suggestion_accepted` for the operator's own box, `sdk` for a non-interactive run, and
    /// `system` for anything the harness itself submitted. Claude Code stamps `system` on
    /// EVERY `isMeta` submission (one ternary on the submit path), and its own reader calls the
    /// pair "a system-injected turn prompt" - which is exactly what
    /// [`Class::ScheduleFire`] names. Absent on a record the operator typed, so the field is
    /// also Claude Code's own human-entry test. Additive + tolerant.
    #[serde(default, rename = "promptSource")]
    pub prompt_source: Option<String>,

    /// Provenance of a message Claude Code did not receive from the operator's own prompt
    /// box. An inbound cross-session PEER message carries `{kind:"peer", from,
    /// verifiedPeerPid, msg_id?, name?, fromMode?, body?, …}`; other kinds (`human`,
    /// `task-notification`, …) ride the same field. Kept as a raw `Value` (a tiny flat object)
    /// and read tolerantly through [`Record::peer_origin`], so an added key or an odd shape can
    /// never fail the record. Absent on every record the operator typed.
    #[serde(default)]
    pub origin: Option<serde_json::Value>,

    /// `system` record subtype: stop_hook_summary | turn_duration | away_summary
    /// | compact_boundary | informational | api_error | model_refusal_* | agents_killed
    /// | local_command | scheduled_task_fire | …
    #[serde(default)]
    pub subtype: Option<String>,

    /// `system` record severity (`"warning"` on the Remote Control disconnect notice,
    /// `"info"` elsewhere); rides the `harness.system.*` excerpt head.
    #[serde(default)]
    pub level: Option<String>,

    /// `system` record inline content (e.g. away_summary text, or the `compact_boundary`
    /// `"Conversation compacted …"` line). Read by `search` as the message-less fallback text.
    #[serde(default)]
    pub content: Option<serde_json::Value>,

    /// `compact_boundary` metrics (§3.5): `{trigger, preTokens, postTokens, durationMs}` on a
    /// `type:"system"`/`subtype:"compact_boundary"` record. Kept RAW; `search` renders it as a
    /// readable excerpt (`record_raw_text`) so `-t harness.compaction.boundary` can enumerate
    /// compaction points and inspect what each clipped. Absent on every other record. Tolerant.
    #[serde(default, rename = "compactMetadata")]
    pub compact_metadata: Option<serde_json::Value>,

    /// `last-prompt` (a metadata line, no uuid of its own): the uuid Claude Code's
    /// loader starts its conversation-chain walk from, re-snapshotted with every write
    /// batch. Read by [`crate::model::Chain`] as the leaf HINT; a `compact_boundary`
    /// seen later in the file wipes it, exactly as the loader does. Additive + tolerant.
    #[serde(default, rename = "leafUuid")]
    pub leaf_uuid: Option<String>,

    /// `last-prompt` `explicit`: the leaf was pinned by a writer (an exit/relaunch
    /// flush, or the remote-control rewind handler) rather than snapshotted. Measured 0
    /// occurrences in the corpus; parsed and DISCLOSED (the chain's `leaf_source`),
    /// never otherwise steered on.
    #[serde(default)]
    pub explicit: Option<bool>,

    /// `last-prompt` `rewound`: the pin came from the remote-control rewind handler.
    /// Measured 0 occurrences; the local rewind writes nothing at all. Disclosed only.
    #[serde(default)]
    pub rewound: Option<bool>,
    /// `summarizeMetadata` on a compaction SUMMARY record: `{messagesSummarized, direction,
    /// userContext?}`. Claude Code writes it INSTEAD of `isVisibleInTranscriptOnly` (a genuine
    /// either/or in the writer) when the compaction came from a `/rewind` summarize gesture, so
    /// its presence is what separates the two summarize modes from an ordinary compaction.
    /// `direction` (`"up_to"` / `"from"`) is the discriminator - `userContext` is written from a
    /// possibly-undefined value and can be absent. A tiny flat object, so kept as a tolerant
    /// `Value` like `compact_metadata`; read through [`Record::summarize_direction`].
    #[serde(default, rename = "summarizeMetadata")]
    pub summarize_metadata: Option<serde_json::Value>,

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

    /// The PARSED form of `attachment`, held here so one record is parsed once.
    ///
    /// The payload answers several questions of one record and they are asked from
    /// different layers: `classify` reads its `type` and, for two payload types, one more
    /// field; the delivery override reads the type again; a family's own facts read
    /// whatever their leaf needs. Parsing on each call re-walked a payload that can hold
    /// a whole file snapshot, per question, per record.
    ///
    /// A `OnceLock` rather than a `RefCell`: records are built inside rayon workers and
    /// read from them, so the memo has to be `Sync`, and it holds the `Option` (not just
    /// the value) so a payload that does not parse is remembered as unparseable instead
    /// of being retried. `serde(skip)` - it is derived from `attachment`, never read off
    /// a line.
    #[serde(skip)]
    pub attachment_parsed: std::sync::OnceLock<Option<serde_json::Value>>,

    /// The text the harness's attachment CONVERTER produced for this record, written as
    /// a TOP-LEVEL key of the attachment record (a sibling of `attachment`, never a key
    /// inside the payload). Present only when the whole conversion was text, so its
    /// presence WITNESSES that a record converted; its absence is not evidence of the
    /// opposite, because a mixed text-and-image conversion writes none. Kept unparsed
    /// (a conversion can embed a whole file) and read by [`Record::delivery_override`]'s
    /// `hook_success` arm and by [`Record::converted_text`].
    #[serde(default)]
    pub rendered: Option<Box<serde_json::value::RawValue>>,

    /// The SECOND conversion the same record can carry: the text the converter produces
    /// when the record is replayed into a turn that ALSO carries a genuine human message.
    /// A top-level key like `rendered`, never a key inside the payload.
    ///
    /// Measured over one projects root: 1,507 lines carry it, every one of them beside a
    /// `rendered`, every one of them a `queued_command` payload, and on every one of the
    /// 1,507 the two texts DIFFER. Which of the two the model received depends on the
    /// turn the record was replayed into, and the record does not say - so csift reports
    /// both rather than choosing (claim REC-112).
    #[serde(default, rename = "renderedInHumanTurn")]
    pub rendered_in_human_turn: Option<Box<serde_json::value::RawValue>>,

    /// One parse for both conversions, on the same `OnceLock` reasoning as
    /// [`Record::attachment_parsed`]: a conversion can embed a whole file, and it is
    /// asked for by the delivery override and by a family's facts. Holds the PAIR, so the
    /// second key costs nothing on the overwhelming majority of records that lack it.
    /// `serde(skip)` - derived from the two fields above, never read off a line.
    ///
    /// BOXED, and `Record`'s width is why. Every parsed line of every scan pays
    /// `size_of::<Record>()`, so a memo that stores its payload INLINE widens the record
    /// for a field almost no record uses - the same width discipline that keeps a spine
    /// row at 72 bytes. Behind a pointer the field costs 16 bytes
    /// whatever the payload grows to, and the box is allocated only for a record that
    /// actually carries a conversion. `None` means it carries neither key, which is why
    /// [`Record::converted_text`] answers `Option`: the common case allocates nothing.
    #[serde(skip)]
    pub conversion_parsed: std::sync::OnceLock<Option<Box<ConversionText>>>,

    /// THE EXPOSED KEYS ([`Exposed`], `record/keys.rs`): the eighteen top-level keys a
    /// record states about the CALL behind it - which agent definition, skill, plugin and
    /// MCP tool the request ran under, which API request and block it came from, why it
    /// failed, what a person typed refusing a tool, which scheduled task submitted the
    /// prompt, which hook run closed the turn.
    ///
    /// ONE field, BOXED, because a `Record`'s width is paid on every parsed line of every
    /// scan: eighteen `Option`s here cost 216 bytes on every row, where a
    /// pointer costs 8 and the allocation is paid only by a record that names at least one
    /// of the keys. `#[serde(flatten)]` is what keeps that to the SAME pass - serde reads
    /// the keys where they sit, at the record's own top level, rather than csift walking
    /// the line a second time (the alternative, measured at the `record/keys.rs` doc).
    #[serde(flatten, deserialize_with = "keys::exposed_keys")]
    pub exposed: Option<Box<Exposed>>,

    /// `file-history-snapshot` payload (a top-level sibling). Carries
    /// `{messageId, trackedFileBackups: {<path>: {backupFileName, version, backupTime}}}`.
    /// Read only by `recover` to know a disk backup EXISTED for a path at a time
    /// (a coverage annotation). `backupFileName` is usually present (measured 83-98%
    /// across real corpora), but the store it names is PRUNED and its content has no
    /// transcript anchor, so csift never fabricates content from it; `recover
    /// --list-backups` lists the store itself. Additive + tolerant.
    #[serde(default)]
    pub snapshot: Option<serde_json::Value>,

    /// `file-history-delta`: the ONE path this delta line tracks, beside the
    /// `backup` object below. Absent everywhere else. Additive + tolerant.
    #[serde(default, rename = "trackingPath")]
    pub tracking_path: Option<String>,

    /// `file-history-delta` `backup` object: `{backupFileName, version, backupTime,
    /// realParentDir?}`. Kept as a `Value` (tiny); read by the promoted-leaf render.
    #[serde(default)]
    pub backup: Option<serde_json::Value>,

    /// `queue-operation`: the queue event - `enqueue` | `dequeue` | `remove` |
    /// `popAll` (open set; measured those four). The human-typed (or automation) text
    /// rides top-level `content` on every operation except `dequeue`.
    #[serde(default)]
    pub operation: Option<String>,

    /// `queue-operation` `remove` reason - measured values `absorbed_mid_turn` and
    /// `delivered_to_agent`, both STRUCTURAL evidence that the queued text was
    /// consumed. Absent on every other line. Open set, kept verbatim.
    #[serde(default)]
    pub reason: Option<String>,

    /// `system`/`turn_duration`: wall-clock ms of the turn. Kept as a raw
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

    /// `system`/`stop_hook_summary`: how many Stop hooks ran.
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

/// The provenance value every csift elicitation-sidecar record carries in its `csift`
/// field (§3.10). A line lacking this is a native CC record / foreign line.
pub const ELICITATION_MARKER: &str = "elicitation-marker-v1";
