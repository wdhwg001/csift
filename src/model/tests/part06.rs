use super::*;

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

// ── classify(): dual-label user carriers (GOLD §3) ──

#[test]
fn classify_auq_answer_is_user_answer_plus_tool_result() {
    let r = parse(
        r#"{"type":"user","toolUseResult":{"questions":[{"question":"which?","options":[{"label":"A"}]}],"answers":{"which?":"go with A"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"go with A\"."}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserAnswer, Class::AgentToolResult]
    );
}

#[test]
fn classify_plan_rejection_is_user_rejection_plus_tool_result() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"p","is_error":true,"content":"The user doesn't want to proceed with this tool use. To tell you how to proceed, the user said:\nadd tests first"}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserRejection, Class::AgentToolResult]
    );
}

#[test]
fn classify_plain_tool_result_carrier_is_tool_result_only() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok, ran fine"}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentToolResult]
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
    // resolve it), but it is NOT the child's return — the report arrives LATER via the
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
    // tool_result content prefix alone — and still resolves to tool.result-only.
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
    // self direction — the async-launch ACK fix must NOT regress this.
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

// ── classify(): harness records (GOLD §2/§5) ──

#[test]
fn classify_task_notification_each_kind() {
    // Summaries kept quote-free so they embed cleanly in the JSON content string; the
    // kind classifier keys on the leading word, which is unaffected.
    let cases = [
        ("Dynamic workflow x completed", Class::NotificationWorkflow),
        (
            "Background command build completed (exit code 0)",
            Class::NotificationBackgroundCommand,
        ),
        ("Agent executor finished", Class::NotificationSubagent),
        ("Monitor event tick", Class::NotificationMonitor),
        ("something unclassified", Class::NotificationTask),
    ];
    for (summary, want) in cases {
        let r = parse(&format!(
            r#"{{"type":"user","message":{{"role":"user","content":"<task-notification>\n<task-id>id1</task-id>\n<status>completed</status>\n<summary>{summary}</summary>\n</task-notification>"}}}}"#
        ));
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![want],
            "summary: {summary}"
        );
    }
}

#[test]
fn classify_compaction_summary_and_boundary() {
    let summary = parse(
        r#"{"type":"user","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"message":{"role":"user","content":"This session is being continued..."}}"#,
    );
    assert_eq!(
        summary.classify(&ClassifyCtx::top_level()),
        vec![Class::CompactionSummary]
    );
    let boundary = parse(
        r#"{"type":"system","subtype":"compact_boundary","compactMetadata":{"trigger":"auto","preTokens":1}}"#,
    );
    assert!(boundary.is_compact_boundary());
    assert_eq!(
        boundary.classify(&ClassifyCtx::top_level()),
        vec![Class::CompactionBoundary]
    );
}

#[test]
fn classify_command_invocation_with_and_without_args() {
    let no_args = parse(
        r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-args></command-args>"}}"#,
    );
    assert_eq!(
        no_args.classify(&ClassifyCtx::top_level()),
        vec![Class::CommandInvocation]
    );
    let with_args = parse(
        r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-args>just shipped, summarize</command-args>"}}"#,
    );
    // v0.5: the prose is the RICHER view (richest-view law) — pushed FIRST so the
    // unfiltered record-text emission renders `/name args`, not the wrapper XML.
    assert_eq!(
        with_args.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage, Class::CommandInvocation]
    );
}

#[test]
fn classify_local_command_stdout() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<local-command-stdout>✓ done</local-command-stdout>"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::CommandStdout]
    );
}

#[test]
fn classify_interrupts_user_and_tool() {
    let u = parse(
        r#"{"type":"user","message":{"role":"user","content":"[Request interrupted by user]"}}"#,
    );
    assert_eq!(
        u.classify(&ClassifyCtx::top_level()),
        vec![Class::InterruptUser]
    );
    let t = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user for tool use]"}]}}"#,
    );
    assert_eq!(
        t.classify(&ClassifyCtx::top_level()),
        vec![Class::InterruptTool]
    );
}

#[test]
fn classify_schedule_continuation_and_wakeup() {
    let cont = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":[{"type":"text","text":"Continue from where you left off."}]}}"#,
    );
    assert_eq!(
        cont.classify(&ClassifyCtx::top_level()),
        vec![Class::ScheduleContinuation]
    );
    let wake = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<<autonomous-loop-dynamic>>"}}"#,
    );
    assert_eq!(
        wake.classify(&ClassifyCtx::top_level()),
        vec![Class::ScheduleWakeup]
    );
}
