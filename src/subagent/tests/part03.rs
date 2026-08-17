use super::*;

#[test]
fn frozen_lane_classifies_escalation_blocked_vs_awaiting_execution() {
    let fx = Fixture::new();
    let enc = "-Users-frozen";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n",
    );
    // (1) FROZEN at a dangerous-rm Bash (unreturned), PRECEDED by assistant TEXT — the exact
    // L629→L630 shape that made the old walk-back mis-report `completed`. → escalation-blocked.
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aesc111.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"aesc111\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"teardown\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:01:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Now removing the scratch files.\"}]}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:02:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_rm\",\"name\":\"Bash\",\"input\":{\"command\":\"for f in a b; do rm -rf \\\"$SCRATCH/$f\\\"; done\"}}]}}\n"
            ),
        );
    // (2) FROZEN at a non-danger tool_use (Read, unreturned) → awaiting-execution.
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-await22.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"await22\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"read\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:02:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_read\",\"name\":\"Read\",\"input\":{\"file_path\":\"/x/big.log\"}}]}}\n"
            ),
        );
    // (3) RESOLVED: a dangerous Bash whose tool_result + closing text arrived → NOT pending.
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-done333.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"done333\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"teardown\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:02:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_rm2\",\"name\":\"Bash\",\"input\":{\"command\":\"rm -rf $SCRATCH/*\"}}]}}\n",
                "{\"type\":\"user\",\"timestamp\":\"2026-06-07T05:40:00.000Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_rm2\",\"content\":\"done\"}]}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:41:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Teardown complete.\"}]}}\n"
            ),
        );

    let nodes = build_topology(&session, false).unwrap();
    let esc = nodes.iter().find(|n| n.agent_id == "aesc111").unwrap();
    // The frozen escalation lane is RUNNING (not completed — the bug) + escalation-blocked.
    assert_eq!(esc.status, SubagentStatus::Running);
    assert_eq!(
        esc.pending_classification,
        Some(PendingClassification::EscalationBlocked)
    );
    assert_eq!(esc.pending_tool_name.as_deref(), Some("Bash"));
    assert_eq!(esc.pending_tool_use_id.as_deref(), Some("toolu_rm"));
    assert_eq!(
        esc.pending_since_utc.as_deref(),
        Some("2026-06-07T05:02:00.000Z")
    );

    let awa = nodes.iter().find(|n| n.agent_id == "await22").unwrap();
    assert_eq!(awa.status, SubagentStatus::Running);
    assert_eq!(
        awa.pending_classification,
        Some(PendingClassification::AwaitingExecution)
    );
    assert_eq!(awa.pending_tool_name.as_deref(), Some("Read"));

    let done = nodes.iter().find(|n| n.agent_id == "done333").unwrap();
    assert_eq!(done.status, SubagentStatus::Completed);
    assert!(done.pending_classification.is_none());
}

#[test]
fn discover_workflow_runs_reads_top_level_manifests() {
    let fx = Fixture::new();
    let session = layout(&fx);
    let runs = discover_workflow_runs(&session).unwrap();
    assert_eq!(runs.len(), 1);
    let r = &runs[0];
    assert_eq!(r.run_id, "wf_abc");
    assert_eq!(r.task_id.as_deref(), Some("t9"));
    assert_eq!(r.workflow_name.as_deref(), Some("demo-wf"));
    assert_eq!(r.status.as_deref(), Some("completed"));
    assert_eq!(r.agent_count, Some(1));
    assert_eq!(r.duration_ms, Some(62000));
    assert_eq!(r.total_tokens, Some(12345));
    assert_eq!(r.total_tool_calls, Some(7));
    assert_eq!(r.default_model.as_deref(), Some("claude-opus-4-8[1m]"));
    // The run_id matches the subagents/workflows/wf_abc/ dir → joins to its agent.
    let nodes = build_topology(&session, false).unwrap();
    assert!(nodes
        .iter()
        .any(|n| n.workflow_id.as_deref() == Some("wf_abc")));
}

#[test]
fn discover_workflow_runs_empty_without_workflows_dir() {
    // A session whose sidecar has no top-level workflows/ dir → empty, not an error.
    let fx = Fixture::new();
    let enc = "-Users-nowf";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-q.jsonl"),
            "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"q\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
        );
    assert!(discover_workflow_runs(&session).unwrap().is_empty());
}

#[test]
fn discover_workflow_runs_ignores_non_manifest_entries() {
    // The `workflows/scripts/` subdir + a non-wf_*.json file must be skipped.
    let fx = Fixture::new();
    let session = layout(&fx);
    let enc = "-Users-testuser-Projects-foo";
    fx.write(&format!("{enc}/{SESS}/workflows/scripts/x.js"), "noop");
    fx.write(&format!("{enc}/{SESS}/workflows/not-a-manifest.txt"), "x");
    // Still exactly the one real wf_abc.json manifest.
    let runs = discover_workflow_runs(&session).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].run_id, "wf_abc");
}

#[test]
fn bare_agent_id_strips_prefix_only_when_present() {
    // The one rule, shared by recover/session/files: a subagent stem loses `agent-`;
    // a top-level uuid (no prefix) is unchanged.
    assert_eq!(bare_agent_id("agent-aaa111"), "aaa111");
    assert_eq!(
        bare_agent_id("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"),
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
    );
}

#[test]
fn session_id_from_path_is_canonical_bare_hex() {
    // The SINGLE per-file id derivation every surface (list/search/files/recover/
    // turns) now routes through, so the same transcript reports an IDENTICAL id
    // whichever subcommand prints it. A subagent stem loses its `agent-` prefix; a
    // top-level uuid passes through; a stem-less path yields an empty string.
    assert_eq!(
        session_id_from_path(Path::new(
            "/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d/subagents/agent-a585e25a580c59e7a.jsonl"
        )),
        "a585e25a580c59e7a"
    );
    assert_eq!(
        session_id_from_path(Path::new(
            "/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jsonl"
        )),
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
    );
    // A root path with no file stem → empty (never panics).
    assert_eq!(session_id_from_path(Path::new("/")), "");
}

#[test]
fn parent_session_id_and_is_subagent_from_path() {
    let sub = Path::new(
        "/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d/subagents/agent-a585e25a580c59e7a.jsonl",
    );
    let wf = Path::new(
        "/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d/subagents/workflows/wf_abc/agent-aaa.jsonl",
    );
    let top = Path::new("/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jsonl");
    // A subagent path → parent is the dir before `subagents`, and is_subagent is true.
    assert_eq!(
        parent_session_id_from_path(sub).as_deref(),
        Some("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d")
    );
    assert!(is_subagent_path(sub));
    // A workflow subagent path → same parent (the segment before `subagents`).
    assert_eq!(
        parent_session_id_from_path(wf).as_deref(),
        Some("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d")
    );
    assert!(is_subagent_path(wf));
    // A top-level path → no parent (it IS its own session), is_subagent false.
    assert_eq!(parent_session_id_from_path(top), None);
    assert!(!is_subagent_path(top));
}
