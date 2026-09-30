//! A workflow lane is settled by its run's own files - a journal line closing the agent, or the
//! manifest an ended run writes - and `status` and `agents` read that one rule alike; `agents`
//! also takes the run's WORD, so a lane its run killed or its journal failed never reads
//! `completed`.

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

/// `agents` reads a lane's run before its tail, as `status` does: the killed run's lane reads
/// `killed` and the lane its journal failed reads `failed`, the open run's frozen lane reads
/// `running`, and the lanes `agents` calls running are exactly the lanes `status` lists live.
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
    assert_eq!(status_of("a000000000000000a"), "killed");
    assert_eq!(status_of("a000000000000000b"), "failed");
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
    // The open run's lane is live by its tail AND started in its journal: one live lane.
    let children_line = verdict["evidence"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|e| e["surface"] == "children")
        .expect("the children evidence");
    assert_eq!(
        children_line["value"], "3 lane(s), 1 live",
        "{}",
        status.stdout
    );
}

/// Four ways a run settles a lane whose tail ended its turn cleanly - so the tail alone reads
/// `completed` on all four. The run's word decides: a `killed` manifest and a journal `failed`
/// line are not completions (the run's word, no completion instant, `settled_by` and the same
/// words in brackets on the text row), while a `completed` manifest and a journal `result`
/// are. The reasons are the ones `status` gives a settled lane.
#[test]
fn a_lane_its_run_ended_takes_the_runs_word_and_no_completion_instant() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"run the fleet"}}"#,
    );
    let clean = concat!(
        r#"{"type":"user","uuid":"f1","isSidechain":true,"timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"go"}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"f2","parentUuid":"f1","isSidechain":true,"timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"done"}]}}"#,
        "\n",
    );
    // (run, agent, manifest status, journal closing line)
    let runs = [
        ("wf_K", "a00000000000000k1", Some("killed"), None),
        ("wf_F", "a00000000000000f1", None, Some("failed")),
        ("wf_C", "a00000000000000c1", Some("completed"), None),
        ("wf_R", "a00000000000000r1", None, Some("result")),
    ];
    for (run, id, manifest, line) in runs {
        let wf = |rel: &str| format!("{ENC}/{SESS}/subagents/workflows/{run}/{rel}");
        h.write(&wf(&format!("agent-{id}.jsonl")), clean);
        h.write(
            &wf(&format!("agent-{id}.meta.json")),
            r#"{"agentType":"workflow-subagent"}"#,
        );
        let mut journal = format!("{{\"type\":\"started\",\"agentId\":\"{id}\"}}\n");
        if let Some(l) = line {
            journal.push_str(&format!("{{\"type\":\"{l}\",\"agentId\":\"{id}\"}}\n"));
        }
        h.write(&wf("journal.jsonl"), &journal);
        if let Some(m) = manifest {
            h.write(
                &format!("{ENC}/{SESS}/workflows/{run}.json"),
                &format!("{{\"runId\":\"{run}\",\"status\":\"{m}\"}}"),
            );
        }
    }
    let json = h.run(&["agents", &at(SESS), "--format", "json"]);
    assert!(json.success, "{}", json.stderr);
    let row = |id: &str| {
        rows(&json.stdout, "agent")
            .into_iter()
            .find(|r| r["agent_id"] == id)
            .unwrap_or_else(|| panic!("no row for {id}:\n{}", json.stdout))
    };
    let manifest = |w: &str| format!("its workflow run ended (manifest status {w})");
    let journal = |l: &str| format!("the workflow journal closes this agent ({l})");
    for (id, status, why, done) in [
        ("a00000000000000k1", "killed", manifest("killed"), false),
        ("a00000000000000f1", "failed", journal("failed"), false),
        (
            "a00000000000000c1",
            "completed",
            manifest("completed"),
            true,
        ),
        ("a00000000000000r1", "completed", journal("result"), true),
    ] {
        let r = row(id);
        assert_eq!(r["status"], status, "{r}");
        assert_eq!(r["settled_by"], why.as_str(), "{r}");
        assert_eq!(r["completed_utc"].is_string(), done, "{r}");
        assert_eq!(r["duration"].is_string(), done, "{r}");
        assert!(r["last_activity_utc"].is_string(), "{r}");
    }
    let text = h.run(&["agents", &at(SESS)]);
    assert!(text.success, "{}", text.stderr);
    let line_of = |id: &str| {
        text.stdout
            .lines()
            .find(|l| l.contains(id))
            .unwrap_or_default()
            .to_string()
    };
    assert!(
        line_of("a00000000000000k1").ends_with(&format!("killed  [{}]", manifest("killed"))),
        "{}",
        text.stdout
    );
    assert!(
        line_of("a00000000000000f1").ends_with(&format!("failed  [{}]", journal("failed"))),
        "{}",
        text.stdout
    );
    for id in ["a00000000000000c1", "a00000000000000r1"] {
        assert!(line_of(id).ends_with("completed"), "{}", text.stdout);
    }
}

/// A run resumed under its own id runs under the manifest the killed run wrote until it ends
/// and rewrites it, so a manifest settles only a lane whose newest record is no newer than the
/// manifest's own end instant, within a minute: wf_D's manifest ended eleven minutes before its
/// lane's newest record (the lane is read from its tail - frozen at an unreturned call, so
/// running and live - and the row names the stale manifest), while wf_E's lane wrote its last
/// record 20 ms after its manifest's end, the kill's own write, and stays `killed`.
#[test]
fn a_manifest_older_than_its_lane_settles_nothing() {
    let h = Home::new();
    let sess = "0f1e2d3c-4b5a-4968-8776-a5b4c3d2e1f1";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"resume the fleet"}}"#,
    );
    let wf = |run: &str, rel: &str| format!("{ENC}/{sess}/subagents/workflows/{run}/{rel}");
    let meta = r#"{"agentType":"workflow-subagent"}"#;
    for (run, agent, ended) in [
        ("wf_D", "a000000000000000d", "2026-06-07T04:50:00.000Z"),
        ("wf_E", "a000000000000000e", "2026-06-07T05:01:04.980Z"),
    ] {
        h.write(&wf(run, &format!("agent-{agent}.jsonl")), &frozen());
        h.write(&wf(run, &format!("agent-{agent}.meta.json")), meta);
        h.write(
            &wf(run, "journal.jsonl"),
            &format!("{{\"type\":\"started\",\"agentId\":\"{agent}\"}}\n"),
        );
        h.write(
            &format!("{ENC}/{sess}/workflows/{run}.json"),
            &format!(r#"{{"runId":"{run}","status":"killed","timestamp":"{ended}"}}"#),
        );
    }
    let agents = h.run(&["agents", &at(sess), "--format", "json"]);
    assert!(agents.success, "{}", agents.stderr);
    let row = |id: &str| {
        rows(&agents.stdout, "agent")
            .into_iter()
            .find(|r| r["agent_id"] == id)
            .unwrap_or_else(|| panic!("no row for {id}: {}", agents.stdout))
    };
    let resumed = row("a000000000000000d");
    assert_eq!(resumed["status"], "running", "{resumed}");
    assert!(
        resumed["settled_by"]
            .as_str()
            .unwrap_or_default()
            .contains("manifest (status killed) ended before this lane's newest record"),
        "{resumed}"
    );
    assert_eq!(row("a000000000000000e")["status"], "killed");

    let status = h.run(&["status", &at(sess), "--format", "json"]);
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
    assert_eq!(
        live,
        vec!["a000000000000000d".to_string()],
        "{}",
        status.stdout
    );
    assert_eq!(verdict["settled_children"], 1, "{}", status.stdout);
}
