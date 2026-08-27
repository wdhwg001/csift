//! agents filters: kind/axis windows and the single-agent grab.

use crate::harness::*;

#[test]
fn agents_kind_filter_json_and_tree_json_and_multi_node_text() {
    // `--kind builtin-task --format json` hits the BuiltinTask JSON-label arm; v0.5 JSON
    // is FLAT kind-tagged rows (no children[] nesting - the tree lives in TEXT mode and
    // in parent_agent_id/depth); a multi-node text render shows every node's lifecycle.
    let h = populated_home();
    let bt = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--shape",
        "builtin-task",
        "--format",
        "json",
    ]);
    assert!(bt.success, "stderr: {}", bt.stderr);
    assert!(
        bt.stdout.contains("\"builtin-task\""),
        "builtin-task JSON label missing: {}",
        bt.stdout
    );
    let flat = h.run(&["agents", at(SESS).as_str(), "--format", "json"]);
    assert!(flat.success, "stderr: {}", flat.stderr);
    assert!(
        !flat.stdout.contains("\"children\""),
        "v0.5 flat rows must not nest children[]: {}",
        flat.stdout
    );
    assert!(
        !json_rows(&flat.stdout, "agent").is_empty(),
        "kind:agent rows present: {}",
        flat.stdout
    );
    // Text render with BOTH subagents → both lifecycle blocks print.
    let text = h.run(&["agents", at(SESS).as_str()]);
    assert!(text.success && text.stdout.matches("triggered").count() >= 2);
}

#[test]
fn agents_single_agent_grab_text() {
    // `--agent <hex>` grabs ONE subagent (implies --returned-message), exercising the
    // single-node text path + the guided-error landing flag documented in the EXAMPLES.
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--agent", "aaa111"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("aaa111"),
        "node not grabbed: {}",
        out.stdout
    );
}

#[test]
fn agents_agent_grab_bypasses_time_and_kind_filters() {
    // `--agent <hex>` is a DIRECT lookup: even with a --since window that would exclude the
    // agent's trigger time AND a --kind that does not match, the grab still resolves.
    let h = populated_home();
    // aaa111 is a builtin-task triggered ~05:00; this window + kind would normally exclude it.
    let out = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--agent",
        "aaa111",
        "--since",
        "2026-06-08T00:00:00Z",
        "--shape",
        "workflow",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("aaa111"),
        "direct --agent lookup must bypass time/kind filters: {}",
        out.stdout
    );
}

#[test]
fn agents_agent_grab_renders_single_node_not_whole_workflow() {
    // `--agent <hex>`: the single-node grab renders JUST that node (a tree of one); the
    // always-on whole-workflow tree is NOT dumped. bbb222 is in workflow wf_abc alongside no
    // other agent here, but the grab must render bbb222 and NOT the WORKFLOW run header.
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--agent", "bbb222"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("bbb222"),
        "node grabbed: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("WORKFLOW"),
        "--agent must suppress the whole-workflow tree: {}",
        out.stdout
    );
}

#[test]
fn agents_kind_filter_workflow_only() {
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--shape", "workflow"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("workflow"));
    assert!(!out.stdout.contains("builtin-task"));
    assert!(out.stdout.contains("kind=workflow"));
}

#[test]
fn agents_by_completion_axis_and_window() {
    let h = populated_home();
    let out = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--since",
        "2026-06-07T06:00:30Z",
        "--order-by",
        "completion",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Only the workflow agent completes at 06:01 (after the bound); the built-in
    // completes at 05:03 (before). Window-axis footer reflects completion.
    assert!(out.stdout.contains("window-axis=completion"));
    assert!(out.stdout.contains("workflow"));
    assert!(!out.stdout.contains("builtin-task"));
}

#[test]
fn agents_single_agent_grab_includes_returned_and_files() {
    // `--agent <hex>` selects ONE node and implies the returned message + files.
    let h = topology_home();
    let out = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--agent",
        "topo11",
        "--with-files",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    // envelope v2 flat rows: header + session row + the one agent row + summary -
    // the bare-node special case stays gone (one consumer code path).
    assert_eq!(
        lines.len(),
        4,
        "header + session + agent + summary: {:?}",
        lines
    );
    let agents = json_rows(&out.stdout, "agent");
    assert_eq!(agents.len(), 1, "exactly the one selected node");
    let v = &agents[0];
    assert_eq!(v["agent_id"], "topo11");
    assert_eq!(
        v["returned_message"],
        "SYNC-RETURN: the built-in carry answer"
    );
    let files = v["files_changed"].as_array().expect("files_changed array");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"], "/repo/src/parse.rs");
    assert_eq!(files[0]["op"], "edit");
}

#[test]
fn agents_fork_provenance_and_agent_type_filter() {
    // A /fork child: line 1 is a timestampless fork-context-ref carrying the parent's
    // last uuid at fork time + the carried context length; meta agentType is "fork".
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"work"}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-f0f0f0f0f0f0.jsonl"),
        concat!(
            r#"{"type":"fork-context-ref","agentId":"f0f0f0f0f0f0","parentSessionId":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","parentLastUuid":"aaaa1111-2222-4333-8444-555566667777","contextLength":2513}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"f0f0f0f0f0f0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"forked work"}}"#, "\n",
            r#"{"type":"assistant","isSidechain":true,"agentId":"f0f0f0f0f0f0","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"continuing from the fork"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-f0f0f0f0f0f0.meta.json"),
        r#"{"agentType":"fork","isFork":true}"#,
    );
    // A second, ordinary subagent to prove the filter separates.
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-abcd9999ee00.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"abcd9999ee00","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"plain work"}}"#, "\n",
            r#"{"type":"assistant","isSidechain":true,"agentId":"abcd9999ee00","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-abcd9999ee00.meta.json"),
        r#"{"agentType":"general-purpose","description":"worker","toolUseId":"t1"}"#,
    );

    let out = h.run(&["agents", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("forked-at  aaaa1111-2222-4333-8444-555566667777 (context 2513)"),
        "fork point named: {}",
        out.stdout
    );

    let json = h.run(&["agents", at(SESS).as_str(), "--format", "json"]);
    let fork_row: serde_json::Value = json
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .find(|o: &serde_json::Value| o["kind"] == "agent" && o["agent_type"] == "fork")
        .expect("fork agent row");
    assert_eq!(
        fork_row["fork_parent_last_uuid"],
        "aaaa1111-2222-4333-8444-555566667777"
    );
    assert_eq!(fork_row["fork_context_length"], 2513);
    let plain_row: serde_json::Value = json
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .find(|o: &serde_json::Value| o["kind"] == "agent" && o["agent_type"] == "general-purpose")
        .expect("plain agent row");
    assert_eq!(plain_row["fork_parent_last_uuid"], serde_json::Value::Null);

    // The repeatable exact-match type filter separates the two.
    let only_fork = h.run(&["agents", at(SESS).as_str(), "--agent-type", "fork"]);
    assert!(
        only_fork.stdout.contains("f0f0f0f0f0f0"),
        "{}",
        only_fork.stdout
    );
    assert!(
        !only_fork.stdout.contains("abcd9999ee00"),
        "filter excludes the other type: {}",
        only_fork.stdout
    );
    let both = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--agent-type",
        "fork",
        "--agent-type",
        "general-purpose",
    ]);
    assert!(both.stdout.contains("f0f0f0f0f0f0") && both.stdout.contains("abcd9999ee00"));
}
