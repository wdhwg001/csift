//! On-disk discovery of subagent transcripts, meta companions, and workflow runs.

use super::*;

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
