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
fn discovers_both_kinds_excludes_journal() {
    let fx = Fixture::new();
    let session = layout(&fx);
    let subs = discover_subagents(&session).unwrap();
    // Exactly two transcripts; the journal must NOT appear.
    assert_eq!(subs.len(), 2, "got: {subs:?}");
    let builtin = subs
        .iter()
        .find(|s| s.kind == SubagentKind::BuiltinTask)
        .expect("a builtin");
    // The id is the bare hex (record/journal `agentId`), NOT the `agent-` stem.
    assert_eq!(builtin.agent_id, "aaa111");
    let wf = subs
        .iter()
        .find(|s| s.kind == SubagentKind::Workflow)
        .expect("a workflow");
    assert_eq!(wf.agent_id, "bbb222");
    assert_eq!(wf.workflow_id.as_deref(), Some("wf_abc"));
    // None of the discovered paths is a journal.
    assert!(subs.iter().all(|s| !s.path.ends_with("journal.jsonl")));
}

#[test]
fn transcript_files_helper_excludes_journal_and_meta() {
    let fx = Fixture::new();
    let session = layout(&fx);
    let files = subagent_transcript_files(&session).unwrap();
    assert_eq!(files.len(), 2);
    for f in &files {
        let name = f.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("agent-"), "unexpected file: {name}");
        assert!(name.ends_with(".jsonl"));
        assert_ne!(name, "journal.jsonl");
    }
}

#[test]
fn lifecycle_builtin_from_transcript_terminal_message() {
    let fx = Fixture::new();
    let session = layout(&fx);
    let subs = discover_subagents(&session).unwrap();
    let builtin = subs
        .iter()
        .find(|s| s.kind == SubagentKind::BuiltinTask)
        .unwrap();
    let lc = lifecycle(builtin, &JournalCache::build(std::slice::from_ref(builtin))).unwrap();
    assert_eq!(lc.started_utc.as_deref(), Some("2026-06-07T05:00:00.000Z"));
    assert_eq!(
        lc.completed_utc.as_deref(),
        Some("2026-06-07T05:03:20.000Z")
    );
    // Terminal assistant text ⇒ completed (no journal for a built-in).
    assert_eq!(lc.status, SubagentStatus::Completed);
    assert_eq!(lc.agent_type.as_deref(), Some("oh-my-claudecode:executor"));
    assert_eq!(lc.description.as_deref(), Some("run it"));
}

#[test]
fn lifecycle_workflow_completion_from_journal_result() {
    let fx = Fixture::new();
    let session = layout(&fx);
    let subs = discover_subagents(&session).unwrap();
    let wf = subs
        .iter()
        .find(|s| s.kind == SubagentKind::Workflow)
        .unwrap();
    let lc = lifecycle(wf, &JournalCache::build(std::slice::from_ref(wf))).unwrap();
    // The workflow transcript ends on a tool_use (no terminal text), but the
    // journal carries a `result` event ⇒ completed.
    assert_eq!(lc.status, SubagentStatus::Completed);
    assert_eq!(lc.agent_type.as_deref(), Some("workflow-subagent"));
    assert_eq!(lc.started_utc.as_deref(), Some("2026-06-07T06:00:00.000Z"));
    assert_eq!(
        lc.completed_utc.as_deref(),
        Some("2026-06-07T06:01:00.000Z")
    );
}

#[test]
fn journal_cache_first_result_event_wins_and_renders_nonstring() {
    let fx = Fixture::new();
    let enc = "-Users-testuser-Projects-jc";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    let wf_dir = format!("{enc}/{SESS}/subagents/workflows/wf_jc1");
    for agent in ["aaa111", "bbb222"] {
        fx.write(
                &format!("{wf_dir}/agent-{agent}.jsonl"),
                &format!("{{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"{agent}\",\"timestamp\":\"2026-06-07T07:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"s\"}}}}\n"),
            );
    }
    fx.write(
        &format!("{wf_dir}/journal.jsonl"),
        concat!(
            // FIRST result event for aaa111 carries NO payload — and first wins
            // (the former per-agent scan returned on its first match), so the
            // later "late" payload must never surface.
            "{\"type\":\"result\",\"agentId\":\"aaa111\"}\n",
            "{\"type\":\"result\",\"agentId\":\"aaa111\",\"result\":\"late\"}\n",
            "not json - skipped exactly as the direct scans skipped it\n",
            // A non-string payload is JSON-rendered so it is never lost.
            "{\"type\":\"result\",\"agentId\":\"bbb222\",\"result\":{\"k\":1}}\n",
        ),
    );
    let subs = discover_subagents(&session).unwrap();
    let cache = JournalCache::build(&subs);
    let a = subs.iter().find(|s| s.agent_id == "aaa111").unwrap();
    let b = subs.iter().find(|s| s.agent_id == "bbb222").unwrap();
    // Both report completion (a result event exists, payload or not)...
    assert!(journal_reports_completion(a, &cache));
    assert!(journal_reports_completion(b, &cache));
    // ...first-event-wins keeps aaa111's payload None; bbb222's renders compactly.
    assert_eq!(journal_result(a, &cache), None);
    assert_eq!(journal_result(b, &cache).as_deref(), Some("{\"k\":1}"));
}

#[test]
fn running_when_no_completion_signal() {
    let fx = Fixture::new();
    let enc = "-Users-testuser-Projects-bar";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    // A subagent whose transcript ends mid-tool with NO journal result.
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-ccc333.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"ccc333\",\"timestamp\":\"2026-06-07T07:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"start\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T07:00:10.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
    let subs = discover_subagents(&session).unwrap();
    let lc = lifecycle(
        &subs[0],
        &JournalCache::build(std::slice::from_ref(&subs[0])),
    )
    .unwrap();
    assert_eq!(lc.status, SubagentStatus::Running);
}

#[test]
fn no_sidecar_is_empty_not_error() {
    let fx = Fixture::new();
    let session = fx.write(
        &format!("-Users-x/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    assert!(discover_subagents(&session).unwrap().is_empty());
    assert!(subagent_transcript_files(&session).unwrap().is_empty());
}

#[test]
fn duration_label_formats() {
    assert_eq!(
        duration_label(
            Some("2026-06-07T05:00:00.000Z"),
            Some("2026-06-07T05:03:20.000Z")
        )
        .as_deref(),
        Some("3m20s")
    );
    assert_eq!(
        duration_label(Some("2026-06-07T05:00:00Z"), Some("2026-06-07T07:05:00Z")).as_deref(),
        Some("2h05m")
    );
    assert_eq!(
        duration_label(Some("2026-06-07T05:00:00Z"), Some("2026-06-07T05:00:03Z")).as_deref(),
        Some("3s")
    );
    assert!(duration_label(None, Some("2026-06-07T05:00:00Z")).is_none());
}

#[test]
fn kind_and_status_labels() {
    assert_eq!(SubagentKind::BuiltinTask.label(), "builtin-task");
    assert_eq!(SubagentKind::Workflow.label(), "workflow");
    assert_eq!(SubagentStatus::Completed.label(), "completed");
    assert_eq!(SubagentStatus::Running.label(), "running");
    assert_eq!(SubagentStatus::Unknown.label(), "unknown");
}

// ── Branch-completeness ──

#[test]
fn sidecar_dir_none_for_pathological_paths() {
    // A path with no file stem (root) → the `file_stem()?` None arm.
    assert!(sidecar_dir_for_session(Path::new("/")).is_none());
    // A bare relative filename has no parent dir component that is a real dir, and
    // the sidecar dir won't exist → None.
    assert!(sidecar_dir_for_session(Path::new("nonexistent.jsonl")).is_none());
}

#[test]
fn discover_empty_when_sidecar_exists_but_no_subagents_dir() {
    // The sidecar `<uuid>/` dir exists but has NO `subagents/` child → empty, not
    // an error (the `!subagents_dir.is_dir()` early return).
    let fx = Fixture::new();
    let enc = "-Users-x";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    // Create the sidecar dir (named after the uuid) but with an unrelated child,
    // NOT a `subagents/` dir.
    fx.write(&format!("{enc}/{SESS}/other/file.txt"), "x");
    assert!(discover_subagents(&session).unwrap().is_empty());
}

#[test]
fn discover_ignores_stray_file_under_workflows() {
    // A non-directory entry sitting directly under `subagents/workflows/` must be
    // skipped by `subdirs_in` (the `if is_dir` FALSE arm). Only the real wf_* dir
    // contributes a workflow agent.
    let fx = Fixture::new();
    let enc = "-Users-strayfile";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    // A real workflow dir with an agent.
    fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_real/agent-kkk111.jsonl"),
            "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"kkk111\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
        );
    // A STRAY FILE (not a dir) directly under workflows/ → must be ignored.
    fx.write(
        &format!("{enc}/{SESS}/subagents/workflows/stray-not-a-dir.txt"),
        "ignore me",
    );
    let subs = discover_subagents(&session).unwrap();
    assert_eq!(
        subs.len(),
        1,
        "only the real wf agent; stray file ignored: {subs:?}"
    );
    assert_eq!(subs[0].kind, SubagentKind::Workflow);
    assert_eq!(subs[0].workflow_id.as_deref(), Some("wf_real"));
}

#[test]
fn discover_handles_subagents_dir_without_workflows() {
    // `subagents/` exists with a built-in agent but NO `workflows/` subdir (the
    // `workflows_dir.is_dir()` false arm).
    let fx = Fixture::new();
    let enc = "-Users-y";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-ddd444.jsonl"),
            "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"ddd444\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
        );
    let subs = discover_subagents(&session).unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].kind, SubagentKind::BuiltinTask);
    assert!(subs[0].workflow_id.is_none());
}

#[test]
fn read_meta_none_for_missing_unreadable_and_malformed() {
    // No meta path at all → all-None (the `let Some(p) else` arm).
    assert_eq!(read_meta(None), MetaFields::default());
    let fx = Fixture::new();
    // A path that does not exist → the `std::fs::read` Err arm.
    let missing = fx.root.join("does-not-exist.meta.json");
    assert_eq!(read_meta(Some(&missing)), MetaFields::default());
    // A file with invalid JSON → the `serde_json::from_slice` Err arm.
    let bad = fx.write("bad.meta.json", "{ not valid json");
    assert_eq!(read_meta(Some(&bad)), MetaFields::default());
    // Valid JSON carrying ONLY toolUseId → the topology join key is now CAPTURED
    // (previously dropped to (None,None)); agentType/description/name stay None.
    let only_id = fx.write("only-id.meta.json", "{\"toolUseId\":\"toolu_x\"}");
    assert_eq!(
        read_meta(Some(&only_id)),
        MetaFields {
            agent_type: None,
            description: None,
            tool_use_id: Some("toolu_x".to_string()),
            name: None,
            task_kind: None,
            team_name: None,
        }
    );
}

#[test]
fn read_meta_captures_all_builtin_fields() {
    // A real built-in meta.json carries agentType + description + toolUseId; csift
    // must capture all three (the toolUseId is the topology spawn-link, §1).
    let fx = Fixture::new();
    let full = fx.write(
            "full.meta.json",
            "{\"agentType\":\"oh-my-claudecode:executor\",\"description\":\"run it\",\"toolUseId\":\"toolu_01R7Zi2gHHGkaTvzuDMH7bK3\"}",
        );
    assert_eq!(
        read_meta(Some(&full)),
        MetaFields {
            agent_type: Some("oh-my-claudecode:executor".to_string()),
            description: Some("run it".to_string()),
            tool_use_id: Some("toolu_01R7Zi2gHHGkaTvzuDMH7bK3".to_string()),
            name: None,
            task_kind: None,
            team_name: None,
        }
    );
}

#[test]
fn make_subagent_threads_tool_use_id_onto_struct() {
    // The built-in subagent's `spawn_tool_use_id` must equal its meta's toolUseId
    // (the join key into the parent spawn index); a workflow agent (meta has only
    // agentType) has `None`.
    let fx = Fixture::new();
    let session = layout(&fx);
    let subs = discover_subagents(&session).unwrap();
    let builtin = subs
        .iter()
        .find(|s| s.kind == SubagentKind::BuiltinTask)
        .unwrap();
    assert_eq!(builtin.spawn_tool_use_id.as_deref(), Some("toolu_x"));
    let wf = subs
        .iter()
        .find(|s| s.kind == SubagentKind::Workflow)
        .unwrap();
    assert!(wf.spawn_tool_use_id.is_none());
}

#[test]
fn meta_without_agent_type_yields_none_label() {
    // A workflow agent whose meta.json lacks agentType → agent_type None on the
    // lifecycle, exercising read_meta's both-keys-absent path end to end.
    let fx = Fixture::new();
    let enc = "-Users-z";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-eee555.jsonl"),
            "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"eee555\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
        );
    // meta.json present but empty object.
    fx.write(
        &format!("{enc}/{SESS}/subagents/agent-eee555.meta.json"),
        "{}",
    );
    let subs = discover_subagents(&session).unwrap();
    let lc = lifecycle(
        &subs[0],
        &JournalCache::build(std::slice::from_ref(&subs[0])),
    )
    .unwrap();
    assert!(lc.agent_type.is_none());
    assert!(lc.description.is_none());
}

#[test]
fn journal_completion_false_for_builtin_no_workflow() {
    // A built-in subagent has no workflow_id → journal_reports_completion is false
    // via the `let Some(wf_id) else` arm (reached through lifecycle: a built-in
    // ending on a tool_use with no terminal text → Running, proving no journal).
    let fx = Fixture::new();
    let enc = "-Users-nojournal";
    let session = fx.write(
        &format!("{enc}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    fx.write(
            &format!("{enc}/{SESS}/subagents/agent-fff666.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"fff666\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:00:10.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
    let subs = discover_subagents(&session).unwrap();
    let lc = lifecycle(
        &subs[0],
        &JournalCache::build(std::slice::from_ref(&subs[0])),
    )
    .unwrap();
    assert_eq!(lc.status, SubagentStatus::Running);
}
