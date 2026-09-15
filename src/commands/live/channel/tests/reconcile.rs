//! The intent-against-fact join: lane derivation from a transcript path, the transcript
//! scan, the expiry rule and the verdict ladder.

use super::*;

use std::collections::BTreeMap;
use std::path::Path;

const PROJECT: &str = "/Users/dev/.claude/projects/-Users-dev-relay";

/// A delivery record and a decoy: prose that merely quotes an envelope header carries no
/// header at the START of its content, so it is not a fact.
const FIRST_CHUNK: &str = r#"{"type":"attachment","uuid":"att1","timestamp":"2026-06-07T05:00:01.000Z","attachment":{"type":"hook_additional_context","hookEvent":"UserPromptSubmit","content":["[csift-channel v1 id=0123456789abcdef part=1/2 mode=steer from=aRelay-0123456789abcdef from-session=00000000 relation=sibling to=00000000-0000-4000-8000-000000000001]\nthrottlebeacon the region queue"]}}"#;
const NEXT_CHUNK: &str = r#"{"type":"attachment","uuid":"att2","timestamp":"2026-06-07T05:00:02.000Z","attachment":{"type":"hook_additional_context","hookEvent":"PostToolUse","content":["[csift-channel v1 id=0123456789abcdef part=2/2]\nthe rest\n--- end ---"]}}"#;
const OTHER_ID: &str = r#"{"type":"attachment","uuid":"att3","timestamp":"2026-06-07T05:00:03.000Z","attachment":{"type":"hook_additional_context","hookEvent":"PostToolUse","content":["[csift-channel v1 id=00000000000000ff part=1/1 mode=queue from=external:harbor from-session=unknown relation=external to=00000000-0000-4000-8000-000000000001]\nanother message"]}}"#;
const QUOTING_PROSE: &str = r#"{"type":"user","uuid":"u9","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"user","content":"the header read [csift-channel v1 id=0123456789abcdef part=1/2] and then stopped"}}"#;

/// A scratch transcript, removed on drop.
struct Transcript {
    dir: PathBuf,
    path: PathBuf,
}

impl Transcript {
    fn new(lines: &[&str]) -> Self {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "csift-reconcile-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{SESSION}.jsonl"));
        let mut body = lines.join("\n");
        body.push('\n');
        std::fs::write(&path, body).unwrap();
        Transcript { dir, path }
    }
}

impl Drop for Transcript {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn ctx() -> LaneCtx {
    lane_from_transcript(Path::new(&format!("{PROJECT}/{SESSION}.jsonl"))).unwrap()
}

fn view(state: MessageState, inbox: Option<InboxLine>) -> LaneView {
    let mut states = BTreeMap::new();
    states.insert(MSG_ID.to_string(), state);
    let mut inboxes = BTreeMap::new();
    if let Some(line) = inbox {
        inboxes.insert(MSG_ID.to_string(), line);
    }
    LaneView {
        inboxes,
        states,
        emits: BTreeMap::new(),
        skipped_lines: 0,
    }
}

fn inbox_line(expires: Option<&str>) -> InboxLine {
    InboxLine {
        id: MSG_ID.to_string(),
        enqueued_utc: "2026-06-07T05:00:05Z".to_string(),
        mode: Mode::Steer,
        expires_utc: expires.map(str::to_string),
    }
}

fn verdict(state: MessageState, inbox: Option<InboxLine>) -> MsgVerdict {
    let c = ctx();
    let v = view(state, inbox);
    reconcile(&c, MSG_ID, &v, None)
        .unwrap()
        .expect("a message the lane holds")
        .verdict
}

#[test]
fn a_top_level_lane_is_its_own_session_and_names_its_own_channel_root() {
    let c = ctx();
    assert_eq!(c.lane, SESSION);
    assert_eq!(c.session, SESSION);
    assert_eq!(
        c.root,
        PathBuf::from(format!("{PROJECT}/{SESSION}/csift-channel"))
    );
}

#[test]
fn a_child_lane_keeps_its_own_id_and_its_parent_session_root() {
    // The channel root is keyed by SESSION and the files inside it by LANE, so a subagent
    // and a workflow lane both read the parent session's directory - however deeply the
    // transcript itself is nested.
    let flat = lane_from_transcript(Path::new(&format!(
        "{PROJECT}/{SESSION}/subagents/agent-{AGENT}.jsonl"
    )))
    .unwrap();
    assert_eq!(flat.lane, AGENT);
    assert_eq!(flat.session, SESSION);
    assert_eq!(
        flat.root,
        PathBuf::from(format!("{PROJECT}/{SESSION}/csift-channel"))
    );

    let workflow = lane_from_transcript(Path::new(&format!(
        "{PROJECT}/{SESSION}/subagents/workflows/wf_relay/agent-{AGENT}.jsonl"
    )))
    .unwrap();
    assert_eq!(workflow.lane, AGENT);
    assert_eq!(workflow.root, flat.root);

    let teammate = lane_from_transcript(Path::new(&format!(
        "{PROJECT}/{SESSION}/subagents/agent-{TEAMMATE}.jsonl"
    )))
    .unwrap();
    assert_eq!(teammate.lane, TEAMMATE);
    assert_eq!(teammate.session, SESSION);
}

#[test]
fn the_fact_scan_keeps_the_first_chunk_of_each_id_and_rejects_quoted_prose() {
    let t = Transcript::new(&[FIRST_CHUNK, NEXT_CHUNK, OTHER_ID, QUOTING_PROSE]);
    let (facts, skipped) = scan_facts(&t.path, None).unwrap();
    assert_eq!(skipped, 0);
    assert_eq!(facts.len(), 2);
    // The FIRST chunk record, not the continuation: that is when the lane saw the header.
    assert_eq!(facts[MSG_ID].line, 1);
    assert_eq!(facts[MSG_ID].uuid.as_deref(), Some("att1"));
    assert_eq!(facts["00000000000000ff"].line, 3);

    // Narrowed to one id, the other delivery is not a fact for it - and the human line
    // quoting the header is never one, since a quote carries no header at content start.
    let (one, _) = scan_facts(&t.path, Some(MSG_ID)).unwrap();
    assert_eq!(one.len(), 1);
    assert!(one.contains_key(MSG_ID));
}

#[test]
fn a_corrupt_line_is_counted_never_dropped_in_silence() {
    // The never-silent law survives the byte prefilter: a line that is not brace-framed is
    // counted even though it could not have carried a delivery.
    let t = Transcript::new(&[FIRST_CHUNK, "this is not a record at all"]);
    let (facts, skipped) = scan_facts(&t.path, None).unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(skipped, 1);
}

#[test]
fn an_unreadable_transcript_fails_loudly_instead_of_reading_as_no_fact() {
    // A silent empty here would report a delivered message as INTENT-ONLY: absence of the
    // FILE is not absence of the record.
    let err = scan_facts(Path::new("/nonexistent/relay.jsonl"), None).unwrap_err();
    assert!(
        format!("{err:#}").contains("relay.jsonl"),
        "the error names the file it could not read: {err:#}"
    );
}

#[test]
fn expiry_reads_both_the_ledger_line_and_the_elapsed_ttl() {
    let now = "2026-06-07T06:00:00Z";
    let fresh = MessageState::default();
    // No deadline at all: a message with no ttl never expires on its own.
    assert!(!is_message_expired(&fresh, None, now));
    assert!(!is_message_expired(
        &fresh,
        Some(&inbox_line(Some("2099-01-01T00:00:00Z"))),
        now
    ));
    // The elapsed deadline counts even with an empty ledger: a lane whose hooks never fire
    // writes no `expired` line, and reporting those as merely queued would read as
    // "still on its way".
    assert!(is_message_expired(
        &fresh,
        Some(&inbox_line(Some("2020-01-01T00:00:00Z"))),
        now
    ));
    let expired_line = MessageState {
        expired: true,
        ..MessageState::default()
    };
    assert!(is_message_expired(&expired_line, None, now));
}

#[test]
fn the_verdict_ladder_puts_the_terminal_states_first() {
    // An ack outranks everything, including the emits it followed.
    let acked = MessageState {
        acked: true,
        emitted_parts: [1u32].into_iter().collect(),
        parts_expected: Some(1),
        ..MessageState::default()
    };
    assert_eq!(
        verdict(acked, Some(inbox_line(None))),
        MsgVerdict::Acked,
        "an ack is the receiver's own word and outranks the ledger"
    );

    let refused = MessageState {
        refused_reasons: vec!["served-call".to_string()],
        ..MessageState::default()
    };
    assert_eq!(
        verdict(refused, Some(inbox_line(None))),
        MsgVerdict::Refused
    );

    let held = MessageState {
        held_reasons: vec!["block-cap".to_string()],
        ..MessageState::default()
    };
    assert_eq!(verdict(held, Some(inbox_line(None))), MsgVerdict::Held);

    // Expiry outranks a hold: both say "not delivered", and the deadline is the one that
    // says it can no longer be.
    let held_and_expired = MessageState {
        held_reasons: vec!["block-cap".to_string()],
        expired: true,
        ..MessageState::default()
    };
    assert_eq!(
        verdict(held_and_expired, Some(inbox_line(None))),
        MsgVerdict::Expired
    );

    assert_eq!(
        verdict(MessageState::default(), Some(inbox_line(None))),
        MsgVerdict::Queued
    );
}

#[test]
fn an_emit_without_a_transcript_record_is_intent_never_delivery() {
    let emitted = MessageState {
        emitted_parts: [1u32].into_iter().collect(),
        parts_expected: Some(1),
        ..MessageState::default()
    };
    let c = ctx();
    let v = view(emitted.clone(), Some(inbox_line(None)));
    // The whole reason intent and fact are separate files: csift printed the chunk, and
    // nothing on the receiver's side proves it landed.
    assert_eq!(
        reconcile(&c, MSG_ID, &v, None).unwrap().unwrap().verdict,
        MsgVerdict::IntentOnly
    );
    let fact = Fact {
        line: 12,
        uuid: Some("att1".to_string()),
    };
    let report = reconcile(&c, MSG_ID, &v, Some(fact.clone()))
        .unwrap()
        .unwrap();
    assert_eq!(report.verdict, MsgVerdict::Delivered);
    assert_eq!(report.fact, Some(fact));

    // A ttl that elapsed does NOT retroactively unsend an emitted message.
    let v = view(emitted, Some(inbox_line(Some("2020-01-01T00:00:00Z"))));
    assert_eq!(
        reconcile(&c, MSG_ID, &v, None).unwrap().unwrap().verdict,
        MsgVerdict::IntentOnly
    );
}

#[test]
fn an_id_the_lane_never_held_reconciles_to_nothing() {
    let c = ctx();
    let empty = LaneView {
        inboxes: BTreeMap::new(),
        states: BTreeMap::new(),
        emits: BTreeMap::new(),
        skipped_lines: 0,
    };
    assert!(reconcile(&c, MSG_ID, &empty, None).unwrap().is_none());
}
