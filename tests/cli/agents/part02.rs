use crate::harness::*;

#[test]
fn agents_frozen_lane_reports_escalation_blocked_not_completed() {
    // A background built-in subagent whose teardown Bash (dangerous `rm` of `$VAR/$f`) CC HOISTED
    // to a human approval prompt EVEN under bypass — its transcript freezes at the unreturned
    // tool_use, PRECEDED by assistant text (the L629→L630 shape that made the old walk-back
    // mis-report `completed`). csift must report it running + escalation-blocked, then NOT pending
    // once the result lands (Yes clicked). Mirrors the real fixture agent-ab8a4c5868015a8be.
    let enc = "-Users-testuser-Projects-frozen";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let hex = "ab8a4c5868015a8be";
    let frozen = concat!(
        r#"{"type":"user","isSidechain":true,"agentId":"ab8a4c5868015a8be","timestamp":"2026-06-26T10:40:00.000Z","message":{"role":"user","content":"teardown"}}"#,
        "\n",
        r#"{"type":"assistant","timestamp":"2026-06-26T10:42:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Removing the transient credential files."}]}}"#,
        "\n",
        r#"{"type":"assistant","timestamp":"2026-06-26T10:43:31.906Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_0137gHdLDnXKsa94qGmmnbqV","name":"Bash","input":{"command":"for f in a.txt b.txt; do [ -f \"$SCRATCH/$f\" ] && rm -f \"$SCRATCH/$f\"; done"}}]}}"#,
        "\n",
    );
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        "{\"type\":\"user\",\"timestamp\":\"2026-06-26T09:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n",
    );
    h.write(&format!("{enc}/{sess}/subagents/agent-{hex}.jsonl"), frozen);

    let find = |stdout: &str| -> serde_json::Value {
        json_rows(stdout, "agent")
            .into_iter()
            .find(|n| n["agent_id"] == hex)
            .expect("the subagent row")
    };

    // FROZEN: running + escalation-blocked, NOT completed.
    let out = h.run(&["agents", &format!("@{sess}"), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let node = find(&out.stdout);
    assert_eq!(
        node["status"], "running",
        "a frozen lane must not be completed: {node}"
    );
    assert_eq!(node["pending_classification"], "escalation-blocked");
    assert_eq!(node["pending_tool_name"], "Bash");
    assert_eq!(
        node["pending_tool_use_id"],
        "toolu_0137gHdLDnXKsa94qGmmnbqV"
    );
    // completed_* is STATUS-GATED: a frozen lane carries no completion instant (the naive
    // `if completed_utc: done` consumer must not false-positive); its tail ts lives in
    // last_activity_* and equals the freeze instant.
    assert!(
        node["completed_utc"].is_null() && node["completed_local"].is_null(),
        "a frozen lane must not carry a completion instant: {node}"
    );
    assert!(
        node["duration"].is_null(),
        "no duration while frozen: {node}"
    );
    assert_eq!(node["last_activity_utc"], "2026-06-26T10:43:31.906Z");
    assert_eq!(node["last_activity_utc"], node["pending_since_utc"]);
    // Text surfaces the disambiguation prominently — and no "completed"/"last-seen" line
    // (the PENDING line already carries the freeze instant).
    let txt = h.run(&["agents", &format!("@{sess}")]);
    assert!(
        txt.stdout.contains("PENDING") && txt.stdout.contains("escalation-blocked"),
        "no pending line: {}",
        txt.stdout
    );
    // Mutation pins: the PENDING detail line sits one indent level UNDER its node head,
    // and the escalation class (and only that class) carries the dangerous-rm explainer.
    assert!(
        txt.stdout.contains("\n    PENDING"),
        "PENDING must be an indented detail line: {}",
        txt.stdout
    );
    assert!(
        txt.stdout.contains("HOISTS"),
        "escalation-blocked carries the hoist explainer: {}",
        txt.stdout
    );
    assert!(
        !txt.stdout.contains("completed  2026") && !txt.stdout.contains("last-seen"),
        "frozen lane must not print a terminal-instant line: {}",
        txt.stdout
    );

    // RESOLVED (Yes clicked → tool_result + closing text) → completed, no pending.
    let resolved = format!(
        "{frozen}{}{}",
        "{\"type\":\"user\",\"timestamp\":\"2026-06-26T11:20:13.911Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_0137gHdLDnXKsa94qGmmnbqV\",\"content\":\"shredded\"}]}}\n",
        "{\"type\":\"assistant\",\"timestamp\":\"2026-06-26T11:21:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Teardown complete.\"}]}}\n"
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{hex}.jsonl"),
        &resolved,
    );
    let out2 = h.run(&["agents", &format!("@{sess}"), "--format", "json"]);
    let node2 = find(&out2.stdout);
    assert_eq!(node2["status"], "completed");
    assert!(
        node2["pending_classification"].is_null(),
        "resolved lane must not be pending: {node2}"
    );
    // Completed lane: completion instant present and == last activity.
    assert_eq!(node2["completed_utc"], "2026-06-26T11:21:00.000Z");
    assert_eq!(node2["last_activity_utc"], node2["completed_utc"]);
}

#[test]
fn agents_returned_message_on_open_lane_carries_history_caution() {
    // R8: a frozen teammate's newest returned message read like a clean finale ("work is
    // complete, confirming shutdown") and nearly fooled a real reader. On a NON-completed
    // lane the text render brands the message as history inline; a completed lane stays
    // unbranded.
    let enc = "-Users-testuser-Projects-rmcaution";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let hex = "ab8a4c5868015a8be";
    let frozen = concat!(
        r#"{"type":"user","isSidechain":true,"agentId":"ab8a4c5868015a8be","timestamp":"2026-06-26T10:40:00.000Z","message":{"role":"user","content":"teardown"}}"#,
        "\n",
        r#"{"type":"assistant","timestamp":"2026-06-26T10:42:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Work is complete — confirming the shutdown request."}]}}"#,
        "\n",
        r#"{"type":"assistant","timestamp":"2026-06-26T10:43:31.906Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_0137gHdLDnXKsa94qGmmnbqV","name":"Bash","input":{"command":"echo wait"}}]}}"#,
        "\n",
    );
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        "{\"type\":\"user\",\"timestamp\":\"2026-06-26T09:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n",
    );
    h.write(&format!("{enc}/{sess}/subagents/agent-{hex}.jsonl"), frozen);

    // FROZEN lane: the returned line must carry the inline history caution.
    let out = h.run(&["agents", &format!("@{sess}"), "--returned-message"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("history — predates the still-open lane, NOT the outcome"),
        "open-lane returned message must be branded as history: {}",
        out.stdout
    );

    // RESOLVED (tool_result + closing text) → completed lane, no caution.
    let resolved = format!(
        "{frozen}{}{}",
        "{\"type\":\"user\",\"timestamp\":\"2026-06-26T11:20:13.911Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_0137gHdLDnXKsa94qGmmnbqV\",\"content\":\"ok\"}]}}\n",
        "{\"type\":\"assistant\",\"timestamp\":\"2026-06-26T11:21:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Teardown complete.\"}]}}\n"
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{hex}.jsonl"),
        &resolved,
    );
    let out2 = h.run(&["agents", &format!("@{sess}"), "--returned-message"]);
    assert!(
        out2.stdout.contains("returned") && !out2.stdout.contains("predates the still-open lane"),
        "a completed lane's returned message must stay unbranded: {}",
        out2.stdout
    );
}

#[test]
fn agents_running_not_frozen_prints_last_seen_not_completed() {
    // A lane whose NEWEST meaningful record is a returned tool_result with NO closing
    // assistant text: not frozen (nothing pending), not completed (no terminal message) —
    // the honest middle. Its tail instant must surface as last_activity/"last-seen",
    // NEVER as a fabricated completion.
    let enc = "-Users-testuser-Projects-midflight";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let hex = "beef4c5868015a8be";
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        "{\"type\":\"user\",\"timestamp\":\"2026-06-26T09:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n",
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{hex}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"beef4c5868015a8be","timestamp":"2026-06-26T10:00:00.000Z","message":{"role":"user","content":"scan"}}"#,
            "\n",
            r#"{"type":"assistant","timestamp":"2026-06-26T10:01:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_mid1","name":"Bash","input":{"command":"ls"}}]}}"#,
            "\n",
            r#"{"type":"user","timestamp":"2026-06-26T10:02:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_mid1","content":"ok"}]}}"#,
            "\n",
        ),
    );
    let out = h.run(&["agents", &format!("@{sess}"), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let node = json_rows(&out.stdout, "agent")
        .into_iter()
        .find(|n| n["agent_id"] == hex)
        .expect("the subagent row");
    assert_eq!(node["status"], "running", "honest middle: {node}");
    assert!(node["pending_classification"].is_null(), "{node}");
    assert!(
        node["completed_utc"].is_null() && node["duration"].is_null(),
        "running lane must not claim completion: {node}"
    );
    assert_eq!(node["last_activity_utc"], "2026-06-26T10:02:00.000Z");
    let txt = h.run(&["agents", &format!("@{sess}")]);
    assert!(
        txt.stdout.contains("last-seen") && !txt.stdout.contains("completed  2026"),
        "text must print last-seen, not completed: {}",
        txt.stdout
    );
}

#[test]
fn agents_nested_subagent_topology_links_parent_depth_and_tree() {
    // A NESTED subagent (agent spawned BY another agent). On-disk the layout is FLAT — both
    // agents sit directly under <session>/subagents/ — because CC writes every subagent's
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
    // (id call_child) recorded HERE — this is the linkage a main-only scan would miss.
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
    // CHILD transcript — FLAT in the SAME subagents/ dir (not nested on disk). Its meta
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

    // JSON (v0.5 FLAT rows): one `kind:"agent"` row per node in tree PRE-ORDER — the
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
fn agents_text_returned_files_and_tree_render() {
    // Exercise the TEXT-render branches for `--returned-message` / `--with-files` + the
    // always-on tree topology (the print_node `returned`/`files`/workflow-run arms) and the
    // one_line returned-message preview path. A node with no resolvable returned message
    // renders `(unresolved)`; a node with no files renders `files (none)`.
    let h = populated_home();
    let out = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--returned-message",
        "--with-files",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The returned-message line renders (resolved or `(unresolved)`).
    assert!(
        out.stdout.contains("returned"),
        "returned line missing: {}",
        out.stdout
    );
    // The with-files line renders (`files … changed` or `files (none)`).
    assert!(
        out.stdout.contains("files"),
        "files line missing: {}",
        out.stdout
    );
    // Tree topology: the workflow run parents its agent.
    assert!(
        out.stdout.contains("wf_abc") || out.stdout.contains("workflow"),
        "tree topology missing: {}",
        out.stdout
    );
}

#[test]
fn agents_kind_filter_json_and_tree_json_and_multi_node_text() {
    // `--kind builtin-task --format json` hits the BuiltinTask JSON-label arm; v0.5 JSON
    // is FLAT kind-tagged rows (no children[] nesting — the tree lives in TEXT mode and
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
fn agents_clean_run_text_hygiene() {
    // Mutation pins on the tree renderer: a single-session run has NO leading blank line
    // and no blank-before-first-SESSION; a corpus with no teammates prints NO teammate
    // control hint; a clean lane never prints a zero-count malformed note.
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.starts_with('\n') && !out.stdout.contains("\n\nSESSION"),
        "no blank line ahead of the first session: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("teammate rows are in-process"),
        "teammate control hint must be gated on a teammate being present: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("0 malformed"),
        "a clean lane never prints a zero-count malformed note: {}",
        out.stdout
    );
}

#[test]
fn image_spans_subagents_by_default_and_restricts() {
    // Same span-contract pin for `image`: an image carried ONLY by a subagent transcript is
    // listed by default and disappears under --no-subagents.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"no images up here"}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-sub222.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"sub222","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"[Image #1] look"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgo="}}]}}"#, "\n",
        ),
    );
    let span = h.run(&["image", at(SESS).as_str()]);
    assert!(span.success, "stderr: {}", span.stderr);
    assert!(
        span.stdout.contains("png"),
        "image spans subagents by default: {}",
        span.stdout
    );
    let top = h.run(&["image", at(SESS).as_str(), "--no-subagents"]);
    assert!(top.success, "stderr: {}", top.stderr);
    assert!(
        !top.stdout.contains("png"),
        "--no-subagents restricts image to the top level: {}",
        top.stdout
    );
}

#[test]
fn plan_spans_subagents_by_default_and_restricts() {
    // Same span-contract pin for `plan`: a plan_mode binding carried ONLY by a subagent
    // transcript resolves by default and disappears under --no-subagents.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"no plan up here"}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-sub333.jsonl"),
        concat!(
            r#"{"type":"attachment","isSidechain":true,"agentId":"sub333","attachment":{"type":"plan_mode","reminderType":"full","isSubAgent":true,"planFilePath":"/p/plans/quiet-harbor-relay.md","planExists":false},"uuid":"att1","timestamp":"2026-06-07T05:00:01.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}"#, "\n",
        ),
    );
    let span = h.run(&["plan", at(SESS).as_str()]);
    assert!(span.success, "stderr: {}", span.stderr);
    assert!(
        span.stdout.contains("quiet-harbor-relay"),
        "plan spans subagents by default: {}",
        span.stdout
    );
    let top = h.run(&["plan", at(SESS).as_str(), "--no-subagents"]);
    assert!(
        !top.stdout.contains("quiet-harbor-relay"),
        "--no-subagents restricts plan to the top level: {}",
        top.stdout
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
