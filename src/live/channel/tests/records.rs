//! The three record files: the message source, the sender's outbox, the receiver's
//! inbox. The two csift reads back round-trip through their projections and count what
//! they cannot read; the write-only outbox is checked on the shape it writes.

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
}

#[test]
fn an_absent_or_unreadable_message_source_reads_as_none_not_an_error() {
    let fx = Fixture::new();
    assert!(read_message(&fx.root, MSG_ID).unwrap().is_none());
    let path = message_path(&fx.root, MSG_ID).unwrap();
    write_atomic(&path, "{ not json").unwrap();
    assert!(read_message(&fx.root, MSG_ID).unwrap().is_none());
}

#[test]
fn the_outbox_writes_one_line_per_send_with_the_delegated_official_call() {
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

    let written = std::fs::read_to_string(outbox_path(&fx.root)).unwrap();
    let lines: Vec<&str> = written.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(lines[0]).unwrap(),
        outbox_line().to_json()
    );
    let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(second["verdict"], "MAY-FAIL");
    assert_eq!(second["channel"], "official-mailbox");
    assert_eq!(second["official"]["delegated"], true);
    assert_eq!(second["official"]["tool"], "SendMessage");
    assert_eq!(second["official"]["to_form"], "Relay@beacon");
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
}
