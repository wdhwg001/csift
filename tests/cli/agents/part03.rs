use crate::harness::*;

#[test]
fn agents_bad_hex_errors_with_discovery_guidance() {
    // A typo'd / non-existent --agent hex is a HARD error (non-zero) with discovery
    // guidance — NOT the ambiguous `no subagents found` that a zero-subagent session prints.
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--agent", "deadbeefcafe"]);
    assert!(!out.success, "a bad hex must be a hard error");
    assert!(
        out.stderr.contains("no subagent matched") && out.stderr.contains("agents @<uuid>"),
        "error must name the bad id + the discovery path; stderr: {}",
        out.stderr
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
fn agents_rejects_no_subagents_with_pointed_error() {
    // `agents --no-subagents` (a flag it does not have) is rejected with a pointed message,
    // NOT swallowed as a bogus PATH value by allow_hyphen_values.
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--no-subagents"]);
    assert!(!out.success, "the no-op span flag must error");
    assert!(
        out.stderr.contains("no subagent-span flag"),
        "stderr should explain agents has no span flag; got: {}",
        out.stderr
    );
}

#[test]
fn agents_json_rows() {
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // v0.5 FLAT rows: every node is its own `kind:"agent"` row — the uniform envelope
    // idiom (`jq 'select(.kind=="agent")'`) reaches all shapes directly.
    let kinds: Vec<String> = json_rows(&out.stdout, "agent")
        .iter()
        .filter_map(|n| n.get("shape").and_then(|k| k.as_str()).map(String::from))
        .collect();
    assert!(kinds.iter().any(|k| k == "builtin-task"));
    assert!(kinds.iter().any(|k| k == "workflow"));
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
fn agents_no_subagents_says_none() {
    let h = Home::new();
    // A session with no sidecar at all.
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    let out = h.run(&["agents", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no subagents found"),
        "got: {}",
        out.stdout
    );
}

#[test]
fn agents_unknown_session_errors() {
    let h = populated_home();
    let out = h.run(&[
        "agents",
        at("deadbeef-0000-0000-0000-000000000000").as_str(),
    ]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("no session file found"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn agents_via_project_path_target() {
    // Drive agents with a PATH target (not --session): resolve_target_sessions takes
    // the explicit-paths branch, enumerates the project's sessions, groups subagents.
    let h = populated_home();
    let out = h.run(&["agents", ENC]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("builtin-task"));
    assert!(out.stdout.contains("workflow"));
    // The agentType sub-label is rendered in [brackets].
    assert!(
        out.stdout.contains("[oh-my-claudecode:executor]")
            || out.stdout.contains("[workflow-subagent]")
    );
}

#[test]
fn agents_all_projects_default_scan() {
    // No PATH and no --session → scan every project (the all_project_dirs branch).
    let h = populated_home();
    let out = h.run(&["agents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("subagent(s)"));
}

#[test]
fn agents_path_with_no_sessions_and_no_session_flag_is_empty_not_error() {
    // A project dir that exists but has ZERO session files, with NO --session given →
    // `resolve_target_sessions` finds no files but does NOT bail (the `if let
    // Some(sid)` FALSE arm of the empty-files guard). Output: "no subagents found".
    let h = Home::new();
    std::fs::create_dir_all(h.projects().join(ENC)).unwrap(); // empty project dir
    let out = h.run(&["agents", ENC]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no subagents found"),
        "got: {}",
        out.stdout
    );
}

#[test]
fn agents_row_without_timestamps_omits_duration() {
    // A subagent whose transcript records carry NO timestamps → started/completed are
    // both absent → `duration_label` returns None (the `if let Some(dur)` FALSE arm),
    // so NO "duration" line is rendered for that row; status is `unknown`.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-nots99.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"nots99","message":{"role":"user","content":"start, no timestamp"}}"#, "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#, "\n",
        ),
    );
    let out = h.run(&["agents", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("unknown"),
        "status unknown: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("duration"),
        "no duration line: {}",
        out.stdout
    );
}

#[test]
fn agents_reports_skipped_lines_note() {
    // A subagent transcript with a malformed line → the per-row "malformed line(s)
    // skipped" note (agents.rs render skipped_lines arm).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    // The malformed line is the NEWEST (last) record so the TAIL scan reaches and
    // counts it (head stops at the first record; tail walks newest-first).
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-broken1.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"broken1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#, "\n",
            "{ this is a malformed newest line }\n",
        ),
    );
    let out = h.run(&["agents", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("malformed line(s) skipped"),
        "got: {}",
        out.stdout
    );
}

#[test]
fn agents_true_trigger_time_is_the_parent_tool_use_ts() {
    // The default axis is TRIGGER: the built-in's `trigger_utc` is the parent Agent
    // tool_use ts (04:59:58), which DIVERGES from its child-head `started_utc`
    // (05:00:00) — proving the topology recovered the true spawn instant.
    let h = topology_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // v0.5 flat rows: the built-in topo11 is its own `kind:"agent"` row.
    let rows = json_rows(&out.stdout, "agent");
    let builtin = rows
        .iter()
        .find(|v| v.get("agent_id").and_then(|a| a.as_str()) == Some("topo11"))
        .expect("the built-in topo11 row");
    assert_eq!(builtin["trigger_utc"], "2026-06-07T04:59:58.000Z");
    assert_eq!(builtin["started_utc"], "2026-06-07T05:00:00.000Z");
    assert_ne!(builtin["trigger_utc"], builtin["started_utc"]);
    assert_eq!(builtin["spawn_tool"], "Agent");
    assert_eq!(builtin["spawn_tool_use_id"], "toolu_x");
}

#[test]
fn agents_default_axis_is_trigger_not_start() {
    // A bound BETWEEN the trigger (04:59:58) and the start (05:00:00): the DEFAULT
    // (trigger) axis EXCLUDES the built-in (triggered before the bound); `--order-by start`
    // INCLUDES it (started after the bound). Proves the default flipped to trigger.
    let h = topology_home();
    let default_axis = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--since",
        "2026-06-07T04:59:59Z",
        "--shape",
        "builtin-task",
        "--format",
        "json",
    ]);
    assert!(default_axis.success, "stderr: {}", default_axis.stderr);
    let default_has_topo11 = default_axis.stdout.contains("topo11");
    assert!(
        !default_has_topo11,
        "default (trigger) axis must EXCLUDE topo11 triggered before the bound: {}",
        default_axis.stdout
    );
    let by_start = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--since",
        "2026-06-07T04:59:59Z",
        "--order-by",
        "start",
        "--shape",
        "builtin-task",
        "--format",
        "json",
    ]);
    assert!(by_start.success, "stderr: {}", by_start.stderr);
    assert!(
        by_start.stdout.contains("topo11"),
        "--order-by start must INCLUDE topo11 started after the bound: {}",
        by_start.stdout
    );
    // The footer reflects the default axis.
    let footer = h.run(&["agents", at(SESS).as_str()]);
    assert!(footer.stdout.contains("window-axis=trigger"));
}

#[test]
fn agents_returned_message_three_way_resolution() {
    let h = topology_home();
    let out = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--returned-message",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // v0.5 flat rows: builtin + workflow agents are both `kind:"agent"` rows; the run is
    // its own `kind:"run"` row (workflow_id joins them).
    let rows = json_rows(&out.stdout, "agent");
    let builtin = rows
        .iter()
        .find(|v| v["agent_id"] == "topo11")
        .expect("topo11");
    // SYNC built-in → parent tool_result text.
    assert_eq!(
        builtin["returned_message"],
        "SYNC-RETURN: the built-in carry answer"
    );
    assert_eq!(builtin["returned_message_source"], "sync-tool-result");
    let wf = rows
        .iter()
        .find(|v| v["agent_id"] == "topo22")
        .expect("topo22");
    // WORKFLOW → journal result payload.
    assert_eq!(wf["returned_message"], "WF-RETURN: journal payload");
    assert_eq!(wf["returned_message_source"], "workflow-journal");
    // The run row precedes its member agent row and carries the run metadata.
    let runs = json_rows(&out.stdout, "run");
    assert_eq!(runs.len(), 1, "{}", out.stdout);
    assert_eq!(runs[0]["run_id"], "wf_topo");
    assert_eq!(runs[0]["workflow_name"], "carry-wf");
    assert_eq!(wf["workflow_id"], "wf_topo", "join key intact: {wf}");
}

#[test]
fn agents_returned_message_omitted_by_default() {
    // Without --returned-message (and without --agent), the returned message is NOT in
    // the JSON — keeping a plain listing compact.
    let h = topology_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows = json_rows(&out.stdout, "agent");
    assert!(!rows.is_empty());
    assert!(
        rows.iter().all(|n| n.get("returned_message").is_none()),
        "returned_message must be omitted by default: {rows:?}"
    );
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
    // envelope v2 flat rows: header + session row + the one agent row + summary —
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
fn agents_tree_renders_workflow_run_as_parent_of_its_agents() {
    let h = topology_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // v0.5 flat rows: the run is a `kind:"run"` row; its member agent row FOLLOWS it
    // (emission order = run, then its agents pre-order, then built-ins).
    let runs = json_rows(&out.stdout, "run");
    assert_eq!(runs.len(), 1, "one workflow run");
    let run = &runs[0];
    assert_eq!(run["run_id"], "wf_topo");
    assert_eq!(run["workflow_name"], "carry-wf");
    assert_eq!(run["agent_count"], 1);
    let agents = json_rows(&out.stdout, "agent");
    let wf_member = agents
        .iter()
        .find(|a| a["agent_id"] == "topo22")
        .expect("workflow member row");
    assert_eq!(wf_member["workflow_id"], "wf_topo", "join key: {wf_member}");
    // The built-in (no workflow_id) is its own row with a null workflow_id.
    let builtin = agents
        .iter()
        .find(|a| a["agent_id"] == "topo11")
        .expect("builtin row");
    assert_eq!(builtin["workflow_id"], serde_json::Value::Null);
    // Emission order: run row before its member's agent row.
    let run_pos = out.stdout.find(r#""kind":"run""#).unwrap();
    let member_pos = out.stdout.find("topo22").unwrap();
    assert!(run_pos < member_pos, "run row precedes its members");

    // Text tree shows the WORKFLOW header with its run id + the nested agent.
    let text = h.run(&["agents", at(SESS).as_str()]);
    assert!(
        text.stdout.contains("WORKFLOW  wf_topo"),
        "got: {}",
        text.stdout
    );
    assert!(text.stdout.contains("[carry-wf]"));
    assert!(text.stdout.contains("topo22"));
}

#[test]
fn agents_tree_keeps_workflow_agents_without_a_run_manifest() {
    // A workflow dir can have a journal + agents BEFORE its top-level
    // `workflows/wf_*.json` run-manifest is written (an in-flight run), or after the
    // manifest is pruned. Such agents must NOT vanish from `--tree`: the tree renders a
    // workflow agent only as a child of a run, so without a synthesized stand-in run the
    // agent is silently dropped. Build a `wf_orphan` with an agent + journal but NO
    // manifest and assert the tree surfaces it. Regression: real session 0a1b2c3d's
    // in-flight `wf_132003a7-de2` (10 agents, journal, no manifest) was dropped from the
    // tree (552 of 562 agents) until this stand-in was added.
    let h = topology_home();
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_orphan/agent-topo33.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"topo33","timestamp":"2026-06-07T07:00:00.000Z","message":{"role":"user","content":"orphan wf"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T07:01:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"orphan done"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_orphan/agent-topo33.meta.json"),
        r#"{"agentType":"workflow-subagent"}"#,
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_orphan/journal.jsonl"),
        concat!(
            r#"{"type":"started","agentId":"topo33","key":"v2:orphan"}"#, "\n",
            r#"{"type":"result","agentId":"topo33","key":"v2:orphan","result":"ORPHAN-RETURN: in-flight payload"}"#, "\n",
        ),
    );
    // No `{ENC}/{SESS}/workflows/wf_orphan.json` manifest is written on purpose.

    // Discovery is lossless: the session has these three agents (topo11 built-in, topo22 in
    // the manifested run, topo33 in the manifest-less wf_orphan). The tree must surface ALL.
    let expected_ids = ["topo11", "topo22", "topo33"];

    // v0.5 flat rows: topo33 must still surface — a SYNTHESIZED `kind:"run"` row stands
    // in for the manifest-less wf_orphan (null run fields), and the agent rides as its
    // own `kind:"agent"` row joined by workflow_id.
    let tree = h.run(&["agents", at(SESS).as_str(), "--format", "json"]);
    assert!(tree.success, "stderr: {}", tree.stderr);
    let runs = json_rows(&tree.stdout, "run");
    let orphan_run = runs
        .iter()
        .find(|r| r["run_id"] == "wf_orphan")
        .expect("a synthesized run row for the manifest-less wf_orphan");
    assert!(orphan_run["status"].is_null(), "no manifest → null status");
    assert!(
        orphan_run["agent_count"].is_null(),
        "no manifest → null agent_count"
    );

    // No agent is lost: every discovered agent has its own row.
    let tree_ids: std::collections::BTreeSet<String> = json_rows(&tree.stdout, "agent")
        .iter()
        .filter_map(|a| a["agent_id"].as_str().map(String::from))
        .collect();
    for id in expected_ids {
        assert!(
            tree_ids.contains(id),
            "flat rows dropped agent {id} (rows={tree_ids:?})"
        );
    }
    let orphan_member = json_rows(&tree.stdout, "agent")
        .into_iter()
        .find(|a| a["agent_id"] == "topo33")
        .expect("topo33 row");
    assert_eq!(orphan_member["workflow_id"], "wf_orphan", "join key");

    // Text tree shows the orphan run header + the agent (not silently omitted).
    let text = h.run(&["agents", at(SESS).as_str()]);
    assert!(
        text.stdout.contains("WORKFLOW  wf_orphan"),
        "text tree must show the stand-in run header: {}",
        text.stdout
    );
    assert!(text.stdout.contains("topo33"), "got: {}", text.stdout);
}

#[test]
fn agents_id_form_is_bare_hex_joinable_across_files_and_recover() {
    // The subagent's id is the BARE hex everywhere: `agents` prints `topo11`, and
    // `files --session <subagent-hex>` / `recover` print the SAME bare hex (not the
    // `agent-` stem) — so a consumer can join file mutations back to the agent node.
    let h = topology_home();
    let agents_json = h.run(&["agents", at(SESS).as_str(), "--format", "json"]);
    assert!(agents_json.stdout.contains(r#""agent_id":"topo11""#));
    // files spans subagents by default; the subagent row's session_id is bare hex.
    let files_json = h.run(&[
        "files",
        at(SESS).as_str(),
        "--format",
        "json",
        "--by",
        "file",
    ]);
    assert!(files_json.success, "stderr: {}", files_json.stderr);
    assert!(
        files_json.stdout.contains(r#""session_id":"topo11""#)
            || files_json.stdout.contains("topo11"),
        "files must carry the bare-hex subagent id (joinable to agents): {}",
        files_json.stdout
    );
    assert!(
        !files_json.stdout.contains("agent-topo11"),
        "the un-stripped agent- stem must NOT appear: {}",
        files_json.stdout
    );
}
