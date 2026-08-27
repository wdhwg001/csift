//! Taxonomy drift guards, comm direction, and ClassifyCtx support.

use super::*;

// ── Class::path() + role() for EVERY variant (the canonical wire forms) ──

#[test]
fn class_path_for_every_variant() {
    let table = [
        (Class::UserMessage, "user.message"),
        (Class::UserAnswer, "user.answer"),
        (Class::UserRejection, "user.rejection"),
        (Class::AgentMessage, "agent.message"),
        (Class::AgentThinking, "agent.thinking"),
        (Class::AgentToolUse, "agent.tool.use"),
        (Class::AgentToolResult, "agent.tool.result"),
        (Class::CommInbox, "agent.communication.inbox"),
        (Class::CommSent, "agent.communication.sent"),
        (Class::CommSignal, "agent.communication.signal"),
        (Class::NotificationWorkflow, "harness.notification.workflow"),
        (Class::NotificationMonitor, "harness.notification.monitor"),
        (Class::NotificationSubagent, "harness.notification.subagent"),
        (
            Class::NotificationBackgroundCommand,
            "harness.notification.background-command",
        ),
        (Class::NotificationTask, "harness.notification.task"),
        (Class::CompactionSummary, "harness.compaction.summary"),
        (Class::CompactionBoundary, "harness.compaction.boundary"),
        (Class::CommandInvocation, "harness.command.invocation"),
        (Class::CommandStdout, "harness.command.stdout"),
        (Class::InterruptUser, "harness.interrupt.user"),
        (Class::InterruptTool, "harness.interrupt.tool"),
        (Class::ScheduleWakeup, "harness.schedule.wakeup"),
        (Class::ScheduleContinuation, "harness.schedule.continuation"),
        (Class::MetaHook, "harness.meta.hook"),
        (Class::MetaLoop, "harness.meta.loop"),
    ];
    for (c, p) in table {
        assert_eq!(c.path(), p, "path mismatch for {c:?}");
        // The role is always the first dot-segment of the path.
        let head = p.split('.').next().unwrap();
        assert_eq!(c.role().as_str(), head, "role/path head mismatch for {c:?}");
    }
    // No two leaves share a path (the selector space is unambiguous).
    let mut paths: Vec<&str> = table.iter().map(|(c, _)| c.path()).collect();
    paths.sort_unstable();
    let n = paths.len();
    paths.dedup();
    assert_eq!(paths.len(), n, "duplicate Class path");
}

#[test]
fn all_classes_cover_the_enum() {
    // Class::ALL must list EVERY variant (the local table here is the independent oracle);
    // a variant added to the enum but missing from ALL is caught by the path/role coverage.
    for &c in Class::ALL {
        // path() is total + role()'s as_str() is the path head - exercised for every leaf.
        let head = c.path().split('.').next().unwrap();
        assert_eq!(c.role().as_str(), head, "role/path head mismatch for {c:?}");
    }
    // ALL has no duplicates and matches the verified table size (26 leaves).
    let mut seen: Vec<&str> = Class::ALL.iter().map(|c| c.path()).collect();
    seen.sort_unstable();
    let n = seen.len();
    seen.dedup();
    assert_eq!(seen.len(), n, "duplicate in Class::ALL");
    assert_eq!(n, 26, "Class::ALL leaf count drifted");
}

#[test]
fn role_as_str_and_class_role_partition() {
    assert_eq!(Role::User.as_str(), "user");
    assert_eq!(Role::Agent.as_str(), "agent");
    assert_eq!(Role::Harness.as_str(), "harness");
    // Spot-check the role partition.
    assert_eq!(Class::UserAnswer.role(), Role::User);
    assert_eq!(Class::CommSignal.role(), Role::Agent);
    assert_eq!(Class::AgentToolResult.role(), Role::Agent);
    assert_eq!(Class::CompactionBoundary.role(), Role::Harness);
    assert_eq!(Class::ScheduleWakeup.role(), Role::Harness);
}

// ── direction() (GOLD §4) ──

#[test]
fn direction_teammate_inbox_from_peer_to_self() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"g4g5-probe\">verdicts</teammate-message>"}}"#,
    );
    let ctx = ClassifyCtx {
        owner_id: Some("session-uuid-1"),
        ..ClassifyCtx::top_level()
    };
    assert_eq!(
        r.direction(&ctx),
        Some(("g4g5-probe".to_string(), "session-uuid-1".to_string()))
    );
    // Without an owner id, the self side falls back to the literal "self".
    assert_eq!(
        r.direction(&ClassifyCtx::top_level()),
        Some(("g4g5-probe".to_string(), "self".to_string()))
    );
}

#[test]
fn direction_sendmessage_self_to_recipient() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"to":"ab9018739543b1df0","message":"hi"}}]}}"#,
    );
    let ctx = ClassifyCtx {
        owner_id: Some("me"),
        ..ClassifyCtx::top_level()
    };
    assert_eq!(
        r.direction(&ctx),
        Some(("me".to_string(), "ab9018739543b1df0".to_string()))
    );
}

#[test]
fn direction_sendmessage_recipient_fallback_field() {
    // No `to`, only `recipient`.
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"recipient":"team-lead","type":"shutdown_response","approve":true}}]}}"#,
    );
    assert_eq!(
        r.direction(&ClassifyCtx::top_level()),
        Some(("self".to_string(), "team-lead".to_string()))
    );
}

#[test]
fn direction_spawn_resolves_child_via_lookup_then_degrades() {
    // id-join hit.
    let by_id = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_spawn","name":"Task","input":{"subagent_type":"executor"}}]}}"#,
    );
    let ctx = ClassifyCtx {
        owner_id: Some("parent"),
        spawn: Some(&FakeSpawn),
        ..ClassifyCtx::top_level()
    };
    assert_eq!(
        by_id.direction(&ctx),
        Some(("parent".to_string(), "child-abc".to_string()))
    );
    // name-join hit (the teammate spawn: meta has no toolUseId, joins by input.name).
    let by_name = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"other","name":"Agent","input":{"name":"VSRepro","subagent_type":"qa-tester"}}]}}"#,
    );
    assert_eq!(
        by_name.direction(&ctx),
        Some(("parent".to_string(), "aVSRepro-deadbeef".to_string()))
    );
    // No lookup at all → degrade to the raw spawn name.
    assert_eq!(
        by_name.direction(&ClassifyCtx::top_level()),
        Some(("self".to_string(), "VSRepro".to_string()))
    );
}

#[test]
fn direction_subagent_return_child_to_self() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_spawn","content":"done"}]}}"#,
    );
    let ctx = ClassifyCtx {
        owner_id: Some("parent"),
        spawn: Some(&FakeSpawn),
        ..ClassifyCtx::top_level()
    };
    assert_eq!(
        r.direction(&ctx),
        Some(("child-abc".to_string(), "parent".to_string()))
    );
}

#[test]
fn direction_subagent_opener_parent_to_self() {
    let r = parse(
        r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"go do the thing"}}"#,
    );
    let ctx = ClassifyCtx {
        owner_id: Some("child-id"),
        parent_id: Some("parent-id"),
        is_subagent: true,
        is_transcript_opener: true,
        ..ClassifyCtx::top_level()
    };
    assert_eq!(
        r.direction(&ctx),
        Some(("parent-id".to_string(), "child-id".to_string()))
    );
}

#[test]
fn direction_none_for_non_comm_records() {
    let user = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
    assert!(user.direction(&ClassifyCtx::top_level()).is_none());
    let agent = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#,
    );
    assert!(agent.direction(&ClassifyCtx::top_level()).is_none());
    // A plain (non-spawn) tool_result is not a comm without a spawn match.
    let tr = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok"}]}}"#,
    );
    assert!(tr.direction(&ClassifyCtx::top_level()).is_none());
}

// ── send_message_is_signal helper edge arms ──

#[test]
fn send_message_is_signal_arms() {
    use serde_json::json;
    assert!(!send_message_is_signal(Some(&json!({"type":"message"}))));
    assert!(!send_message_is_signal(Some(&json!({"type":"direct"}))));
    assert!(send_message_is_signal(Some(
        &json!({"type":"shutdown_request"})
    )));
    assert!(send_message_is_signal(Some(
        &json!({"message":{"type":"shutdown_response"}})
    )));
    assert!(!send_message_is_signal(Some(&json!({"to":"x"})))); // no type → message
    assert!(!send_message_is_signal(None));
}

#[test]
fn classify_ctx_debug_renders_spawn_presence() {
    let ctx = ClassifyCtx {
        spawn: Some(&FakeSpawn),
        ..ClassifyCtx::top_level()
    };
    let dbg = format!("{ctx:?}");
    assert!(dbg.contains("has_spawn_lookup: true"), "got: {dbg}");
}

#[test]
fn hook_additional_context_text_shapes_and_classify() {
    // Array content (the real on-disk shape) joins with `\n`; classify → MetaHook only.
    let arr: Record = serde_json::from_str(
            r#"{"type":"attachment","uuid":"x1","attachment":{"type":"hook_additional_context","content":["alpha block","beta block"],"hookEvent":"SessionStart"}}"#,
        )
        .unwrap();
    assert_eq!(
        arr.hook_additional_context_text().as_deref(),
        Some("alpha block\nbeta block")
    );
    let ctx = ClassifyCtx::top_level();
    assert_eq!(arr.classify(&ctx), vec![Class::MetaHook]);
    assert!(!arr.opens_turn());

    // Bare-string content is tolerated (trimmed); a different attachment type is None.
    let s: Record = serde_json::from_str(
            r#"{"type":"attachment","attachment":{"type":"hook_additional_context","content":" solo "}}"#,
        )
        .unwrap();
    assert_eq!(s.hook_additional_context_text().as_deref(), Some("solo"));
    let other: Record = serde_json::from_str(
        r#"{"type":"attachment","attachment":{"type":"file_snapshot","content":"zz"}}"#,
    )
    .unwrap();
    assert_eq!(other.hook_additional_context_text(), None);
    // A non-hook payload is no longer invisible: it classifies the generic attachment leaf.
    assert_eq!(other.classify(&ctx), vec![Class::MetaAttachment]);
}

#[test]
fn attachment_type_and_payload_text_extraction() {
    // A generic attachment: type extracted, payload text VERBATIM, MetaAttachment leaf,
    // never a turn opener.
    let rec: Record = serde_json::from_str(
        r#"{"type":"attachment","uuid":"x2","attachment":{"type":"edited_text_file","filePath":"/tmp/a.rs","snippet":"fn x() {}"}}"#,
    )
    .unwrap();
    assert_eq!(rec.attachment_type().as_deref(), Some("edited_text_file"));
    let text = rec.attachment_payload_text().unwrap();
    assert!(text.contains("edited_text_file") && text.contains("fn x() {}"));
    let ctx = ClassifyCtx::top_level();
    assert_eq!(rec.classify(&ctx), vec![Class::MetaAttachment]);
    assert!(!rec.opens_turn());
    // A hook payload keeps the more specific MetaHook leaf but still censuses by type.
    let hook: Record = serde_json::from_str(
        r#"{"type":"attachment","attachment":{"type":"hook_additional_context","content":["ctx"]}}"#,
    )
    .unwrap();
    assert_eq!(
        hook.attachment_type().as_deref(),
        Some("hook_additional_context")
    );
    assert_eq!(hook.classify(&ctx), vec![Class::MetaHook]);
    // A non-attachment record: both extractors None.
    let user: Record =
        serde_json::from_str(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#)
            .unwrap();
    assert_eq!(user.attachment_type(), None);
    assert_eq!(user.attachment_payload_text(), None);
}
