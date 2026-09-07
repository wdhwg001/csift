//! The spawn NAME join behind a teammate's comm direction. A teammate's meta carries no
//! `toolUseId`, so the usual id-join from the spawning `tool_use` cannot reach it; the
//! name join (the meta's `name` against the `Agent` call's `input.name`) is the only link,
//! and it is what turns the direction's target from the typed NAME into the child's
//! transcript id - the form that round-trips as an `@` target.

use crate::harness::*;

const SPAWN_ENC: &str = "-Users-dev-example-project";
const SPAWN_SESS: &str = "00000000-0000-4000-8000-000000000041";
const MATE: &str = "aRelay-0123456789abcdef";

fn spawn_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{SPAWN_ENC}/{SPAWN_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"bring the relay up"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sp1","name":"Agent","input":{"subagent_type":"general-purpose","name":"Relay","prompt":"zzspawn carry the traffic"}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{SPAWN_ENC}/{SPAWN_SESS}/subagents/agent-{MATE}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aRelay-0123456789abcdef","uuid":"s0","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"zzspawn carry the traffic"}}"#, "\n",
        ),
    );
    // A teammate meta: a NAME and a team, and deliberately NO toolUseId.
    h.write(
        &format!("{SPAWN_ENC}/{SPAWN_SESS}/subagents/agent-{MATE}.meta.json"),
        r#"{"agentType":"Relay","taskKind":"in_process_teammate","name":"Relay","teamName":"crew"}"#,
    );
    h
}

#[test]
fn a_teammate_spawn_resolves_its_target_through_the_name_join() {
    let h = spawn_home();
    let out = h.run(&[
        "search",
        "zzspawn",
        &at(SPAWN_SESS),
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows = json_rows(&out.stdout, "exchange");
    let hit = rows
        .iter()
        .flat_map(|r| r["hits"].as_array().unwrap().clone())
        .find(|h| h["label"] == "agent.communication.sent")
        .unwrap_or_else(|| panic!("no sent hit in:\n{}", out.stdout));
    assert_eq!(
        hit["to"],
        serde_json::json!(MATE),
        "the join must yield the TRANSCRIPT id, not the typed name: {hit}"
    );
    assert_eq!(hit["from"], serde_json::json!("self"));
    // The text render carries the same resolved pair.
    let text = h.run(&["search", "zzspawn", &at(SPAWN_SESS), "--no-subagents"]);
    assert!(
        text.stdout.contains(&format!("self \u{21e8} {MATE}")),
        "text direction:\n{}",
        text.stdout
    );
}
