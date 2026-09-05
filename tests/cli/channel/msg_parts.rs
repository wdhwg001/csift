//! What the join reports about a message csift has ALREADY emitted: a delivery that took two
//! chunks, and one whose deadline passed after it went out.
//!
//! Both are places where the ledger and the transcript could be read as disagreeing. They do
//! not: the ledger says what csift printed, the transcript says what the lane saw, and the
//! verdict names which half is missing rather than promoting one into the other.

use crate::harness::*;

const ENC: &str = "-Users-dev-relay-harbor";
const SESS: &str = "00000000-0000-4000-8000-000000000041";
const SENDER: &str = "aRelay-0123456789abcdef";

const TWO_PART: &str = "1111111111111111";
const SENT_EXPIRED: &str = "2222222222222222";
const STILL_QUEUED: &str = "3333333333333333";

fn channel_path(rel: &str) -> String {
    format!("{ENC}/{SESS}/csift-channel/{rel}")
}

fn jsonl(lines: &[String]) -> String {
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

/// The FIRST chunk of a delivery: a full header naming the sender.
fn first_chunk(uuid: &str, ts: &str, id: &str) -> String {
    format!(
        r#"{{"type":"attachment","uuid":"{uuid}","timestamp":"{ts}","attachment":{{"type":"hook_additional_context","hookEvent":"UserPromptSubmit","hookName":"csift deliver --slot 1","content":["[csift-channel v1 id={id} part=1/2 mode=steer from={SENDER} from-session=00000000 relation=sibling to={SESS}]\nthe first half of the beacon"]}}}}"#
    )
}

/// A CONTINUATION chunk: the header carries only the id and the part.
fn continuation_chunk(uuid: &str, ts: &str, id: &str) -> String {
    format!(
        r#"{{"type":"attachment","uuid":"{uuid}","timestamp":"{ts}","attachment":{{"type":"hook_additional_context","hookEvent":"UserPromptSubmit","hookName":"csift deliver --slot 2","content":["[csift-channel v1 id={id} part=2/2]\nthe second half of the beacon\n--- end ---"]}}}}"#
    )
}

fn inbox_line(id: &str, ts: &str, expires: &str) -> String {
    format!(r#"{{"id":"{id}","enqueued_utc":"{ts}","mode":"steer","expires_utc":"{expires}"}}"#)
}

fn emit_line(id: &str, slot: u32, part: u32, parts: u32, ts: &str) -> String {
    format!(
        r#"{{"id":"{id}","kind":"emit","event":"UserPromptSubmit","slot":{slot},"part":{part},"parts":{parts},"vehicle":"additionalContext","ts_utc":"{ts}","hook_session":"{SESS}","hook_agent_id":null,"block_count":null}}"#
    )
}

/// `records` is the delivery evidence the receiver's transcript carries, in order.
fn relay_home(records: &[String]) -> Home {
    let h = Home::new();
    let mut lines = vec![
        r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start the relay work"}}"#.to_string(),
    ];
    lines.extend(records.iter().cloned());
    h.write(&format!("{ENC}/{SESS}.jsonl"), &jsonl(&lines));
    h.write(
        &channel_path(&format!("inbox/{SESS}.jsonl")),
        &jsonl(&[
            inbox_line(TWO_PART, "2026-06-07T05:00:01Z", "2099-01-01T00:00:00Z"),
            inbox_line(SENT_EXPIRED, "2026-06-07T05:00:02Z", "2020-01-01T00:00:00Z"),
            inbox_line(STILL_QUEUED, "2026-06-07T05:00:03Z", "2099-01-01T00:00:00Z"),
        ]),
    );
    h.write(
        &channel_path(&format!("ledger/{SESS}.jsonl")),
        &jsonl(&[
            emit_line(TWO_PART, 1, 1, 2, "2026-06-07T05:00:05Z"),
            emit_line(TWO_PART, 2, 2, 2, "2026-06-07T05:00:06Z"),
            emit_line(SENT_EXPIRED, 1, 1, 1, "2026-06-07T05:00:07Z"),
        ]),
    );
    h
}

#[test]
fn a_two_chunk_delivery_reports_both_emits_and_cites_the_first_record_as_its_fact() {
    let h = relay_home(&[
        first_chunk("att1", "2026-06-07T05:00:05.000Z", TWO_PART),
        continuation_chunk("att2", "2026-06-07T05:00:06.000Z", TWO_PART),
    ]);
    let out = h.run(&["msg", TWO_PART, "--lane", &at(SESS)]);
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout.contains(&format!("{TWO_PART}  DELIVERED")),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("emit        UserPromptSubmit slot 1 part 1/2")
            && out
                .stdout
                .contains("emit        UserPromptSubmit slot 2 part 2/2"),
        "both chunks are reported, one line each:\n{}",
        out.stdout
    );
    // One fact per message: the FIRST record carrying the id, which is when the lane saw the
    // header. A second line would double-count one delivery.
    assert!(
        out.stdout.contains("fact        L2 uuid att1"),
        "the earliest record is the fact:\n{}",
        out.stdout
    );

    let json = h.run(&["msg", TWO_PART, "--lane", &at(SESS), "--format", "json"]);
    let row: serde_json::Value = serde_json::from_str(json.stdout.lines().nth(1).unwrap()).unwrap();
    assert_eq!(row["emits"].as_array().unwrap().len(), 2);
    assert_eq!(row["emits"][1]["part"], 2);
    assert_eq!(row["emits"][1]["parts"], 2);
    assert_eq!(row["fact"]["line"], 2);
}

#[test]
fn the_same_two_emits_with_no_record_in_the_lane_read_intent_only() {
    // The whole reason the two halves stay separate files: csift printing a chunk is not the
    // lane receiving it. A crash window or a flush that has not landed looks exactly like this,
    // and it is not allowed to read as a delivery.
    let h = relay_home(&[]);
    let out = h.run(&["msg", TWO_PART, "--lane", &at(SESS)]);
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout.contains(&format!("{TWO_PART}  INTENT-ONLY")),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("fact        none in this lane's transcript"),
        "the missing half is named:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("emit        UserPromptSubmit slot 1 part 1/2"),
        "the intent is still reported in full:\n{}",
        out.stdout
    );
}

#[test]
fn a_message_that_went_out_before_it_expired_stays_in_the_sent_view() {
    // `--sent` asks what csift EMITTED, not what is still live: a message whose deadline has
    // since passed was still put on the wire, and dropping it from the sent view would hide a
    // delivery that really happened. The expiry is reported on the row instead.
    let h = relay_home(&[first_chunk(
        "att1",
        "2026-06-07T05:00:07.000Z",
        SENT_EXPIRED,
    )]);
    let sent = h.run(&["msg", "--lane", &at(SESS), "--sent"]);
    assert!(sent.success, "{}", sent.stderr);
    assert!(
        sent.stdout.contains(SENT_EXPIRED),
        "an emitted message stays in the sent view after its ttl:\n{}",
        sent.stdout
    );
    assert!(
        !sent.stdout.contains(STILL_QUEUED),
        "a message nothing emitted is not in it:\n{}",
        sent.stdout
    );

    // Its verdict is what actually happened to it, not EXPIRED: the deadline only decides the
    // verdict of a message that never went out.
    let one = h.run(&["msg", SENT_EXPIRED, "--lane", &at(SESS)]);
    assert!(
        one.stdout.contains(&format!("{SENT_EXPIRED}  DELIVERED")),
        "{}",
        one.stdout
    );
    let json = h.run(&["msg", SENT_EXPIRED, "--lane", &at(SESS), "--format", "json"]);
    let row: serde_json::Value = serde_json::from_str(json.stdout.lines().nth(1).unwrap()).unwrap();
    assert_eq!(
        row["expired"], true,
        "the elapsed deadline is still reported on the row: {row}"
    );
    assert_eq!(row["verdict"], "DELIVERED");

    // The message that never went out is the one the deadline decides.
    let never = h.run(&["msg", STILL_QUEUED, "--lane", &at(SESS)]);
    assert!(
        never.stdout.contains(&format!("{STILL_QUEUED}  QUEUED")),
        "{}",
        never.stdout
    );
}
