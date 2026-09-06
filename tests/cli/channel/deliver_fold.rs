//! What one firing READS back before it emits: the ledger prefix it folds, the re-entry it
//! counts, the vehicle a partly-recorded message may still use, and the version it stamps on
//! the armed marker.
//!
//! Each of these is a decision made from bytes csift wrote earlier, so each is driven here
//! from a channel directory laid out by hand - the shapes a crashed hook, a second compaction
//! or a version-less transcript really leave behind.

use crate::harness::*;

const ENC: &str = "-Users-dev-fold";
const CWD: &str = "/Users/dev/fold";
const SENDER: &str = "00000000-0000-4000-8000-0000000000ff";
// One session id per test, and none shared with another file: the slot chain lives in the
// system temp directory keyed by (parent pid, event, lane), and the whole suite runs in ONE
// process - so two tests sharing a lane id would wipe each other's chain directory mid-firing.
const FIRST: &str = "0123456789abcdef";
const SECOND: &str = "fedcba9876543210";

fn transcript_path(h: &Home, session: &str) -> String {
    jpath(
        &h.projects()
            .join(ENC)
            .join(format!("{session}.jsonl"))
            .display()
            .to_string(),
    )
}

fn payload(h: &Home, session: &str, event: &str, extra: &str) -> String {
    let tp = transcript_path(h, session);
    format!(
        r#"{{"session_id":"{session}","transcript_path":"{tp}","cwd":"{CWD}","hook_event_name":"{event}"{extra}}}"#
    )
}

/// A one-record transcript carrying a version stamp.
fn write_transcript(h: &Home, session: &str) {
    h.write(
        &format!("{ENC}/{session}.jsonl"),
        &format!(
            "{}\n",
            format_args!(
                r#"{{"type":"user","uuid":"u0","sessionId":"{session}","cwd":"{CWD}","version":"2.1.258","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"go"}}}}"#
            )
        ),
    );
}

/// Write a message source and enqueue it for the session's own lane.
fn seed(h: &Home, session: &str, id: &str, mode: &str, body: &str) {
    let body = serde_json::to_string(body).unwrap();
    h.write(
        &format!("{ENC}/{session}/csift-channel/messages/{id}.json"),
        &format!(
            r#"{{"id":"{id}","ts_utc":"2026-06-07T05:00:05Z","from":{{"kind":"lane","session":"{SENDER}","lane":"{SENDER}","label":null,"cwd":"{CWD}"}},"to":{{"session":"{session}","lane":"{session}","form":"transcript","routing_id":null}},"mode":"{mode}","ttl_secs":43200,"relation":"sibling","cross_project":false,"body":{body}}}"#
        ),
    );
    let rel = format!("{ENC}/{session}/csift-channel/inbox/{session}.jsonl");
    let existing = std::fs::read_to_string(h.projects().join(&rel)).unwrap_or_default();
    h.write(
        &rel,
        &format!(
            "{existing}{}\n",
            format_args!(
                r#"{{"id":"{id}","enqueued_utc":"2026-06-07T05:00:05Z","mode":"{mode}","expires_utc":null}}"#
            )
        ),
    );
}

fn emit_line(id: &str, event: &str) -> String {
    format!(
        r#"{{"id":"{id}","kind":"emit","event":"{event}","slot":1,"part":1,"parts":1,"vehicle":"additionalContext","ts_utc":"2026-06-07T05:00:06Z"}}"#
    )
}

fn channel_file(h: &Home, session: &str, rel: &str) -> String {
    std::fs::read_to_string(
        h.projects()
            .join(format!("{ENC}/{session}/csift-channel/{rel}")),
    )
    .unwrap_or_default()
}

fn context(stdout: &str) -> String {
    let line = stdout.trim();
    assert!(!line.is_empty(), "a delivery prints one object");
    let v: serde_json::Value = serde_json::from_str(line).expect("valid hook output json");
    v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("the chunk")
        .to_string()
}

#[test]
fn a_ledger_line_with_no_newline_after_it_is_dropped_rather_than_folded() {
    // The fold reads a BYTE prefix, so its last line can be half of an append that never
    // finished. Counting such a fragment as a real emit would mark a message delivered that
    // nobody ever saw, and the lane would never offer it again - so the tail goes, and the
    // message stays pending. The line before it is complete and still folds.
    let session = "00000000-0000-4000-8000-0000000000b1";
    let h = Home::new();
    write_transcript(&h, session);
    seed(&h, session, FIRST, "steer", "beacon alpha already went out");
    seed(&h, session, SECOND, "steer", "beacon bravo never left");
    h.write(
        &format!("{ENC}/{session}/csift-channel/ledger/{session}.jsonl"),
        &format!(
            "{}\n{}",
            emit_line(FIRST, "PostToolUse"),
            emit_line(SECOND, "PostToolUse")
        ),
    );

    let out = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, "PostToolUse", ""),
    );
    assert!(out.success, "{}", out.stderr);
    let chunk = context(&out.stdout);
    assert!(
        chunk.contains("beacon bravo never left"),
        "the torn tail line is not an emit, so its message is still pending:\n{chunk}"
    );
    assert!(
        !chunk.contains("beacon alpha already went out"),
        "the complete line before it still folds:\n{chunk}"
    );
}

#[test]
fn a_second_compaction_offers_the_message_again_after_the_re_emit() {
    // A compaction throws away the context the delivery landed in, so an unacked message is
    // offered once more - once per compaction. The rule is "is the newest line a redelivery or
    // an emit", not "has this ever been redelivered": reading it the second way would offer a
    // message exactly once however many compactions the lane lives through.
    let session = "00000000-0000-4000-8000-0000000000b2";
    let h = Home::new();
    write_transcript(&h, session);
    seed(&h, session, FIRST, "steer", "still worth reading");

    let sent = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, "PostToolUse", ""),
    );
    assert!(
        !sent.stdout.is_empty(),
        "the first delivery: {}",
        sent.stderr
    );
    let first = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, "SessionStart", r#","source":"compact""#),
    );
    assert!(first.success, "{}", first.stderr);
    assert!(context(&first.stdout).contains("still worth reading"));

    let second = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, "SessionStart", r#","source":"compact""#),
    );
    assert!(second.success, "{}", second.stderr);
    assert!(
        context(&second.stdout).contains("still worth reading"),
        "the re-emit is newer than the redelivery, so the next compaction offers it again:\n{}",
        second.stdout
    );
    let ledger = channel_file(&h, session, &format!("ledger/{session}.jsonl"));
    assert_eq!(
        ledger
            .lines()
            .filter(|l| l.contains(r#""kind":"redelivered""#))
            .count(),
        2,
        "one redelivery per compaction:\n{ledger}"
    );
}

#[test]
fn a_message_that_has_never_blocked_a_turn_still_gets_the_blocking_vehicle() {
    // The rule is "this message has already spent a block", counted from its emits. A message
    // whose only ledger line is a HOLD has spent none, so the turn-blocking vehicle is still
    // open to it - reading any recorded state as a prior block would quietly downgrade every
    // queue message that was held once.
    let session = "00000000-0000-4000-8000-0000000000b3";
    let h = Home::new();
    write_transcript(&h, session);
    seed(&h, session, FIRST, "queue", "review before you stop");
    h.write(
        &format!("{ENC}/{session}/csift-channel/ledger/{session}.jsonl"),
        &format!(
            "{}\n",
            format_args!(
                r#"{{"id":"{FIRST}","kind":"held","reason":"block-cap","ts_utc":"2026-06-07T05:00:06Z"}}"#
            )
        ),
    );

    let out = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, "Stop", r#","stop_hook_active":false"#),
    );
    assert_eq!(out.code, Some(2), "exit 2 blocks the turn: {}", out.stdout);
    assert!(
        out.stderr.contains("review before you stop"),
        "{}",
        out.stderr
    );
    let ledger = channel_file(&h, session, &format!("ledger/{session}.jsonl"));
    assert!(
        ledger.contains(r#""vehicle":"exit2""#),
        "the emit records the vehicle it used:\n{ledger}"
    );
}

#[test]
fn the_marker_version_falls_back_to_the_environment_only_when_the_tail_has_none() {
    // The marker says which Claude Code wrote it, and it is never invented: the tail first,
    // then the running version the harness exports. A transcript that carries no stamp at all
    // is exactly when the environment has to answer.
    let session = "00000000-0000-4000-8000-0000000000b4";
    let h = Home::new();
    h.write(
        &format!("{ENC}/{session}.jsonl"),
        &format!(
            "{}\n",
            format_args!(
                r#"{{"type":"user","uuid":"u0","sessionId":"{session}","cwd":"{CWD}","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"go"}}}}"#
            )
        ),
    );

    let out = h.run_with_stdin_env(
        &["deliver", "--slot", "1"],
        &payload(&h, session, "PreCompact", ""),
        &[("CLAUDE_CODE_VERSION", "2.1.999")],
    );
    assert!(out.success, "{}", out.stderr);
    let marker: serde_json::Value =
        serde_json::from_str(&channel_file(&h, session, &format!("armed/{session}.json")))
            .expect("the armed marker");
    assert_eq!(marker["claude_code_version"], "2.1.999");
}

#[test]
fn the_marker_version_is_read_from_the_whole_tail_window_not_the_last_line() {
    // Real records run to kilobytes - one tool result fills more than a page - so a window
    // that only reaches the newest record would report the version as unknown on any busy
    // lane. The stamped record here sits behind four kilobytes of a record that carries none,
    // and the environment names a DIFFERENT version, so a short window is visible as such.
    let session = "00000000-0000-4000-8000-0000000000b5";
    let h = Home::new();
    let padding = "pad ".repeat(1024);
    h.write(
        &format!("{ENC}/{session}.jsonl"),
        &format!(
            "{}\n{}\n",
            format_args!(
                r#"{{"type":"user","uuid":"u0","sessionId":"{session}","cwd":"{CWD}","version":"2.1.258","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"go"}}}}"#
            ),
            format_args!(
                r#"{{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:05.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"{padding}"}}]}}}}"#
            ),
        ),
    );

    let out = h.run_with_stdin_env(
        &["deliver", "--slot", "1"],
        &payload(&h, session, "PreCompact", ""),
        &[("CLAUDE_CODE_VERSION", "0.0.0-from-the-environment")],
    );
    assert!(out.success, "{}", out.stderr);
    let marker: serde_json::Value =
        serde_json::from_str(&channel_file(&h, session, &format!("armed/{session}.json")))
            .expect("the armed marker");
    assert_eq!(
        marker["claude_code_version"], "2.1.258",
        "the transcript outranks the environment whenever the window reaches a stamp"
    );
}
