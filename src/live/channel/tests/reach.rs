//! The reach section builders, the slot census and the two sentences a prediction adds.
//!
//! Everything here reads a fixture tree by PATH: the section builders never take a claude home,
//! so a unit test can pin what they say without the process-global data root any e2e run needs.

use super::*;

use crate::live::channel::caller::GateVerdict;
use crate::live::channel::policy::{self, ReceiverKind, ReceiverState};
use crate::live::channel::reach::{
    armed_line, inference_note, lane_facts, live_child_lanes, receiver_kind_of, send_context,
    slot_line, target_facts, ReachSlots, NOT_A_LANE,
};
use crate::path::settings;

/// A lane whose tail is an unreturned tool call: in flight at any age, which is what makes it a
/// deterministic fixture (a recency-based state would age out of its own test).
const IN_FLIGHT_LANE: &str = concat!(
    r#"{"type":"user","uuid":"i1","timestamp":"2026-06-07T05:00:00.000Z","version":"2.1.258","message":{"role":"user","content":"go"}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"i2","timestamp":"2026-06-07T05:00:05.000Z","version":"2.1.258","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"sleep 1"}}]}}"#,
    "\n",
);

/// A lane that ended cleanly long ago: settled under every rule.
const SETTLED_LANE: &str = concat!(
    r#"{"type":"user","uuid":"s1","timestamp":"2026-06-07T05:00:00.000Z","version":"2.1.258","message":{"role":"user","content":"go"}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"s2","timestamp":"2026-06-07T05:00:05.000Z","version":"2.1.258","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"done"}]}}"#,
    "\n",
);

/// A session with a teammate lane (in flight), a lane it spawned (in flight) and a settled
/// built-in lane. Returns the main transcript path.
fn reach_tree(f: &Fixture) -> PathBuf {
    let session_path = f.scratch().join(format!("{SESSION}.jsonl"));
    let subagents = f.scratch().join(SESSION).join("subagents");
    std::fs::create_dir_all(&subagents).unwrap();
    std::fs::write(&session_path, LANE_TRANSCRIPT).unwrap();

    std::fs::write(
        subagents.join(format!("agent-{TEAMMATE}.jsonl")),
        IN_FLIGHT_LANE,
    )
    .unwrap();
    std::fs::write(
        subagents.join(format!("agent-{TEAMMATE}.meta.json")),
        r#"{"agentType":"Relay","taskKind":"in_process_teammate","name":"Relay","teamName":"harbor"}"#,
    )
    .unwrap();

    std::fs::write(
        subagents.join(format!("agent-{AGENT}.jsonl")),
        IN_FLIGHT_LANE,
    )
    .unwrap();
    std::fs::write(
        subagents.join(format!("agent-{AGENT}.meta.json")),
        format!(r#"{{"agentType":"general-purpose","parentAgentId":"{TEAMMATE}"}}"#),
    )
    .unwrap();

    let settled = "a0123456789abcde9";
    std::fs::write(
        subagents.join(format!("agent-{settled}.jsonl")),
        SETTLED_LANE,
    )
    .unwrap();
    std::fs::write(
        subagents.join(format!("agent-{settled}.meta.json")),
        r#"{"agentType":"general-purpose"}"#,
    )
    .unwrap();
    session_path
}

#[test]
fn lane_facts_name_a_teammate_in_both_forms() {
    let f = Fixture::new();
    let session_path = reach_tree(&f);
    let lane = session_path
        .with_extension("")
        .join("subagents")
        .join(format!("agent-{TEAMMATE}.jsonl"));
    let facts = lane_facts(&lane).unwrap();
    // The transcript form is the id; the routing form is the one the official send takes, and
    // a teammate must print BOTH because only the first is unique.
    assert_eq!(facts.lane, TEAMMATE);
    assert_eq!(facts.routing_id.as_deref(), Some("Relay@harbor"));
    assert_eq!(facts.kind, ReceiverKind::Teammate);
    assert_eq!(facts.session, SESSION);
    assert_eq!(facts.depth, Some(0));
    // An unreturned tool call at the tail is a lane blocked there, never a finished one.
    assert_eq!(facts.state, ReceiverState::Frozen);
    assert_eq!(facts.version.as_deref(), Some("2.1.258"));
}

#[test]
fn lane_facts_link_a_lane_to_the_agent_that_spawned_it() {
    let f = Fixture::new();
    let session_path = reach_tree(&f);
    let lane = session_path
        .with_extension("")
        .join("subagents")
        .join(format!("agent-{AGENT}.jsonl"));
    let facts = lane_facts(&lane).unwrap();
    assert_eq!(facts.parent_agent_id.as_deref(), Some(TEAMMATE));
    assert_eq!(facts.kind, ReceiverKind::UnnamedSubagent);
}

#[test]
fn a_top_level_lane_reports_no_parent_and_depth_zero() {
    let f = Fixture::new();
    let session_path = reach_tree(&f);
    let facts = lane_facts(&session_path).unwrap();
    assert_eq!(facts.kind, ReceiverKind::TopLevel);
    assert_eq!(facts.lane, facts.session);
    assert_eq!(facts.depth, Some(0));
    assert!(facts.parent_agent_id.is_none());
}

#[test]
fn live_child_lanes_keep_only_the_unsettled_ones() {
    let f = Fixture::new();
    let session_path = reach_tree(&f);
    let rows = live_child_lanes(&session_path, None);
    let ids: Vec<&str> = rows.iter().map(|r| r.lane.as_str()).collect();
    assert_eq!(rows.len(), 2, "the settled lane is not live: {ids:?}");
    assert!(ids.contains(&TEAMMATE) && ids.contains(&AGENT), "{ids:?}");
    assert!(rows.iter().all(|r| r.state == "in-flight"), "{rows:?}");
    let teammate = rows.iter().find(|r| r.lane == TEAMMATE).unwrap();
    assert_eq!(teammate.kind, ReceiverKind::Teammate);
    assert!(teammate.last_activity_utc.is_some());
}

#[test]
fn live_child_lanes_restrict_to_one_subtree() {
    let f = Fixture::new();
    let session_path = reach_tree(&f);
    let only: std::collections::HashSet<String> = [AGENT.to_string()].into_iter().collect();
    let rows = live_child_lanes(&session_path, Some(&only));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].lane, AGENT);
}

#[test]
fn the_on_disk_kinds_map_onto_the_policy_table_receivers() {
    use crate::subagent::SubagentKind;
    assert_eq!(
        receiver_kind_of(SubagentKind::Teammate),
        ReceiverKind::Teammate
    );
    assert_eq!(
        receiver_kind_of(SubagentKind::Workflow),
        ReceiverKind::WorkflowLane
    );
    assert_eq!(
        receiver_kind_of(SubagentKind::BuiltinTask),
        ReceiverKind::UnnamedSubagent
    );
}

/// A claude home carrying `slots` delivery entries on PostToolUse and one on Stop.
fn home_with_slots(f: &Fixture, slots: usize) -> PathBuf {
    let home = f.scratch().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let entries: Vec<String> = (1..=slots)
        .map(|k| format!(r#"{{"type":"command","command":"csift deliver --slot {k}"}}"#))
        .collect();
    std::fs::write(
        home.join("settings.json"),
        format!(
            r#"{{"hooks":{{"PostToolUse":[{{"hooks":[{}]}}],"Stop":[{{"hooks":[{{"type":"command","command":"csift deliver --slot 1"}}]}}]}}}}"#,
            entries.join(",")
        ),
    )
    .unwrap();
    home
}

#[test]
fn reach_slots_read_every_configured_delivery_event() {
    let f = Fixture::new();
    let home = home_with_slots(&f, 2);
    let slots = ReachSlots::read(&settings::merged(&home, None));
    assert_eq!(slots.best, 2);
    assert_eq!(slots.best_event.as_deref(), Some("PostToolUse"));
    assert_eq!(
        slot_line(&slots),
        "PostToolUse 1,2  Stop 1",
        "the line names every event with slots, in delivery-event order"
    );
    assert!(!slots.async_rewake_on_stop);
}

#[test]
fn an_empty_cascade_says_none_configured_rather_than_printing_nothing() {
    let f = Fixture::new();
    let home = f.scratch().join("bare-home");
    std::fs::create_dir_all(&home).unwrap();
    let slots = ReachSlots::read(&settings::merged(&home, None));
    assert_eq!(slots.best, 0);
    assert_eq!(slot_line(&slots), "none configured on any delivery event");
    assert_eq!(
        armed_line(&[]),
        "none (no delivery hook has run in this lane)"
    );
    assert_eq!(armed_line(&[1, 2]), "1,2");
}

/// A running workflow lane as a prediction target: the row the official transports fail closed
/// on, so csift's own channel is the only carrier and the fallback is the interesting half.
fn workflow_target() -> crate::live::channel::reach::TargetFacts {
    let mut t = target_facts(&lane_stub());
    t.kind = ReceiverKind::WorkflowLane;
    t
}

/// A minimal set of lane facts to build a target from, with no disk behind it.
fn lane_stub() -> crate::live::channel::reach::LaneFacts {
    crate::live::channel::reach::LaneFacts {
        lane: AGENT.to_string(),
        session: SESSION.to_string(),
        session_path: PathBuf::from("/nowhere"),
        path: PathBuf::from("/nowhere"),
        kind: ReceiverKind::UnnamedSubagent,
        routing_id: None,
        depth: Some(0),
        parent_agent_id: None,
        state: ReceiverState::Running,
        version: Some("2.1.258".to_string()),
        cwd: None,
        last_activity_utc: None,
        socket_present: false,
        headless: false,
    }
}

fn external_caller() -> crate::live::channel::caller::Caller {
    crate::live::channel::caller::Caller {
        kind: SenderKind::External,
        session: None,
        lane: None,
        label: Some("relay-probe".to_string()),
        lane_exact: true,
    }
}

#[test]
fn an_agent_prediction_names_its_instruments_and_the_csift_fallback() {
    let f = Fixture::new();
    let merged = settings::merged(&home_with_slots(&f, 2), None);
    let slots = ReachSlots::read(&merged);
    let t = workflow_target();
    let ctx = send_context(
        &external_caller(),
        &t,
        &merged,
        &slots,
        0,
        Relation::External,
    );
    let note = inference_note(&t, &policy::decide(&ctx), &ctx).expect("an agent target");
    assert!(
        note.contains("transcript tail and a pid probe") && note.contains("not the harness's own"),
        "{note}"
    );
    assert!(note.ends_with("fallback: csift steer"), "{note}");
}

#[test]
fn an_agent_prediction_with_no_configured_slot_falls_back_to_held() {
    let f = Fixture::new();
    let bare = f.scratch().join("bare-home");
    std::fs::create_dir_all(&bare).unwrap();
    let merged = settings::merged(&bare, None);
    let slots = ReachSlots::read(&merged);
    let t = workflow_target();
    let ctx = send_context(
        &external_caller(),
        &t,
        &merged,
        &slots,
        0,
        Relation::External,
    );
    // Queued, but nothing is configured to emit it: the honest fallback is that it waits.
    let note = inference_note(&t, &policy::decide(&ctx), &ctx).expect("an agent target");
    assert!(note.ends_with("fallback: HELD"), "{note}");
}

#[test]
fn a_top_level_prediction_carries_no_inference_sentence() {
    let f = Fixture::new();
    let merged = settings::merged(&home_with_slots(&f, 1), None);
    let slots = ReachSlots::read(&merged);
    let mut t = target_facts(&lane_stub());
    t.kind = ReceiverKind::TopLevel;
    let ctx = send_context(
        &external_caller(),
        &t,
        &merged,
        &slots,
        0,
        Relation::External,
    );
    assert!(
        inference_note(&t, &policy::decide(&ctx), &ctx).is_none(),
        "a top-level lane is answered from the registry row the harness wrote"
    );
}

#[test]
fn the_gate_verdicts_name_a_scope_or_the_evidence_they_could_not_settle() {
    let enabled = GateVerdict::teams(Some("projectSettings"), 2, 1);
    assert!(enabled.enabled);
    assert_eq!(
        enabled.verdict,
        "enabled via settings env (projectSettings)"
    );

    let unknown = GateVerdict::teams(None, 2, 1);
    assert!(!unknown.enabled);
    assert!(
        unknown.verdict.contains("not observable -> unknown")
            && unknown.verdict.contains("teams directories 2"),
        "an unsettled gate reports the evidence, never an assumption: {}",
        unknown.verdict
    );

    assert_eq!(
        GateVerdict::harbor(true).verdict,
        "registry messagingSocketPath present -> on and bound"
    );
    assert_eq!(
        GateVerdict::harbor(false).verdict,
        "registry messagingSocketPath absent -> unknown"
    );
}

#[test]
fn the_not_a_lane_answer_is_one_fixed_phrase() {
    // The e2e pins the whole block; this pins the phrase both renderings share, so the text and
    // the JSON row cannot drift apart.
    assert_eq!(NOT_A_LANE, "not a Claude Code lane");
}
