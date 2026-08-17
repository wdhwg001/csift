//! agents topology: nesting, workflow runs, teammates, joinable ids.

use crate::harness::*;

#[test]
fn agents_classifies_teammate_and_id_round_trips() {
    // The NEW "teammate" subagent (taskKind:in_process_teammate). On disk it sits at the
    // built-in location (subagents/agent-<id>.jsonl) with a NAME-embedded id and a meta that
    // omits toolUseId + overloads agentType with the handle. csift must: (1) classify it as
    // `teammate`, (2) recover the real subagent_type + spawn linkage via the NAME-join to the
    // `Agent` tool_use, and (3) let the printed id round-trip as an `@<id>` target.
    let enc = "-Users-testuser-Projects-team";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let tid = "aVSRepro-68a2a1661c9390c1";
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"fix the slider"}}"#, "\n",
            // The Agent tool_use that spawned the teammate: input.name is the join key, and
            // subagent_type is the REAL type (the teammate meta only has the handle).
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_team","name":"Agent","input":{"description":"repro the bug","subagent_type":"oh-my-claudecode:qa-tester","name":"VSRepro"}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{tid}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aVSRepro-68a2a1661c9390c1","timestamp":"2026-06-07T05:00:01.500Z","message":{"role":"user","content":"<teammate-message teammate_id=\"team-lead\">repro the speed slider</teammate-message>"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:20:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"the multi-region matrix result"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{tid}.meta.json"),
        r#"{"agentType":"VSRepro","description":"repro the bug","name":"VSRepro","taskKind":"in_process_teammate","teamName":"session-25f56dee","color":"purple"}"#,
    );

    // (1)+(2) classification + name-join recovery, via JSON.
    let out = h.run(&["agents", &format!("@{sess}"), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let node = json_rows(&out.stdout, "agent")
        .into_iter()
        .find(|n| n.get("shape").and_then(|k| k.as_str()) == Some("teammate"))
        .expect("a teammate row in agents JSON");
    assert_eq!(node["agent_id"], tid);
    assert_eq!(node["agent_type"], "oh-my-claudecode:qa-tester"); // real type, not the handle
    assert_eq!(node["name"], "VSRepro");
    assert_eq!(node["team_name"], "session-25f56dee");
    assert_eq!(node["spawn_tool"], "Agent");
    assert_eq!(node["spawn_tool_use_id"], "toolu_team"); // recovered via the name-join
    assert_eq!(node["trigger_utc"], "2026-06-07T05:00:01.000Z"); // the TRUE spawn instant
                                                                 // The JSON node carries the control-mechanism pointer (the wrong-tool guard).
    let chint = node["control_hint"].as_str().unwrap_or("");
    assert!(
        chint.contains("SendMessage") && chint.contains("shutdown_request"),
        "teammate node missing control_hint: {node}"
    );

    // `--kind teammate` filters to it; text shows the team line.
    let kind = h.run(&["agents", &format!("@{sess}"), "--shape", "teammate"]);
    assert!(kind.success, "stderr: {}", kind.stderr);
    assert!(
        kind.stdout.contains(tid),
        "kind filter dropped it: {}",
        kind.stdout
    );
    assert!(kind.stdout.contains("teammate"));
    assert!(
        kind.stdout.contains("session-25f56dee"),
        "no team line: {}",
        kind.stdout
    );
    // The control-mechanism hint points at the CORRECT tool (SendMessage shutdown_request)
    // and warns off the wrong one (TaskStop) - the exact 30-min failure it prevents.
    assert!(
        kind.stdout.contains("SendMessage")
            && kind.stdout.contains("shutdown_request")
            && kind.stdout.contains("TaskStop"),
        "no teammate control hint: {}",
        kind.stdout
    );
    // A scope with NO teammate (filter to builtin-task; the fixture has none) → no hint noise.
    let bt = h.run(&["agents", &format!("@{sess}"), "--shape", "builtin-task"]);
    assert!(
        !bt.stdout.contains("shutdown_request"),
        "control hint must not appear without a teammate in scope: {}",
        bt.stdout
    );

    // (3) the printed id round-trips as an `@<id>` target (previously failed - fell through to
    // path resolution). search default-spans the teammate subtree and finds its content.
    let refed = h.run(&["search", "matrix result", &format!("@{tid}"), "-t", "agent"]);
    assert!(refed.success, "re-feed failed: {}", refed.stderr);
    assert!(
        refed.stdout.contains("multi-region matrix result"),
        "re-fed teammate search found nothing: {}",
        refed.stdout
    );
}

#[test]
fn agents_nested_subagent_topology_links_parent_depth_and_tree() {
    // A NESTED subagent (agent spawned BY another agent). On-disk the layout is FLAT - both
    // agents sit directly under <session>/subagents/ - because CC writes every subagent's
    // transcript under getSessionId()=<main> regardless of depth (verified vs the cleanroom).
    // The agent→agent link is LOGICAL: the child's spawning Task tool_use is recorded in the
    // PARENT's transcript (not main), and the child's meta.json toolUseId points at it.
    let enc = "-Users-testuser-Projects-nested";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let h = Home::new();
    // Main session: spawns PARENT via an Agent tool_use (id call_parent).
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_parent","name":"Agent","input":{"description":"parent agent","subagent_type":"general-purpose"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:09:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call_parent","content":"parent done"}]}}"#, "\n",
        ),
    );
    // PARENT transcript (flat under subagents/). It SPAWNS the child via an Agent tool_use
    // (id call_child) recorded HERE - this is the linkage a main-only scan would miss.
    h.write(
        &format!("{enc}/{sess}/subagents/agent-parentaaa.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"parentaaa","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"parent: do work"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_child","name":"Agent","input":{"description":"child agent","subagent_type":"Explore"}}]}}"#, "\n",
            r#"{"type":"user","timestamp":"2026-06-07T05:08:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call_child","content":"child done"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-parentaaa.meta.json"),
        r#"{"agentType":"general-purpose","description":"parent agent","toolUseId":"call_parent"}"#,
    );
    // CHILD transcript - FLAT in the SAME subagents/ dir (not nested on disk). Its meta
    // toolUseId=call_child points at the spawn recorded in PARENT's transcript.
    h.write(
        &format!("{enc}/{sess}/subagents/agent-childbbb.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"childbbb","timestamp":"2026-06-07T05:01:30.000Z","message":{"role":"user","content":"child: explore"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:07:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"child result"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-childbbb.meta.json"),
        r#"{"agentType":"Explore","description":"child agent","toolUseId":"call_child"}"#,
    );

    // JSON (v0.5 FLAT rows): one `kind:"agent"` row per node in tree PRE-ORDER - the
    // child follows its parent, links via parent_agent_id, carries depth 1, and the
    // child's trigger/description come from the PARENT transcript's spawn (not main).
    let j = h.run(&["agents", at(sess).as_str(), "--format", "json"]);
    assert!(j.success, "stderr: {}", j.stderr);
    let agents = json_rows(&j.stdout, "agent");
    let parent = agents
        .iter()
        .find(|o| o["agent_id"] == "parentaaa")
        .expect("parent row");
    let child = agents
        .iter()
        .find(|o| o["agent_id"] == "childbbb")
        .expect("child row");
    assert_eq!(
        child["parent_agent_id"], "parentaaa",
        "child links to parent: {child}"
    );
    assert_eq!(
        child["depth"],
        serde_json::json!(1),
        "child depth 1: {child}"
    );
    assert_eq!(
        parent["parent_agent_id"],
        serde_json::Value::Null,
        "parent is a root: {parent}"
    );
    assert_eq!(
        parent["depth"],
        serde_json::json!(0),
        "parent depth 0: {parent}"
    );
    // Pre-order: the parent row precedes the child row; no children[] nesting in JSON.
    let pi = agents
        .iter()
        .position(|o| o["agent_id"] == "parentaaa")
        .unwrap();
    let ci = agents
        .iter()
        .position(|o| o["agent_id"] == "childbbb")
        .unwrap();
    assert!(pi < ci, "pre-order: parent before child");
    assert!(
        agents.iter().all(|o| o.get("children").is_none()),
        "flat rows carry no children[]: {agents:?}"
    );
    // The session row is a light counts-only grouping marker.
    let sess_row = json_rows(&j.stdout, "session").remove(0);
    assert_eq!(sess_row["agents"], serde_json::json!(2), "{sess_row}");

    // Tree TEXT (always on): child indented one level deeper than parent.
    let tt = h.run(&["agents", at(sess).as_str()]);
    assert!(tt.success, "stderr: {}", tt.stderr);
    let pidx = tt.stdout.find("parentaaa").expect("parent in tree text");
    let cidx = tt.stdout.find("childbbb").expect("child in tree text");
    assert!(cidx > pidx, "child printed after parent: {}", tt.stdout);
    // child line has more leading spaces than the parent line
    let line_indent = |needle: &str| -> usize {
        let li = tt.stdout[..tt.stdout.find(needle).unwrap()]
            .rfind('\n')
            .map_or(0, |p| p + 1);
        tt.stdout[li..].chars().take_while(|c| *c == ' ').count()
    };
    assert!(
        line_indent("childbbb") > line_indent("parentaaa"),
        "child is indented deeper than parent: {}",
        tt.stdout
    );
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

    // v0.5 flat rows: topo33 must still surface - a SYNTHESIZED `kind:"run"` row stands
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
    // `agent-` stem) - so a consumer can join file mutations back to the agent node.
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
