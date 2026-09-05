//! agents: a teammate's two id forms - the transcript id and the `Name@Team` routing id.

use crate::harness::*;

const ENC_R: &str = "-Users-dev-Projects-harbor";
const SESS_R: &str = "00000000-0000-4000-8000-000000000002";
const RELAY_ID: &str = "aRelay-0123456789abcdef";

/// A session owning the teammate `Relay`. `team` = the meta's `teamName`; `None` writes a
/// teammate meta WITHOUT one, the shape that has no routing form to print.
fn teammate_home(team: Option<&str>) -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC_R}/{SESS_R}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start the relay"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_relay","name":"Agent","input":{"description":"relay work","subagent_type":"general-purpose","name":"Relay"}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC_R}/{SESS_R}/subagents/agent-{RELAY_ID}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aRelay-0123456789abcdef","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"<teammate-message teammate_id=\"team-lead\">relay the beacon</teammate-message>"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"beacon relayed"}]}}"#, "\n",
        ),
    );
    let meta = match team {
        Some(t) => format!(
            r#"{{"agentType":"Relay","description":"relay work","name":"Relay","taskKind":"in_process_teammate","teamName":"{t}"}}"#
        ),
        None => r#"{"agentType":"Relay","description":"relay work","name":"Relay","taskKind":"in_process_teammate"}"#.to_string(),
    };
    h.write(
        &format!("{ENC_R}/{SESS_R}/subagents/agent-{RELAY_ID}.meta.json"),
        &meta,
    );
    h
}

#[test]
fn agents_prints_both_teammate_id_forms() {
    // The two ids are minted apart at spawn, so neither is derivable from the other: the node
    // line carries the transcript id AND the routing id, and JSON carries both as fields.
    let h = teammate_home(Some("harbor"));
    let text = h.run(&["agents", &at(SESS_R), "--shape", "teammate"]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(
        text.stdout.contains(RELAY_ID) && text.stdout.contains("routing: Relay@harbor"),
        "the node line must carry both id forms: {}",
        text.stdout
    );

    let json = h.run(&["agents", &at(SESS_R), "--format", "json"]);
    assert!(json.success, "stderr: {}", json.stderr);
    let node = json_rows(&json.stdout, "agent")
        .into_iter()
        .find(|n| n.get("shape").and_then(|k| k.as_str()) == Some("teammate"))
        .expect("a teammate row in agents JSON");
    assert_eq!(node["agent_id"], RELAY_ID);
    assert_eq!(node["routing_id"], "Relay@harbor");
    assert_eq!(node["team_name"], "harbor");
}

#[test]
fn agents_routing_target_behaves_exactly_like_the_transcript_id() {
    // The routing form is resolved by the SHARED resolver, so it lands on the same transcript
    // as the id it names: `agents` then enumerates that lane's own children (a teammate has
    // none here), identically for both forms. The parity is the point - the routing form adds
    // an address, never a second behaviour.
    let h = teammate_home(Some("harbor"));
    let by_routing = h.run(&["agents", "@Relay@harbor"]);
    let by_id = h.run(&["agents", &at(RELAY_ID)]);
    assert!(by_routing.success, "stderr: {}", by_routing.stderr);
    assert!(by_id.success, "stderr: {}", by_id.stderr);
    assert_eq!(
        by_routing.stdout, by_id.stdout,
        "the two id forms must address the same lane"
    );
}

#[test]
fn agents_teammate_hint_states_the_two_id_rule() {
    // The hint exists because the wrong id in the wrong tool is the observed failure: say
    // which tool takes which form, and which of the two can collide.
    let h = teammate_home(Some("harbor"));
    let text = h.run(&["agents", &at(SESS_R)]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(
        text.stdout.contains("routing id Name@Team")
            && text.stdout.contains("a<Name>-<hex>")
            && text.stdout.contains("collide"),
        "the teammate hint must state the two-id rule: {}",
        text.stdout
    );

    let json = h.run(&["agents", &at(SESS_R), "--format", "json"]);
    let node = json_rows(&json.stdout, "agent")
        .into_iter()
        .find(|n| n.get("shape").and_then(|k| k.as_str()) == Some("teammate"))
        .expect("a teammate row in agents JSON");
    let hint = node["control_hint"].as_str().unwrap_or("");
    assert!(
        hint.contains("routing_id") && hint.contains("agent_id"),
        "the JSON hint must name both id fields: {hint}"
    );
}

#[test]
fn agents_teammate_without_a_team_has_a_null_routing_id() {
    // A meta carrying only half the pair has no addressable routing form: report null rather
    // than fabricate an id that names nothing.
    let h = teammate_home(None);
    let json = h.run(&["agents", &at(SESS_R), "--format", "json"]);
    assert!(json.success, "stderr: {}", json.stderr);
    let node = json_rows(&json.stdout, "agent")
        .into_iter()
        .find(|n| n.get("shape").and_then(|k| k.as_str()) == Some("teammate"))
        .expect("a teammate row in agents JSON");
    assert!(
        node["routing_id"].is_null(),
        "routing_id must be null without a team name: {node}"
    );
    let text = h.run(&["agents", &at(SESS_R), "--shape", "teammate"]);
    let head = text
        .stdout
        .lines()
        .find(|l| l.contains(RELAY_ID))
        .unwrap_or_default();
    assert!(
        !head.contains("routing:"),
        "no routing form to print on the node line: {head}"
    );
}
