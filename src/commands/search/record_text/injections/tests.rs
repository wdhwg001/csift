//! The injection family's own facts: one relation per leaf, the two relay shapes read
//! apart, the relayed person's message and the textless image record.

use super::*;
use crate::model::LineOfUuid;

/// A uuid map with one entry, so a RESOLVED edge and an UNRESOLVED one are both reachable
/// without building a chain.
struct OneLine;
impl LineOfUuid for OneLine {
    fn line_of_uuid(&self, uuid: &str) -> Option<usize> {
        (uuid == "parent-1").then_some(7)
    }
}

fn ctx_with_lines() -> ClassifyCtx<'static> {
    static LOOKUP: OneLine = OneLine;
    let mut ctx = ClassifyCtx::top_level();
    ctx.lines = Some(&LOOKUP);
    ctx
}

fn run(line: &str, class: Option<Class>) -> (Vec<HitFact>, Vec<Marker>) {
    let rec: Record = serde_json::from_str(line).expect("valid record");
    facts(&rec, Some(line), &Section::record(class), &ctx_with_lines())
}

/// The label-zone form of the facts, which is what a reader sees.
fn zone(out: &(Vec<HitFact>, Vec<Marker>)) -> String {
    out.0
        .iter()
        .filter_map(HitFact::label_zone)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every JSON pair the facts project, folded the way a hit row folds them.
fn wire(out: &(Vec<HitFact>, Vec<Marker>)) -> serde_json::Value {
    let mut obj = serde_json::json!({});
    crate::commands::search::attach_facts(&mut obj, &out.0);
    obj
}

fn meta(uuid: &str, parent: &str, text: &str) -> String {
    let body = serde_json::to_string(text).expect("json string");
    format!(
        r#"{{"type":"user","isMeta":true,"uuid":"{uuid}","parentUuid":"{parent}","message":{{"role":"user","content":{body}}}}}"#
    )
}

/// The gate is two field compares and BOTH have to hold: a record that is not a user
/// record, or one that is but carries no authorship flag, is not this family's - whatever
/// leaf the unit carries.
#[test]
fn a_record_without_the_authorship_flag_gets_nothing() {
    let plain = r#"{"type":"user","uuid":"u1","parentUuid":"parent-1","message":{"role":"user","content":"hello"}}"#;
    let out = run(plain, Some(Class::UserMessage));
    assert!(out.0.is_empty() && out.1.is_empty());
    let assistant = r#"{"type":"assistant","isMeta":true,"uuid":"a1","parentUuid":"parent-1","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#;
    let out = run(assistant, Some(Class::AgentMessagePlain));
    assert!(out.0.is_empty() && out.1.is_empty());
    // THE HALF A LEAF ALONE CANNOT SEPARATE. A TEAMMATE relay is an inbound message like
    // the coordinator's and the peer session's and carries the SAME leaf, but the harness
    // writes it WITHOUT the authorship flag - so it is not this family's record and must
    // take none of the relay facts. A gate that refused only a record failing BOTH tests
    // would stamp a `replies_to` reason on every teammate message in the corpus, and the
    // leaf is no help: all three arrive as `agent.communication.inbox`.
    let teammate = r#"{"type":"user","uuid":"t1","parentUuid":"parent-1","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"lead\">go</teammate-message>"}}"#;
    let out = run(teammate, Some(Class::CommInbox));
    assert!(
        out.0.is_empty() && out.1.is_empty(),
        "a teammate relay is not this family's: {}",
        zone(&out)
    );
}

/// Each leaf names its OWN relation, so a reader learns what the injection was for rather
/// than only which record sits above it. The line comes from the uuid map, and the basis
/// is POSITION for every one of them because the parent link says where the record sits.
#[test]
fn each_leaf_names_its_own_relation() {
    let table: &[(Class, &str)] = &[
        (Class::EnforceRetry, "redoes"),
        (Class::EnforceBrief, "redoes"),
        (Class::EnforceHandback, "redoes"),
        (Class::EnforceStructuredOutput, "redoes"),
        (Class::HookFeedback, "blocked"),
        (Class::ScheduleFire, "fires"),
        (Class::CommandCaveat, "attached_to"),
        (Class::ContextReminder, "attached_to"),
        (Class::ContextSkill, "attached_to"),
    ];
    for &(class, kind) in table {
        let out = run(&meta("m1", "parent-1", "whatever"), Some(class));
        assert_eq!(zone(&out), format!("[for: {kind} L7]"), "{kind}");
        let edge = &wire(&out)["for"][0];
        assert_eq!(edge["kind"], kind);
        assert_eq!(edge["basis"], "position");
        assert_eq!(edge["target"]["line"], 7);
        assert_eq!(edge["target"]["uuid"], "parent-1");
    }
}

/// Three leaves this family classifies carry no edge, because no key on those records
/// names one - an empty arm rather than an invented relation.
#[test]
fn the_edgeless_leaves_carry_no_edge() {
    for class in [
        Class::ScheduleWakeup,
        Class::ScheduleTick,
        Class::ResumePrompt,
    ] {
        let out = run(&meta("m1", "parent-1", "whatever"), Some(class));
        assert!(
            out.0.is_empty() && out.1.is_empty(),
            "{:?} took an edge it has no key for: {}",
            class,
            zone(&out)
        );
    }
}

/// A parent no record of this file carries keeps the ID and drops the LINE - a different
/// answer from carrying no key at all, and the two must not render alike.
#[test]
fn an_unresolved_parent_keeps_the_id_and_drops_the_line() {
    let out = run(&meta("m1", "elsewhere", "x"), Some(Class::CommandCaveat));
    assert_eq!(zone(&out), "[for: attached_to elsewher (not in this file)]");
    let edge = &wire(&out)["for"][0];
    assert_eq!(edge["target"]["uuid"], "elsewhere");
    assert!(edge["target"]["line"].is_null());
    assert!(edge["reason"].is_null());
}

/// No key at all states its REASON, because an edge left out of the output reads as an
/// edge that does not exist.
#[test]
fn a_missing_key_states_its_reason() {
    let orphan =
        r#"{"type":"user","isMeta":true,"uuid":"m1","message":{"role":"user","content":"x"}}"#;
    let out = run(orphan, Some(Class::CommandCaveat));
    assert_eq!(
        zone(&out),
        "[for: attached_to unknown (the record carries no such key)]"
    );
    let edge = &wire(&out)["for"][0];
    assert!(edge["target"].is_null() && edge["basis"].is_null());
    assert_eq!(edge["reason"], "the record carries no such key");
}

/// THE TWO RELAY SHAPES, which share a leaf and must not share a reason. The arm reads the
/// record's own `origin.kind`: a reason naming the coordinator on a peer record is a false
/// statement about the record in front of the reader, and the leaf cannot tell them apart.
#[test]
fn the_two_inbound_relays_answer_for_their_own_shape() {
    let coordinator = r#"{"type":"user","isMeta":true,"uuid":"c1","parentUuid":"parent-1","origin":{"kind":"coordinator"},"message":{"role":"user","content":"The coordinator sent a message"}}"#;
    let out = run(coordinator, Some(Class::CommInbox));
    assert_eq!(
        zone(&out),
        "[for: replies_to unknown (a coordinator relay's origin carries only its kind)]"
    );

    let peer = r#"{"type":"user","isMeta":true,"uuid":"p1","parentUuid":"parent-1","origin":{"kind":"peer","from":"uds:/tmp/s.sock","senderTaskId":"task-9"},"message":{"role":"user","content":"Another Claude session sent a message:\n<cross-session-message from=\"uds:/tmp/s.sock\">ping</cross-session-message>"}}"#;
    let out = run(peer, Some(Class::CommInbox));
    assert_eq!(
        zone(&out),
        "[peer lane task-9] [for: replies_to unknown (a peer relay names the lane that sent it, \
         not a message it answers)]"
    );
    assert_eq!(wire(&out)["peer_sender_task_id"], "task-9");

    // The minority peer shape: an origin with no sending lane keeps the honest empty and
    // pushes no lane fact, rather than printing an empty one.
    let bare = r#"{"type":"user","isMeta":true,"uuid":"p2","parentUuid":"parent-1","origin":{"kind":"peer","from":"uds:/tmp/s.sock"},"message":{"role":"user","content":"Another Claude session sent a message:\n<cross-session-message from=\"uds:/tmp/s.sock\">ping</cross-session-message>"}}"#;
    let out = run(bare, Some(Class::CommInbox));
    assert_eq!(
        zone(&out),
        "[for: replies_to unknown (a peer relay names the lane that sent it, not a message it \
         answers)]"
    );
    assert!(wire(&out).get("peer_sender_task_id").is_none());
}

/// The person's relayed message is marked in TEXT and carries the same fact in JSON,
/// because on every other surface it is byte-identical to a turn they typed - while it
/// opens no turn at all.
#[test]
fn the_relayed_human_message_is_marked_on_both_surfaces() {
    let line = r#"{"type":"user","isMeta":true,"uuid":"m1","parentUuid":"parent-1","origin":{"kind":"human"},"message":{"role":"user","content":"ship it"}}"#;
    let out = run(line, Some(Class::UserMessage));
    assert_eq!(
        out.1.iter().map(Marker::render).collect::<Vec<_>>(),
        vec!["[relayed]"]
    );
    assert_eq!(zone(&out), "[for: attached_to L7]");
    assert_eq!(wire(&out)["relayed"], true);
    // An isMeta user.message with any OTHER origin is not a person's relay.
    let other = r#"{"type":"user","isMeta":true,"uuid":"m2","parentUuid":"parent-1","origin":{"kind":"coordinator"},"message":{"role":"user","content":"ship it"}}"#;
    let out = run(other, Some(Class::UserMessage));
    assert!(out.1.is_empty() && out.0.is_empty());
}

/// The frame carrier reports on the tool_use its pulse names. That id is not a record
/// uuid, so the shared builder finds no line and the edge renders the id with its note.
#[test]
fn the_frame_carrier_reports_the_pulse_tool_use_id() {
    let text = "[SYSTEM NOTIFICATION - NOT USER INPUT]\n<task-notification><task-id>b1234567</task-id><tool-use-id>toolu_77</tool-use-id><summary>done</summary></task-notification>";
    let out = run(
        &meta("m1", "parent-1", text),
        Some(Class::NotificationBackgroundCommand),
    );
    assert_eq!(zone(&out), "[for: reports toolu_77 (not in this file)]");
    let edge = &wire(&out)["for"][0];
    assert_eq!(edge["basis"], "key");
    assert_eq!(edge["target"]["uuid"], "toolu_77");
    assert!(edge["target"]["line"].is_null());
    // A pulse with no tool-use-id says so rather than dropping the edge.
    let bare = "[SYSTEM NOTIFICATION - NOT USER INPUT]\n<task-notification><summary>done</summary></task-notification>";
    let out = run(
        &meta("m2", "parent-1", bare),
        Some(Class::NotificationBackgroundCommand),
    );
    assert_eq!(
        zone(&out),
        "[for: reports unknown (the record carries no such key)]"
    );
}

/// A textless image record renders its block COUNTS, which is what an address has to show
/// for a record whose only content is a picture.
#[test]
fn a_textless_image_record_renders_its_block_counts() {
    let line = r#"{"type":"user","isMeta":true,"uuid":"m1","parentUuid":"parent-1","message":{"role":"user","content":[{"type":"image","source":{"data":"iVBORw0KGgo="}},{"type":"document","source":{"data":"JVBERi0="}},{"type":"image","source":{"data":"iVBORw0KGgo="}}]}}"#;
    let out = run(line, Some(Class::ContextImage));
    assert_eq!(
        zone(&out),
        "[context image-only: 2 image block(s), 1 document block(s)] [for: attached_to L7]"
    );
    assert_eq!(wire(&out)["image_only"]["images"], 2);
    assert_eq!(wire(&out)["image_only"]["documents"], 1);
}

/// The `[Image: ` REFERENCE form carries text, so it is not the image-ONLY shape and takes
/// no counts - only the edge. Nothing about the leaf changes.
#[test]
fn the_image_reference_form_carries_no_counts() {
    let out = run(
        &meta("m1", "parent-1", "[Image: source: shot.png]"),
        Some(Class::ContextImage),
    );
    assert_eq!(zone(&out), "[for: attached_to L7]");
    // A block record carrying BOTH text and an image is the person's own message shape,
    // not this one, so it takes no counts either.
    let mixed = r#"{"type":"user","isMeta":true,"uuid":"m2","parentUuid":"parent-1","message":{"role":"user","content":[{"type":"text","text":"look"},{"type":"image","source":{"data":"iVBORw0KGgo="}}]}}"#;
    let out = run(mixed, Some(Class::ContextImage));
    assert_eq!(zone(&out), "[for: attached_to L7]");
}

/// The RESIDUE - an isMeta record matching no template - carries no edge, because an edge
/// is a statement about a leaf and this record has none.
#[test]
fn the_residue_carries_no_edge() {
    let out = run(&meta("m1", "parent-1", "some unmodeled prose"), None);
    assert!(out.0.is_empty() && out.1.is_empty());
}
