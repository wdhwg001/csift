//! Spawn indexing and tree reconstruction: id-joins, name-joins, depth, recursion guard.

use super::*;

#[test]
fn classifies_teammate_and_recovers_spawn_via_name_join() {
    let fx = Fixture::new();
    let session = teammate_layout(&fx);

    // Discovery + classification: the in_process_teammate taskKind upgrades the kind.
    let subs = discover_subagents(&session).unwrap();
    assert_eq!(subs.len(), 1, "got: {subs:?}");
    let s = &subs[0];
    assert_eq!(s.kind, SubagentKind::Teammate);
    assert_eq!(s.agent_id, "aVSRepro-68a2a1661c9390c1");
    assert_eq!(s.name.as_deref(), Some("VSRepro"));
    assert_eq!(s.team_name.as_deref(), Some("session-25f56dee"));
    // The teammate meta carries NO toolUseId — the id-join would find nothing.
    assert_eq!(s.spawn_tool_use_id, None);

    // The full node: the NAME-join recovers the spawn linkage the id-join can't.
    let nodes = build_topology(&session, false).unwrap();
    let n = nodes
        .iter()
        .find(|n| n.kind == SubagentKind::Teammate)
        .expect("the teammate node");
    // agent_type prefers the spawn's REAL subagent_type over the meta's overloaded handle.
    assert_eq!(n.agent_type.as_deref(), Some("oh-my-claudecode:qa-tester"));
    assert_eq!(n.spawn_tool.as_deref(), Some("Agent"));
    assert_eq!(n.spawn_tool_use_id.as_deref(), Some("toolu_team"));
    // trigger = the Agent tool_use ts (the TRUE spawn instant), earlier than the child head.
    assert_eq!(n.trigger_utc.as_deref(), Some("2026-06-07T05:00:00.000Z"));
    assert_eq!(n.started_utc.as_deref(), Some("2026-06-07T05:00:00.500Z"));
    assert_eq!(n.name.as_deref(), Some("VSRepro"));
    assert_eq!(n.team_name.as_deref(), Some("session-25f56dee"));
    // The returned message still resolves (child tail), unaffected by the name-join.
    assert_eq!(n.returned_message.as_deref(), Some("the matrix result"));
}

#[test]
fn defensive_recursion_catches_a_hypothetical_nested_sub_sub_agent() {
    // The REAL layout is flat (no sub-sub-agents exist on disk — verified across 2348
    // transcripts). This test fabricates the FUTURE nested layout the defensive walk
    // insures against: a child transcript under
    // `subagents/agent-<hex>/subagents/agent-<hex>.jsonl`. (A)/(B) alone would drop it;
    // the bounded recursive walk must discover it as a built-in subagent.
    let fx = Fixture::new();
    let session = layout(&fx);
    let enc = "-Users-testuser-Projects-foo";
    // A nested sub-sub-agent transcript two `subagents/` levels deep.
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aaa111/subagents/agent-ccc333.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"ccc333\",\"timestamp\":\"2026-06-07T07:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"nested task\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T07:01:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"nested done\"}]}}\n"
            ),
        );
    let subs = discover_subagents(&session).unwrap();
    // The two original transcripts PLUS the nested one — none dropped, none duplicated.
    assert_eq!(
        subs.len(),
        3,
        "nested sub-sub-agent must be discovered: {subs:?}"
    );
    let nested = subs
        .iter()
        .find(|s| s.agent_id == "ccc333")
        .expect("the nested sub-sub-agent");
    // Classified by path location: under a `subagents/` dir ⇒ BuiltinTask.
    assert_eq!(nested.kind, SubagentKind::BuiltinTask);
    assert!(nested.path.ends_with("agent-ccc333.jsonl"));
    // No path is double-counted (dedup-by-absolute-path holds).
    let mut paths: Vec<_> = subs.iter().map(|s| s.path.clone()).collect();
    paths.sort();
    let n = paths.len();
    paths.dedup();
    assert_eq!(paths.len(), n, "no duplicate paths");
}

#[test]
fn defensive_recursion_does_not_change_the_flat_real_layout() {
    // The insurance must be a NO-OP on the real flat layout: exactly the (A)+(B) two,
    // no spurious extra rows from the recursive walk over the normal tree.
    let fx = Fixture::new();
    let session = layout(&fx);
    let subs = discover_subagents(&session).unwrap();
    assert_eq!(subs.len(), 2, "flat layout unchanged by the defensive walk");
}

// ───────────────────── TOPOLOGY (Part A) tests ─────────────────────

#[test]
fn index_parent_spawns_finds_agent_and_workflow_tool_uses() {
    let fx = Fixture::new();
    let session = layout(&fx);
    let idx = index_parent_spawns(&session).unwrap();
    // Both spawns indexed: the Agent (toolu_x) + the Workflow (toolu_w). Task is
    // matched defensively but absent in this fixture (so it stays unindexed).
    let agent = idx.spawn("toolu_x").expect("the Agent spawn");
    assert_eq!(agent.name.as_deref(), Some("Agent"));
    assert_eq!(
        agent.trigger_utc.as_deref(),
        Some("2026-06-07T04:59:58.000Z")
    );
    assert_eq!(
        agent.subagent_type.as_deref(),
        Some("oh-my-claudecode:executor")
    );
    // The paired SYNC tool_result is indexed as the returned-message source.
    assert_eq!(
        idx.tool_result_text("toolu_x"),
        Some("SYNC RETURN: the built-in answer")
    );
    let wf = idx.spawn("toolu_w").expect("the Workflow spawn");
    assert_eq!(wf.name.as_deref(), Some("Workflow"));
}

#[test]
fn index_parent_spawns_empty_for_empty_parent_transcript() {
    // An EMPTY parent jsonl (mmap → Ok(None)) → an empty index (degrade, never error).
    // This is the real graceful path; a TRULY-missing file is a genuine I/O error
    // (and `build_topology` only indexes a session whose file exists).
    let fx = Fixture::new();
    let empty = fx.write("-Users-empty/empty-session.jsonl", "");
    let idx = index_parent_spawns(&empty).unwrap();
    assert!(idx.spawn("anything").is_none());
    assert!(idx.tool_result_text("anything").is_none());
}

#[test]
fn journal_result_captures_the_payload_not_just_a_bool() {
    let fx = Fixture::new();
    let session = layout(&fx);
    let subs = discover_subagents(&session).unwrap();
    let wf = subs
        .iter()
        .find(|s| s.kind == SubagentKind::Workflow)
        .unwrap();
    assert_eq!(
        journal_result(wf, &JournalCache::build(std::slice::from_ref(wf))).as_deref(),
        Some("WF RETURN: workflow journal payload")
    );
    // A built-in has no journal → None.
    let builtin = subs
        .iter()
        .find(|s| s.kind == SubagentKind::BuiltinTask)
        .unwrap();
    assert!(journal_result(builtin, &JournalCache::build(std::slice::from_ref(builtin))).is_none());
}

#[test]
fn build_topology_links_trigger_time_and_sync_returned_message() {
    let fx = Fixture::new();
    let session = layout(&fx);
    let nodes = build_topology(&session, false).unwrap();
    assert_eq!(nodes.len(), 2);
    let builtin = nodes
        .iter()
        .find(|n| n.kind == SubagentKind::BuiltinTask)
        .unwrap();
    // TRUE trigger = the parent Agent tool_use ts (04:59:58), NOT the child-head ts
    // (05:00:00) — they DIVERGE, proving the trigger axis is real.
    assert_eq!(
        builtin.trigger_utc.as_deref(),
        Some("2026-06-07T04:59:58.000Z")
    );
    assert_eq!(
        builtin.started_utc.as_deref(),
        Some("2026-06-07T05:00:00.000Z")
    );
    assert_ne!(builtin.trigger_utc, builtin.started_utc);
    assert_eq!(builtin.spawn_tool.as_deref(), Some("Agent"));
    // SYNC built-in → the returned message is the parent tool_result text.
    assert_eq!(
        builtin.returned_message.as_deref(),
        Some("SYNC RETURN: the built-in answer")
    );
    assert_eq!(
        builtin.returned_message_source,
        Some(ReturnedMsgSource::SyncToolResult)
    );
}

#[test]
fn build_topology_resolves_workflow_returned_message_from_journal() {
    let fx = Fixture::new();
    let session = layout(&fx);
    let nodes = build_topology(&session, false).unwrap();
    let wf = nodes
        .iter()
        .find(|n| n.kind == SubagentKind::Workflow)
        .unwrap();
    // Workflow → the journal `result` payload, NOT the parent Workflow-tool echo.
    assert_eq!(
        wf.returned_message.as_deref(),
        Some("WF RETURN: workflow journal payload")
    );
    assert_eq!(
        wf.returned_message_source,
        Some(ReturnedMsgSource::WorkflowJournal)
    );
}

#[test]
fn async_launch_falls_back_to_child_transcript_tail() {
    // A built-in whose parent tool_result is the `Async agent launched …` sentinel
    // must resolve its returned message from the CHILD transcript tail.
    let fx = Fixture::new();
    let enc = "-Users-async";
    let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            concat!(
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T04:00:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_a\",\"name\":\"Agent\",\"input\":{\"run_in_background\":true}}]}}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a\",\"content\":\"Async agent launched successfully.\\nagentId: zzz999\"}]}}\n"
            ),
        );
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-zzz999.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"zzz999\",\"timestamp\":\"2026-06-07T04:00:05.000Z\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T04:05:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"ASYNC TAIL: the real async answer\"}]}}\n"
            ),
        );
    fx.write(
        &format!("{enc}/{SESS}/subagents/agent-zzz999.meta.json"),
        "{\"agentType\":\"general-purpose\",\"toolUseId\":\"toolu_a\"}",
    );
    let nodes = build_topology(&session, false).unwrap();
    assert_eq!(nodes.len(), 1);
    let n = &nodes[0];
    assert_eq!(
        n.returned_message.as_deref(),
        Some("ASYNC TAIL: the real async answer")
    );
    assert_eq!(
        n.returned_message_source,
        Some(ReturnedMsgSource::AsyncChildTail)
    );
}

#[test]
fn build_topology_with_files_attaches_node_files_changed() {
    // A built-in whose transcript edits a file → its files_changed lists that path.
    let fx = Fixture::new();
    let enc = "-Users-nodefiles";
    let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T04:00:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_f\",\"name\":\"Agent\",\"input\":{}}]}}\n",
        );
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-fff111.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"fff111\",\"timestamp\":\"2026-06-07T04:00:05.000Z\",\"message\":{\"role\":\"user\",\"content\":\"edit\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T04:01:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"e1\",\"name\":\"Edit\",\"input\":{\"file_path\":\"/repo/src/lib.rs\"}}]}}\n"
            ),
        );
    fx.write(
        &format!("{enc}/{SESS}/subagents/agent-fff111.meta.json"),
        "{\"agentType\":\"general-purpose\",\"toolUseId\":\"toolu_f\"}",
    );
    let nodes = build_topology(&session, true).unwrap();
    assert_eq!(nodes.len(), 1);
    let files = &nodes[0].files_changed;
    assert_eq!(files.len(), 1, "got: {files:?}");
    assert_eq!(files[0].0, "/repo/src/lib.rs");
    assert_eq!(files[0].1, "edit");
    // with_files=false leaves it empty (the cheap default).
    let lean = build_topology(&session, false).unwrap();
    assert!(lean[0].files_changed.is_empty());
}
