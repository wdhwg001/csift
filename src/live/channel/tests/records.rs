//! The three record files: the message source, the sender's outbox, the receiver's
//! inbox. Each round-trips through its projection and counts what it cannot read.

use super::*;

fn outbox_line() -> OutboxLine {
    OutboxLine {
        id: MSG_ID.to_string(),
        ts_utc: "2026-06-07T05:00:05Z".to_string(),
        to_lane: RECEIVER.to_string(),
        to_session: RECEIVER.to_string(),
        mode: Mode::Steer,
        verdict: Verdict::Ok,
        channel: "csift-steer".to_string(),
        official: OfficialRef::none(),
    }
}

fn inbox_line() -> InboxLine {
    InboxLine {
        id: MSG_ID.to_string(),
        enqueued_utc: "2026-06-07T05:00:05Z".to_string(),
        mode: Mode::Queue,
        expires_utc: Some("2026-06-07T17:00:05Z".to_string()),
    }
}

#[test]
fn a_message_source_is_written_once_and_reads_back_whole() {
    let fx = Fixture::new();
    let msg = message("bring the beacon up");
    let path = write_message(&fx.root, &msg).unwrap();
    assert!(path.ends_with(format!("messages/{MSG_ID}.json")));
    let back = read_message(&fx.root, MSG_ID).unwrap().expect("a source");
    assert_eq!(back.body, msg.body);
    assert_eq!(back.to.lane, msg.to.lane);
    assert_eq!(back.relation, msg.relation);
    assert_eq!(message_ids(&fx.root), vec![MSG_ID.to_string()]);
}

#[test]
fn an_absent_or_unreadable_message_source_reads_as_none_not_an_error() {
    let fx = Fixture::new();
    assert!(read_message(&fx.root, MSG_ID).unwrap().is_none());
    assert!(message_ids(&fx.root).is_empty());
    let path = message_path(&fx.root, MSG_ID).unwrap();
    write_atomic(&path, "{ not json").unwrap();
    assert!(read_message(&fx.root, MSG_ID).unwrap().is_none());
    // A stray file in the directory is ignored rather than reported as a message.
    write_atomic(&fx.root.join("messages").join("notes.txt"), "hi").unwrap();
    assert_eq!(message_ids(&fx.root), vec![MSG_ID.to_string()]);
}

#[test]
fn the_outbox_round_trips_and_keeps_the_delegated_official_call() {
    let fx = Fixture::new();
    append_outbox(&fx.root, &outbox_line()).unwrap();
    let mut delegated = outbox_line();
    delegated.verdict = Verdict::MayFail;
    delegated.channel = "official-mailbox".to_string();
    delegated.official = OfficialRef {
        delegated: true,
        tool: Some("SendMessage".to_string()),
        to_form: Some("Relay@beacon".to_string()),
    };
    append_outbox(&fx.root, &delegated).unwrap();

    let (lines, skipped) = read_outbox(&fx.root).unwrap();
    assert_eq!(skipped, 0);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], outbox_line());
    assert_eq!(lines[1].verdict, Verdict::MayFail);
    assert!(lines[1].official.delegated);
    assert_eq!(lines[1].official.tool.as_deref(), Some("SendMessage"));
    assert_eq!(lines[1].official.to_form.as_deref(), Some("Relay@beacon"));
}

#[test]
fn the_inbox_round_trips_and_a_missing_ttl_stays_absent() {
    let fx = Fixture::new();
    append_inbox(&fx.root, RECEIVER, &inbox_line()).unwrap();
    let mut forever = inbox_line();
    forever.expires_utc = None;
    forever.mode = Mode::Steer;
    append_inbox(&fx.root, RECEIVER, &forever).unwrap();

    let (lines, skipped) = read_inbox(&fx.root, RECEIVER).unwrap();
    assert_eq!(skipped, 0);
    assert_eq!(lines, vec![inbox_line(), forever]);
    // Another lane's inbox is a different file and stays empty.
    let (other, _) = read_inbox(&fx.root, AGENT).unwrap();
    assert!(other.is_empty());
}

#[test]
fn a_line_the_current_schema_cannot_read_is_counted_not_dropped_silently() {
    let fx = Fixture::new();
    append_inbox(&fx.root, RECEIVER, &inbox_line()).unwrap();
    // Shaped like a future csift wrote it: valid JSON, unreadable mode.
    append_line(
        &inbox_path(&fx.root, RECEIVER).unwrap(),
        "{\"id\":\"0123456789abcdef\",\"enqueued_utc\":\"2026-06-07T05:00:05Z\",\"mode\":\"burst\"}",
    )
    .unwrap();
    let (lines, skipped) = read_inbox(&fx.root, RECEIVER).unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(skipped, 1);

    append_outbox(&fx.root, &outbox_line()).unwrap();
    append_line(&outbox_path(&fx.root), "{\"id\":\"0123456789abcdef\"}").unwrap();
    let (lines, skipped) = read_outbox(&fx.root).unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(skipped, 1);
}

#[test]
fn an_outbox_line_without_an_official_block_reads_as_not_delegated() {
    let fx = Fixture::new();
    let mut v = outbox_line().to_json();
    v.as_object_mut().unwrap().remove("official");
    append_line(&outbox_path(&fx.root), &serde_json::to_string(&v).unwrap()).unwrap();
    let (lines, skipped) = read_outbox(&fx.root).unwrap();
    assert_eq!(skipped, 0);
    assert_eq!(lines[0].official, OfficialRef::none());
}
