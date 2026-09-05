//! `csift send` end to end: the policy rows a real tree can exercise, the three files a
//! queued send writes, and the two things a refusal must not do.

use crate::harness::*;

const ENC: &str = "-Users-dev-Projects-relay";
const SESS: &str = "00000000-0000-4000-8000-000000000001";
const PEER: &str = "00000000-0000-4000-8000-000000000002";
const RUNNING: &str = "a1000000000000001";
const DONE: &str = "a1000000000000002";
const STOPPED: &str = "a1000000000000003";
const WORKFLOW: &str = "a1000000000000004";
const OLD: &str = "a1000000000000005";
/// A lane spawned BY `RUNNING`: its meta carries the harness's own `parentAgentId`, which is
/// the only place the flat on-disk layout keeps that link.
const NESTED: &str = "a1000000000000006";
const TEAMMATE: &str = "aRelay-0123456789abcdef";
const CWD: &str = "/Users/dev/Projects/relay";

/// The user-scope settings block: N delivery slots on SessionStart, plus an asyncRewake
/// hook on Stop (the one thing that can wake an idle top-level session).
fn settings(slots: usize, rewake: bool) -> String {
    let entries: Vec<String> = (1..=slots)
        .map(|k| format!(r#"{{"type":"command","command":"csift deliver --slot {k}"}}"#))
        .collect();
    let stop = if rewake {
        r#","Stop":[{"hooks":[{"type":"command","command":"csift deliver --slot 1","asyncRewake":true}]}]"#
    } else {
        ""
    };
    format!(
        r#"{{"hooks":{{"SessionStart":[{{"hooks":[{}]}}]{stop}}}}}"#,
        entries.join(",")
    )
}

fn user_line(uuid: &str, ts: &str, text: &str, version: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{SESS}","cwd":"{CWD}","version":"{version}","message":{{"role":"user","content":"{text}"}}}}"#
    )
}

fn agent_line(uuid: &str, ts: &str, text: &str, version: &str, end_turn: bool) -> String {
    let stop = if end_turn { "\"end_turn\"" } else { "null" };
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{ts}","sessionId":"{SESS}","cwd":"{CWD}","version":"{version}","message":{{"role":"assistant","stop_reason":{stop},"content":[{{"type":"text","text":"{text}"}}]}}}}"#
    )
}

/// One lane transcript: an opener and one assistant record, ended cleanly or not.
fn lane(version: &str, end_turn: bool) -> String {
    format!(
        "{}\n{}\n",
        user_line("l1", "2026-06-07T05:00:00.000Z", "go", version),
        agent_line(
            "l2",
            "2026-06-07T05:00:05.000Z",
            "working",
            version,
            end_turn
        )
    )
}

/// A home with the main session, the four subagent lanes, and `slots` delivery slots.
fn home(slots: usize, rewake: bool) -> Home {
    let h = Home::new();
    h.write_claude("settings.json", &settings(slots, rewake));
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!(
            "{}\n{}\n{}\n",
            user_line("m1", "2026-06-07T04:00:00.000Z", "hello", "2.1.258"),
            agent_line("m2", "2026-06-07T04:00:05.000Z", "hi", "2.1.258", true),
            stopped_agent_records()
        ),
    );
    for (id, done, version) in [
        (RUNNING, false, "2.1.258"),
        (DONE, true, "2.1.258"),
        (STOPPED, false, "2.1.258"),
        (OLD, false, "2.1.100"),
    ] {
        h.write(
            &format!("{ENC}/{SESS}/subagents/agent-{id}.jsonl"),
            &lane(version, done),
        );
        h.write(
            &format!("{ENC}/{SESS}/subagents/agent-{id}.meta.json"),
            r#"{"agentType":"general-purpose","description":"relay probe"}"#,
        );
    }
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{NESTED}.jsonl"),
        &lane("2.1.258", false),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{NESTED}.meta.json"),
        &format!(
            r#"{{"agentType":"general-purpose","description":"nested probe","parentAgentId":"{RUNNING}"}}"#
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{TEAMMATE}.jsonl"),
        &lane("2.1.258", false),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{TEAMMATE}.meta.json"),
        r#"{"agentType":"Relay","taskKind":"in_process_teammate","name":"Relay","teamName":"harbor"}"#,
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_1/agent-{WORKFLOW}.jsonl"),
        &lane("2.1.258", false),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_1/agent-{WORKFLOW}.meta.json"),
        r#"{"agentType":"workflow-subagent"}"#,
    );
    h
}

/// The two main-transcript records that make STOPPED a lane the harness itself killed: an
/// async-agent launch naming the lane, and the completion notification that closes it with a
/// terminal status, joined by the launching tool_use id.
fn stopped_agent_records() -> String {
    let launch = format!(
        r#"{{"type":"user","uuid":"m3","timestamp":"2026-06-07T04:01:00.000Z","sessionId":"{SESS}","cwd":"{CWD}","version":"2.1.258","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"tu_stop","content":"launched"}}]}},"toolUseResult":{{"isAsync":true,"status":"async_launched","agentId":"{STOPPED}","description":"relay probe","outputFile":"/dev/null"}}}}"#
    );
    let notice = format!(
        r#"{{"type":"user","uuid":"m4","timestamp":"2026-06-07T04:02:00.000Z","sessionId":"{SESS}","cwd":"{CWD}","version":"2.1.258","message":{{"role":"user","content":"<task-notification>\n<task-id>bstopped1</task-id>\n<tool-use-id>tu_stop</tool-use-id>\n<status>stopped</status>\n<summary>Background agent \"relay probe\" was stopped</summary>\n</task-notification>"}}}}"#
    );
    format!("{launch}\n{notice}")
}

/// Record that slots have actually run in a lane, so a send can reach OK.
fn arm(h: &Home, lane: &str, slots: &str) {
    h.write(
        &format!("{ENC}/{SESS}/csift-channel/armed/{lane}.json"),
        &format!(
            r#"{{"slots_seen":[{slots}],"last_event":"SessionStart","last_ts_utc":"2026-06-07T05:00:00Z","hook_session":"{SESS}","claude_code_version":"2.1.258"}}"#
        ),
    );
}

fn lane_env() -> Vec<(&'static str, &'static str)> {
    vec![("CLAUDE_CODE_SESSION_ID", SESS)]
}

fn read(h: &Home, rel: &str) -> String {
    std::fs::read_to_string(h.projects().join(rel)).unwrap_or_default()
}

/// A lane's queued inbox, empty when the send wrote none.
fn inbox(h: &Home, lane: &str) -> String {
    read(h, &format!("{ENC}/{SESS}/csift-channel/inbox/{lane}.jsonl"))
}

/// The `relation` recorded on the Nth message queued into a lane's inbox.
///
/// It is the value the envelope keys on: the chunks a receiver reads are rendered at DELIVERY,
/// so nothing renders them at send time, and the recorded relation is the whole of what decides
/// whether the peer caution appears (the envelope unit tests pin that step).
fn queued_relation(h: &Home, lane: &str, nth: usize) -> String {
    let queued = inbox(h, lane);
    let line = queued.lines().nth(nth).expect("a queued inbox line").trim();
    let id = serde_json::from_str::<serde_json::Value>(line).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let msg = read(h, &format!("{ENC}/{SESS}/csift-channel/messages/{id}.json"));
    serde_json::from_str::<serde_json::Value>(&msg).unwrap()["relation"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn a_lane_caller_to_a_running_subagent_delegates_the_official_arm_and_still_queues() {
    let h = home(2, false);
    arm(&h, RUNNING, "1,2");
    let out = h.run_with_env(&["send", &at(RUNNING), "stop after this file"], &lane_env());
    assert!(out.success, "{}", out.stderr);
    assert!(out.stdout.contains("verdict     OK"), "{}", out.stdout);
    assert!(out.stdout.contains("channel     official in-process queue"));
    assert!(
        out.stdout
            .contains(&format!("SendMessage(to: \"{RUNNING}\"")),
        "the receipt prints the exact official call: {}",
        out.stdout
    );
    assert!(out.stdout.contains("csift never performs an official send"));

    let inbox = inbox(&h, RUNNING);
    assert_eq!(inbox.lines().count(), 1, "the message is queued too");
    let outbox = read(&h, &format!("{ENC}/{SESS}/csift-channel/outbox.jsonl"));
    assert!(outbox.contains("\"to_lane\":\"a1000000000000001\""));
    assert!(outbox.contains("\"delegated\":true"));
    let id = serde_json::from_str::<serde_json::Value>(inbox.trim()).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let msg = read(
        &h,
        &format!("{ENC}/{SESS}/csift-channel/messages/{id}.json"),
    );
    assert!(msg.contains("stop after this file"));
    assert!(msg.contains("\"relation\":\"parent\""));
}

#[test]
fn a_lane_addressing_a_subagent_it_spawned_records_the_parent_relation_not_sibling() {
    let h = home(2, false);
    arm(&h, NESTED, "1,2");
    let (to, spawner, peer) = (at(NESTED), at(RUNNING), at(TEAMMATE));
    let out = h.run_with_env(
        &["send", &to, "narrow the scope", "--from", &spawner],
        &lane_env(),
    );
    assert!(out.success, "{}", out.stderr);
    // The channel is untouched by this direction: the in-process queue reaches a child by its
    // own id whoever sends. The relation is the whole point.
    assert!(
        out.stdout.contains("channel     official in-process queue"),
        "{}",
        out.stdout
    );
    assert_eq!(
        queued_relation(&h, NESTED, 0),
        "parent",
        "the target's meta names this lane as the agent that spawned it, so the envelope must \
         not introduce it as a peer"
    );

    let sibling = h.run_with_env(
        &["send", &to, "narrow the scope", "--from", &peer],
        &lane_env(),
    );
    assert!(sibling.success, "{}", sibling.stderr);
    assert_eq!(
        queued_relation(&h, NESTED, 1),
        "sibling",
        "a lane that spawned nothing here is still a peer, and still gets the caution"
    );
}

#[test]
fn a_subagent_addressing_the_lane_that_spawned_it_gets_the_csift_channel_and_no_official_call() {
    let h = home(2, false);
    arm(&h, RUNNING, "1,2");
    let out = h.run_with_env(
        &["send", &at(RUNNING), "done here", "--from", &at(NESTED)],
        &lane_env(),
    );
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout.contains("channel     csift steer"),
        "{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("SendMessage"),
        "the official `to` grammar has no parent arm, so no call is printed: {}",
        out.stdout
    );
    assert!(out.stdout.contains("no parent arm"), "{}", out.stdout);

    let inbox = inbox(&h, RUNNING);
    assert_eq!(inbox.lines().count(), 1, "the message is still queued");
    assert_eq!(
        queued_relation(&h, RUNNING, 0),
        "child",
        "the sender is the receiver's child, not its sibling"
    );

    // The same send from a lane the receiver did NOT spawn keeps the official delegation.
    let sibling = h.run_with_env(
        &["send", &at(RUNNING), "done here", "--from", &at(TEAMMATE)],
        &lane_env(),
    );
    assert!(sibling.success, "{}", sibling.stderr);
    assert!(
        sibling
            .stdout
            .contains(&format!("SendMessage(to: \"{RUNNING}\"")),
        "a genuine sibling still delegates: {}",
        sibling.stdout
    );
}

#[test]
fn a_workflow_lane_gets_the_csift_channel_and_no_official_call() {
    let h = home(2, false);
    arm(&h, WORKFLOW, "1,2");
    let out = h.run_with_env(&["send", &at(WORKFLOW), "hold"], &lane_env());
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout.contains("channel     csift steer"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("kind      workflow lane"));
    assert!(
        !out.stdout.contains("official    "),
        "the official send fails closed on a workflow lane: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("the official \nsend fails closed")
            || out.stdout.contains("official send fails closed")
    );
}

#[test]
fn a_completed_lane_is_refused_and_writes_nothing_until_resume_is_passed() {
    let h = home(2, false);
    arm(&h, DONE, "1,2");
    let out = h.run_with_env(&["send", &at(DONE), "one more thing"], &lane_env());
    assert!(
        out.success,
        "a refusal is a definitive answer, not an error"
    );
    assert!(out.stdout.contains("verdict     REFUSED"), "{}", out.stdout);
    assert!(out.stdout.contains("queued      no"));
    assert!(out.stderr.contains("csift: REFUSED"), "{}", out.stderr);
    assert!(inbox(&h, DONE).is_empty(), "a refusal queues nothing");
    assert!(read(&h, &format!("{ENC}/{SESS}/csift-channel/outbox.jsonl")).is_empty());

    let resumed = h.run_with_env(
        &["send", &at(DONE), "one more thing", "--resume"],
        &lane_env(),
    );
    assert!(
        resumed.stdout.contains("channel     official resume"),
        "{}",
        resumed.stdout
    );
    assert!(resumed.stdout.contains("respawns"));
    assert_eq!(
        inbox(&h, DONE).lines().count(),
        1,
        "--resume delegates AND queues"
    );
}

#[test]
fn a_lane_the_harness_stopped_is_refused_whatever_the_flags_say() {
    let h = home(2, false);
    arm(&h, STOPPED, "1,2");
    let out = h.run_with_env(&["send", &at(STOPPED), "carry on", "--resume"], &lane_env());
    assert!(out.success);
    assert!(out.stdout.contains("verdict     REFUSED"), "{}", out.stdout);
    assert!(out.stdout.contains("state     stopped-by-user"));
    assert!(out.stdout.contains("stopped by the user"));
    assert!(inbox(&h, STOPPED).is_empty());
}

#[test]
fn a_teammate_addressed_by_its_routing_form_queues_under_its_transcript_id() {
    let h = home(2, false);
    arm(&h, TEAMMATE, "1,2");
    let out = h.run_with_env(
        &["send", "@Relay@harbor", "status?", "--format", "json"],
        &lane_env(),
    );
    assert!(out.success, "{}", out.stderr);
    let row = &json_rows(&out.stdout, "send")[0];
    assert_eq!(row["receiver"]["lane"], TEAMMATE);
    assert_eq!(row["receiver"]["routing_id"], "Relay@harbor");
    assert_eq!(row["receiver"]["kind"], "teammate");
    assert!(
        inbox(&h, TEAMMATE).contains("\"mode\":\"steer\""),
        "the queue is keyed by the transcript id, never the routing form"
    );
}

#[test]
fn a_teammate_with_the_teams_gate_enabled_delegates_the_mailbox_by_routing_id() {
    let h = home(2, false);
    h.write_claude(
        "settings.json",
        &format!(
            r#"{{"env":{{"CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS":"1"}},"hooks":{}}}"#,
            &settings(2, false)[9..settings(2, false).len() - 1]
        ),
    );
    arm(&h, TEAMMATE, "1,2");
    let out = h.run_with_env(&["send", "@Relay@harbor", "status?"], &lane_env());
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout.contains("channel     official mailbox"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("SendMessage(to: \"Relay@harbor\""));
    assert!(out
        .stdout
        .contains("teams: enabled via settings env (user)"));
}

#[test]
fn a_receiver_below_the_official_floor_is_csift_only() {
    let h = home(2, false);
    arm(&h, OLD, "1,2");
    let out = h.run_with_env(&["send", &at(OLD), "hello"], &lane_env());
    assert!(out.success, "{}", out.stderr);
    assert!(out.stdout.contains("version   2.1.100"), "{}", out.stdout);
    assert!(out.stdout.contains("channel     csift steer"));
    assert!(out.stdout.contains("below the official floor"));
    assert!(!out.stdout.contains("SendMessage"));
}

#[test]
fn an_external_caller_with_delivery_hooks_installed_gets_ok_and_no_tool_to_call() {
    let h = home(2, true);
    h.write(
        &format!("{ENC}/{PEER}.jsonl"),
        &format!(
            "{}\n{}\n",
            user_line("p1", "2026-06-07T04:00:00.000Z", "hello", "2.1.258"),
            agent_line("p2", "2026-06-07T04:00:05.000Z", "hi", "2.1.258", true)
        ),
    );
    h.write(
        &format!("{ENC}/{PEER}/csift-channel/armed/{PEER}.json"),
        &format!(
            r#"{{"slots_seen":[1,2],"last_event":"SessionStart","last_ts_utc":"2026-06-07T05:00:00Z","hook_session":"{PEER}","claude_code_version":"2.1.258"}}"#
        ),
    );
    h.write_session_registry(
        std::process::id(),
        &format!(
            r#"{{"pid":{},"sessionId":"{PEER}","status":"idle","entrypoint":"cli"}}"#,
            std::process::id()
        ),
    );
    let out = h.run(&["send", &at(PEER), "ping", "--from", "ci-runner"]);
    assert!(out.success, "{}", out.stderr);
    assert!(out.stdout.contains("verdict     OK"), "{}", out.stdout);
    assert!(out.stdout.contains("slots       SessionStart 1,2  Stop 1"));
    assert!(out.stdout.contains("armed       1,2"));
    assert!(
        !out.stdout.contains("SendMessage"),
        "an external caller is never told to call a tool: {}",
        out.stdout
    );
    let msg_dir = h
        .projects()
        .join(format!("{ENC}/{PEER}/csift-channel/messages"));
    let one = std::fs::read_dir(&msg_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let body = std::fs::read_to_string(one.path()).unwrap();
    assert!(body.contains("\"kind\":\"external\""));
    assert!(body.contains("\"label\":\"ci-runner\""));
    assert!(
        read(&h, &format!("{ENC}/{PEER}/csift-channel/outbox.jsonl"))
            .contains("\"to_lane\":\"00000000-0000-4000-8000-000000000002\""),
        "an external sender's outbox rides under the receiver's session"
    );
}

#[test]
fn an_external_caller_without_delivery_hooks_is_unpredictable_and_names_the_missing_hook() {
    let h = home(0, false);
    let out = h.run(&["send", &at(RUNNING), "ping"]);
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout.contains("verdict     UNPREDICTABLE"),
        "{}",
        out.stdout
    );
    assert!(out
        .stdout
        .contains("slots       none configured on any delivery event"));
    assert!(out
        .stdout
        .contains("no `csift deliver --slot k` hook is configured"));
    assert!(out.stdout.contains("armed       none"));
}

#[test]
fn a_message_needing_more_chunks_than_slots_is_full_and_says_how_many_are_needed() {
    let h = home(1, false);
    arm(&h, RUNNING, "1");
    let long = "x".repeat(20_000);
    let out = h.run_with_env(
        &["send", &at(RUNNING), &long, "--format", "json"],
        &lane_env(),
    );
    assert!(out.success, "{}", out.stderr);
    let row = &json_rows(&out.stdout, "send")[0];
    assert_eq!(row["verdict"], "FULL");
    let summary = json_summary(&out.stdout);
    assert!(summary["chunks"].as_u64().unwrap() > 1);
    assert_eq!(summary["queued"], true);

    let text = h.run_with_env(&["send", &at(RUNNING), &long], &lane_env());
    assert!(text.stdout.contains("full        "), "{}", text.stdout);
    assert!(text.stdout.contains("more slot(s)"));
}

#[test]
fn the_lane_is_assumed_when_no_from_names_it_and_the_note_says_so() {
    let h = home(2, false);
    let out = h.run_with_env(&["send", &at(RUNNING), "ping"], &lane_env());
    assert!(out.stderr.contains("lane unknown"), "{}", out.stderr);
    assert!(out.stderr.contains("--from @<your lane id>"));

    let exact = h.run_with_env(
        &["send", &at(RUNNING), "ping", "--from", "@main"],
        &lane_env(),
    );
    assert!(
        !exact.stderr.contains("lane unknown"),
        "an exact --from claims the lane: {}",
        exact.stderr
    );
}

#[test]
fn official_only_prints_the_call_and_writes_nothing() {
    let h = home(2, false);
    arm(&h, RUNNING, "1,2");
    let out = h.run_with_env(
        &["send", &at(RUNNING), "ping", "--official-only"],
        &lane_env(),
    );
    assert!(out.success, "{}", out.stderr);
    assert!(out.stdout.contains("queued      no"), "{}", out.stdout);
    assert!(out.stdout.contains("SendMessage(to:"));
    assert!(inbox(&h, RUNNING).is_empty());
}

#[test]
fn the_body_comes_from_a_file_or_stdin_and_a_target_must_name_one_lane() {
    let h = home(2, false);
    let file = h.root.join("note.md");
    std::fs::write(&file, "from a file").unwrap();
    let out = h.run_with_env(
        &["send", &at(RUNNING), "-f", file.to_str().unwrap()],
        &lane_env(),
    );
    assert!(out.success, "{}", out.stderr);
    let piped = h.run_with_stdin(&["send", &at(RUNNING)], "from stdin");
    assert!(piped.success, "{}", piped.stderr);

    let missing = h.run_with_env(
        &["send", "@00000000-0000-4000-8000-00000000ffff", "x"],
        &lane_env(),
    );
    assert!(!missing.success, "an unresolvable target is a hard error");
}

#[test]
fn the_ttl_flag_sets_the_inbox_expiry_and_a_bad_duration_is_refused() {
    let h = home(2, false);
    let out = h.run_with_env(
        &[
            "send",
            &at(RUNNING),
            "ping",
            "--ttl",
            "30s",
            "--format",
            "json",
        ],
        &lane_env(),
    );
    assert!(out.success, "{}", out.stderr);
    assert_eq!(json_summary(&out.stdout)["ttl_secs"], 30);
    let inbox = inbox(&h, RUNNING);
    assert!(inbox.contains("\"expires_utc\":\""));

    let bad = h.run_with_env(
        &["send", &at(RUNNING), "ping", "--ttl", "soon"],
        &lane_env(),
    );
    assert!(!bad.success);
    assert!(bad.stderr.contains("--ttl"), "{}", bad.stderr);
}
