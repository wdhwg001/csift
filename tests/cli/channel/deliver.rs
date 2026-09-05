//! `csift deliver` end to end: the refusal shapes, the two vehicles, the slot chain, the
//! armed marker, the two re-entry paths, and the recipe.
//!
//! Every case drives the real binary with a hook payload on stdin, which is the only way
//! this command is ever called.

use crate::harness::*;

/// The project directory these fixtures live in, and the cwd it encodes.
const ENC: &str = "-Users-dev-relay";
const CWD: &str = "/Users/dev/relay";

/// The sender lane every fixture message comes from.
const SENDER: &str = "00000000-0000-4000-8000-0000000000ff";

const MSG: &str = "0123456789abcdef";
const OTHER_MSG: &str = "fedcba9876543210";

/// The transcript path the hook payload names, as the harness laid it out.
fn transcript(h: &Home, session: &str) -> String {
    jpath(
        &h.projects()
            .join(ENC)
            .join(format!("{session}.jsonl"))
            .display()
            .to_string(),
    )
}

/// A hook payload. `extra` is appended raw, for the per-event fields.
fn payload(h: &Home, session: &str, agent: Option<&str>, event: &str, extra: &str) -> String {
    let tp = transcript(h, session);
    let agent_field = agent.map_or_else(String::new, |a| format!(r#","agent_id":"{a}""#));
    format!(
        r#"{{"session_id":"{session}","transcript_path":"{tp}","cwd":"{CWD}"{agent_field},"hook_event_name":"{event}"{extra}}}"#
    )
}

/// A one-record transcript, so the armed marker can read a version off it.
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

/// Write a message source and enqueue it for one lane.
fn seed(h: &Home, session: &str, lane: &str, id: &str, mode: &str, body: &str) {
    let body = serde_json::to_string(body).unwrap();
    h.write(
        &format!("{ENC}/{session}/csift-channel/messages/{id}.json"),
        &format!(
            r#"{{"id":"{id}","ts_utc":"2026-06-07T05:00:05Z","from":{{"kind":"lane","session":"{SENDER}","lane":"{SENDER}","label":null,"cwd":"/Users/dev/harbor"}},"to":{{"session":"{session}","lane":"{lane}","form":"transcript","routing_id":null}},"mode":"{mode}","ttl_secs":43200,"relation":"sibling","cross_project":false,"body":{body}}}"#
        ),
    );
    let inbox = format!("{ENC}/{session}/csift-channel/inbox/{lane}.jsonl");
    let line = format!(
        "{}\n",
        format_args!(
            r#"{{"id":"{id}","enqueued_utc":"2026-06-07T05:00:05Z","mode":"{mode}","expires_utc":null}}"#
        )
    );
    let path = h.projects().join(&inbox);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    h.write(&inbox, &format!("{existing}{line}"));
}

fn channel_file(h: &Home, session: &str, rel: &str) -> String {
    std::fs::read_to_string(
        h.projects()
            .join(format!("{ENC}/{session}/csift-channel/{rel}")),
    )
    .unwrap_or_default()
}

/// The one JSON object a delivery prints, parsed.
fn hook_output(stdout: &str) -> serde_json::Value {
    let line = stdout.trim();
    assert!(!line.is_empty(), "a delivery prints one object");
    assert_eq!(line.lines().count(), 1, "exactly one line: {line}");
    serde_json::from_str(line).expect("valid hook output json")
}

// ------------------------------------------------------------------ the refusal shapes

#[test]
fn a_served_call_is_refused_without_a_word() {
    let h = Home::new();
    let out = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        r#"{"session_id":"served:cli","transcript_path":"","hook_event_name":"PostToolUse"}"#,
    );
    assert!(out.success, "a refusal exits 0: {}", out.stderr);
    assert_eq!(out.stdout, "", "and prints nothing");
    assert!(
        !h.projects().join(ENC).exists(),
        "a served call writes no channel directory"
    );
}

#[test]
fn stdin_that_is_not_an_object_is_no_payload_at_all() {
    let h = Home::new();
    for raw in ["", "[]", "not json at all"] {
        let out = h.run_with_stdin(&["deliver", "--slot", "1"], raw);
        assert!(out.success, "exit 0 for `{raw}`: {}", out.stderr);
        assert_eq!(out.stdout, "", "nothing printed for `{raw}`");
    }
}

#[test]
fn a_payload_with_no_transcript_path_is_refused_into_the_ledger() {
    let session = "00000000-0000-4000-8000-000000000011";
    let h = Home::new();
    write_transcript(&h, session);
    let out = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &format!(
            r#"{{"session_id":"{session}","transcript_path":"","cwd":"{CWD}","hook_event_name":"Stop"}}"#
        ),
    );
    assert!(out.success, "a refusal exits 0: {}", out.stderr);
    assert_eq!(out.stdout, "");
    let ledger = channel_file(&h, session, &format!("ledger/{session}.jsonl"));
    let row: serde_json::Value = serde_json::from_str(ledger.trim()).expect("one ledger line");
    assert_eq!(row["kind"], "refused");
    assert_eq!(row["id"], serde_json::Value::Null);
    assert!(
        row["reason"].as_str().unwrap().contains("transcript path"),
        "the refusal names its reason: {row}"
    );
}

#[test]
fn an_agent_id_that_is_no_id_is_refused_under_the_session() {
    let session = "00000000-0000-4000-8000-000000000012";
    let h = Home::new();
    write_transcript(&h, session);
    // Short enough that it is neither a bare agent hex nor a name-embedded teammate id.
    let out = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, Some("a7"), "PostToolUse", ""),
    );
    assert!(out.success, "a refusal exits 0: {}", out.stderr);
    assert_eq!(out.stdout, "");
    let ledger = channel_file(&h, session, &format!("ledger/{session}.jsonl"));
    let row: serde_json::Value = serde_json::from_str(ledger.trim()).expect("one ledger line");
    assert_eq!(row["kind"], "refused");
    assert!(
        row["reason"].as_str().unwrap().contains("agent id"),
        "the refusal names its reason: {row}"
    );
    assert!(
        !h.projects()
            .join(format!("{ENC}/{session}/csift-channel/ledger/a7.jsonl"))
            .exists(),
        "nothing is filed under the id csift refused to accept"
    );
}

// -------------------------------------------------------------------- the two vehicles

#[test]
fn a_subagent_lane_gets_its_steer_message_at_a_tool_event() {
    let session = "00000000-0000-4000-8000-000000000021";
    let lane = "a00000000000abcd";
    let h = Home::new();
    write_transcript(&h, session);
    seed(&h, session, lane, MSG, "steer", "check the throttle beacon");

    let out = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, Some(lane), "PostToolUse", ""),
    );
    assert!(out.success, "{}", out.stderr);
    let v = hook_output(&out.stdout);
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    let chunk = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(chunk.starts_with(&format!("[csift-channel v1 id={MSG} part=1/1 mode=steer ")));
    assert!(chunk.contains(&format!("to={lane}]")));
    assert!(chunk.contains("check the throttle beacon"));
    assert!(chunk.contains("--- end ---"));
    assert!(chunk.contains(&format!("Reply: csift send @{SENDER}")));

    let ledger = channel_file(&h, session, &format!("ledger/{lane}.jsonl"));
    let row: serde_json::Value = serde_json::from_str(ledger.trim()).unwrap();
    assert_eq!(row["kind"], "emit");
    assert_eq!(row["vehicle"], "additionalContext");
    assert_eq!(row["slot"], 1);
    assert_eq!(row["hook_agent_id"], lane);

    // A second firing has nothing left to say.
    let again = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, Some(lane), "PostToolUse", ""),
    );
    assert!(again.success);
    assert_eq!(again.stdout, "", "an emitted message is not re-emitted");
}

#[test]
fn a_top_level_lane_gets_its_message_at_the_prompt() {
    let session = "00000000-0000-4000-8000-000000000022";
    let h = Home::new();
    write_transcript(&h, session);
    seed(&h, session, session, MSG, "steer", "the harbor is open");

    let out = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, None, "UserPromptSubmit", ""),
    );
    assert!(out.success, "{}", out.stderr);
    let v = hook_output(&out.stdout);
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit");
    assert!(v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("the harbor is open"));
    let ledger = channel_file(&h, session, &format!("ledger/{session}.jsonl"));
    let row: serde_json::Value = serde_json::from_str(ledger.trim()).unwrap();
    assert_eq!(row["hook_agent_id"], serde_json::Value::Null);
}

#[test]
fn a_queue_message_waits_for_the_stop_and_blocks_the_turn() {
    let session = "00000000-0000-4000-8000-000000000023";
    let h = Home::new();
    write_transcript(&h, session);
    seed(&h, session, session, MSG, "queue", "review before you stop");

    let mid_turn = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, None, "PostToolUse", ""),
    );
    assert!(mid_turn.success);
    assert_eq!(mid_turn.stdout, "", "a queue message is not a mid-turn one");

    let at_stop = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, None, "Stop", r#","stop_hook_active":false"#),
    );
    assert_eq!(at_stop.code, Some(2), "exit 2 blocks the turn from ending");
    assert_eq!(at_stop.stdout, "", "the exit-2 vehicle rides stderr");
    assert!(at_stop.stderr.contains("review before you stop"));
    assert!(at_stop.stderr.starts_with("[csift-channel v1 "));

    let ledger = channel_file(&h, session, &format!("ledger/{session}.jsonl"));
    let row: serde_json::Value = serde_json::from_str(ledger.trim()).unwrap();
    assert_eq!(row["vehicle"], "exit2");
    assert_eq!(row["block_count"], 1);
    assert_eq!(row["event"], "Stop");
}

#[test]
fn at_the_block_cap_the_queue_message_rides_additional_context() {
    let session = "00000000-0000-4000-8000-000000000024";
    let h = Home::new();
    write_transcript(&h, session);
    seed(&h, session, session, MSG, "queue", "no room left to block");

    let out = h.run_with_stdin_env(
        &["deliver", "--slot", "1"],
        &payload(&h, session, None, "Stop", ""),
        &[("CLAUDE_CODE_STOP_HOOK_BLOCK_CAP", "1")],
    );
    assert!(out.success, "the fallback exits 0: {}", out.stderr);
    let v = hook_output(&out.stdout);
    assert!(v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("no room left to block"));

    let ledger = channel_file(&h, session, &format!("ledger/{session}.jsonl"));
    let rows: Vec<serde_json::Value> = ledger
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), 2, "the downgrade is recorded beside the emit");
    assert_eq!(rows[0]["kind"], "held");
    assert_eq!(rows[0]["reason"], "block-cap");
    assert_eq!(rows[1]["kind"], "emit");
    assert_eq!(rows[1]["vehicle"], "additionalContext");
}

// ---------------------------------------------------------------------- the slot chain

#[test]
fn two_slots_carry_the_two_parts_of_one_long_message_in_order() {
    let session = "00000000-0000-4000-8000-000000000031";
    let h = Home::new();
    write_transcript(&h, session);
    // Long enough that the 9200-character chunk budget needs two parts.
    seed(&h, session, session, MSG, "steer", &"beacon ".repeat(2000));

    let first = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, None, "PostToolUse", ""),
    );
    assert!(first.success, "{}", first.stderr);
    let one = hook_output(&first.stdout);
    let one = one["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(one.starts_with(&format!("[csift-channel v1 id={MSG} part=1/2 ")));

    let second = h.run_with_stdin(
        &["deliver", "--slot", "2"],
        &payload(&h, session, None, "PostToolUse", ""),
    );
    assert!(second.success, "{}", second.stderr);
    let two = hook_output(&second.stdout);
    let two = two["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        two.lines().next().unwrap(),
        format!("[csift-channel v1 id={MSG} part=2/2]"),
        "slot 2 takes the second chunk, not a re-run of the first"
    );
    assert!(two.ends_with(
        "--- end ---\nReply: csift send @00000000-0000-4000-8000-0000000000ff \"<your reply>\""
    ));

    let ledger = channel_file(&h, session, &format!("ledger/{session}.jsonl"));
    let rows: Vec<serde_json::Value> = ledger
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0]["slot"].as_u64(), rows[0]["part"].as_u64()),
        (Some(1), Some(1))
    );
    assert_eq!(
        (rows[1]["slot"].as_u64(), rows[1]["part"].as_u64()),
        (Some(2), Some(2))
    );
    assert_eq!(rows[1]["parts"], 2);
}

#[test]
fn several_small_messages_take_one_slot_each() {
    let session = "00000000-0000-4000-8000-000000000032";
    let h = Home::new();
    write_transcript(&h, session);
    seed(&h, session, session, MSG, "steer", "the first beacon");
    seed(
        &h,
        session,
        session,
        OTHER_MSG,
        "steer",
        "the second beacon",
    );

    let first = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, None, "PostToolUse", ""),
    );
    let second = h.run_with_stdin(
        &["deliver", "--slot", "2"],
        &payload(&h, session, None, "PostToolUse", ""),
    );
    let one = hook_output(&first.stdout);
    let two = hook_output(&second.stdout);
    let one = one["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    let two = two["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(one.contains("the first beacon"));
    assert!(two.contains("the second beacon"));
    // Each carries its OWN full header, not a continuation of the other.
    assert!(one.starts_with(&format!("[csift-channel v1 id={MSG} part=1/1 mode=steer ")));
    assert!(two.starts_with(&format!(
        "[csift-channel v1 id={OTHER_MSG} part=1/1 mode=steer "
    )));
}

// --------------------------------------------------------------------- the armed marker

#[test]
fn the_armed_marker_records_every_slot_that_has_run() {
    let session = "00000000-0000-4000-8000-000000000041";
    let h = Home::new();
    write_transcript(&h, session);

    // An event that carries no delivery still refreshes the marker: the marker answers
    // "do delivery hooks really run in this lane", not "was anything delivered".
    let out = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, None, "PreCompact", ""),
    );
    assert!(out.success, "{}", out.stderr);
    assert_eq!(out.stdout, "");
    let marker: serde_json::Value =
        serde_json::from_str(&channel_file(&h, session, &format!("armed/{session}.json"))).unwrap();
    assert_eq!(marker["slots_seen"], serde_json::json!([1]));
    assert_eq!(marker["last_event"], "PreCompact");
    assert_eq!(marker["hook_session"], session);
    assert_eq!(
        marker["claude_code_version"], "2.1.258",
        "the version comes off the transcript, not out of thin air"
    );

    h.run_with_stdin(
        &["deliver", "--slot", "3"],
        &payload(&h, session, None, "Stop", ""),
    );
    let marker: serde_json::Value =
        serde_json::from_str(&channel_file(&h, session, &format!("armed/{session}.json"))).unwrap();
    assert_eq!(
        marker["slots_seen"],
        serde_json::json!([1, 3]),
        "the slot set accumulates"
    );
    assert_eq!(marker["last_event"], "Stop");
}

// ------------------------------------------------------------------------ the re-entries

#[test]
fn a_compaction_offers_an_unacked_message_once_more() {
    let session = "00000000-0000-4000-8000-000000000051";
    let h = Home::new();
    write_transcript(&h, session);
    seed(&h, session, session, MSG, "steer", "still worth reading");

    let sent = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, None, "PostToolUse", ""),
    );
    assert!(!sent.stdout.is_empty());

    let compact = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, None, "SessionStart", r#","source":"compact""#),
    );
    assert!(compact.success, "{}", compact.stderr);
    assert!(
        hook_output(&compact.stdout)["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("still worth reading"),
        "the compaction threw away the context it landed in"
    );
    let ledger = channel_file(&h, session, &format!("ledger/{session}.jsonl"));
    assert_eq!(
        ledger
            .lines()
            .filter(|l| l.contains(r#""kind":"redelivered""#))
            .count(),
        1
    );

    // A startup is not a re-entry, and a second compaction with no new emit between them
    // does not offer it a third time.
    let startup = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, None, "SessionStart", r#","source":"startup""#),
    );
    assert_eq!(startup.stdout, "");
}

#[test]
fn a_resume_delivers_a_queue_message_that_had_no_lane() {
    let session = "00000000-0000-4000-8000-000000000052";
    let h = Home::new();
    write_transcript(&h, session);
    seed(
        &h,
        session,
        session,
        MSG,
        "queue",
        "held until you came back",
    );
    h.write(
        &format!("{ENC}/{session}/csift-channel/ledger/{session}.jsonl"),
        &format!(
            "{}\n",
            format_args!(
                r#"{{"id":"{MSG}","kind":"held","reason":"no-lane","ts_utc":"2026-06-07T05:10:00Z"}}"#
            )
        ),
    );

    let out = h.run_with_stdin(
        &["deliver", "--slot", "1"],
        &payload(&h, session, None, "SessionStart", r#","source":"resume""#),
    );
    assert!(out.success, "{}", out.stderr);
    assert!(
        hook_output(&out.stdout)["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("held until you came back")
    );
}

// ------------------------------------------------------------------------- the recipe

#[test]
fn the_recipe_prints_the_block_and_says_who_installs_it() {
    let h = Home::new();
    let out = h.run(&["deliver", "--recipe", "--slots", "2"]);
    assert!(out.success, "{}", out.stderr);
    let frag: serde_json::Value =
        serde_json::from_str(&out.stdout).expect("stdout is the fragment");
    let hooks = frag["hooks"].as_object().unwrap();
    assert_eq!(hooks.len(), 8, "one chain per delivery event");
    for event in [
        "SessionStart",
        "SubagentStart",
        "PreToolUse",
        "PostToolUse",
        "PostToolBatch",
        "UserPromptSubmit",
        "Stop",
        "SubagentStop",
    ] {
        let entries = hooks[event][0]["hooks"].as_array().unwrap();
        assert_eq!(entries.len(), 2, "{event} carries both slots");
        assert_eq!(entries[0]["command"], "csift deliver --slot 1");
        assert_eq!(entries[1]["command"], "csift deliver --slot 2");
    }
    assert_eq!(out.stderr.lines().count(), 2, "a two-line note");
    assert!(out.stderr.contains("never writes a settings file"));

    let ps = h.run(&[
        "deliver",
        "--recipe",
        "--slots",
        "1",
        "--shell",
        "powershell",
    ]);
    let frag: serde_json::Value = serde_json::from_str(&ps.stdout).unwrap();
    assert_eq!(frag["hooks"]["Stop"][0]["hooks"][0]["shell"], "powershell");
}

#[test]
fn deliver_without_a_slot_is_a_usage_error_and_a_bad_slot_names_the_range() {
    let h = Home::new();
    let missing = h.run(&["deliver"]);
    assert!(!missing.success, "--slot is required unless --recipe");
    assert!(missing.stderr.contains("--slot"));

    let bad = h.run(&["deliver", "--slot", "0"]);
    assert!(!bad.success);
    assert!(
        bad.stderr.contains("out of range"),
        "the error names the chain range: {}",
        bad.stderr
    );
}
