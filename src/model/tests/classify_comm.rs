//! classify(): the agent.communication family (SendMessage, spawns, teammates, subagent returns).

use super::*;

// ── classify(): communication (the leaf and its direction) ──

#[test]
fn classify_sendmessage_message_is_tool_use_plus_sent() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"to":"ab9018739543b1df0","type":"message","message":"do the thing"}}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentToolUse, Class::CommSent]
    );
}

#[test]
fn classify_sendmessage_direct_is_sent_not_signal() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"to":"x","type":"direct","message":"hi"}}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentToolUse, Class::CommSent]
    );
}

#[test]
fn classify_sendmessage_no_type_defaults_to_sent() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"to":"x","message":"hi"}}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentToolUse, Class::CommSent]
    );
}

#[test]
fn classify_sendmessage_shutdown_request_is_signal() {
    // Top-level type:"shutdown_request" (the real shape).
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"to":"S-Sync","type":"shutdown_request","recipient":"S-Sync","reason":"done"}}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentToolUse, Class::CommSignal]
    );
}

#[test]
fn classify_sendmessage_nested_shutdown_payload_is_signal() {
    // The nested message:{type:shutdown_request} form (also real).
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"to":"x","message":{"type":"shutdown_request","reason":"done"}}}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentToolUse, Class::CommSignal]
    );
}

#[test]
fn classify_spawn_tool_use_is_tool_use_plus_sent() {
    for tool in ["Task", "Agent", "Workflow"] {
        let r = parse(&format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t","name":"{tool}","input":{{"subagent_type":"executor","description":"go"}}}}]}}}}"#
        ));
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::AgentToolUse, Class::CommSent],
            "spawn tool {tool}"
        );
    }
}

#[test]
fn classify_teammate_prose_is_inbox() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"reviewer\">verdict: LGTM</teammate-message>"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::CommInbox]
    );
}

#[test]
fn classify_teammate_signal_is_signal() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"peer\">\n{\"type\":\"idle_notification\",\"from\":\"peer\"}\n</teammate-message>"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::CommSignal]
    );
}

#[test]
fn classify_subagent_return_is_tool_result_plus_inbox() {
    // A tool_result whose tool_use_id was a spawn → the subagent return (child ⇨ self).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_spawn","content":"subagent done: 3 files changed"}]}}"#,
    );
    let ctx = ClassifyCtx {
        spawn: Some(&FakeSpawn),
        ..ClassifyCtx::top_level()
    };
    assert_eq!(
        r.classify(&ctx),
        vec![Class::AgentToolResult, Class::CommInbox]
    );
    // Without a spawn lookup it is just a tool_result (no return detection).
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentToolResult]
    );
}

#[test]
fn classify_teammate_spawn_ack_is_tool_result_only_not_inbox() {
    // Edge-fixtures correction #3: a persistent teammate `Agent` spawn's tool_result is an
    // immediate {status:"teammate_spawned"} ACK that shares the spawn tool_use_id (so the
    // lookup WOULD resolve it), but it is NOT the child's return → agent.tool.result ONLY.
    let r = parse(
        r#"{"type":"user","toolUseResult":{"status":"teammate_spawned","name":"P1-engine","agent_id":"aP1-engine-9cf2","agent_type":"executor"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_spawn","content":"teammate spawned"}]}}"#,
    );
    let ctx = ClassifyCtx {
        owner_id: Some("parent"),
        spawn: Some(&FakeSpawn),
        ..ClassifyCtx::top_level()
    };
    assert!(r.is_teammate_spawn_ack());
    assert_eq!(
        r.classify(&ctx),
        vec![Class::AgentToolResult],
        "the ACK must NOT carry agent.communication.inbox"
    );
    // And it produces NO child ⇨ self direction (the real reply arrives later).
    assert!(r.direction(&ctx).is_none());
}

#[test]
fn classify_async_launch_ack_is_tool_result_only_not_inbox() {
    // Smoke-found bug: an ASYNC/background `Agent` spawn's tool_result is the LAUNCH ack
    // (`toolUseResult.{isAsync:true,status:"async_launched"}`, content begins "Async agent
    // launched successfully…"). It shares the spawn tool_use_id (so the lookup WOULD
    // resolve it), but it is NOT the child's return - the report arrives LATER via the
    // <task-notification> <result> (G1 → inbox). So agent.tool.result ONLY, no …inbox.
    let r = parse(
        r#"{"type":"user","toolUseResult":{"isAsync":true,"status":"async_launched","agentId":"ad8012462a52f5c25","description":"draft the fold"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_spawn","content":[{"type":"text","text":"Async agent launched successfully.\nagentId: ad8012462a52f5c25 (internal ID - do not mention to user.)"}]}]}}"#,
    );
    let ctx = ClassifyCtx {
        owner_id: Some("parent"),
        spawn: Some(&FakeSpawn),
        ..ClassifyCtx::top_level()
    };
    assert!(r.is_async_launch_ack());
    assert!(r.is_spawn_launch_ack());
    assert_eq!(
        r.classify(&ctx),
        vec![Class::AgentToolResult],
        "the async-launch ACK must NOT carry agent.communication.inbox"
    );
    // And it produces NO child ⇨ self direction (the real report arrives later).
    assert!(
        r.direction(&ctx).is_none(),
        "a launch ack carries no child ⇨ self direction"
    );
}

#[test]
fn classify_async_launch_ack_detected_by_content_prefix_fallback() {
    // A record lacking the structured `toolUseResult` still detects the ack from the
    // tool_result content prefix alone - and still resolves to tool.result-only.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_spawn","content":"Async agent launched successfully.\nagentId: ad80 (internal ID)"}]}}"#,
    );
    let ctx = ClassifyCtx {
        spawn: Some(&FakeSpawn),
        ..ClassifyCtx::top_level()
    };
    assert!(r.is_async_launch_ack());
    assert_eq!(r.classify(&ctx), vec![Class::AgentToolResult]);
    assert!(r.direction(&ctx).is_none());
}

#[test]
fn classify_sync_task_return_still_tool_result_plus_inbox_vs_async_ack() {
    // Contrast guard: a SYNC one-shot Task tool_result IS the child's reply (no ack shape,
    // no launch-ack prefix) → [agent.tool.result, agent.communication.inbox] with a child ⇨
    // self direction - the async-launch ACK fix must NOT regress this.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_spawn","content":"subagent done: 3 files changed"}]}}"#,
    );
    let ctx = ClassifyCtx {
        owner_id: Some("parent"),
        spawn: Some(&FakeSpawn),
        ..ClassifyCtx::top_level()
    };
    assert!(!r.is_spawn_launch_ack());
    assert_eq!(
        r.classify(&ctx),
        vec![Class::AgentToolResult, Class::CommInbox]
    );
    assert_eq!(
        r.direction(&ctx),
        Some(("child-abc".to_string(), "parent".to_string()))
    );
}

#[test]
fn classify_teammate_terminated_signal_id_is_system() {
    // Edge-fixtures correction #5: a teammate_terminated payload has teammate_id="system"
    // on the attr; the dead agent is named in the BODY, not the attr. Direction FROM is
    // "system", never the dead agent.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<teammate-message teammate_id=\"system\">\n{\"type\":\"teammate_terminated\",\"message\":\"B38F1Check has shut down.\"}\n</teammate-message>"}}"#,
    );
    let tm = r.teammate_message().expect("teammate message");
    assert_eq!(tm.teammate_id.as_deref(), Some("system"));
    assert_eq!(tm.signal_type.as_deref(), Some("teammate_terminated"));
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::CommSignal]
    );
    let ctx = ClassifyCtx {
        owner_id: Some("me"),
        ..ClassifyCtx::top_level()
    };
    assert_eq!(
        r.direction(&ctx),
        Some(("system".to_string(), "me".to_string())),
        "FROM is system, NOT the dead agent named in the body"
    );
}

#[test]
fn classify_shutdown_approved_signal() {
    // The real f-shutdown_approved shape (JSON body with requestId etc.).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<teammate-message teammate_id=\"B38F1Check\" color=\"yellow\">\n{\"type\":\"shutdown_approved\",\"requestId\":\"shutdown-1@B38F1Check\",\"from\":\"B38F1Check\",\"backendType\":\"in-process\"}\n</teammate-message>"}}"#,
    );
    let tm = r.teammate_message().unwrap();
    assert_eq!(tm.signal_type.as_deref(), Some("shutdown_approved"));
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::CommSignal]
    );
}

#[test]
fn classify_sendmessage_real_shutdown_request_dict_message_shape() {
    // The exact real f-shutdown_request shape: top-level type=shutdown_request, message is
    // a DICT {type,reason} (polymorphic vs the string form), to==recipient = a NAME.
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"SendMessage","input":{"to":"GraftBoard","message":{"type":"shutdown_request","reason":"done"},"summary":"shut down","type":"shutdown_request","recipient":"GraftBoard","content":"done"}}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentToolUse, Class::CommSignal]
    );
    // Direction TO is the teammate NAME (not always a re-feedable id).
    assert_eq!(
        r.direction(&ClassifyCtx::top_level()),
        Some(("self".to_string(), "GraftBoard".to_string()))
    );
}

#[test]
fn classify_subagent_opener_is_inbox_via_ctx() {
    // The spawn-prompt seed of a subagent transcript: a genuine-user-shaped record that
    // ctx reclassifies as parent ⇨ self inbox.
    let r = parse(
        r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"go repro the bug"}}"#,
    );
    let ctx = ClassifyCtx {
        is_subagent: true,
        is_transcript_opener: true,
        ..ClassifyCtx::top_level()
    };
    assert_eq!(r.classify(&ctx), vec![Class::CommInbox]);
    // The SAME record on a top-level transcript is a plain user message.
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage]
    );
}

// ── P1c M1: <agent-message from="…"> peer form → agent.communication.inbox ──

#[test]
fn classify_agent_message_peer_form_is_inbox() {
    // Real shape: an isMeta type:user string carrying an <agent-message from="…"> peer reply
    // relayed into this session. Must classify agent.communication.inbox, NOT user.message -
    // and the isMeta guard (M2b) must NOT suppress it (the peer marker is matched first).
    let r = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<agent-message from=\"oh-my-claudecode:architect\">\n[Reply intended for the executor peer]\nuse the shared resolver.\n</agent-message>"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::CommInbox]
    );
    // Direction: the from="" attribute ⇨ self.
    let ctx = ClassifyCtx {
        owner_id: Some("me"),
        ..ClassifyCtx::top_level()
    };
    assert_eq!(
        r.direction(&ctx),
        Some(("oh-my-claudecode:architect".to_string(), "me".to_string()))
    );
}

#[test]
fn classify_agent_message_no_from_degrades_to_peer_direction() {
    let r = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<agent-message>no sender attr</agent-message>"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::CommInbox]
    );
    assert_eq!(
        r.direction(&ClassifyCtx::top_level()),
        Some(("peer".to_string(), "self".to_string()))
    );
}

/// The subagent-opener arm needs BOTH halves of its conjunction, and the one it was
/// never asked about is `is_subagent`.
///
/// `parent ⇨ self` says a PARENT spawned this transcript and seeded it with a prompt.
/// A top-level transcript has no parent, so its first opener is the person opening a
/// conversation and carries no direction at all - and `ctx.parent_id` is `None` there, so
/// an arm that fired anyway would render the literal `parent ⇨ self` on a human's first
/// message. P2 sets `is_transcript_opener` positionally and only for a subagent, so the
/// combination does not arise on disk; the arm is a pure function and the assertion costs
/// one fixture.
#[test]
fn a_top_level_first_opener_is_nobodys_spawn_seed() {
    let r =
        parse(r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"open the lane"}}"#);
    let mut ctx = ClassifyCtx::top_level();
    ctx.is_transcript_opener = true;
    assert_eq!(r.direction(&ctx), None, "no parent, so no parent arrow");
    // And with the other half set too, the arm fires and names the parent.
    ctx.is_subagent = true;
    ctx.parent_id = Some("a1b2c3d4-0000-4000-8000-000000000002");
    assert_eq!(
        r.direction(&ctx),
        Some((
            "a1b2c3d4-0000-4000-8000-000000000002".to_string(),
            "self".to_string()
        ))
    );
}

/// A COORDINATOR relay renders `coordinator ⇨ self` - the one inbound leaf that said
/// nothing about where its message came from.
///
/// The FROM is the literal word because the record carries no sender to read: over one
/// projects root all 147 coordinator relays carry an `origin` object whose key set is
/// exactly `{kind}`. Inventing an id from that would be inventing it, so the arrow names
/// the lane the harness itself names.
#[test]
fn a_coordinator_relay_renders_its_arrow() {
    let r = parse(
        r#"{"type":"user","uuid":"c1","isMeta":true,"origin":{"kind":"coordinator"},"message":{"role":"user","content":"The coordinator sent a message while you were working:\nhold the release"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::CommInbox],
        "the leaf is unchanged - this arm adds the arrow, not a label"
    );
    assert_eq!(
        r.direction(&ClassifyCtx::top_level()),
        Some(("coordinator".to_string(), "self".to_string()))
    );
    // The `self` side is the transcript owner wherever one is known, like every other
    // comm direction.
    let mut ctx = ClassifyCtx::top_level();
    ctx.owner_id = Some("a1b2c3d4-0000-4000-8000-000000000001");
    assert_eq!(
        r.direction(&ctx),
        Some((
            "coordinator".to_string(),
            "a1b2c3d4-0000-4000-8000-000000000001".to_string()
        ))
    );
}

/// Another `origin.kind` is NOT a coordinator relay, and the arm says so on the value
/// rather than on the presence of the key: the same `origin` object carries `human`,
/// `peer` and `task-notification` (3,081, 55 and 3,567 records over one projects root),
/// and an arm keyed on the key alone would put a coordinator arrow on every one of them.
#[test]
fn another_origin_kind_gets_no_coordinator_arrow() {
    for kind in ["human", "task-notification", "observer"] {
        let line = format!(
            r#"{{"type":"user","uuid":"o1","isMeta":true,"origin":{{"kind":"{kind}"}},"message":{{"role":"user","content":"plain text with no relay framing"}}}}"#
        );
        assert_eq!(
            parse(&line).direction(&ClassifyCtx::top_level()),
            None,
            "{kind}"
        );
    }
}
