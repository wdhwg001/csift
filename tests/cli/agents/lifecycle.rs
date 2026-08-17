//! agents lifecycle: frozen lanes, returned messages, trigger-time truth.

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
