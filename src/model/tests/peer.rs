//! Teammate/agent peer-message detection, parsing, and inbound previews.

use super::*;

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
fn every_relay_preamble_form_is_a_section_boundary() {
    // CC 2.1.258 relays a peer message under one of THREE preambles; a tag right after any
    // of them is a real delivery, and an `<agent-message>` behind the mid-turn form was
    // unlabeled until the set was widened (29 of 47 corpus records).
    for pre in [
        "Another Claude session sent a message:",
        "Another Claude session sent a message while you were working:",
        "A peer session sent a message while you were working:",
    ] {
        assert!(is_section_boundary(&format!("{pre}\n")), "{pre}");
        assert!(is_section_boundary(pre), "{pre} (no newline)");
        let rec: Record = serde_json::from_str(&format!(
            r#"{{"type":"user","isMeta":true,"message":{{"role":"user","content":"{pre}\n<agent-message from=\"peer-1\">ship it</agent-message>"}}}}"#
        ))
        .unwrap();
        let labels = rec.classify(&ClassifyCtx::top_level());
        assert!(
            labels.contains(&Class::CommInbox),
            "{pre}: inbound peer, got {labels:?}"
        );
        assert!(!rec.is_genuine_user(), "{pre}: never the human");
    }
    // A preamble that merely resembles one is NOT a boundary (no silent widening).
    assert!(!is_section_boundary(
        "Another Claude session mentioned a message:"
    ));
    assert!(!is_section_boundary("A peer session sent a message"));
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
    // (the FROM), and ONLY the peer's prose - the relay preamble, the `<teammate-message …>`
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
    // The same content can arrive as a single text block - still excluded.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<teammate-message teammate_id=\"x\">hi</teammate-message>"}]}}"#,
    );
    assert!(!r.is_genuine_user());
    assert!(r.opens_turn());
}

#[test]
fn is_teammate_message_detects_only_at_section_boundary() {
    // FINDING-1: a teammate tag is detected ONLY at a section boundary - the content start,
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
    // A tag QUOTED mid-prose is NOT a teammate message (the FINDING-1 fix - was TRUE before).
    assert!(!is_teammate_message(
        "noise before <teammate-message teammate_id=\"x\">hi</teammate-message> noise after"
    ));
    assert!(!is_teammate_message("no tag at all"));
}

#[test]
fn embedded_teammate_tag_mid_prose_stays_user_message() {
    // FINDING-1 (FLIPPED from the former accepted-tradeoff): a genuine user message that merely
    // QUOTES the tag mid-prose is NOT a peer message - it stays `user.message` (this bites
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
    // `<teammate-message>` mid-text classifies `user.message` ONLY - not harness.notification,
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

// ── C-30: the `<cross-session-message>` peer framing ──

/// The full on-disk shape of one inbound cross-session message: the relay preamble, the tag with
/// its attributes, the body, the close tag and the security footer, plus the `origin` object and
/// the `isMeta`/`promptSource`/`userType` stamps Claude Code writes beside them.
fn cross_session_record() -> Record {
    parse(
        r#"{"type":"user","uuid":"00000000-0000-4000-8000-000000000030","timestamp":"2026-06-07T05:00:00.000Z","isMeta":true,"promptSource":"system","userType":"external","queueSkipAttachments":true,"origin":{"kind":"peer","from":"uds:/Users/dev/relay.sock","verifiedPeerPid":4242,"msg_id":"00000000-0000-4000-8000-0000000000a1","name":"relay-7","fromMode":"bypass","body":"the shared resolver landed"},"message":{"role":"user","content":"Another Claude session sent a message:\n<cross-session-message from=\"uds:/Users/dev/relay.sock\" from-name=\"relay-7\" from-mode=\"bypass\">\nthe shared resolver landed\n</cross-session-message>\n\nThis came from another Claude session."}}"#,
    )
}

#[test]
fn cross_session_message_is_the_third_peer_framing() {
    // C-30: the framing a session-to-session send lands in. It is `type:user`/`role:user`/string
    // and matches no synthetic marker, so before it was folded into `is_peer_message` the isMeta
    // gate dropped it and the record carried NO label at all - invisible to every search.
    let r = cross_session_record();
    assert!(is_cross_session_message(
        "Another Claude session sent a message:\n<cross-session-message from=\"uds:/Users/dev/relay.sock\">hi</cross-session-message>"
    ));
    assert!(is_peer_message(
        "<cross-session-message from=\"uds:/Users/dev/relay.sock\">hi</cross-session-message>"
    ));
    assert!(!r.is_genuine_user(), "a peer is never the operator");
    assert!(r.opens_turn(), "but a delivered message still opens a turn");
    assert!(r.is_peer_message_record());
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::CommInbox]
    );
}

#[test]
fn cross_session_tag_quoted_mid_prose_stays_user_message() {
    // FINDING-1 for the third framing: this repo's own docs quote the literal tag, so a
    // `contains` check would reclassify the operator's prose as an inbound peer message.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"csift now classifies the <cross-session-message from=\"...\"> framing alongside the other two."}}"#,
    );
    assert!(!r.is_peer_message_record());
    assert!(r.is_genuine_user());
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage]
    );
}

#[test]
fn cross_session_attributes_pick_the_name_over_the_address() {
    // The open tag carries `from` (a transport address), `from-name` (the sender session's
    // display name) and `from-mode`. The direction renders the NAME - `from` is a socket path.
    let section = r#"<cross-session-message from="uds:/Users/dev/relay.sock" from-name="relay-7" from-mode="bypass">body</cross-session-message>"#;
    assert_eq!(
        extract_xml_attr(section, "from").as_deref(),
        Some("uds:/Users/dev/relay.sock")
    );
    assert_eq!(
        extract_xml_attr(section, "from-name").as_deref(),
        Some("relay-7")
    );
    assert_eq!(
        extract_xml_attr(section, "from-mode").as_deref(),
        Some("bypass")
    );
    assert_eq!(cross_session_sender(section).as_deref(), Some("relay-7"));
    // With no `from-name` the address is the honest fallback, never a fabricated name.
    let bare =
        r#"<cross-session-message from="uds:/Users/dev/relay.sock">body</cross-session-message>"#;
    assert_eq!(
        cross_session_sender(bare).as_deref(),
        Some("uds:/Users/dev/relay.sock")
    );
}

#[test]
fn cross_session_direction_and_body_render_without_the_xml() {
    let r = cross_session_record();
    let ctx = ClassifyCtx::top_level();
    assert_eq!(
        r.direction(&ctx),
        Some(("relay-7".to_string(), "self".to_string()))
    );
    let preview = r.inbound_comm_preview().expect("inbound preview");
    assert_eq!(preview.class, Class::CommInbox);
    assert_eq!(preview.from, "relay-7");
    assert_eq!(preview.body, "the shared resolver landed");
    let sections = r.record_text_sections(&ctx);
    assert_eq!(sections.len(), 1, "one peer section: {sections:?}");
    assert_eq!(sections[0].class, Class::CommInbox);
    assert_eq!(sections[0].text, "the shared resolver landed");
    assert!(
        !sections[0].text.contains("cross-session-message"),
        "the wrapper tags, the relay preamble and the footer are all stripped"
    );
}

#[test]
fn cross_session_origin_wins_for_a_single_section_and_yields_for_a_batch() {
    // `origin.name` / `origin.body` are derived by Claude Code from the very attributes and body
    // the tag carries, so preferring the structured pair is the same text read from a field
    // instead of a scan - but ONE origin describes ONE message, so a BATCHED record falls back
    // to each section's own attribute rather than stamping the first sender onto all of them.
    let single = parse(
        r#"{"type":"user","isMeta":true,"origin":{"kind":"peer","from":"uds:/Users/dev/relay.sock","name":"relay-7","body":"structured body"},"message":{"role":"user","content":"Another Claude session sent a message:\n<cross-session-message from=\"uds:/Users/dev/relay.sock\" from-name=\"tag-name\">\ntag body\n</cross-session-message>"}}"#,
    );
    let preview = single.inbound_comm_preview().expect("inbound preview");
    assert_eq!(preview.from, "relay-7");
    assert_eq!(preview.body, "structured body");

    let batched = parse(
        r#"{"type":"user","isMeta":true,"origin":{"kind":"peer","from":"uds:/Users/dev/relay.sock","name":"relay-7","body":"structured body"},"message":{"role":"user","content":"Another Claude session sent a message:\n<cross-session-message from-name=\"relay-7\">\nfirst\n</cross-session-message>\n<cross-session-message from-name=\"relay-8\">\nsecond\n</cross-session-message>"}}"#,
    );
    let sections = batched.record_text_sections(&ClassifyCtx::top_level());
    assert_eq!(sections.len(), 2, "two peer sections: {sections:?}");
    assert_eq!(sections[0].text, "first");
    assert_eq!(sections[1].text, "second");
    assert_eq!(
        sections[1].direction.as_ref().map(|(f, _)| f.as_str()),
        Some("relay-8"),
        "the second section keeps its OWN sender"
    );
    // A record with no `origin` at all reads the tag, exactly as before.
    let no_origin = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<cross-session-message from-name=\"relay-7\">\ntag only\n</cross-session-message>"}}"#,
    );
    let preview = no_origin.inbound_comm_preview().expect("inbound preview");
    assert_eq!(preview.from, "relay-7");
    assert_eq!(preview.body, "tag only");
}

#[test]
fn queued_cross_session_rider_is_never_the_human() {
    // A `queue-operation` enqueue line carries the SAME framed content one line before the user
    // record. It is a harness RIDER, not the operator's typed text, and the peer detector is
    // what `queued_class` consults - so folding the third framing in refuses it by construction.
    let rider = parse(
        r#"{"type":"queue-operation","operation":"enqueue","content":"<cross-session-message from=\"uds:/Users/dev/relay.sock\" from-name=\"relay-7\" from-mode=\"bypass\">\nthe shared resolver landed\n</cross-session-message>"}"#,
    );
    assert_eq!(
        rider.promoted_class(),
        None,
        "a peer rider is not user.queued"
    );
    // The human's own queued text still is.
    let typed = parse(
        r#"{"type":"queue-operation","operation":"enqueue","content":"run the release audit"}"#,
    );
    assert_eq!(typed.promoted_class(), Some(Class::UserQueued));
}
