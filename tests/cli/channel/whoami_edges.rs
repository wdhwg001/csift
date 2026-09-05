//! `whoami` edges the three happy-path files do not reach: the machine form of the
//! outside-Claude-Code answer, a lane whose own tail decides its state, a lane nested under
//! another lane, and the two ways a target can fail to name exactly one lane.

use crate::harness::*;

const ENC: &str = "-Users-dev-Projects-nest";
const EMPTY_ENC: &str = "-Users-dev-Projects-void";
const SESS: &str = "00000000-0000-4000-8000-000000000081";
const SECOND: &str = "00000000-0000-4000-8000-000000000082";
const PARENT_LANE: &str = "aLead-0123456789abcdef";
const CHILD_LANE: &str = "aScout-0123456789abcdef";
const CWD: &str = "/Users/dev/Projects/nest";

/// A lane whose tail is an unreturned tool call: in flight whenever the suite runs.
fn in_flight_lane() -> String {
    format!(
        "{}\n{}\n",
        format_args!(
            r#"{{"type":"user","uuid":"l1","timestamp":"2026-06-07T05:00:00.000Z","cwd":"{CWD}","version":"2.1.258","message":{{"role":"user","content":"go"}}}}"#
        ),
        r#"{"type":"assistant","uuid":"l2","timestamp":"2026-06-07T05:00:05.000Z","version":"2.1.258","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"sleep 1"}}]}}"#
    )
}

/// A lane whose tail is a clean `end_turn`: finished, with nothing left to run.
fn finished_lane() -> String {
    format!(
        "{}\n{}\n",
        format_args!(
            r#"{{"type":"user","uuid":"f1","timestamp":"2026-06-07T05:00:00.000Z","cwd":"{CWD}","version":"2.1.258","message":{{"role":"user","content":"go"}}}}"#
        ),
        r#"{"type":"assistant","uuid":"f2","timestamp":"2026-06-07T05:00:05.000Z","version":"2.1.258","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"done"}]}}"#
    )
}

/// A lane still mid-turn: no unreturned call, and no `end_turn` either.
fn open_turn_lane() -> String {
    format!(
        "{}\n{}\n",
        format_args!(
            r#"{{"type":"user","uuid":"o1","timestamp":"2026-06-07T05:00:00.000Z","cwd":"{CWD}","version":"2.1.258","message":{{"role":"user","content":"go"}}}}"#
        ),
        r#"{"type":"assistant","uuid":"o2","timestamp":"2026-06-07T05:00:05.000Z","version":"2.1.258","message":{"role":"assistant","stop_reason":null,"content":[{"type":"text","text":"still working"}]}}"#
    )
}

fn session_transcript(sess: &str) -> String {
    format!(
        "{}\n",
        format_args!(
            r#"{{"type":"user","uuid":"m1","timestamp":"2026-06-07T04:00:00.000Z","sessionId":"{sess}","cwd":"{CWD}","version":"2.1.258","message":{{"role":"user","content":"hello"}}}}"#
        )
    )
}

/// One session owning two lanes: `PARENT_LANE` has finished, and `CHILD_LANE`'s meta names it
/// as the agent that spawned it. The nesting lives only in the reconstruction - the two files
/// sit flat beside each other on disk.
fn nested_home() -> Home {
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &session_transcript(SESS));
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{PARENT_LANE}.jsonl"),
        &finished_lane(),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{PARENT_LANE}.meta.json"),
        r#"{"agentType":"Lead","taskKind":"in_process_teammate","name":"Lead","teamName":"nest"}"#,
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{CHILD_LANE}.jsonl"),
        &in_flight_lane(),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{CHILD_LANE}.meta.json"),
        &format!(r#"{{"agentType":"general-purpose","parentAgentId":"{PARENT_LANE}"}}"#),
    );
    h
}

#[test]
fn the_outside_claude_code_answer_has_a_machine_form_too() {
    // The text answer is pinned elsewhere; the JSON form is the one a hook or a script reads,
    // and it has to carry the same three facts: there is no lane, there is one way out, and
    // the receiver needs hook lines the sender cannot install.
    let h = Home::new();
    let out = h.run(&["whoami", "--format", "json"]);
    assert!(!out.success, "the identity half still fails loudly");
    let me = json_rows(&out.stdout, "self").remove(0);
    assert!(
        me["lane"].is_null(),
        "an external caller holds no lane: {me}"
    );
    assert_eq!(me["lane_kind"], "external");
    assert_eq!(me["lane_exact"], true);
    assert_eq!(me["resolved_via"], "environment");
    assert_eq!(me["note"], "not a Claude Code lane");
    assert!(
        me["channel_out"]
            .as_str()
            .is_some_and(|s| s.contains("csift send @<lane>")),
        "{me}"
    );
    assert!(
        me["receiver_needs"]
            .as_str()
            .is_some_and(|s| s.contains("csift deliver --slot k")),
        "{me}"
    );
    assert_eq!(json_summary(&out.stdout)["identities"], 0);
}

#[test]
fn a_target_that_is_no_id_form_at_all_is_refused_by_name() {
    // A bare word is neither of the two lane forms, so it never reaches the resolver: the
    // error lists what `whoami` does accept instead of failing as a missing project.
    let h = nested_home();
    let out = h.run(&["whoami", "relay"]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("whoami accepts no target except")
            && out.stderr.contains("@trap:<marker>")
            && out.stderr.contains("Got `relay`"),
        "{}",
        out.stderr
    );
}

#[test]
fn a_lane_state_is_read_from_its_own_tail_not_from_the_session() {
    // Neither lane's session has a registry row, so a state that came from the session would
    // be `unknown` for both. Each is decided by its own tail instead.
    let h = nested_home();
    let done = h.run(&["whoami", &at(PARENT_LANE)]);
    assert!(done.success, "stderr: {}", done.stderr);
    assert!(
        done.stdout.contains("state    completed"),
        "a clean end_turn with no live child is a finished lane:\n{}",
        done.stdout
    );

    // Same session, same absence of a registry row: an open turn is still running.
    let open = Home::new();
    open.write(&format!("{ENC}/{SESS}.jsonl"), &session_transcript(SESS));
    open.write(
        &format!("{ENC}/{SESS}/subagents/agent-{PARENT_LANE}.jsonl"),
        &open_turn_lane(),
    );
    let out = open.run(&["whoami", &at(PARENT_LANE)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("state    running"),
        "a tail that never ended the turn is not finished:\n{}",
        out.stdout
    );
}

#[test]
fn an_empty_lane_transcript_is_unknown_rather_than_finished() {
    // Zero records is no evidence either way, and reading it as `completed` would tell a
    // sender the lane is unreachable when nothing has been observed at all.
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &session_transcript(SESS));
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{PARENT_LANE}.jsonl"),
        "",
    );
    let out = h.run(&["whoami", &at(PARENT_LANE)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("state    unknown"), "{}", out.stdout);
}

#[test]
fn a_nested_lane_names_the_lane_above_it_and_reports_it_dead() {
    // The spawning agent is the lane above a nested one - not the session's own conversation,
    // which is what a flat directory listing would suggest. And a parent that has finished is
    // reported as not alive, because a reply to it has nowhere to land.
    let h = nested_home();
    let out = h.run(&["whoami", &at(CHILD_LANE)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("parent   {PARENT_LANE}")),
        "the spawning agent is the parent lane:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("alive    no"),
        "a finished parent lane is not alive:\n{}",
        out.stdout
    );
    let json = h.run(&["whoami", &at(CHILD_LANE), "--format", "json"]);
    let parent = json_rows(&json.stdout, "parent").remove(0);
    assert_eq!(parent["lane"], PARENT_LANE);
    assert_eq!(parent["alive"], false);
}

#[test]
fn the_topology_section_of_a_lane_is_its_own_subtree_not_the_whole_session() {
    // `CHILD_LANE` is in flight and belongs to `PARENT_LANE`'s subtree, so it shows there.
    // Asked about the child, the same session yields no child lanes at all.
    let h = nested_home();
    let parent = h.run(&["whoami", &at(PARENT_LANE)]);
    assert!(parent.success, "stderr: {}", parent.stderr);
    assert!(
        parent
            .stdout
            .contains("1 live child lane(s) in this lane's subtree")
            && parent
                .stdout
                .contains(&format!("{CHILD_LANE}  unnamed subagent  in-flight")),
        "{}",
        parent.stdout
    );
    let child = h.run(&["whoami", &at(CHILD_LANE)]);
    assert!(
        child
            .stdout
            .contains("no live child lanes in this lane's subtree"),
        "the child owns no lane of its own:\n{}",
        child.stdout
    );
}

#[test]
fn a_reach_target_that_names_several_lanes_lists_them_instead_of_picking_one() {
    // A project dir names every session under it. Answering for one of several would answer
    // about the wrong lane, so the error hands back the ids that address them one at a time.
    let h = nested_home();
    h.write(
        &format!("{ENC}/{SECOND}.jsonl"),
        &session_transcript(SECOND),
    );
    let out = h.run(&["whoami", "--to", &at(ENC)]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("resolved to 2 transcripts")
            && out.stderr.contains("addresses exactly ONE lane")
            && out.stderr.contains(SESS)
            && out.stderr.contains(SECOND),
        "{}",
        out.stderr
    );
}

#[test]
fn a_reach_target_that_names_no_lane_says_so_rather_than_answering_emptily() {
    let h = nested_home();
    std::fs::create_dir_all(h.projects().join(EMPTY_ENC)).unwrap();
    let out = h.run(&["whoami", "--to", &at(EMPTY_ENC)]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("resolved to no transcript"),
        "{}",
        out.stderr
    );
}
