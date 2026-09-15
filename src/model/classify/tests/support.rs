//! ClassifyCtx support, comm direction and the attachment-payload extractors.
//!
//! The leaf space itself (the 73 paths, the visible face, the category partition) is
//! guarded in `model/taxonomy/tests.rs`, beside the table it pins.

use super::*;

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
    assert_eq!(arr.classify(&ctx), vec![Class::HookContext]);
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
    // A payload type csift models no leaf for carries none: an address then renders it
    // as an unmodeled unit naming the type.
    assert!(other.classify(&ctx).is_empty());
}

#[test]
fn attachment_type_and_payload_text_extraction() {
    // A file attachment: type extracted, payload text VERBATIM, the context.file leaf,
    // never a turn opener.
    let rec: Record = serde_json::from_str(
        r#"{"type":"attachment","uuid":"x2","attachment":{"type":"edited_text_file","filePath":"/tmp/a.rs","snippet":"fn x() {}"}}"#,
    )
    .unwrap();
    assert_eq!(rec.attachment_type().as_deref(), Some("edited_text_file"));
    let text = rec.attachment_payload_text().unwrap();
    assert!(text.contains("edited_text_file") && text.contains("fn x() {}"));
    let ctx = ClassifyCtx::top_level();
    assert_eq!(rec.classify(&ctx), vec![Class::ContextFile]);
    assert!(!rec.opens_turn());
    // A hook payload keeps the harness.hook.context leaf but still censuses by type.
    let hook: Record = serde_json::from_str(
        r#"{"type":"attachment","attachment":{"type":"hook_additional_context","content":["ctx"]}}"#,
    )
    .unwrap();
    assert_eq!(
        hook.attachment_type().as_deref(),
        Some("hook_additional_context")
    );
    assert_eq!(hook.classify(&ctx), vec![Class::HookContext]);
    // A non-attachment record: both extractors None.
    let user: Record =
        serde_json::from_str(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#)
            .unwrap();
    assert_eq!(user.attachment_type(), None);
    assert_eq!(user.attachment_payload_text(), None);
}
