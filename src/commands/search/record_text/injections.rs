//! The INJECTION family: the isMeta `type:"user"` templates the harness writes into a
//! turn - `harness.enforce.*`, `harness.command.caveat`, `harness.hook.feedback`,
//! `harness.context.{image,skill,reminder}`, the fired scheduler prompt, the notification
//! FRAME and the two records in the family that are a PERSON or a peer underneath - their
//! markers and the records each one points at.
//!
//! Every one of these looks like the operator's own message on disk, so the two things a
//! reader needs are WHO wrote it (the leaf, decided in `model/classify/injections.rs`) and
//! WHAT it was written for. The second is this file: one `for` edge per leaf, read from
//! the key the producer actually leaves on the record, and an honest reason when the
//! producer leaves none.

use super::*;
use crate::model::extract_xml_tag;

/// What an isMeta INJECTION says - the family's half of the render seam.
///
/// EMPTY, and not because the move was skipped: an isMeta injection is a `type:"user"`
/// record whose text is its own `message.content`, so it renders through the same generic
/// arm every conversation record does and there is no per-family arm in the shared chain
/// to move. What this family adds instead rides the LABEL ZONE ([`facts`]), which is the
/// right place for it: a fabricated body would be text a scan could match that no line
/// carries, and the section 7 prefilter laws would then need a synth marker for every one
/// of these templates.
///
/// The one thing that is NOT empty is the test probe below, and it belongs to this family
/// on purpose: an isMeta injection is the shape with no leaf of its own, so this render is
/// reachable only through the caller's CLASS, and a seam nothing reaches through a class
/// is a seam the whole-file gate cannot see.
pub(crate) fn render(
    rec: &Record,
    raw: Option<&str>,
    class: Class,
    ctx: &ClassifyCtx,
) -> Option<String> {
    #[cfg(test)]
    if let Some(text) = super::probe::render(rec, class) {
        return Some(text);
    }
    let _ = (rec, raw, class, ctx);
    None
}

/// The isMeta-injection family's own facts.
///
/// An EDGE to another record is never a variant here - those are the shared
/// [`HitFact::For`] built through [`super::for_edge`]. What is here is what only this
/// family knows about its own records.
#[derive(Debug, Clone)]
pub(crate) enum Fact {
    /// The record is a PERSON's message that the harness relayed into a running lane
    /// (`origin.kind` reads `human`), not one they typed into this lane.
    ///
    /// The JSON twin of the `[relayed]` marker. Both are pushed, because they answer two
    /// different readers and neither can stand in for the other: `--format json` is the
    /// machine contract (AGENTS section 4) and a marker is display-only by construction,
    /// so without this a machine consumer cannot tell a relayed message from a typed one
    /// at all. Its `label_zone` is deliberately `None` - the marker already prints the
    /// word, and two renderers of one word drift.
    Relayed,
    /// The counts of a TEXTLESS image record's content blocks.
    ///
    /// Such a record renders no text anywhere (its `message.content` holds no `text`
    /// block), so without this an address on one falls through to the key=value excerpt of
    /// the line's own top-level keys and prints the head of a base64 payload.
    ImageOnly {
        /// `image` blocks on the record.
        images: usize,
        /// `document` blocks beside them.
        documents: usize,
    },
    /// The lane a PEER relay came from, as the sending task id its `origin` names.
    ///
    /// Not the same value as the direction arrow's FROM, which is the transport address
    /// out of the relay's own open tag, so this is a second fact rather than a second
    /// render of one: the arrow says which socket delivered it, this says which lane sent
    /// it. Absent from the record on the minority shape, and then simply not pushed.
    PeerSender(String),
}

impl Fact {
    /// The label-zone tag this fact adds, if any.
    pub(crate) fn label_zone(&self) -> Option<String> {
        match self {
            // The `[relayed]` MARKER carries this word; see the variant's doc.
            Fact::Relayed => None,
            Fact::ImageOnly { images, documents } => Some(format!(
                "[context image-only: {images} image block(s), {documents} document block(s)]"
            )),
            Fact::PeerSender(task) => Some(format!("[peer lane {task}]")),
        }
    }

    /// The JSON key(s) and value(s) this fact adds to its hit object.
    pub(crate) fn json(&self) -> Vec<(&'static str, serde_json::Value)> {
        match self {
            Fact::Relayed => vec![("relayed", serde_json::json!(true))],
            Fact::ImageOnly { images, documents } => vec![(
                "image_only",
                serde_json::json!({"images": images, "documents": documents}),
            )],
            Fact::PeerSender(task) => vec![("peer_sender_task_id", serde_json::json!(task))],
        }
    }
}

/// The injection family's per-unit facts and markers.
///
/// Called for EVERY emitted unit through [`crate::commands::search::family_facts`], so
/// the first thing it does is refuse a unit that is not its own: the whole family is
/// `type:"user"` records carrying the authorship flag, which is two field compares and no
/// allocation for every other unit in the crate.
///
/// An edge this family resolves to another record of the same file goes through
/// [`super::for_edge`], the one builder - never a second index and never a hand-built
/// target, so every family's edge resolves and renders alike.
pub(crate) fn facts(
    rec: &Record,
    raw: Option<&str>,
    section: &Section<'_>,
    ctx: &ClassifyCtx,
) -> (Vec<HitFact>, Vec<Marker>) {
    // The gate. `isMeta` on a `type:"user"` record IS the family: it is the authorship
    // flag the injecting call site stamps, and no template of this family is written
    // without it. A unit of any other family answers here for the cost of two compares.
    if !rec.is_type("user") || !rec.is_meta.unwrap_or(false) {
        return (Vec::new(), Vec::new());
    }
    let Some(class) = section.class else {
        // The RESIDUE: an isMeta record matching no template csift models. It renders as
        // an unmodeled unit under an address and carries no edge, because the edge is a
        // statement about a leaf and this record has none.
        return (Vec::new(), Vec::new());
    };
    let mut facts = Vec::new();
    let mut markers = Vec::new();
    match class {
        // A PERSON's message, delivered into a running lane by the harness rather than
        // typed into this lane's own prompt box. It is the person's own prose and it
        // carries `user.message` like any other, so the marker is the only thing that
        // says the delivery was not theirs - and it matters, because an isMeta record
        // opens no turn, so this message is searchable under `user.message` while the
        // turn numbering never moves for it.
        Class::UserMessage if rec.origin_kind() == Some("human") => {
            markers.push(Marker::plain("relayed"));
            facts.push(HitFact::Injections(Fact::Relayed));
            facts.push(parent_edge(EdgeKind::AttachedTo, rec, ctx));
        }
        // An inbound message like a peer's or a coordinator's. The edge is `replies_to`
        // and BOTH shapes answer it empty - but for DIFFERENT reasons, which is why this
        // arm reads the record's own `origin.kind` and never the leaf: a peer relay and a
        // coordinator relay carry the same leaf, and a reason that names the wrong shape
        // is a false statement about the record in front of the reader.
        Class::CommInbox => inbox_facts(rec, &mut facts),
        // The four REDO nudges: the harness sending the model back to a call it just
        // made, so the record they point at is the assistant turn being redone.
        Class::EnforceStructuredOutput
        | Class::EnforceBrief
        | Class::EnforceHandback
        | Class::EnforceRetry => facts.push(parent_edge(EdgeKind::Redoes, rec, ctx)),
        // A blocking hook's feedback closes an assistant turn that tried to end, so its
        // parent is the record the hook BLOCKED.
        Class::HookFeedback => facts.push(parent_edge(EdgeKind::Blocked, rec, ctx)),
        // The scheduler appends the fire RECORD and then submits the armed prompt under
        // it, so the prompt's parent IS the event it fired from (claim CLS-031) - which
        // is also the only mark the producer leaves on the prompt itself.
        Class::ScheduleFire => facts.push(parent_edge(EdgeKind::Fires, rec, ctx)),
        // The notification FRAME is a second carrier of a `<task-notification>` pulse,
        // and the pulse names the tool_use it reports on.
        Class::NotificationWorkflow
        | Class::NotificationMonitor
        | Class::NotificationSubagent
        | Class::NotificationBackgroundCommand
        | Class::NotificationTask => facts.push(reports_edge(rec, ctx)),
        // The image record. When its content is BLOCKS and no text it renders nothing at
        // all, so the counts are what an address has to show instead of the head of a
        // base64 payload.
        Class::ContextImage => {
            if let Some((images, documents)) = image_only_blocks(rec, raw) {
                facts.push(HitFact::Injections(Fact::ImageOnly { images, documents }));
            }
            facts.push(parent_edge(EdgeKind::AttachedTo, rec, ctx));
        }
        // Everything else the harness injects under a turn: the caveat the slash-command
        // executor writes, a reminder, a skill body preloaded into a lane's opening
        // array. Each was written under a particular record, and that is what the edge
        // says.
        Class::CommandCaveat | Class::ContextReminder | Class::ContextSkill => {
            facts.push(parent_edge(EdgeKind::AttachedTo, rec, ctx));
        }
        // Every OTHER leaf this family's classify file can assign - the two schedule
        // ticks and the resume repair prompt - carries no edge, because no key on those
        // records names a record they were written for. An empty arm rather than an
        // invented relation.
        _ => {}
    }
    (facts, markers)
}

/// The two inbound-relay shapes, split on the record's OWN `origin.kind`.
///
/// Both carry `agent.communication.inbox` and neither names a message it answers, so both
/// end in an honest empty - but the REASON is the whole content of the fact here, and the
/// two shapes have different ones. Measured over two projects roots, one count per
/// `type:"user"` record carrying an `origin` object: every COORDINATOR relay's origin key
/// set is exactly `{kind}` (149 and 57 records), while every PEER relay carries `from` and
/// most carry `senderTaskId` (50 of 55, and 109 of 109) - so the coordinator has nothing
/// to say about its sender and the peer has plenty, and neither has a message id it is
/// answering (claim CLS-046).
fn inbox_facts(rec: &Record, facts: &mut Vec<HitFact>) {
    match rec.origin_kind() {
        Some("coordinator") => facts.push(HitFact::ForUnknown {
            kind: EdgeKind::RepliesTo,
            reason: "a coordinator relay's origin carries only its kind".to_string(),
        }),
        Some("peer") => {
            if let Some(task) = rec
                .peer_origin()
                .and_then(|o| o.get("senderTaskId"))
                .and_then(serde_json::Value::as_str)
            {
                facts.push(HitFact::Injections(Fact::PeerSender(task.to_string())));
            }
            facts.push(HitFact::ForUnknown {
                kind: EdgeKind::RepliesTo,
                reason: "a peer relay names the lane that sent it, not a message it answers"
                    .to_string(),
            });
        }
        // Any other origin (or none) on an isMeta inbox record is a shape csift has no
        // reading of, and an invented reason would be worse than no fact.
        _ => {}
    }
}

/// The edge every injection carries: the record it was written UNDER, read from
/// `parentUuid` and resolved to a line through the chain's own uuid map.
///
/// [`Basis::Position`] and not `Key`, because the parent link says where the record SITS
/// rather than naming a target the producer chose - the append path stamps it from the
/// running tail of the transcript (claim CLS-049), so what it names is the record above.
fn parent_edge(kind: EdgeKind, rec: &Record, ctx: &ClassifyCtx) -> HitFact {
    super::for_edge(kind, Basis::Position, rec.parent_uuid.as_deref(), None, ctx)
}

/// The FRAME carrier's edge: the `<tool-use-id>` its wrapped pulse names.
///
/// [`Basis::Key`] - the producer wrote that tag to say which call this pulse is about.
/// The value is a tool_use id rather than a record uuid, so the shared builder's lookup
/// finds no line and the edge renders the id with its note. That is the honest shape: the
/// id IS the join a reader needs, and it is the same answer the builder gives any target
/// this transcript does not carry.
fn reports_edge(rec: &Record, ctx: &ClassifyCtx) -> HitFact {
    let id = rec
        .raw_message_text()
        .and_then(|s| extract_xml_tag(&s, "tool-use-id"));
    super::for_edge(EdgeKind::Reports, Basis::Key, id.as_deref(), None, ctx)
}

/// The image and document block counts of a TEXTLESS block record, or `None` when the
/// record is not that shape.
///
/// The parsed block model folds every type it does not name into one tolerant arm, so a
/// `document` and a `fallback` are the same value there - which is exactly the
/// distinction this render has to make. The types are therefore read off the SOURCE
/// line, and only for a record the parsed form has already shown to be textless, so the
/// read is bounded to a shape that is rare by construction (measured over two projects
/// roots, one count per isMeta user record: 97 of 3,713 and 78 of 1,825 carry no text
/// block at all).
fn image_only_blocks(rec: &Record, raw: Option<&str>) -> Option<(usize, usize)> {
    let blocks = match rec.message.as_ref()?.content.as_ref()? {
        Content::Blocks(b) => b,
        Content::Text(_) => return None,
    };
    if blocks.iter().any(|b| matches!(b, Block::Text { .. })) {
        return None;
    }
    let types = block_types(raw?)?;
    let images = types.iter().filter(|t| *t == "image").count();
    let documents = types.iter().filter(|t| *t == "document").count();
    Some((images, documents))
}

/// The `type` of every content block of a record, read off its raw jsonl line.
///
/// Narrow by construction: the deserialize target names `message.content[].type` and
/// nothing else, so the base64 an image block carries is walked and discarded rather
/// than copied. `None` when the line does not parse or carries no block array.
fn block_types(raw: &str) -> Option<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct Line {
        message: Option<Msg>,
    }
    #[derive(serde::Deserialize)]
    struct Msg {
        content: Option<Vec<Kind>>,
    }
    #[derive(serde::Deserialize)]
    struct Kind {
        r#type: Option<String>,
    }
    let line: Line = serde_json::from_str(raw).ok()?;
    let content = line.message?.content?;
    Some(
        content
            .into_iter()
            .map(|k| k.r#type.unwrap_or_default())
            .collect(),
    )
}

#[cfg(test)]
mod tests;
