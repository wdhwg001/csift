//! `csift deliver` at its edges: the message it will NOT put on the wire, the slot that ran
//! out of order, and the recipe that touches nothing.
//!
//! Each of these is a place the hook has to say something other than "here is your chunk", and
//! each leaves its answer in a different place: the ledger for a message that expired or lost
//! its source, the chunk itself for a disturbed order, and nothing at all for the recipe.

use crate::harness::*;

const ENC: &str = "-Users-dev-relay";
const CWD: &str = "/Users/dev/relay";
const SENDER: &str = "00000000-0000-4000-8000-0000000000ff";
const MSG: &str = "0123456789abcdef";
const OTHER_MSG: &str = "fedcba9876543210";

fn transcript(h: &Home, session: &str) -> String {
    jpath(
        &h.projects()
            .join(ENC)
            .join(format!("{session}.jsonl"))
            .display()
            .to_string(),
    )
}

fn payload(h: &Home, session: &str, event: &str) -> String {
    let tp = transcript(h, session);
    format!(
        r#"{{"session_id":"{session}","transcript_path":"{tp}","cwd":"{CWD}","hook_event_name":"{event}"}}"#
    )
}

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

/// Write the message SOURCE for one id.
fn write_source(h: &Home, session: &str, lane: &str, id: &str, body: &str) {
    let body = serde_json::to_string(body).unwrap();
    h.write(
        &format!("{ENC}/{session}/csift-channel/messages/{id}.json"),
        &format!(
            r#"{{"id":"{id}","ts_utc":"2026-06-07T05:00:05Z","from":{{"kind":"lane","session":"{SENDER}","lane":"{SENDER}","label":null,"cwd":"/Users/dev/harbor"}},"to":{{"session":"{session}","lane":"{lane}","form":"transcript","routing_id":null}},"mode":"steer","ttl_secs":43200,"relation":"sibling","cross_project":false,"body":{body}}}"#
        ),
    );
}

/// Append one inbox line, with an explicit expiry (`None` = never expires).
fn enqueue(h: &Home, session: &str, lane: &str, id: &str, expires: Option<&str>) {
    let expires = expires.map_or_else(|| "null".to_string(), |e| format!("\"{e}\""));
    let rel = format!("{ENC}/{session}/csift-channel/inbox/{lane}.jsonl");
    let existing = std::fs::read_to_string(h.projects().join(&rel)).unwrap_or_default();
    let line = format!(
        r#"{{"id":"{id}","enqueued_utc":"2026-06-07T05:00:05Z","mode":"steer","expires_utc":{expires}}}"#
    );
    h.write(&rel, &format!("{existing}{line}\n"));
}

fn ledger(h: &Home, session: &str, lane: &str) -> Vec<serde_json::Value> {
    std::fs::read_to_string(
        h.projects()
            .join(format!("{ENC}/{session}/csift-channel/ledger/{lane}.jsonl")),
    )
    .unwrap_or_default()
    .lines()
    .map(|l| serde_json::from_str(l).expect("a ledger line"))
    .collect()
}

#[test]
fn a_message_whose_deadline_passed_is_booked_expired_and_never_emitted() {
    // The ttl is the sender's promise about how long the message is worth reading. A hook
    // that fires after it must record the outcome rather than deliver a stale steer - and
    // must record it ONCE, so a later firing does not re-book the same expiry.
    let session = "00000000-0000-4000-8000-000000000061";
    let h = Home::new();
    write_transcript(&h, session);
    write_source(
        &h,
        session,
        session,
        MSG,
        "this was worth reading yesterday",
    );
    enqueue(&h, session, session, MSG, Some("2020-01-01T00:00:00Z"));

    let out = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, "PostToolUse"),
    );
    assert!(out.success, "a bookkeeping firing exits 0: {}", out.stderr);
    assert_eq!(out.stdout, "", "an expired message is not put on the wire");

    let rows = ledger(&h, session, session);
    assert_eq!(rows.len(), 1, "one line, and it is the expiry: {rows:?}");
    assert_eq!(rows[0]["kind"], "expired");
    assert_eq!(rows[0]["id"], MSG);

    // A second firing sees the expiry already booked and says nothing more.
    let again = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, "PostToolUse"),
    );
    assert!(again.success);
    assert_eq!(again.stdout, "");
    assert_eq!(
        ledger(&h, session, session).len(),
        1,
        "the expiry is booked once, not once per hook point"
    );
}

#[test]
fn an_enqueued_message_whose_source_is_gone_is_held_with_the_reason() {
    // A hole in the channel's own directory: the inbox says a message is waiting and
    // `messages/<id>.json` is not there to render. There is nothing to deliver and nothing to
    // guess, so the hook records the hole once and stays quiet.
    let session = "00000000-0000-4000-8000-000000000062";
    let h = Home::new();
    write_transcript(&h, session);
    enqueue(&h, session, session, MSG, None);

    let out = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, "PostToolUse"),
    );
    assert!(out.success, "{}", out.stderr);
    assert_eq!(
        out.stdout, "",
        "nothing can be rendered from a missing source"
    );

    let rows = ledger(&h, session, session);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["kind"], "held");
    assert_eq!(rows[0]["reason"], "source-missing");
    assert_eq!(rows[0]["id"], MSG);

    let again = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, "PostToolUse"),
    );
    assert!(again.success);
    assert_eq!(
        ledger(&h, session, session).len(),
        1,
        "a hole is worth saying once"
    );

    // `csift msg` reads the same two files and reports the hold rather than a delivery.
    let msg = h.run(&["msg", MSG, "--lane", &at(session)]);
    assert!(msg.success, "{}", msg.stderr);
    assert!(
        msg.stdout.contains("HELD") && msg.stdout.contains("source-missing"),
        "{}",
        msg.stdout
    );
}

#[test]
fn a_slot_that_gave_up_waiting_still_emits_and_says_the_order_may_be_disturbed() {
    // Claude Code runs the same event's hooks CONCURRENTLY, so slot 2 waits for slot 1's
    // marker before printing its chunk. The wait is bounded and never blocks: with the chain
    // directory present but slot 1's marker absent, slot 2 emits anyway and tells the receiver
    // the order may be disturbed. This case costs the full 5s slot deadline by construction.
    let session = "00000000-0000-4000-8000-000000000063";
    let h = Home::new();
    write_transcript(&h, session);
    // Two small messages, so the firing's chunk list has a second entry for slot 2 to take.
    write_source(&h, session, session, MSG, "the first beacon");
    enqueue(&h, session, session, MSG, None);
    write_source(&h, session, session, OTHER_MSG, "the second beacon");
    enqueue(&h, session, session, OTHER_MSG, None);

    // The chain csift will open: keyed by the hook process's PARENT pid, which is this test.
    let chain = std::env::temp_dir().join(format!(
        "csift-deliver-{}-PostToolUse-{session}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&chain);
    std::fs::create_dir_all(&chain).unwrap();

    let out = h.run_with_stdin(
        &["deliver", "--slot", "2"],
        &payload(&h, session, "PostToolUse"),
    );
    assert!(
        out.success,
        "a timeout never fails a delivery: {}",
        out.stderr
    );
    let v: serde_json::Value = serde_json::from_str(out.stdout.trim()).expect("hook output");
    let chunk = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert_eq!(
        chunk.lines().next().unwrap(),
        "[csift-channel warning: slot 2 emitted before slot 1; order may be disturbed]",
        "the warning is the chunk's FIRST line, ahead of the envelope: {chunk}"
    );
    assert!(
        chunk.contains("the second beacon"),
        "and the message is still delivered: {chunk}"
    );
    assert!(
        chain.join("s2.done").exists(),
        "the run went through the pre-made chain directory"
    );
    let _ = std::fs::remove_dir_all(&chain);
}

/// Every file under the temp home, keyed by its path relative to that home, with its bytes.
fn snapshot(root: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(bytes) = std::fs::read(&p) {
                let rel = p.strip_prefix(root).unwrap_or(&p).display().to_string();
                out.insert(rel, bytes);
            }
        }
    }
    out
}

#[test]
fn the_recipe_writes_nothing_at_all() {
    // csift never installs its own hook: the recipe PRINTS the block and the receiver's
    // operator pastes it. A recipe run that wrote a settings file, a channel directory or an
    // armed marker would make csift a writer of state it does not own.
    let session = "00000000-0000-4000-8000-000000000064";
    let h = Home::new();
    write_transcript(&h, session);
    write_source(&h, session, session, MSG, "not for the recipe to touch");
    enqueue(&h, session, session, MSG, None);

    let before = snapshot(&h.root);
    let out = h.run(&["deliver", "--recipe", "--slots", "3"]);
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout.contains("csift deliver --slot 3"),
        "the block is printed: {}",
        out.stdout
    );
    let after = snapshot(&h.root);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "the recipe created or removed no file"
    );
    assert!(
        before == after,
        "the recipe changed a file's bytes: {:?}",
        before
            .iter()
            .filter(|(k, v)| after.get(*k) != Some(v))
            .map(|(k, _)| k)
            .collect::<Vec<_>>()
    );
}
