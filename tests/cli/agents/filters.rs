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
