//! The `agent.communication.*` acceptance cases: which records are inbound messages,
//! which are outbound, and what a hit says about who was talking to whom.

use crate::harness::*;

#[test]
fn acceptance_communication_sent_spawn_and_subagent_opener() {
    // §C8 a Task/Agent spawn tool_use → `agent.communication.sent` (self ⇨ child); §C6 SendMessage
    // message → sent; §C9 the subagent transcript opener → `agent.communication.inbox` (parent ⇨ self).
    let h = acceptance_home();

    let spawn = acc(&h, "zzspawn", "agent.communication.sent");
    assert!(spawn.success, "C8: stderr {}", spawn.stderr);
    assert!(
        spawn.stdout.contains("agent.communication.sent")
            && spawn.stdout.contains("self ⇨ audit-x"),
        "C8 spawn → comm.sent (self ⇨ audit-x):\n{}",
        spawn.stdout
    );

    let sent = acc(&h, "zzsent", "agent.communication.sent");
    assert!(
        sent.stdout.contains("agent.communication.sent")
            && sent.stdout.contains("self ⇨ GraftBoard"),
        "C6 SendMessage message → comm.sent (self ⇨ GraftBoard):\n{}",
        sent.stdout
    );

    // C9 needs subagent span (the opener lives in the subagent transcript).
    let opener = h.run(&[
        "search",
        "zzopener",
        "-t",
        "agent.communication.inbox",
        &at(ACC_SESS),
    ]);
    assert!(opener.success, "C9: stderr {}", opener.stderr);
    assert!(
        opener.stdout.contains("agent.communication.inbox") && opener.stdout.contains("⇨ self"),
        "C9 subagent opener → comm.inbox (parent ⇨ self):\n{}",
        opener.stdout
    );
}

#[test]
fn acceptance_communication_signals_render_direction() {
    // §C2 bare-lead inbox · §C3 idle_notification · §C4 teammate_terminated · §C5 shutdown_approved
    // · §C7 SendMessage shutdown_request - each renders `from ⇨ to` (the owner side is `self`).
    let h = acceptance_home();
    let cases = [
        (
            "C2 inbox",
            "zzbareinbox",
            "agent.communication.inbox",
            "team-lead ⇨ self",
        ),
        (
            "C3 idle",
            "zzidle",
            "agent.communication.signal",
            "SOurDnd ⇨ self",
        ),
        (
            "C4 terminated",
            "zzterminated",
            "agent.communication.signal",
            "system ⇨ self",
        ),
        (
            "C5 approved",
            "zzapproved",
            "agent.communication.signal",
            "B38 ⇨ self",
        ),
        (
            "C7 shutdown_req",
            "zzshutdownreq",
            "agent.communication.signal",
            "self ⇨ GraftBoard",
        ),
    ];
    for (oracle, token, selector, dir) in cases {
        let out = acc(&h, token, selector);
        assert!(out.success, "{oracle}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains(selector),
            "{oracle} must classify {selector}:\n{}",
            out.stdout
        );
        assert!(
            out.stdout.contains(dir),
            "{oracle} must render direction `{dir}`:\n{}",
            out.stdout
        );
    }
}

#[test]
fn search_teammate_message_is_inbox_not_user_regression() {
    // GOLD §1 / oracle §H: an inbound `<teammate-message>` is `type:user/role:user/string` and
    // matches NO synthetic marker, so the OLD `is_genuine_user` counted it as the human. The
    // cutover classifies it `agent.communication.inbox` (from ⇨ self) and DROPS it from `user`.
    let h = Home::new();
    let sess = "11111111-2222-3333-4444-555555555555";
    let lines = [
        r#"{"type":"user","uuid":"u0","sessionId":"11111111-2222-3333-4444-555555555555","cwd":"/Users/x/team","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"the human asks about throughput"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
        r#"{"type":"user","uuid":"tm0","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"VSMultiRegion\" color=\"blue\">\nplease check the rate limit handling\n</teammate-message>"}}"#,
    ];
    h.write(
        &format!("-Users-x-team/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );

    // Under `-t agent.communication.inbox` it surfaces WITH the `from ⇨ to` direction.
    let inbox = h.run(&[
        "search",
        "rate limit",
        "-t",
        "agent.communication.inbox",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(inbox.success, "stderr: {}", inbox.stderr);
    assert!(
        inbox.stdout.contains("agent.communication.inbox"),
        "teammate message must classify as inbox; got: {}",
        inbox.stdout
    );
    assert!(
        inbox.stdout.contains("VSMultiRegion ⇨"),
        "the comm direction `from ⇨ to` must render; got: {}",
        inbox.stdout
    );

    // Under `-t user` it must NOT appear (the §1 bug fix) - the human turn does, the peer does not.
    let user = h.run(&[
        "search",
        "rate limit",
        "-t",
        "user",
        at(sess).as_str(),
        "--no-subagents",
    ]);
    assert!(user.success, "stderr: {}", user.stderr);
    assert!(
        user.stdout.contains("no matching exchanges"),
        "a teammate message must NOT surface under -t user; got: {}",
        user.stdout
    );
}

/// A SIGNAL's body is the payload the tag wrapped, never the wrapper.
///
/// `record_text_emission` gives `agent.communication.signal` its own arm - the opener
/// renderer, which flattens the record and strips the relay framing, with the raw text
/// only as a fallback. Without that arm the class falls to the engine's generic arm and
/// the body becomes the `<teammate-message …>` XML, tag and attributes and all: the same
/// loss the peer-render law names for every other inbound shape. The direction is
/// asserted elsewhere; this is about the BODY.
#[test]
fn a_teammate_signals_body_is_the_payload_not_the_wrapper() {
    let h = acceptance_home();
    let out = acc(&h, "zzidle", "agent.communication.signal");
    assert!(out.success, "stderr {}", out.stderr);
    let line = out
        .stdout
        .lines()
        .find(|l| l.contains("agent.communication.signal"))
        .unwrap_or_else(|| panic!("no signal hit:\n{}", out.stdout));
    assert!(
        line.contains(r#"{"type":"idle_notification""#),
        "the body is the wrapped payload:\n{line}"
    );
    assert!(
        !line.contains("<teammate-message") && !line.contains("teammate_id="),
        "and never the wrapper tag:\n{line}"
    );
}

/// An ABANDONED peer opener carries no comm direction, because its label is not a comm
/// label any more.
///
/// The scan layer replaces an abandoned opener's whole label set with `user.unsent` or
/// `user.rewound` - the record is no longer "an inbound message", it is a draft the
/// conversation left behind - and the direction is gated on the label set for exactly
/// that reason. A peer relay is the shape where the two disagree: the RECORD still has a
/// direction to compute, so a gate that answered for every class would print
/// `Zed ⇨ self` beside `user.unsent` and say the draft was delivered by someone.
#[test]
fn an_abandoned_peer_opener_carries_no_comm_direction() {
    let h = Home::new();
    let sess = "bbbbbbbb-1111-4222-8333-444444444444";
    let lines = [
        r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzdir kick off"}}"#,
        r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzdir ok"}]}}"#,
        // The DRAFT: a peer relay sharing its parent with the opener below and carrying no
        // assistant descendant, which is what makes it abandoned.
        r#"{"type":"user","uuid":"p1","parentUuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"<teammate-message teammate_id=\"Zed\">\nzzdirdraft please check the rate limit\n</teammate-message>"}}"#,
        r#"{"type":"user","uuid":"p2","parentUuid":"a1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":"zzdir the message that was sent"}}"#,
        r#"{"type":"assistant","uuid":"a2","parentUuid":"p2","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzdir done"}]}}"#,
    ];
    h.write(
        &format!("-Users-dev-example-project/{sess}.jsonl"),
        &format!("{}\n", lines.join("\n")),
    );
    let out = h.run(&["search", "zzdirdraft", &at(sess), "-t", "user.unsent"]);
    assert!(out.success, "stderr {}", out.stderr);
    let line = out
        .stdout
        .lines()
        .find(|l| l.contains("user.unsent"))
        .unwrap_or_else(|| panic!("no draft hit:\n{}", out.stdout));
    assert!(
        !line.contains("⇨"),
        "an abandoned opener is not an inbound message:\n{line}"
    );
    // And the LIVE peer relay in the same file does render one, so the assertion above is
    // about the abandoned label rather than about peer relays generally.
    let live = h.run(&["search", "zzdirdraft", &at(sess), "-t", "user.rewound"]);
    assert!(
        live.stdout.contains("no matching exchanges"),
        "the draft has no assistant below it, so it is unsent rather than rewound:\n{}",
        live.stdout
    );
}
