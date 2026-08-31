//! Classify support: notification mapping, batched sections, ClassifyCtx.

use super::*;

/// Map a parsed `<task-notification>` [`AutomationKind`] to its `harness.notification.*`
/// [`Class`] (GOLD §2 - `Agent` becomes `subagent` to avoid the `agent` role collision).
pub(crate) fn notification_class(kind: AutomationKind) -> Class {
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
pub(crate) const NOTIFICATION_RESULT_TAG: &str = "<result>";

/// Build the `[<kind> <id> <status>] <summary>` attribution label for ONE
/// `<task-notification>…</task-notification>` section string. Shared by
/// [`Record::automation_label`] (whole-record = the single section) and the batched per-section
/// render ([`Record::record_text_sections`]) so the two never drift. The status slot prefers the
/// explicit `<status>`; absent (the common Monitor/ScheduleWakeup case), the real outcome lives in
/// `<event>` so render THAT rather than fabricating `completed`; only when BOTH are missing do we
/// fall back to `completed`. A missing field is elided gracefully.
pub(crate) fn automation_label_for_section(section: &str) -> String {
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
/// IGNORED - so a notification never leaks a spurious comm label. CROSS-FAMILY (M3b): a record
/// carrying a real notification section AND a real peer section (outside any notification span)
/// unions both families' labels.
///
/// Returns `true` iff ≥1 section matched (the caller's classification is then complete); `false`
/// leaves the record to the plain marker/prose classifier.
pub(crate) fn classify_batched_sections(raw: &str, out: &mut Vec<Class>) -> bool {
    let mut matched = false;
    // (a) <task-notification> sections - classify each, recording its byte span to mask the
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
/// [`Record::classify`] / [`Record::opens_turn`] - a peer opener still opens a turn and classifies
/// `agent.communication.{inbox,signal}` through the engine; this is only the human-facing render of
/// that opener so the previews no longer dump the raw `<teammate-message …>` XML blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundComm {
    /// [`Class::CommInbox`] (a prose message) or [`Class::CommSignal`] (a control payload).
    pub class: Class,
    /// The sender id (the comm FROM); the comm TO is always the transcript owner (`self`).
    pub from: String,
    /// The peer's own message body - the `<teammate-message …>` / `<agent-message …>` wrapper tags
    /// AND the trailing harness security footer stripped, normalized to one line (only the prose).
    pub body: String,
}

/// Push `c` into `out` only if not already present (multi-label dedup, GOLD §3) - preserves
/// first-seen order so the richest/most-salient label leads.
pub(crate) fn push_unique(out: &mut Vec<Class>, c: Class) {
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
    /// `Some` ⇒ that id spawned a subagent - used for the spawn TO and the return FROM.
    fn child_for_spawn_tool_use_id(&self, tool_use_id: &str) -> Option<String>;
    /// The spawned child's agent id for a spawn by NAME / `subagent_type` (the teammate
    /// name-join, where the meta carries no `toolUseId`). The fallback when the id-join misses.
    fn child_for_spawn_name(&self, name: &str) -> Option<String>;
}

/// Cross-record context [`Record::classify`] / [`Record::direction`] need that a single record
/// cannot supply (GOLD §6). Construct with [`ClassifyCtx::top_level`] and set the relevant
/// fields. **What P2 must populate per record:**
/// - `owner_id`: the transcript owner's re-feedable id - the session uuid for a top-level
///   transcript, or the bare agent id for a subagent (the `self` of every comm direction).
/// - `owner_name`: the owner's teammate/agent NAME when known (display only; optional).
/// - `is_subagent`: whether THIS transcript lives under `subagents/`
///   (`subagent::is_subagent_path`).
/// - `parent_id`: the owning/parent session-or-agent id (the FROM of a subagent opener) -
///   `subagent::parent_session_id_from_path` / the topology `parent_agent_id`.
/// - `is_transcript_opener`: `true` ONLY for the positional FIRST turn-opener of a subagent
///   transcript (the spawn-prompt seed) - flips that genuine-user-shaped record from
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

    /// True when this record is an inbound TEAMMATE message specifically (GOLD §1) - a
    /// `<teammate-message>` at a section boundary. Used by the `list`/`turns` clean-preview gate.
    #[must_use]
    pub fn is_teammate_message_record(&self) -> bool {
        self.teammate_message().is_some()
    }
}
