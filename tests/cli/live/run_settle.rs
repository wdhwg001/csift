//! A workflow lane is settled by its run's own files - the manifest an ended run writes, or a
//! journal line closing the agent - and `status` and `agents` read that one rule alike.

use crate::harness::*;

pub(super) const ENC: &str = "-Users-dev-Projects-fleet";
pub(super) const SESS: &str = "0f1e2d3c-4b5a-4968-8776-a5b4c3d2e1f0";

fn frozen() -> String {
    concat!(
        r#"{"type":"user","uuid":"f1","isSidechain":true,"timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
        r#"{"type":"assistant","uuid":"f2","parentUuid":"f1","isSidechain":true,"timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/x"}}]}}"#, "\n",
    )
    .to_string()
}

/// wf_A ended (its manifest says `killed`) with its lane frozen at an unreturned call; wf_B's
/// journal closes its lane with `failed` while the lane's tail is mid-turn; wf_C is open, its
/// journal starting its lane and the lane frozen at an unreturned call.
pub(super) fn fleet_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"run the fleet"}}"#,
    );
    let wf = |run: &str, rel: &str| format!("{ENC}/{SESS}/subagents/workflows/{run}/{rel}");
    let meta = r#"{"agentType":"workflow-subagent"}"#;
    h.write(&wf("wf_A", "agent-a000000000000000a.jsonl"), &frozen());
    h.write(&wf("wf_A", "agent-a000000000000000a.meta.json"), meta);
    h.write(
        &wf("wf_A", "journal.jsonl"),
        "{\"type\":\"started\",\"agentId\":\"a000000000000000a\"}\n",
    );
    h.write(
        &format!("{ENC}/{SESS}/workflows/wf_A.json"),
        r#"{"runId":"wf_A","status":"killed"}"#,
    );
    let midturn = frozen()
        + r#"{"type":"user","uuid":"f3","parentUuid":"f2","isSidechain":true,"timestamp":"2026-06-07T05:01:06.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#
        + "\n";
    h.write(&wf("wf_B", "agent-a000000000000000b.jsonl"), &midturn);
    h.write(&wf("wf_B", "agent-a000000000000000b.meta.json"), meta);
    h.write(
        &wf("wf_B", "journal.jsonl"),
        concat!(
            "{\"type\":\"started\",\"agentId\":\"a000000000000000b\"}\n",
            "{\"type\":\"failed\",\"agentId\":\"a000000000000000b\"}\n",
        ),
    );
    h.write(&wf("wf_C", "agent-a000000000000000c.jsonl"), &frozen());
    h.write(&wf("wf_C", "agent-a000000000000000c.meta.json"), meta);
    h.write(
        &wf("wf_C", "journal.jsonl"),
        "{\"type\":\"started\",\"agentId\":\"a000000000000000c\"}\n",
    );
    h
}

pub(super) fn rows(stdout: &str, kind: &str) -> Vec<serde_json::Value> {
    stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == kind)
        .collect()
}

/// `agents` reads a lane's run before its tail, as `status` does: the ended run's lane and the
/// lane its journal failed read `completed`, the open run's frozen lane reads `running`, and
/// the lanes `agents` calls running are exactly the lanes `status` lists live.
#[test]
fn agents_and_status_settle_a_workflow_lane_by_the_same_run_rule() {
    let h = fleet_home();
    let agents = h.run(&["agents", &at(SESS), "--format", "json"]);
    assert!(agents.success, "{}", agents.stderr);
    let status_of = |id: &str| {
        rows(&agents.stdout, "agent")
            .into_iter()
            .find(|r| r["agent_id"] == id)
            .map(|r| r["status"].as_str().unwrap_or_default().to_string())
            .unwrap_or_default()
    };
    assert_eq!(status_of("a000000000000000a"), "completed");
    assert_eq!(status_of("a000000000000000b"), "completed");
    assert_eq!(status_of("a000000000000000c"), "running");
    let running: Vec<String> = rows(&agents.stdout, "agent")
        .into_iter()
        .filter(|r| r["status"] == "running")
        .map(|r| r["agent_id"].as_str().unwrap_or_default().to_string())
        .collect();

    let status = h.run(&["status", &at(SESS), "--format", "json"]);
    assert!(status.success, "{}", status.stderr);
    let verdict = rows(&status.stdout, "verdict")
        .into_iter()
        .next()
        .expect("the verdict row");
    let live: Vec<String> = verdict["children"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|c| c["session_id"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(running, live, "{}", status.stdout);
    assert_eq!(verdict["settled_children"], 2, "{}", status.stdout);
}
