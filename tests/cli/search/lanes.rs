//! search classification across LANES: the subagent spawn-prompt seed and the fork clone.

use crate::harness::*;

#[test]
fn a_fork_clone_has_no_spawn_seed_but_a_genuine_child_does() {
    // A `/fork` child's line 1 is a `fork-context-ref` record and its transcript is a
    // clone of the parent's: its first turn-opener is the parent's own human message,
    // never a spawn-prompt seed (v0.10.2). A genuine Task child keeps the seed.
    let h = Home::new();
    let enc = "-Users-dev-example-project";
    let sess = "13131313-2424-4535-8646-757575757575";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"charting"}]}}"#, "\n",
        ),
    );
    let fork = "f0f0f0f0f0f0f0f01";
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{fork}.jsonl"),
        &format!(
            concat!(
                r#"{{"type":"fork-context-ref","agentId":"{fork}","parentSessionId":"{sess}","parentLastUuid":"a0","contextLength":120}}"#, "\n",
                r#"{{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"chart the reef"}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"charting"}}]}}}}"#, "\n",
            ),
            fork = fork,
            sess = sess
        ),
    );
    let genuine = "c1c1c1c1c1c1c1c1";
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{genuine}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"s0","isSidechain":true,"timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"chart the reef for the parent"}}"#, "\n",
        ),
    );
    let j = h.run(&["search", "chart the reef", &at(sess), "--format", "json"]);
    assert!(j.success, "stderr: {}", j.stderr);
    let mut by_lane: Vec<(String, String)> = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|r| r["kind"] == "exchange")
        .flat_map(|r| {
            let sid = r["session_id"].as_str().unwrap_or("").to_string();
            r["hits"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(move |h| (sid.clone(), h["label"].as_str().unwrap_or("").to_string()))
        })
        .collect();
    by_lane.sort();
    let of = |lane: &str| -> Vec<&str> {
        by_lane
            .iter()
            .filter(|(s, _)| s == lane)
            .map(|(_, l)| l.as_str())
            .collect()
    };
    assert_eq!(
        of(fork),
        vec!["user.message"],
        "the clone's opener is the human's message: {}",
        j.stdout
    );
    assert_eq!(
        of(genuine),
        vec!["agent.communication.inbox"],
        "the genuine child keeps its spawn seed: {}",
        j.stdout
    );
    assert_eq!(of(sess), vec!["user.message"], "{}", j.stdout);
}
