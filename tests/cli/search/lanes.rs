//! search classification across LANES: the subagent spawn-prompt seed, the fork clone, and the
//! inbound cross-session peer message another session's lane delivers into this one.

use crate::harness::*;

const PEER_ENC: &str = "-Users-dev-example-project";
const PEER_SESS: &str = "00000000-0000-4000-8000-0000000000c3";

/// A receiver transcript carrying the full on-disk shape of one delivered cross-session
/// message: the `queue-operation` enqueue rider that precedes it, then the `isMeta` user record
/// with its `origin` object, the relay preamble, the `<cross-session-message>` tag, the body,
/// the close tag and the security footer.
fn peer_receiver_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{PEER_ENC}/{PEER_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"queue-operation","operation":"enqueue","sessionId":"00000000-0000-4000-8000-0000000000c3","timestamp":"2026-06-07T05:00:01.000Z","content":"<cross-session-message from=\"uds:/Users/dev/relay.sock\" from-name=\"relay-7\" from-mode=\"bypass\">\nzzpeerbody the shared resolver landed\n</cross-session-message>"}"#, "\n",
            r#"{"type":"user","uuid":"p0","timestamp":"2026-06-07T05:00:02.000Z","isMeta":true,"promptSource":"system","userType":"external","queueSkipAttachments":true,"origin":{"kind":"peer","from":"uds:/Users/dev/relay.sock","verifiedPeerPid":4242,"msg_id":"00000000-0000-4000-8000-0000000000a1","name":"relay-7","fromMode":"bypass","body":"zzpeerbody the shared resolver landed"},"message":{"role":"user","content":"Another Claude session sent a message:\n<cross-session-message from=\"uds:/Users/dev/relay.sock\" from-name=\"relay-7\" from-mode=\"bypass\">\nzzpeerbody the shared resolver landed\n</cross-session-message>\n\nThis came from another Claude session."}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"p0","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"acknowledged"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn a_cross_session_peer_message_is_inbox_never_the_operator() {
    // C-30: the third peer framing. It is `type:user`/`role:user`/string and matches no
    // synthetic marker, so before it was folded into the peer detector the isMeta gate left
    // it with NO label at all - invisible to every search in the receiving session.
    let h = peer_receiver_home();
    let inbox = h.run(&[
        "search",
        "zzpeerbody",
        at(PEER_SESS).as_str(),
        "-t",
        "agent.communication.inbox",
    ]);
    assert!(inbox.success, "stderr: {}", inbox.stderr);
    assert!(
        inbox.stdout.contains("agent.communication.inbox"),
        "the delivered message classifies as an inbound comm:\n{}",
        inbox.stdout
    );
    assert!(
        inbox.stdout.contains("relay-7 ⇨ self"),
        "the direction names the sender SESSION, not its socket address:\n{}",
        inbox.stdout
    );
    assert!(
        !inbox.stdout.contains("cross-session-message"),
        "and the body renders without the wrapper XML:\n{}",
        inbox.stdout
    );

    for selector in ["user", "user.message"] {
        let out = h.run(&[
            "search",
            "zzpeerbody",
            at(PEER_SESS).as_str(),
            "-t",
            selector,
        ]);
        assert!(out.success, "stderr: {}", out.stderr);
        assert!(
            out.stdout.contains("no matching exchanges"),
            "-t {selector} must not surface a peer as the human:\n{}",
            out.stdout
        );
    }
}

#[test]
fn the_cross_session_direction_rides_the_json_hit() {
    let h = peer_receiver_home();
    let out = h.run(&[
        "search",
        "zzpeerbody",
        at(PEER_SESS).as_str(),
        "-t",
        "agent.communication.inbox",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows = json_rows(&out.stdout, "exchange");
    assert_eq!(rows.len(), 1, "one exchange in:\n{}", out.stdout);
    let hit = &rows[0]["hits"][0];
    assert_eq!(hit["label"], "agent.communication.inbox");
    assert_eq!(hit["from"], "relay-7");
    assert_eq!(hit["to"], "self");
    assert_eq!(hit["line"], 3);
}

#[test]
fn the_queue_rider_is_not_counted_as_the_humans_queued_text() {
    // The enqueue line carries the SAME framed content one line earlier. It is a harness
    // rider, not typed text, and the peer detector is what `user.queued` consults - so the
    // census shows the inbound comm and no `user.queued` key at all.
    let h = peer_receiver_home();
    let census = h.run(&[
        "search",
        "",
        at(PEER_SESS).as_str(),
        "-t",
        "user.queued",
        "--count-by",
        "label",
    ]);
    assert!(census.success, "stderr: {}", census.stderr);
    assert!(
        !census.stdout.contains("user.queued"),
        "a peer rider is never the human's queued text:\n{}",
        census.stdout
    );
    let all = h.run(&["search", "", at(PEER_SESS).as_str(), "--count-by", "label"]);
    assert!(all.success, "stderr: {}", all.stderr);
    assert!(
        all.stdout.contains("agent.communication.inbox"),
        "the delivered message is censused:\n{}",
        all.stdout
    );
    assert!(
        !all.stdout.contains("user.queued"),
        "and the rider adds no user key:\n{}",
        all.stdout
    );
}

#[test]
fn the_peer_record_refetches_by_its_own_line() {
    let h = peer_receiver_home();
    let out = h.run(&["show", at(PEER_SESS).as_str(), "--line", "3"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("agent.communication.inbox"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("relay-7 ⇨ self"), "{}", out.stdout);
    assert!(
        out.stdout.contains("zzpeerbody the shared resolver landed"),
        "{}",
        out.stdout
    );
}

#[test]
fn a_quoted_cross_session_tag_mid_prose_stays_the_human() {
    // FINDING-1 for the third framing: this repo's own docs quote the literal tag, so a
    // `contains` check would reclassify the operator's prose as an inbound peer message.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-0000000000c4";
    h.write(
        &format!("{PEER_ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzquotedtag csift classifies the <cross-session-message from=\"...\"> framing now."}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"noted"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "zzquotedtag",
        at(sess).as_str(),
        "-t",
        "user.message",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("user.message"),
        "a quoted tag mid-prose is still the human:\n{}",
        out.stdout
    );
}

#[test]
fn a_wrapper_attribute_is_unsearchable_and_raw_does_not_recover_it() {
    // T1. The body render strips the wrapper, and `search` matches RENDERED text only, so the
    // attributes left the matchable surface entirely. `--raw` is OUTPUT-only - the per-file
    // mmap backfill runs AFTER the match phase over lines that already matched - so it cannot
    // bring them back. Fails on the pre-fix render, which emitted the whole tagged section and
    // matched `teammate_id=` once.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-0000000000c5";
    h.write(
        &format!("{PEER_ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"user","uuid":"p0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"Another Claude session sent a message:\n<teammate-message teammate_id=\"team-lead\">\nzzwrapbody rebase onto main\n</teammate-message>"}}"#, "\n",
        ),
    );
    for pattern in ["teammate_id=", "team-lead"] {
        let plain = h.run(&["search", pattern, &at(sess)]);
        assert!(
            plain.success,
            "a definitive absence is exit 0: {}",
            plain.stderr
        );
        assert!(
            plain.stdout.contains("no matching exchanges"),
            "{pattern} is not in any matchable text:\n{}",
            plain.stdout
        );
        let raw = h.run(&["search", pattern, &at(sess), "--raw"]);
        assert!(raw.success, "stderr: {}", raw.stderr);
        assert!(
            raw.stdout.trim().is_empty(),
            "--raw prints the line of a HIT; with no hit there is nothing to print:\n{}",
            raw.stdout
        );
    }
    // The peer's own words match, with and without the flag - the loss is the wrapper only.
    let body = h.run(&["search", "zzwrapbody", &at(sess), "-c"]);
    assert_eq!(body.stdout.trim(), "1", "stderr: {}", body.stderr);
    let body_raw = h.run(&["search", "zzwrapbody", &at(sess), "--raw"]);
    assert_eq!(
        body_raw
            .stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count(),
        1,
        "one verbatim line for the one hit:\n{}",
        body_raw.stdout
    );
    assert!(
        body_raw.stdout.contains("teammate_id="),
        "and THAT line still carries the wrapper - it just cannot be matched on:\n{}",
        body_raw.stdout
    );
    // The address needs no match at all, which is the actual escape hatch.
    let addressed = h.run(&["show", &at(sess), "--line", "2", "--raw"]);
    assert!(addressed.success, "stderr: {}", addressed.stderr);
    assert!(
        addressed.stdout.contains("teammate_id="),
        "{}",
        addressed.stdout
    );
}

#[test]
fn the_stripped_body_stays_a_verbatim_substring_of_the_raw_line() {
    // T2. The section 7d/7f contract: the rendered body must be a VERBATIM substring of the
    // source line, because the literal prefilter and the whole-file gate scan RAW bytes. A body
    // carrying a JSON-escaped quote and an embedded newline is the shape that breaks a renderer
    // which unescapes or re-fabricates: the gate would prune the file and the hit would vanish
    // with no disclosure. Fails on any variant that renders a synthesized form without
    // registering a synth marker.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-0000000000c6";
    h.write(
        &format!("{PEER_ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"user","uuid":"p0","timestamp":"2026-06-07T05:00:01.000Z","isMeta":true,"origin":{"kind":"peer","from":"uds:/Users/dev/relay.sock","name":"relay-7"},"message":{"role":"user","content":"Another Claude session sent a message:\n<cross-session-message from=\"uds:/Users/dev/relay.sock\" from-name=\"relay-7\">\nhe said \"zzescaped\" out loud\nand then a second line: zznewline\n</cross-session-message>\n\nThis came from another Claude session."}}"#, "\n",
        ),
    );
    // A plain needle on either side of the escape is prefilter-eligible and must land.
    for needle in ["zzescaped", "zznewline"] {
        let out = h.run(&["search", needle, &at(sess), "-c"]);
        assert_eq!(
            out.stdout.trim(),
            "1",
            "{needle} must survive the prefilter and land: {}",
            out.stderr
        );
    }
    // The DISCRIMINATING half: these patterns exist in the RENDERED body but NOT contiguously in
    // the raw line - one spans the quote (raw: `said \"zzescaped\"`), the other spans the newline
    // (raw: a `\n` escape, collapsed to a space by the render). The gate scans RAW bytes, so it
    // may only derive a needle that is safe on both sides; a prefilter that took the rendered
    // form would find nothing in the file and prune it, losing the match with no disclosure.
    for spanning in ["said \"zzescaped\" out", "out loud and then a second"] {
        let out = h.run(&["search", spanning, &at(sess), "-c"]);
        assert_eq!(
            out.stdout.trim(),
            "1",
            "a pattern spanning a JSON escape must still match: {spanning:?}: {}",
            out.stderr
        );
    }
    // And the rendered excerpt really is the tag-stripped body, quote and all.
    let shown = h.run(&["show", &at(sess), "--line", "2"]);
    assert!(shown.success, "stderr: {}", shown.stderr);
    assert!(
        shown.stdout.contains("he said \"zzescaped\" out loud"),
        "the escaped quote renders as the byte it was:\n{}",
        shown.stdout
    );
    assert!(
        !shown.stdout.contains("cross-session-message"),
        "with no wrapper:\n{}",
        shown.stdout
    );
}

#[test]
fn all_five_open_tag_attributes_parse_in_their_fixed_order() {
    // T4. The writer pushes up to five attributes in ONE fixed order - from, from-session,
    // hop-chain, from-name, from-mode - each conditional. With the two middle ones present, a
    // naive attribute scan can slice the wrong value (`from="` must not match `from-session="`
    // or `from-name="`), and the body must strip past all five.
    //
    // DISCRIMINATION: this pins the attribute READER, not the detector. Swapping the two arms of
    // `cross_session_sender` so the ADDRESS is taken when the NAME is asked panics at the
    // `hit["from"]` assertion below (lanes.rs:334, left `uds:/Users/dev/relay.sock`, right
    // `relay-7`). Note what does NOT fail it: dropping `is_cross_session_message` from
    // `is_peer_message`, because `classify` reaches the section through `parse_all_peer_sections`
    // directly, never through that predicate - two unit tests cover that arm instead.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-0000000000c7";
    h.write(
        &format!("{PEER_ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"user","uuid":"p0","timestamp":"2026-06-07T05:00:01.000Z","isMeta":true,"promptSource":"system","userType":"external","message":{"role":"user","content":"Another Claude session sent a message:\n<cross-session-message from=\"uds:/Users/dev/relay.sock\" from-session=\"relay-seven\" hop-chain=\"aaaaaaaaaaaaaaaaaaaaaaaa,bbbbbbbbbbbbbbbbbbbbbbbb\" from-name=\"relay-7\" from-mode=\"bypass\">\nzzfiveattrs the shared resolver landed\n</cross-session-message>\n\nThis came from another Claude session."}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "zzfiveattrs",
        &at(sess),
        "-t",
        "agent.communication.inbox",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows = json_rows(&out.stdout, "exchange");
    assert_eq!(rows.len(), 1, "one exchange in:\n{}", out.stdout);
    let hit = &rows[0]["hits"][0];
    assert_eq!(hit["label"], "agent.communication.inbox");
    assert_eq!(
        hit["from"], "relay-7",
        "the NAME wins over the address even with from-session and hop-chain between them"
    );
    assert_eq!(hit["to"], "self");
    assert_eq!(
        hit["excerpt"], "zzfiveattrs the shared resolver landed",
        "and the body strips cleanly past all five attributes"
    );
}

#[test]
fn a_fork_clone_has_no_spawn_seed_but_a_genuine_child_does() {
    // A `/fork` child's line 1 is a `fork-context-ref` record and its transcript is a
    // clone of the parent's: its first turn-opener is the parent's own human message,
    // never a spawn-prompt seed (v0.10.2). A genuine Task child keeps the seed.
    let h = Home::new();
    let enc = "-Users-dev-example-project";
    let sess = "13131313-2424-4535-8646-757575757575";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"charting"}]}}"#, "\n",
        ),
    );
    let fork = "f0f0f0f0f0f0f0f01";
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{fork}.jsonl"),
        &format!(
            concat!(
                r#"{{"type":"fork-context-ref","agentId":"{fork}","parentSessionId":"{sess}","parentLastUuid":"a0","contextLength":120}}"#, "\n",
                r#"{{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"chart the reef"}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"charting"}}]}}}}"#, "\n",
            ),
            fork = fork,
            sess = sess
        ),
    );
    let genuine = "c1c1c1c1c1c1c1c1";
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{genuine}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"s0","isSidechain":true,"timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"chart the reef for the parent"}}"#, "\n",
        ),
    );
    let j = h.run(&["search", "chart the reef", &at(sess), "--format", "json"]);
    assert!(j.success, "stderr: {}", j.stderr);
    let mut by_lane: Vec<(String, String)> = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|r| r["kind"] == "exchange")
        .flat_map(|r| {
            let sid = r["session_id"].as_str().unwrap_or("").to_string();
            r["hits"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(move |h| (sid.clone(), h["label"].as_str().unwrap_or("").to_string()))
        })
        .collect();
    by_lane.sort();
    let of = |lane: &str| -> Vec<&str> {
        by_lane
            .iter()
            .filter(|(s, _)| s == lane)
            .map(|(_, l)| l.as_str())
            .collect()
    };
    assert_eq!(
        of(fork),
        vec!["user.message"],
        "the clone's opener is the human's message: {}",
        j.stdout
    );
    assert_eq!(
        of(genuine),
        vec!["agent.communication.inbox"],
        "the genuine child keeps its spawn seed: {}",
        j.stdout
    );
    assert_eq!(of(sess), vec!["user.message"], "{}", j.stdout);
}
