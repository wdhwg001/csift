//! `csift whoami` reach end to end: the three sections, the two terminal modes, and the answer
//! a process outside Claude Code gets.

use crate::harness::*;

const ENC: &str = "-Users-dev-Projects-relay";
const SESS: &str = "00000000-0000-4000-8000-000000000001";
const TEAMMATE: &str = "aRelay-0123456789abcdef";
const WORKFLOW: &str = "a1000000000000004";
const CWD: &str = "/Users/dev/Projects/relay";

/// A lane whose tail is an unreturned tool call: in flight at any age, so the fixture states do
/// not depend on when the suite runs.
fn in_flight_lane() -> String {
    format!(
        "{}\n{}\n",
        format_args!(
            r#"{{"type":"user","uuid":"l1","timestamp":"2026-06-07T05:00:00.000Z","cwd":"{CWD}","version":"2.1.258","message":{{"role":"user","content":"go"}}}}"#
        ),
        r#"{"type":"assistant","uuid":"l2","timestamp":"2026-06-07T05:00:05.000Z","version":"2.1.258","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"sleep 1"}}]}}"#
    )
}

/// The user-scope settings block: two delivery slots on PostToolUse and two on Stop.
const SETTINGS: &str = concat!(
    r#"{"hooks":{"PostToolUse":[{"hooks":[{"type":"command","command":"csift deliver --slot 1"},"#,
    r#"{"type":"command","command":"csift deliver --slot 2"}]}],"#,
    r#""Stop":[{"hooks":[{"type":"command","command":"csift deliver --slot 1"},"#,
    r#"{"type":"command","command":"csift deliver --slot 2"}]}]}}"#,
);

/// A session with two child lanes - one teammate carrying its meta, one workflow lane - plus a
/// registry row for the session and the delivery slots in the settings cascade.
fn home(socket: bool) -> Home {
    let h = Home::new();
    h.write_claude("settings.json", SETTINGS);
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!(
            "{}\n{}\n",
            format_args!(
                r#"{{"type":"user","uuid":"m1","timestamp":"2026-06-07T04:00:00.000Z","sessionId":"{SESS}","cwd":"{CWD}","version":"2.1.258","message":{{"role":"user","content":"hello"}}}}"#
            ),
            r#"{"type":"assistant","uuid":"m2","timestamp":"2026-06-07T04:00:05.000Z","version":"2.1.258","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"hi"}]}}"#
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{TEAMMATE}.jsonl"),
        &in_flight_lane(),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{TEAMMATE}.meta.json"),
        r#"{"agentType":"Relay","taskKind":"in_process_teammate","name":"Relay","teamName":"harbor"}"#,
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_1/agent-{WORKFLOW}.jsonl"),
        &in_flight_lane(),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_1/agent-{WORKFLOW}.meta.json"),
        r#"{"agentType":"workflow-subagent"}"#,
    );
    // The registry row is stamped on THIS test process, the one pid a probe is sure to find.
    let socket_field = if socket {
        r#","messagingSocketPath":"/tmp/relay.sock""#
    } else {
        ""
    };
    h.write_session_registry(
        std::process::id(),
        &format!(
            r#"{{"pid":{},"sessionId":"{SESS}","status":"busy","entrypoint":"cli"{socket_field}}}"#,
            std::process::id()
        ),
    );
    h
}

#[test]
fn a_teammate_target_prints_both_forms_its_parent_and_the_topology() {
    let h = home(false);
    let out = h.run(&["whoami", &at(TEAMMATE)]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Both id forms: the routing id is what the official send takes and it can collide; the
    // transcript id names the file and never does.
    assert!(
        out.stdout.contains(&format!("self     {TEAMMATE}")),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("routing  Relay@harbor"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("kind     teammate"), "{}", out.stdout);
    assert!(out.stdout.contains("depth    0"), "{}", out.stdout);
    // The parent of a lane with no recorded spawning agent is the session's own conversation.
    assert!(
        out.stdout.contains(&format!("parent   {SESS}")) && out.stdout.contains("alive    yes"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("reply    "), "{}", out.stdout);
    assert!(
        out.stdout.contains("other live lane(s) (--peers to list)"),
        "{}",
        out.stdout
    );
}

#[test]
fn the_routing_form_resolves_the_same_lane_as_the_transcript_form() {
    let h = home(false);
    let out = h.run(&["whoami", "@Relay@harbor"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("self     {TEAMMATE}")),
        "the routing form resolves to the transcript form: {}",
        out.stdout
    );
}

#[test]
fn a_top_level_target_shows_its_live_child_lanes_and_no_parent() {
    let h = home(false);
    // The session's own lane is reached through the environment form, which resolves it.
    let out = h.run_with_env(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("parent   none (a top-level session has no lane above it)"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("2 live child lane(s) in this session"),
        "both child lanes are in flight: {}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains(&format!("{TEAMMATE}  teammate  in-flight"))
            && out
                .stdout
                .contains(&format!("{WORKFLOW}  workflow lane  in-flight")),
        "{}",
        out.stdout
    );
    // The environment named the session, not necessarily the caller, and the section says so.
    assert!(
        out.stdout.contains("kind     top-level session (assumed"),
        "{}",
        out.stdout
    );
}

#[test]
fn to_a_workflow_lane_predicts_the_csift_channel_with_the_inference_sentence() {
    let h = home(false);
    let out = h.run(&["whoami", "--to", &at(WORKFLOW)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("kind      workflow lane"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("channel     csift steer"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("the official send fails closed on it"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(
            "inference   prediction is an inference from the transcript tail and a pid probe"
        ),
        "an agent target names what the prediction is made of: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("fallback: csift steer"),
        "{}",
        out.stdout
    );
    // The configured cascade and the runtime arming are both reported, and they differ.
    assert!(
        out.stdout.contains("slots       PostToolUse 1,2  Stop 1,2"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("armed       none (no delivery hook has run in this lane)"),
        "{}",
        out.stdout
    );
    // A prediction writes nothing.
    assert!(
        !h.projects()
            .join(format!("{ENC}/{SESS}/csift-channel"))
            .exists(),
        "a reach question queues nothing"
    );
}

#[test]
fn to_a_session_with_a_socket_predicts_the_official_uds() {
    let h = home(true);
    let out = h.run_with_env(
        &["whoami", "--to", &at(SESS)],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("channel     official uds"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("harbor: registry messagingSocketPath present -> on and bound"),
        "{}",
        out.stdout
    );
    // A top-level lane is answered from the registry row the harness itself wrote, so the
    // inference sentence does not apply to it.
    assert!(!out.stdout.contains("inference   "), "{}", out.stdout);
}

#[test]
fn the_teams_gate_reports_the_evidence_when_no_scope_enables_it() {
    let h = home(false);
    let out = h.run(&["whoami", "--to", &at(TEAMMATE)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(
            "teams: no settings-level enable; shell env and CLI flags are not observable -> \
             unknown; use evidence: teams directories 0"
        ),
        "{}",
        out.stdout
    );
}

#[test]
fn peers_lists_ids_kinds_and_states_and_nothing_else() {
    let h = home(false);
    let out = h.run(&["whoami", "--peers"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains(&format!("{SESS}  top-level session  busy")),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains(&format!("{TEAMMATE}  teammate  in-flight")),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("3 live lane(s)"), "{}", out.stdout);
    // The anti-collusion rule: no description, no agent type, no name read as a role.
    for leak in [
        "Relay@harbor",
        "workflow-subagent",
        "general-purpose",
        "name",
    ] {
        assert!(
            !out.stdout.contains(leak),
            "the census publishes ids, kinds and states only, found `{leak}`: {}",
            out.stdout
        );
    }
}

#[test]
fn a_caller_outside_claude_code_gets_the_not_a_lane_answer() {
    let h = home(false);
    // The harness clears the session variable, which IS the external-caller shape.
    let out = h.run(&["whoami"]);
    assert!(
        out.stdout.contains("not a Claude Code lane"),
        "stdout carries the channel answer: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("csift send @<lane>"),
        "the one channel out: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("csift deliver --slot k")
            && out.stdout.contains("csift deliver --recipe"),
        "what a receiver needs installed: {}",
        out.stdout
    );
    // The identity question still has no answer, and guessing one is the documented trap.
    assert!(!out.success, "the identity half still fails loudly");
    assert!(out.stderr.contains("@<uuid>") && out.stderr.contains("mtime"));
}

#[test]
fn a_session_uuid_is_still_not_a_whoami_target() {
    let h = home(false);
    let out = h.run(&["whoami", &at(SESS), "--format", "json"]);
    assert!(!out.success, "a session target is not a whoami question");
    assert!(
        out.stderr.contains("whoami accepts no target except"),
        "{}",
        out.stderr
    );
}

#[test]
fn the_json_stream_carries_the_new_row_kinds() {
    let h = home(false);

    let target = h.run(&["whoami", &at(TEAMMATE), "--format", "json"]);
    assert!(target.success, "stderr: {}", target.stderr);
    let me = json_rows(&target.stdout, "self").remove(0);
    assert_eq!(me["lane"], TEAMMATE);
    assert_eq!(me["routing_id"], "Relay@harbor");
    assert_eq!(me["lane_kind"], "teammate");
    assert_eq!(me["is_subagent"], true);
    assert_eq!(me["depth"], 0);
    let parent = json_rows(&target.stdout, "parent").remove(0);
    assert_eq!(parent["lane"], SESS);
    assert_eq!(parent["alive"], true);
    assert!(parent["reply_channel"].is_string());
    json_summary(&target.stdout);

    let session = h.run_with_env(
        &["whoami", "--format", "json"],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    let lanes = json_rows(&session.stdout, "lane");
    assert_eq!(lanes.len(), 2, "{}", session.stdout);
    assert!(lanes.iter().all(|l| l["state"] == "in-flight"));
    // The timestamp pair law: every machine instant carries both renderings.
    assert!(lanes
        .iter()
        .all(|l| l["last_activity_utc"].is_string() && l["last_activity_local"].is_string()));
    // The identity row is unchanged, and its lane fields stay null.
    let identity = json_rows(&session.stdout, "identity").remove(0);
    assert_eq!(identity["session_id"], SESS);
    assert!(identity["is_subagent"].is_null());
    let summary = json_summary(&session.stdout);
    assert_eq!(summary["live_child_lanes"], 2);

    let reach = h.run(&["whoami", "--to", &at(WORKFLOW), "--format", "json"]);
    let row = json_rows(&reach.stdout, "reach").remove(0);
    assert_eq!(row["lane"], WORKFLOW);
    assert_eq!(row["channel"], "csift steer");
    assert_eq!(
        row["configured_slots"]["PostToolUse"],
        serde_json::json!([1, 2])
    );
    assert!(row["inference"].as_str().unwrap().contains("fallback:"));
    assert_eq!(json_summary(&reach.stdout)["queued"], false);

    let peers = h.run(&["whoami", "--peers", "--format", "json"]);
    let rows = json_rows(&peers.stdout, "peer");
    assert_eq!(rows.len(), 3, "{}", peers.stdout);
    assert_eq!(json_summary(&peers.stdout)["peers"], 3);
}

#[test]
fn the_self_section_reads_frozen_from_an_unreturned_tool_call() {
    // A lane whose tail is a tool_use with no result is BLOCKED there, not done: its next hook
    // point comes only when that call returns, which is exactly what a sender needs to know.
    let h = home(false);
    let out = h.run(&["whoami", &at(TEAMMATE)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("state    frozen"),
        "an unreturned tool call is a frozen lane, never a finished one:\n{}",
        out.stdout
    );
    let json = h.run(&["whoami", &at(TEAMMATE), "--format", "json"]);
    assert_eq!(json_rows(&json.stdout, "self").remove(0)["state"], "frozen");
}

#[test]
fn peers_with_no_live_lane_says_so_instead_of_printing_an_empty_list() {
    // No registry row means no session csift can prove is alive, and a child of a dead session
    // cannot be live either. The census reports the count rather than trailing off.
    let h = home(false);
    // The fixture's registry row is what makes its lanes live; a home without one has none.
    let empty = Home::new();
    empty.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!(
            "{}\n",
            format_args!(
                r#"{{"type":"user","uuid":"m1","timestamp":"2026-06-07T04:00:00.000Z","cwd":"{CWD}","version":"2.1.258","message":{{"role":"user","content":"hello"}}}}"#
            )
        ),
    );
    let out = empty.run(&["whoami", "--peers"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("csift channel · peers") && out.stdout.contains("0 live lane(s)"),
        "an empty census is an honest empty, with its count:\n{}",
        out.stdout
    );
    let json = empty.run(&["whoami", "--peers", "--format", "json"]);
    assert!(
        json_rows(&json.stdout, "peer").is_empty(),
        "{}",
        json.stdout
    );
    assert_eq!(json_summary(&json.stdout)["peers"], 0);
    // The populated fixture is the control: the same command does find lanes there.
    assert!(h
        .run(&["whoami", "--peers"])
        .stdout
        .contains("3 live lane(s)"));
}

#[test]
fn a_routing_form_that_names_no_teammate_fails_loud_through_whoami() {
    let h = home(false);
    let out = h.run(&["whoami", "@Nobody@harbor"]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("no teammate `Nobody@harbor`")
            && out.stderr.contains("ROUTING form")
            && out.stderr.contains("--shape teammate"),
        "the miss names the grammar and how to list the real ids:\n{}",
        out.stderr
    );
}

#[test]
fn a_colliding_routing_form_lists_both_transcript_ids_through_whoami() {
    // Two teammates can share one routing id, and the transcript id never collides - so
    // `whoami` refuses to pick and hands back both ids the caller can address instead.
    let h = home(false);
    let twin = "aRelay-fedcba9876543210";
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{twin}.jsonl"),
        &in_flight_lane(),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{twin}.meta.json"),
        r#"{"agentType":"Relay","taskKind":"in_process_teammate","name":"Relay","teamName":"harbor"}"#,
    );
    let out = h.run(&["whoami", "@Relay@harbor"]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("AMBIGUOUS")
            && out.stderr.contains(TEAMMATE)
            && out.stderr.contains(twin),
        "every matching transcript id is listed:\n{}",
        out.stderr
    );
    // Each transcript id still answers on its own: the collision belongs to the routing form.
    let one = h.run(&["whoami", &at(twin)]);
    assert!(one.success, "stderr: {}", one.stderr);
    assert!(
        one.stdout.contains(&format!("self     {twin}")),
        "{}",
        one.stdout
    );
}

#[test]
fn the_two_terminal_modes_refuse_to_be_combined() {
    let h = home(false);
    let both = h.run(&["whoami", "--peers", "--to", &at(TEAMMATE)]);
    assert!(!both.success, "one question at a time");
    let with_target = h.run(&["whoami", &at(TEAMMATE), "--peers"]);
    assert!(
        !with_target.success,
        "a target and a census are two questions"
    );
}
