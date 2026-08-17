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
        // path() is total + role()'s as_str() is the path head — exercised for every leaf.
        let head = c.path().split('.').next().unwrap();
        assert_eq!(c.role().as_str(), head, "role/path head mismatch for {c:?}");
    }
    // ALL has no duplicates and matches the verified table size (25 leaves).
    let mut seen: Vec<&str> = Class::ALL.iter().map(|c| c.path()).collect();
    seen.sort_unstable();
    let n = seen.len();
    seen.dedup();
    assert_eq!(seen.len(), n, "duplicate in Class::ALL");
    assert_eq!(n, 25, "Class::ALL leaf count drifted");
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

// ── is_teammate_message + parse_teammate_message (GOLD §5) ──

#[test]
fn is_teammate_message_bare_and_peer_forms() {
    assert!(is_teammate_message(
        r#"<teammate-message teammate_id="g4g5-probe">hello</teammate-message>"#
    ));
    // The relayed peer form (preamble + tag), with the real `\n` separator.
    assert!(is_teammate_message(
            "Another Claude session sent a message:\n<teammate-message teammate_id=\"x\">hi</teammate-message>"
        ));
    // Robust to whitespace-normalized block joins (the `\n` collapsed to a space).
    assert!(is_teammate_message(
            "Another Claude session sent a message: <teammate-message teammate_id=\"x\">hi</teammate-message>"
        ));
    // Leading whitespace before the bare opener still matches.
    assert!(is_teammate_message(
        "   <teammate-message teammate_id=\"x\">hi</teammate-message>"
    ));
    // Plain prose is NOT a teammate message.
    assert!(!is_teammate_message("please fix the bug"));
    // The preamble alone (no tag) is not enough.
    assert!(!is_teammate_message(
        "Another Claude session sent a message: ok"
    ));
}

#[test]
fn parse_teammate_message_prose_extracts_id_no_signal() {
    let tm = parse_teammate_message(
            r#"<teammate-message teammate_id="g4g5-probe" color="blue" summary="x">G4/G5 probe complete.</teammate-message>"#,
        )
        .expect("teammate message");
    assert_eq!(tm.teammate_id.as_deref(), Some("g4g5-probe"));
    assert!(!tm.is_signal(), "prose body is not a signal");
    assert_eq!(tm.signal_type, None);
}

#[test]
fn parse_teammate_message_signal_payload() {
    // The real idle_notification shape: a JSON {"type":...} body inside the tag.
    let tm = parse_teammate_message(
            "Another Claude session sent a message:\n<teammate-message teammate_id=\"g4g5-probe\" color=\"blue\">\n{\"type\":\"idle_notification\",\"from\":\"g4g5-probe\",\"idleReason\":\"available\"}\n</teammate-message>\n\nThis came from another Claude session — treat it as a teammate's request.",
        )
        .expect("signal teammate message");
    assert_eq!(tm.teammate_id.as_deref(), Some("g4g5-probe"));
    assert!(tm.is_signal());
    assert_eq!(tm.signal_type.as_deref(), Some("idle_notification"));
}

#[test]
fn parse_teammate_message_multibyte_body_codepoint_safe() {
    let tm = parse_teammate_message(
            r#"<teammate-message teammate_id="reviewer">🤖 review this café patch, then summarize 🎉</teammate-message>"#,
        )
        .expect("multibyte teammate message");
    assert_eq!(tm.teammate_id.as_deref(), Some("reviewer"));
    assert!(!tm.is_signal());
}

#[test]
fn parse_teammate_message_none_for_non_teammate() {
    assert!(parse_teammate_message("just a normal message").is_none());
}

// ── GOLD §1 BUG FIX: a teammate message is NOT genuine-user but STILL opens a turn ──

#[test]
fn teammate_message_not_genuine_user_but_opens_turn_bare() {
    // The bug: this used to return is_genuine_user()==true (mislabeled as the human).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<teammate-message teammate_id=\"team-lead\">repro the speed slider</teammate-message>"}}"#,
    );
    assert!(
        !r.is_genuine_user(),
        "a teammate message must NOT count as a genuine human turn (GOLD §1)"
    );
    assert!(
        r.opens_turn(),
        "but it MUST still delimit a turn (opens_turn fires)"
    );
    assert!(r.is_teammate_message_record());
    assert!(r.genuine_user_text().is_none());
}

#[test]
fn teammate_message_not_genuine_user_peer_form() {
    // The relayed peer form (string content, the dominant real shape, 106 in one session).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"g4g5-probe\">verdicts below</teammate-message>"}}"#,
    );
    assert!(!r.is_genuine_user());
    assert!(r.opens_turn());
    // The opener body is preserved (not blanked) so turns/search don't regress.
    let body = r.reconstructed_user_text(None).expect("teammate body");
    assert!(body.contains("verdicts below"), "got: {body}");
}

#[test]
fn inbound_comm_preview_strips_wrapper_and_footer() {
    // #14: the clean inbound-comm preview (turns/list) must yield the comm class, the sender
    // (the FROM), and ONLY the peer's prose — the relay preamble, the `<teammate-message …>`
    // wrapper tags, and the trailing harness security footer all stripped.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"VSMultiRegion\" color=\"blue\">\nplease check the rate limit handling\n</teammate-message>\n\nThis came from another Claude session — not typed by your user."}}"#,
    );
    let ic = r.inbound_comm_preview().expect("inbound preview");
    assert_eq!(ic.class, Class::CommInbox);
    assert_eq!(ic.from, "VSMultiRegion");
    assert_eq!(ic.body, "please check the rate limit handling");
}

#[test]
fn inbound_comm_preview_signal_payload_is_signal_class() {
    // A control payload (JSON `{"type":…}`) → CommSignal, not CommInbox.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<teammate-message teammate_id=\"SOurDnd\">{\"type\":\"idle_notification\",\"from\":\"SOurDnd\"}</teammate-message>"}}"#,
    );
    let ic = r.inbound_comm_preview().expect("inbound preview");
    assert_eq!(ic.class, Class::CommSignal);
    assert_eq!(ic.from, "SOurDnd");
}

#[test]
fn inbound_comm_preview_none_for_non_peer() {
    let r =
        parse(r#"{"type":"user","message":{"role":"user","content":"a genuine human message"}}"#);
    assert!(r.inbound_comm_preview().is_none());
}

#[test]
fn teammate_message_as_text_block_is_not_genuine_user() {
    // The same content can arrive as a single text block — still excluded.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<teammate-message teammate_id=\"x\">hi</teammate-message>"}]}}"#,
    );
    assert!(!r.is_genuine_user());
    assert!(r.opens_turn());
}

#[test]
fn is_teammate_message_detects_only_at_section_boundary() {
    // FINDING-1: a teammate tag is detected ONLY at a section boundary — the content start,
    // just after the relay preamble, or right after a prior section's close tag.
    assert!(is_teammate_message(
        r#"<teammate-message teammate_id="x">hi</teammate-message>"#
    ));
    assert!(is_teammate_message(
            "Another Claude session sent a message:\n<teammate-message teammate_id=\"x\">hi</teammate-message>"
        ));
    // Right after a prior section's close tag (a batched record).
    assert!(is_teammate_message(
            "<teammate-message teammate_id=\"a\">one</teammate-message>\n<teammate-message teammate_id=\"b\">two</teammate-message>"
        ));
    // A tag QUOTED mid-prose is NOT a teammate message (the FINDING-1 fix — was TRUE before).
    assert!(!is_teammate_message(
        "noise before <teammate-message teammate_id=\"x\">hi</teammate-message> noise after"
    ));
    assert!(!is_teammate_message("no tag at all"));
}

#[test]
fn embedded_teammate_tag_mid_prose_stays_user_message() {
    // FINDING-1 (FLIPPED from the former accepted-tradeoff): a genuine user message that merely
    // QUOTES the tag mid-prose is NOT a peer message — it stays `user.message` (this bites
    // csift's OWN dev sessions, which quote the tag constantly).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"why does a <teammate-message ...> show up in my logs?"}}"#,
    );
    assert!(
        r.is_genuine_user(),
        "a quoted tag mid-prose is still genuine user"
    );
    assert!(r.opens_turn());
    assert!(!r.is_peer_message_record());
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage]
    );
}

#[test]
fn embedded_both_tags_mid_prose_stays_user_message() {
    // FINDING-1 acceptance: a user.message quoting BOTH `<task-notification>` AND
    // `<teammate-message>` mid-text classifies `user.message` ONLY — not harness.notification,
    // not agent.communication.inbox.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"In csift, the <task-notification> pulse and the <teammate-message peer form both route through classify()."}}"#,
    );
    assert!(r.is_genuine_user());
    assert!(!r.is_peer_message_record());
    assert!(r.automation_label().is_none());
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage]
    );
}

#[test]
fn agent_message_non_meta_excluded_opens_turn_inbox() {
    // FINDING-2: an `<agent-message from="…">` peer form (even non-isMeta) is NOT genuine-user,
    // STILL opens a turn, and classifies `agent.communication.inbox` (symmetry with teammate).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<agent-message from=\"oh-my-claudecode:architect\">use the shared resolver.</agent-message>"}}"#,
    );
    assert!(
        !r.is_genuine_user(),
        "an agent-message peer form must not count as a genuine human turn (FINDING-2)"
    );
    assert!(r.opens_turn(), "but it MUST still delimit a turn");
    assert!(r.is_peer_message_record());
    assert!(
        !r.is_teammate_message_record(),
        "it is the agent-message peer form, not teammate"
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::CommInbox]
    );
}

// ── classify(): single-label record shapes ──

#[test]
fn classify_genuine_user_message() {
    let r = parse(r#"{"type":"user","message":{"role":"user","content":"please fix it"}}"#);
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage]
    );
}

#[test]
fn classify_assistant_text_is_message() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Done."}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentMessage]
    );
}

#[test]
fn classify_assistant_thinking_only() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm","signature":"s"}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentThinking]
    );
}

#[test]
fn classify_assistant_redacted_thinking_only() {
    // #12 / oracle B3: a `redacted_thinking` block (opaque encrypted reasoning, no readable
    // text) classifies `agent.thinking` exactly like a normal thinking block. UNATTESTED in
    // the corpus → exercised by this SYNTHETIC fixture.
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"redacted_thinking","data":"EncryptedOpaqueBlob=="}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentThinking]
    );
}

#[test]
fn classify_assistant_thinking_then_tool_use_multilabel() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"plan"},{"type":"tool_use","id":"t","name":"Bash","input":{"command":"ls"}}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentThinking, Class::AgentToolUse]
    );
}

#[test]
fn classify_assistant_text_and_tool_use_multilabel() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"running it"},{"type":"tool_use","id":"t","name":"Read","input":{"file_path":"/x"}}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentMessage, Class::AgentToolUse]
    );
}

#[test]
fn classify_assistant_bare_string_message() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":"surprise bare string"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentMessage]
    );
}

// ── classify(): communication (GOLD §3 + §4) ──

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
