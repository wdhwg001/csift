//! Classify support: notification mapping, batched sections, ClassifyCtx.

use super::*;

/// Map a parsed `<task-notification>` [`AutomationKind`] to its `harness.notification.*`
/// [`Class`] (`Agent` becomes `subagent` to avoid the `agent` role collision).
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

/// The `type:"system"` subtype Claude Code writes at the instant a scheduled task fires -
/// the sibling record a [`Class::ScheduleFire`] prompt is parented to. Also the candidate
/// needle the `search` prefilter uses for the same lines (a bare VALUE substring, R13-safe).
pub const SCHEDULED_TASK_FIRE_SUBTYPE: &str = "scheduled_task_fire";

impl Record {
    /// Is this the `system`/`scheduled_task_fire` record the scheduler writes when a cron
    /// entry or a wakeup timer comes due? It carries the fired instant in its `content` and
    /// nothing else csift needs; the PROMPT is the separate record parented to it.
    #[must_use]
    pub fn is_scheduled_task_fire(&self) -> bool {
        self.is_type("system") && self.subtype.as_deref() == Some(SCHEDULED_TASK_FIRE_SUBTYPE)
    }

    /// The instant a `scheduled_task_fire` record names, verbatim - the text inside the
    /// TRAILING parentheses of its `content`. Two wordings ship that one line (`Claude
    /// resuming /loop wakeup (<when>)` for a wakeup, `Running scheduled task (<when>)` for a
    /// cron task), both built by the same call, so the parenthesised tail is read rather than
    /// either sentence: a third wording changes the prose, not the shape. `None` when the
    /// content is not a string, carries no parenthesised tail, or leaves it empty - an
    /// unreadable instant is reported as absent, never as a guessed one.
    #[must_use]
    pub fn scheduled_fire_instant(&self) -> Option<&str> {
        let content = self.content_str()?.trim_end();
        let inner = content.strip_suffix(')')?;
        let open = inner.rfind('(')?;
        let when = inner[open + 1..].trim();
        (!when.is_empty()).then_some(when)
    }
}

/// Per-file join from a fired PROMPT to the instant its `system`/`scheduled_task_fire`
/// sibling names: the prompt's `parentUuid` IS that record's `uuid` (the scheduler appends
/// the fire record and then submits the prompt, so the submission's parent is the record it
/// just wrote). A uuid join, never a positional "line before" guess - a queue rider can sit
/// between the two lines.
///
/// Empty on every transcript that never ran a scheduled task, which is nearly all of them,
/// and empty as well on a scan whose `-t` cannot reach [`Class::ScheduleFire`] (the caller
/// skips building it, so the extra `system` lines are never even admitted).
#[derive(Debug, Clone, Default)]
pub struct ScheduleFireIndex {
    by_parent: HashMap<String, String>,
    /// Every `scheduled_task_fire` record's own uuid, instant or not. The instant map
    /// above cannot stand in for this: a fire record whose parenthesised tail is
    /// unreadable still FIRED a prompt, and gating the prompt's leaf on a readable
    /// instant would lose the label to a display fact.
    fire_uuids: HashSet<String>,
}

impl ScheduleFireIndex {
    /// Index every `scheduled_task_fire` record by its own uuid (= the fired prompt's
    /// `parentUuid`), plus the instant for those that name a readable one.
    #[must_use]
    pub fn from_records<'a>(records: impl Iterator<Item = &'a Record>) -> Self {
        let mut by_parent = HashMap::new();
        let mut fire_uuids = HashSet::new();
        for rec in records {
            if !rec.is_scheduled_task_fire() {
                continue;
            }
            let Some(uuid) = rec.uuid.as_deref() else {
                continue;
            };
            fire_uuids.insert(uuid.to_string());
            if let Some(when) = rec.scheduled_fire_instant() {
                by_parent.insert(uuid.to_string(), when.to_string());
            }
        }
        Self {
            by_parent,
            fire_uuids,
        }
    }

    /// Does `parent_uuid` name a `system`/`scheduled_task_fire` record in this
    /// transcript? The structural gate behind [`Record::is_scheduled_fire_prompt`].
    #[must_use]
    pub fn is_fire_parent(&self, parent_uuid: Option<&str>) -> bool {
        parent_uuid.is_some_and(|u| self.fire_uuids.contains(u))
    }

    /// The instant this record's fire sibling names, or `None` when the transcript holds no
    /// such sibling - which is the honest answer on the builds that write no fire record at
    /// all, and on a windowed read that never reached it.
    #[must_use]
    pub fn instant_for(&self, parent_uuid: Option<&str>) -> Option<&str> {
        self.by_parent.get(parent_uuid?).map(String::as_str)
    }
}

/// Build the `[<kind> <id>[, <id>…] <status>] <summary>` attribution label for ONE
/// `<task-notification>…</task-notification>` section string. Shared by
/// [`Record::automation_label`] (whole-record = the single section) and the batched per-section
/// render ([`Record::record_text_sections`]) so the two never drift. The status slot prefers the
/// explicit `<status>`; absent (the common Monitor/ScheduleWakeup case), the real outcome lives in
/// `<event>` so render THAT rather than fabricating `completed`; only when BOTH are missing do we
/// fall back to `completed`. A missing field is elided gracefully.
///
/// The id slot names EVERY task the pulse closes, not just the first: an orphan reconciliation
/// closes several at once, and rendering one of them left the rest invisible on every record
/// surface. Its `__orphan_summary__:<kind>` sentinel names no task, so it never enters the id
/// list; it becomes the `(orphan reconciliation: <kind>)` marker instead ([`section_task_ids`]).
pub(crate) fn automation_label_for_section(section: &str) -> String {
    let TaskIds { ids, orphan_kind } = section_task_ids(section);
    let status = extract_xml_tag(section, "status");
    let summary = extract_xml_tag(section, "summary");
    let event = extract_xml_tag(section, "event");
    let kind = AutomationKind::from_summary(summary.as_deref());
    let id = if ids.is_empty() {
        "?".to_string()
    } else {
        ids.join(", ")
    };
    let event_norm = event
        .as_deref()
        .filter(|e| !e.is_empty())
        .map(normalize_line);
    let status = status
        .as_deref()
        .map(str::to_string)
        .or(event_norm)
        .unwrap_or_else(|| "completed".to_string());
    let mut head = format!("[{} {id} {status}]", kind.slug());
    if let Some(k) = orphan_kind {
        head.push_str(&format!(" (orphan reconciliation: {k})"));
    }
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
pub(crate) fn classify_batched_sections(raw: &str, framed: bool, out: &mut Vec<Class>) -> bool {
    let mut matched = false;
    // (a) <task-notification> sections - classify each, recording its byte span to mask the
    //     peer scan against tags quoted inside it.
    let mut notif_spans: Vec<(usize, usize)> = Vec::new();
    scan_tag_sections(
        raw,
        TASK_NOTIFICATION_PREFIX,
        TASK_NOTIFICATION_CLOSE,
        framed,
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

/// One renderable record-level text SECTION of a (possibly batched) `type:"user"` record: its
/// leaf [`Class`], the display text to excerpt, and the comm `from ⇨ to` direction for a
/// communication leaf. Built by [`Record::record_text_sections`] so a
/// record batching several `<task-notification>` / inbound-peer sections of MIXED kind renders ONE
/// hit PER section (each with its own label + direction) rather than collapsing to one.
#[derive(Debug, Clone)]
pub struct RecordTextSection {
    /// The leaf class for THIS section (a `harness.notification.*` / `agent.communication.*`).
    pub class: Class,
    /// The display text to match + excerpt (a per-section automation label, the `<result>` report
    /// body, or the raw peer-message section slice).
    pub text: String,
    /// `from ⇨ to` for a communication leaf; `None` for a `harness.notification.*`.
    pub direction: Option<(String, String)>,
    /// THIS section's `<task-id>` tags, split into the real task ids and the orphan-
    /// reconciliation kind ([`section_task_ids`]). Carried per SECTION because a batched
    /// record's sections name different tasks; default (empty / `None`) on every non-
    /// notification section.
    pub task_ids: TaskIds,
}

/// A CLEAN inbound-communication preview of a peer/teammate turn-opener, for the `turns` / `list`
/// render surfaces. RENDER-ONLY: it does NOT affect
/// [`Record::classify`] / [`Record::opens_turn`] - a peer opener still opens a turn and classifies
/// `agent.communication.{inbox,signal}` through the engine; this is only the human-facing render of
/// that opener, so a preview shows what the peer SAID rather than the raw
/// `<teammate-message …>` XML blob.
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

/// Push `c` into `out` only if not already present (multi-label dedup) - preserves
/// first-seen order so the richest/most-salient label leads.
pub(crate) fn push_unique(out: &mut Vec<Class>, c: Class) {
    if !out.contains(&c) {
        out.push(c);
    }
}

/// A read-only lookup for spawn pairing, supplied via [`ClassifyCtx`]. Backed by the global
/// spawn index (`subagent::ParentSpawnIndex` / `build_global_spawn_index`),
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

/// Resolve a uuid ONE RECORD NAMES to the physical jsonl line of the record carrying it,
/// supplied via [`ClassifyCtx`] the way [`SpawnLookup`] is.
///
/// Half the `for` edges in the taxonomy are a key naming another record of the same
/// file (a `last-prompt` line's `leafUuid`, a file-history delta's `messageId`, a
/// refusal's `refusedUserMessageUuid`), and an edge that renders the uuid alone makes a
/// reader search for it. The lookup is backed by the chain's OWN uuid map
/// ([`crate::model::UuidIndex`]), so it is not a second index and it answers the way the
/// loader would: the surviving line wins a uuid two lines carry.
///
/// Behind a trait because the LINE lives in the surface's row space, not in the model:
/// only the caller knows whether an index is a record or a spine row and what line it sat
/// on. `None` from the lookup means "no record here carries that uuid", which is the
/// honest answer for a wire id, another session's record, or a line a pre-boundary
/// deletion removed.
///
/// It carries NO dead-code allow, and must not be given one: `search::RowLines` implements
/// it on the production path, which is what makes it reachable. An allow here would be a
/// standing claim that nothing uses the trait, and would mask the day that became true.
pub trait LineOfUuid {
    /// The 1-based jsonl line of the record this uuid names, or `None`.
    fn line_of_uuid(&self, uuid: &str) -> Option<usize>;
}

/// Cross-record context [`Record::classify`] / [`Record::direction`] need that a single record
/// cannot supply. Construct with [`ClassifyCtx::top_level`] and set the relevant
/// fields. **What the caller must populate per record:**
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
/// - `resume_prompt_uuids`: the uuids of this file's [`Class::ResumePrompt`] records, so a
///   [`Class::ResumePlaceholder`] can say whether it closes a repair PAIR
///   ([`Record::resume_paired`]). `None` ⇒ no verdict rather than a guessed one.
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
    /// This file's `harness.resume.prompt` uuids, for the placeholder's pair verdict.
    pub resume_prompt_uuids: Option<&'a HashSet<String>>,
    /// Boundary -> compaction-MODE pairing for this transcript ([`SummarizeIndex`]): a
    /// `compact_boundary` carries the metrics but never the direction, so its mode is only
    /// readable through the compaction SUMMARY that follows it. `None` ⇒ a boundary's mode
    /// stays unknown (an honest null, never a guessed `compact`).
    pub summarize: Option<&'a SummarizeIndex>,
    /// Fired-prompt -> fire INSTANT join for this transcript ([`ScheduleFireIndex`]): a
    /// [`Class::ScheduleFire`] record carries the armed text and nothing about WHEN it
    /// fired, which lives on the `system`/`scheduled_task_fire` sibling it is parented to.
    /// `None` => the instant stays unknown (an honest null; the builds that write no fire
    /// record give the same answer).
    pub schedule_fires: Option<&'a ScheduleFireIndex>,
    /// uuid -> jsonl LINE for this transcript ([`LineOfUuid`]), so a `for` edge a record
    /// NAMES can render as an address instead of a bare id. `None` => every lookup
    /// answers `None`, which is what a caller with no row space (a unit test, a
    /// file-oriented surface) gets: an edge then renders its key alone rather than a
    /// guessed line.
    pub lines: Option<&'a dyn LineOfUuid>,
    /// `tool_use` id -> jsonl LINE for this transcript ([`crate::model::ProvIndex`]), the
    /// ONE join the uuid map cannot make: a `tool_use_id` is a wire id and appears in no
    /// `uuid` field, so a `for` edge naming one (an attachment's `toolUseID`, a
    /// notification's `<tool-use-id>`) resolved to nothing and printed `not in this file`
    /// about a call that is three lines up. `None` => that fallback answers `None`, which
    /// is the pre-index behaviour and what a caller with no row space gets.
    pub tool_lines: Option<&'a ProvIndex>,
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
            resume_prompt_uuids: None,
            summarize: None,
            schedule_fires: None,
            lines: None,
            tool_lines: None,
        }
    }

    /// The jsonl LINE of the record carrying the `tool_use` block this id names, or `None`.
    /// The SECOND half of an edge's resolution, tried only when the uuid map has no answer.
    #[must_use]
    pub fn line_of_tool_use(&self, id: &str) -> Option<usize> {
        self.tool_lines?.tool_use_at(id)
    }

    /// The jsonl LINE of the record `uuid` names, or `None` - because no lookup was
    /// supplied, or because no record of this transcript carries it. Both answers are
    /// the same to a caller: render the key, not a line.
    #[must_use]
    pub fn line_of_uuid(&self, uuid: &str) -> Option<usize> {
        self.lines?.line_of_uuid(uuid)
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
            .field(
                "resume_prompts",
                &self.resume_prompt_uuids.map_or(0, HashSet::len),
            )
            .field("has_line_lookup", &self.lines.is_some())
            .field("has_tool_lookup", &self.tool_lines.is_some())
            .finish()
    }
}

#[allow(dead_code)]
impl Record {
    /// The parsed inbound teammate/peer message carried by this `type:"user"`
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

    /// True when this record is an inbound TEAMMATE message specifically - a
    /// `<teammate-message>` at a section boundary. Used by the `list`/`turns` clean-preview gate.
    #[must_use]
    pub fn is_teammate_message_record(&self) -> bool {
        self.teammate_message().is_some()
    }
}
