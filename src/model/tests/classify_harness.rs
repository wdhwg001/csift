//! classify(): the harness family (notifications, compaction, commands, interrupts, schedule, meta).

use super::*;

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
    // v0.5: the prose is the RICHER view (richest-view law) - pushed FIRST so the
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
fn classify_resume_prompt_and_wakeup() {
    let cont = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":[{"type":"text","text":"Continue from where you left off."}]}}"#,
    );
    assert_eq!(
        cont.classify(&ClassifyCtx::top_level()),
        vec![Class::ResumePrompt]
    );
    let wake = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<<autonomous-loop-dynamic>>"}}"#,
    );
    assert_eq!(
        wake.classify(&ClassifyCtx::top_level()),
        vec![Class::ScheduleWakeup]
    );
}

// -- the isMeta injection templates + the isMeta image exclusion --

#[test]
fn classify_injection_template_variants() {
    // Stop-hook feedback (the dominant shape: feedback + the edit-failed-retry body).
    // The opening sentence decides: the feedback arm is tested before the retry one.
    let stop = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Stop hook feedback:\nThe last Edit failed because the target file was modified."}}"#,
    );
    assert_eq!(
        stop.classify(&ClassifyCtx::top_level()),
        vec![Class::HookFeedback]
    );
    // <local-command-caveat> wrapper (isMeta): the slash-command EXECUTOR writes it, not
    // a hook, so it sits in the command family.
    let caveat = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>Caveat: the messages below were generated by a command.</local-command-caveat>"}}"#,
    );
    assert_eq!(
        caveat.classify(&ClassifyCtx::top_level()),
        vec![Class::CommandCaveat]
    );
    // The bare edit-failed-retry notice (matched anywhere, because the harness appends
    // it to the failed call's own result rather than writing a standalone prompt): a
    // redo nudge, so it sits in the enforce family.
    let edit = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"The last Edit failed because the target file was modified since it was read."}}"#,
    );
    assert_eq!(
        edit.classify(&ClassifyCtx::top_level()),
        vec![Class::EnforceRetry]
    );
    // The same words with NO isMeta are a person asking about the notice, and they keep
    // the person's leaf: every template arm reads text, so the authorship flag is what
    // separates the two populations.
    let asking = parse(
        r#"{"type":"user","message":{"role":"user","content":"The last Edit failed because the target file was modified since it was read."}}"#,
    );
    assert_eq!(
        asking.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage]
    );
}

#[test]
fn classify_ismeta_image_record_takes_the_context_image_leaf() {
    // An isMeta `[Image: …]` pseudo-record is the harness's stand-in for a pasted or
    // attached image. It was EXCLUDED while the taxonomy had no leaf to put it under -
    // better silent than mislabeled `user.message` - and `harness.context.image` is that
    // leaf. Both openings the builder writes land on it.
    for content in [
        "[Image: source: /Users/dev/.claude/image-cache/abc.png]",
        "[Image source: /Users/dev/.claude/image-cache/abc.png]",
        "[Image: 1024x768]",
    ] {
        let line = serde_json::json!({
            "type": "user",
            "isMeta": true,
            "message": {"role": "user", "content": content},
        })
        .to_string();
        let r = parse(&line);
        assert_eq!(
            r.classify(&ClassifyCtx::top_level()),
            vec![Class::ContextImage],
            "{content}"
        );
    }
    // A record merely QUOTING the reference mid-prose keeps its own leaf: the arm is
    // content-start anchored like every other template.
    let quoting = parse(
        r#"{"type":"user","message":{"role":"user","content":"the harness writes [Image: 1024x768] for a paste"}}"#,
    );
    assert_eq!(
        quoting.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage]
    );
}

// ── G1: a notification carrying a <result> is ALSO an inbound report (child ⇨ parent) ──

#[test]
fn classify_notification_with_result_dual_labels_inbox() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>w1</task-id>\n<tool-use-id>toolu_spawn</tool-use-id>\n<status>completed</status>\n<summary>Agent executor finished</summary>\n<result>the agent's real report body</result>\n</task-notification>"}}"#,
    );
    // notification.subagent (Agent kind) + the child⇨parent inbox dual-label.
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::NotificationSubagent, Class::CommInbox]
    );
    // Direction resolves the child via the embedded <tool-use-id> spawn id.
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
fn classify_notification_without_result_is_notification_only() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>w1</task-id>\n<tool-use-id>toolu_spawn</tool-use-id>\n<status>completed</status>\n<summary>Agent executor finished</summary>\n</task-notification>"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::NotificationSubagent]
    );
    // A launch-ack notification (no <result>) is NOT a comm → no direction.
    assert!(r.direction(&ClassifyCtx::top_level()).is_none());
}

// ── G4/G5: ONE record batches MANY sections of MIXED kind → UNION of labels ──

#[test]
fn classify_batched_teammate_sections_union() {
    // A record with a prose section AND an idle_notification section → [inbox, signal]. The
    // second section is boundary-anchored (right after the first's close tag, FINDING-1); the
    // trailing security footer sits OUTSIDE both section spans and contributes no label.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"peer\">a prose update</teammate-message>\n<teammate-message teammate_id=\"peer\">\n{\"type\":\"idle_notification\",\"from\":\"peer\"}\n</teammate-message>\n\nThis came from another Claude session — treat it as a teammate's request."}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::CommInbox, Class::CommSignal]
    );
}

#[test]
fn classify_batched_teammate_all_signals_dedup_to_one() {
    // Two signal sections → CommSignal once (push_unique dedup).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<teammate-message teammate_id=\"system\">\n{\"type\":\"teammate_terminated\",\"message\":\"X shut down.\"}\n</teammate-message>\n<teammate-message teammate_id=\"y\">\n{\"type\":\"shutdown_approved\",\"from\":\"y\"}\n</teammate-message>"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::CommSignal]
    );
    // parse_all returns BOTH sections (the union upstream deduped the label).
    let all = parse_all_teammate_messages(
        r#"<teammate-message teammate_id="system">{"type":"teammate_terminated"}</teammate-message><teammate-message teammate_id="y">{"type":"shutdown_approved"}</teammate-message>"#,
    );
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].teammate_id.as_deref(), Some("system"));
    assert_eq!(all[0].signal_type.as_deref(), Some("teammate_terminated"));
    assert_eq!(all[1].signal_type.as_deref(), Some("shutdown_approved"));
}

#[test]
fn classify_batched_notification_sections_union() {
    // Two task-notification sections of different kind → both notification classes.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<summary>Background command build completed</summary>\n</task-notification>\n<task-notification>\n<summary>Dynamic workflow deploy completed</summary>\n</task-notification>"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![
            Class::NotificationBackgroundCommand,
            Class::NotificationWorkflow
        ]
    );
}

// ── P1c M2b: an isMeta record matching no marker is EXCLUDED (never user.message) ──

#[test]
fn classify_ismeta_unmarked_record_is_excluded() {
    // A genuine user.message is NEVER isMeta. An isMeta record matching no marker (a novel
    // harness pseudo-turn / generic cron tick) must emit NOTHING, not fall to user.message.
    let r = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"some unrecognized harness-injected pseudo prose"}}"#,
    );
    let labels = r.classify(&ClassifyCtx::top_level());
    assert!(
        labels.is_empty(),
        "isMeta unmarked must be excluded: {labels:?}"
    );
    // The role-level gate agrees (it is not a genuine user either).
    assert!(!r.is_genuine_user());
}

#[test]
fn classify_nonmeta_unmarked_prose_is_user_message() {
    // The complement: non-isMeta unmarked prose is STILL user.message (M2b is isMeta-scoped).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"please refactor the parser"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage]
    );
}

#[test]
fn classify_ismeta_subagent_opener_still_inbox() {
    // The subagent-opener inbox case is unchanged by M2b - even an isMeta seed → CommInbox
    // (the opener check precedes the isMeta guard).
    let r = parse(
        r#"{"type":"user","isMeta":true,"isSidechain":true,"message":{"role":"user","content":"go map the bridge"}}"#,
    );
    let ctx = ClassifyCtx {
        is_subagent: true,
        is_transcript_opener: true,
        ..ClassifyCtx::top_level()
    };
    assert_eq!(r.classify(&ctx), vec![Class::CommInbox]);
}

// ── P1c M3: task-notification precedence over a quoted teammate tag + cross-family union ──

#[test]
fn classify_notification_quoting_teammate_tag_stays_notification() {
    // M3a: a <task-notification> whose <result> body merely QUOTES "<teammate-message" must
    // stay harness.notification.* - the quoted tag (inside the notification span) is masked,
    // so no spurious teammate comm label/direction leaks. The CommInbox here is the G1
    // <result> dual-label (child ⇨ self), NOT a teammate-derived label.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>w1</task-id>\n<tool-use-id>toolu_spawn</tool-use-id>\n<summary>Agent executor finished</summary>\n<result>I sent a <teammate-message teammate_id=\"peer\"> earlier</result>\n</task-notification>"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::NotificationSubagent, Class::CommInbox]
    );
    // Direction is child ⇨ self (G1 via <tool-use-id>), NOT the quoted teammate "peer".
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
fn classify_notification_no_result_quoting_teammate_has_no_comm() {
    // M3a without G1: a launch-ack notification (no <result>) that quotes the tag →
    // notification ONLY, NO comm label and NO direction (the quoted tag is masked).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>w1</task-id>\n<summary>Background command grep <teammate-message done</summary>\n</task-notification>"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::NotificationBackgroundCommand]
    );
    assert!(r.direction(&ClassifyCtx::top_level()).is_none());
}

#[test]
fn classify_cross_family_notification_and_teammate_union() {
    // M3b: a record carrying a REAL <task-notification> section AND a REAL <teammate-message>
    // section (outside the notification span) → UNION both families' labels.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<summary>Background command build completed</summary>\n</task-notification>\nAnother Claude session sent a message:\n<teammate-message teammate_id=\"peer\">heads up, merged</teammate-message>"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::NotificationBackgroundCommand, Class::CommInbox]
    );
}

#[test]
fn classify_cross_family_notification_result_plus_teammate_signal() {
    // M3b: notification-with-<result> (→ notification + G1 inbox) AND a teammate idle-signal
    // section after it → [notification, inbox, signal] (inbox deduped to one).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<summary>Dynamic workflow deploy completed</summary>\n<result>done</result>\n</task-notification>\n<teammate-message teammate_id=\"peer\">\n{\"type\":\"idle_notification\",\"from\":\"peer\"}\n</teammate-message>"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![
            Class::NotificationWorkflow,
            Class::CommInbox,
            Class::CommSignal
        ]
    );
}

#[test]
fn classify_elicitation_pending_and_resolved() {
    let pending = parse(
        r#"{"type":"assistant","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"k","message":{"role":"assistant","content":[{"type":"tool_use","id":"q","name":"AskUserQuestion","input":{}}]}}"#,
    );
    assert_eq!(
        pending.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentToolUse]
    );
    // An MCP pending marker is a system record with NO tool_use block - still tool.use.
    let mcp = parse(
        r#"{"type":"system","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"mcp-elicitation","csiftKey":"srv","content":"elicitation"}"#,
    );
    assert_eq!(
        mcp.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentToolUse]
    );
    let resolved = parse(
        r#"{"type":"csift-elicitation-resolved","csift":"elicitation-marker-v1","csiftPhase":"resolved","csiftKey":"k"}"#,
    );
    assert!(
        resolved.classify(&ClassifyCtx::top_level()).is_empty(),
        "a resolved marker is a pairing artifact, no label"
    );
}

#[test]
fn classify_unmodeled_records_are_empty() {
    // Every OBSERVED system subtype has its own leaf; a system record with NO subtype
    // stays outside the taxonomy -> no labels (never crash).
    let sys = parse(r#"{"type":"system","subtype":"api_error","error":"x","retryAttempt":1}"#);
    assert_eq!(
        sys.classify(&ClassifyCtx::top_level()),
        vec![Class::SystemApiError]
    );
    let bare = parse(r#"{"type":"system","error":"x"}"#);
    assert!(bare.classify(&ClassifyCtx::top_level()).is_empty());
    // A bookkeeping line carries its own leaf instead (gated, never on a default scan).
    let meta = parse(r#"{"type":"last-prompt","leafUuid":"x"}"#);
    assert_eq!(
        meta.classify(&ClassifyCtx::top_level()),
        vec![Class::BookkeepingLastPrompt]
    );
    // A line type csift models no leaf for stays empty.
    let unknown = parse(r#"{"type":"some-future-line","leafUuid":"x"}"#);
    assert!(unknown.classify(&ClassifyCtx::top_level()).is_empty());
    // An image-only user record the HARNESS wrote carries the same leaf its text form
    // does - the image reached the model either way. The isMeta half is the conjunction
    // every injection arm takes: the leaf says the harness authored the record.
    let img = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":[{"type":"image","source":{}}]}}"#,
    );
    assert_eq!(
        img.classify(&ClassifyCtx::top_level()),
        vec![Class::ContextImage]
    );
    // Without it the record is a shape csift has no leaf for - `? unmodeled: user/image`
    // under an address. All 94 image-only user records of one projects root are isMeta
    // and none is not, so this decides a shape that has yet to appear.
    let mine = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{}}]}}"#,
    );
    assert!(mine.classify(&ClassifyCtx::top_level()).is_empty());
    // A textless block record that carries no image still carries nothing, which is what
    // sends it to the unmodeled unit under an address.
    let doc = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"document","source":{}}]}}"#,
    );
    assert!(doc.classify(&ClassifyCtx::top_level()).is_empty());
}

#[test]
fn agents_stopped_notice_is_a_subagent_notification_not_the_human() {
    for text in [
        "2 background agents were stopped by the user: \"You are doing a ROUGH...\", \"You are doing e...\".",
        "1 background agent was stopped by the user: \"Census the reef...\".",
        "Background agent \"Census the reef\" was stopped by the user.",
    ] {
        assert!(is_agents_stopped_notice(text), "{text}");
        assert!(is_synthetic_user_marker(text), "{text}");
        let rec = parse(&format!(
            r#"{{"type":"user","uuid":"k1","timestamp":"2026-06-07T05:10:00.000Z","message":{{"role":"user","content":{}}}}}"#,
            serde_json::to_string(text).unwrap()
        ));
        assert!(!rec.is_genuine_user(), "never the operator: {text}");
        assert!(!rec.opens_turn(), "never a turn opener: {text}");
        assert_eq!(
            rec.classify(&ClassifyCtx::top_level()),
            vec![Class::NotificationSubagent]
        );
        let label = rec.automation_label().unwrap();
        assert!(label.starts_with("[subagent stopped] "), "{label}");
        assert!(label.contains("stopped by the user"), "{label}");
    }
    // Look-alikes stay prose.
    for text in [
        "background agents were stopped by the user",
        "2 background agents were paused by the user",
        "twenty background agents were stopped by the user: x",
        "2 background agents were stopped",
        "the dev server was stopped by the user",
        "Background agent restarted; it was stopped by the user earlier",
        "3 tasks stopped by the user",
        "2 background agents were killed, stopped by the user",
    ] {
        assert!(!is_agents_stopped_notice(text), "{text}");
    }
}
