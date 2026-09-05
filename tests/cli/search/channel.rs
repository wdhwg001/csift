//! `agent.communication.channel`: a csift-channel delivery is the one attachment leaf a
//! DEFAULT scan surfaces, it renders its envelope verbatim with the sender's direction, it
//! keeps its harness twin behind that leaf's own flag, and prose quoting the header is not it.

use crate::harness::*;

const ENC: &str = "-Users-dev-relay-harbor";
const SESS: &str = "00000000-0000-4000-8000-000000000011";
const RELAY: &str = "aRelay-0123456789abcdef";

/// L1 user opener · L2 a delivery's first chunk (a full header, so it names its sender) ·
/// L3 an ordinary hook context (the control) · L4 the delivery's continuation chunk (no
/// sender in the header) · L5 a human quoting the envelope in prose · L6 assistant text.
fn channel_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start the relay work"}}"#, "\n",
            r#"{"type":"attachment","uuid":"att1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","attachment":{"type":"hook_additional_context","hookEvent":"UserPromptSubmit","hookName":"csift deliver --slot 1","content":["[csift-channel v1 id=0123456789abcdef part=1/2 mode=steer from=aRelay-0123456789abcdef from-session=00000000 relation=sibling to=00000000-0000-4000-8000-000000000011]\nThis message is not from your user and not from the harness. It was sent by the lane named above through csift.\n--- message ---\nthrottlebeacon the region queue"]}}"#, "\n",
            r#"{"type":"attachment","uuid":"att2","parentUuid":"u1","timestamp":"2026-06-07T05:00:02.000Z","attachment":{"type":"hook_additional_context","hookEvent":"SessionStart","content":["mistgate token applies"]}}"#, "\n",
            r#"{"type":"attachment","uuid":"att3","parentUuid":"u1","timestamp":"2026-06-07T05:00:03.000Z","attachment":{"type":"hook_additional_context","hookEvent":"PostToolUse","content":["[csift-channel v1 id=0123456789abcdef part=2/2]\nregionsweep before the throttle\n--- end ---"]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"the header reads [csift-channel v1 id=0123456789abcdef from=aRelay-0123456789abcdef] - quotedbeacon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u2","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"done with the relay work"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn a_delivery_is_visible_to_a_default_scan() {
    // The whole point of the leaf: a message another lane had injected into this one is
    // found by an ordinary query, with no flag to know about. Its envelope renders
    // VERBATIM and its sender rides the comm direction.
    let h = channel_home();
    let out = h.run(&["search", "throttlebeacon", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("agent.communication.channel")
            && out.stdout.contains(&format!("{RELAY} ⇨ self"))
            && out.stdout.contains("L2")
            && out
                .stdout
                .contains("[csift-channel v1 id=0123456789abcdef part=1/2"),
        "default scan surfaces the delivery verbatim:\n{}",
        out.stdout
    );
    // The control: an ORDINARY hook context is still invisible without its own flag, so
    // the channel keep widened nothing else.
    let plain = h.run(&["search", "mistgate", &at(SESS)]);
    assert!(plain.success, "stderr: {}", plain.stderr);
    assert!(
        plain.stdout.contains("no matching exchanges"),
        "an ordinary hook context stays behind --additional-context:\n{}",
        plain.stdout
    );
}

#[test]
fn the_leaf_answers_the_role_the_prefix_and_the_full_path() {
    let h = channel_home();
    for sel in [
        "agent",
        "agent.communication",
        "agent.communication.channel",
    ] {
        let out = h.run(&["search", "throttlebeacon", &at(SESS), "-t", sel]);
        assert!(out.success, "stderr: {}", out.stderr);
        assert!(
            out.stdout.contains("agent.communication.channel"),
            "-t {sel} must reach the leaf:\n{}",
            out.stdout
        );
    }
    // The leaf is LLM-visible, so the bare role reaches it without a glob.
    let excluded = h.run(&[
        "search",
        "throttlebeacon",
        &at(SESS),
        "-T",
        "agent.communication.channel",
    ]);
    assert!(
        excluded.stdout.contains("no matching exchanges"),
        "-T excludes the leaf and the record with it:\n{}",
        excluded.stdout
    );
}

#[test]
fn the_harness_twin_stays_behind_its_own_flag() {
    let h = channel_home();
    // `-t harness` cannot reach the message view, and the hook view still needs
    // --additional-context - so a harness-role query surfaces nothing.
    let harness = h.run(&["search", "throttlebeacon", &at(SESS), "-t", "harness"]);
    assert!(harness.success, "stderr: {}", harness.stderr);
    assert!(
        harness.stdout.contains("no matching exchanges"),
        "the harness role never surfaces a delivery:\n{}",
        harness.stdout
    );
    // With the flag, the SAME record renders under its harness leaf (the dual label).
    let hook = h.run(&[
        "search",
        "throttlebeacon",
        &at(SESS),
        "--additional-context",
        "-t",
        "harness.meta.hook",
    ]);
    assert!(
        hook.stdout.contains("harness.meta.hook") && hook.stdout.contains("throttlebeacon"),
        "the hook view is still reachable:\n{}",
        hook.stdout
    );
}

#[test]
fn the_census_counts_every_chunk_under_the_leaf() {
    let h = channel_home();
    let out = h.run(&["search", "", &at(SESS), "--count-by", "label"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("agent.communication.channel"),
        "the leaf is a census key:\n{}",
        out.stdout
    );
    let outj = h.run(&[
        "search",
        "",
        &at(SESS),
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    let rows: Vec<serde_json::Value> = outj
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(
        rows.iter().any(|r| r["kind"] == "census"
            && r["axis"] == "label"
            && r["key"] == "agent.communication.channel"
            && r["records"] == 2),
        "both chunks count under the leaf:\n{}",
        outj.stdout
    );
}

#[test]
fn json_carries_the_leaf_the_sender_and_an_unnamed_continuation() {
    let h = channel_home();
    let out = h.run(&["search", "region", &at(SESS), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hits: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|r| r["kind"] == "exchange")
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .collect();
    let first = hits
        .iter()
        .find(|h| h["line"] == 2)
        .expect("the first chunk hit");
    assert_eq!(first["label"], "agent.communication.channel");
    assert_eq!(
        first["labels"],
        serde_json::json!(["agent.communication.channel", "harness.meta.hook"]),
        "the record carries both views, message first"
    );
    assert_eq!(first["from"], RELAY, "the header's from= is the sender");
    assert_eq!(first["to"], "self");
    // A CONTINUATION chunk names no sender, so no direction is fabricated for it.
    let cont = hits
        .iter()
        .find(|h| h["line"] == 4)
        .expect("the continuation hit");
    assert_eq!(cont["label"], "agent.communication.channel");
    assert!(
        cont["from"].is_null() && cont["to"].is_null(),
        "an unnamed sender is reported as no direction: {cont}"
    );
}

#[test]
fn prose_quoting_the_envelope_keeps_its_authors_label() {
    // The detector keys on the attachment payload, never on the text of a message - csift's
    // own sessions quote the envelope constantly.
    let h = channel_home();
    let out = h.run(&["search", "quotedbeacon", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("user.message") && !out.stdout.contains("agent.communication.channel"),
        "a quoted header is the human's own message:\n{}",
        out.stdout
    );
}

#[test]
fn show_renders_an_addressed_delivery_flag_free() {
    // The refetch law: the `csift show` command a hit prints resolves with no flag.
    let h = channel_home();
    let out = h.run(&["show", &at(SESS), "--line", "2"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("agent.communication.channel")
            && out.stdout.contains("throttlebeacon the region queue"),
        "an addressed delivery renders flag-free:\n{}",
        out.stdout
    );
}

#[test]
fn a_hook_context_that_merely_mentions_the_literal_stays_gated() {
    // The channel keep is DEFAULT-ON and admits any line carrying the envelope literal,
    // so an ordinary hook context that only MENTIONS it is parsed too. It must not leak
    // into a flagless scan as bare `harness.meta.hook`: the hook leaf keeps its own flag.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start the relay work"}}"#, "\n",
            r#"{"type":"attachment","uuid":"att9","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","attachment":{"type":"hook_additional_context","hookEvent":"SessionStart","content":["mentionbeacon: the marker [csift-channel v1 id=... is what a delivery opens with"]}}"#, "\n",
        ),
    );
    let bare = h.run(&["search", "mentionbeacon", &at(SESS)]);
    assert!(bare.success, "stderr: {}", bare.stderr);
    assert!(
        bare.stdout.contains("no matching exchanges"),
        "a mere mention never leaks into a flagless scan:\n{}",
        bare.stdout
    );
    // Under its own flag the same record is the hook leaf and nothing more.
    let flagged = h.run(&["search", "mentionbeacon", &at(SESS), "--additional-context"]);
    assert!(flagged.success, "stderr: {}", flagged.stderr);
    assert!(
        flagged.stdout.contains("harness.meta.hook")
            && !flagged.stdout.contains("agent.communication.channel"),
        "under --additional-context it is the hook view, not a delivery:\n{}",
        flagged.stdout
    );
}
