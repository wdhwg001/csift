//! Record-level delivery visibility: a bare ROLE selector selects what the model
//! received, so a `system`/`local_command` record surfaces under `-t harness` although
//! its leaf is invisible, while an `isVirtual` record and the `<synthetic>` API-error
//! placeholder drop out of `-t user` / `-t agent` although their leaves are visible.
//! Every other selector form keeps its full set, and JSON says so per hit.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "5c4b3a20-1d0e-4f9a-8b7c-6d5e4f3a2b1c";

/// Chained head to tail, as the harness writes them - a record nothing points at is
/// off the conversation chain and a bare ROLE selector skips it (the SURVIVAL AXIS).
/// L1 human · L2 assistant reply · L3 a delivered slash-command echo
/// (`system`/`local_command`) · L4 an UNdelivered notice (`system`/`informational`) ·
/// L5 the `<synthetic>` API-error placeholder · L6 an `isVirtual` user record.
fn delivery_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","model":"claude-x","content":[{"type":"text","text":"charting the lagoon now"}]}}"#, "\n",
            r#"{"type":"system","subtype":"local_command","level":"info","uuid":"s1","parentUuid":"a1","timestamp":"2026-06-07T05:00:10.000Z","content":"<local-command-stdout>lagoon depth 12m</local-command-stdout>","isMeta":false}"#, "\n",
            r#"{"type":"system","subtype":"informational","level":"warning","uuid":"s2","parentUuid":"s1","timestamp":"2026-06-07T05:00:11.000Z","content":"Remote Control disconnected from the lagoon rig"}"#, "\n",
            r#"{"type":"assistant","isApiErrorMessage":true,"uuid":"a2","parentUuid":"s2","timestamp":"2026-06-07T05:00:12.000Z","message":{"role":"assistant","model":"<synthetic>","content":[{"type":"text","text":"API Error: lagoon upstream timeout"}]}}"#, "\n",
            r#"{"type":"user","isVirtual":true,"uuid":"u2","parentUuid":"a2","timestamp":"2026-06-07T05:00:13.000Z","message":{"role":"user","content":"lagoon placeholder shown only in the UI"}}"#, "\n",
        ),
    );
    h
}

#[test]
fn a_bare_harness_role_surfaces_the_delivered_local_command_record() {
    let h = delivery_home();
    let out = h.run(&["search", "lagoon", &at(SESS), "-t", "harness"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("lagoon depth 12m"),
        "the model received the slash command's stdout:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("harness.meta.system"),
        "it keeps its leaf label:\n{}",
        out.stdout
    );
    // Every OTHER system subtype stays out: the leaf default still hides them.
    assert!(
        !out.stdout.contains("Remote Control disconnected"),
        "an undelivered notice must not ride the bare role:\n{}",
        out.stdout
    );
}

#[test]
fn the_explicit_leaf_and_the_glob_still_reach_every_system_subtype() {
    let h = delivery_home();
    for sel in ["harness.meta.system", "harness.meta", "harness.*"] {
        let out = h.run(&["search", "lagoon", &at(SESS), "-t", sel]);
        assert!(out.success, "{sel} stderr: {}", out.stderr);
        assert!(
            out.stdout.contains("lagoon depth 12m")
                && out.stdout.contains("Remote Control disconnected"),
            "{sel} must keep its full set:\n{}",
            out.stdout
        );
    }
}

#[test]
fn an_informational_notice_still_needs_an_explicit_selector() {
    let h = delivery_home();
    // A bare scan with no -t at all never parses the gated system lines.
    let bare = h.run(&["search", "lagoon", &at(SESS)]);
    assert!(bare.success, "stderr: {}", bare.stderr);
    assert!(
        !bare.stdout.contains("Remote Control disconnected")
            && !bare.stdout.contains("lagoon depth 12m"),
        "the gate is unchanged for a flagless scan:\n{}",
        bare.stdout
    );
    // And a bare `-t user` does not widen the gate either.
    let user = h.run(&["search", "lagoon", &at(SESS), "-t", "user"]);
    assert!(
        !user.stdout.contains("Remote Control disconnected")
            && !user.stdout.contains("lagoon depth 12m"),
        "{}",
        user.stdout
    );
}

#[test]
fn the_api_error_placeholder_leaves_the_bare_agent_role() {
    let h = delivery_home();
    let bare = h.run(&["search", "lagoon", &at(SESS), "-t", "agent"]);
    assert!(bare.success, "stderr: {}", bare.stderr);
    assert!(
        bare.stdout.contains("charting the lagoon now"),
        "the real reply is still there:\n{}",
        bare.stdout
    );
    assert!(
        !bare.stdout.contains("API Error: lagoon upstream timeout"),
        "a record the harness never sent is not the agent's conversation:\n{}",
        bare.stdout
    );
    // The explicit leaf reaches it, and the label zone says it was dropped.
    let leaf = h.run(&["search", "lagoon", &at(SESS), "-t", "agent.message"]);
    assert!(
        leaf.stdout.contains("API Error: lagoon upstream timeout")
            && leaf.stdout.contains("agent.message [not delivered]"),
        "{}",
        leaf.stdout
    );
    // So does the glob.
    let glob = h.run(&["search", "lagoon", &at(SESS), "-t", "agent.*"]);
    assert!(
        glob.stdout.contains("API Error: lagoon upstream timeout"),
        "{}",
        glob.stdout
    );
}

#[test]
fn an_is_virtual_record_leaves_the_bare_user_role() {
    let h = delivery_home();
    let bare = h.run(&["search", "lagoon", &at(SESS), "-t", "user"]);
    assert!(bare.success, "stderr: {}", bare.stderr);
    assert!(
        bare.stdout.contains("chart the lagoon"),
        "the human's own turn stays:\n{}",
        bare.stdout
    );
    assert!(
        !bare
            .stdout
            .contains("lagoon placeholder shown only in the UI"),
        "{}",
        bare.stdout
    );
    let leaf = h.run(&["search", "lagoon", &at(SESS), "-t", "user.message"]);
    assert!(
        leaf.stdout
            .contains("lagoon placeholder shown only in the UI")
            && leaf.stdout.contains("user.message [not delivered]"),
        "{}",
        leaf.stdout
    );
}

/// The §7f whole-file gate must not prune a file whose only hit sits in the FABRICATED
/// `[<subtype> <level>]` head: that text is not a raw byte substring of the line, so the
/// synthesized-marker set has to be as wide as the candidate gate that admitted it.
#[test]
fn a_pattern_matching_only_the_fabricated_head_survives_the_whole_file_gate() {
    let h = delivery_home();
    for sel in ["harness", "harness.meta.system"] {
        let out = h.run(&["search", r"\[local_command info\]", &at(SESS), "-t", sel]);
        assert!(out.success, "{sel} stderr: {}", out.stderr);
        assert!(
            out.stdout.contains("lagoon depth 12m"),
            "{sel} lost the record to the whole-file gate:\n{}\n{}",
            out.stdout,
            out.stderr
        );
    }
    // The same pattern under a selector that never admits the line is a clean absence.
    let user = h.run(&["search", r"\[local_command info\]", &at(SESS), "-t", "user"]);
    assert!(user.success, "stderr: {}", user.stderr);
    assert!(
        user.stdout.contains("no matching exchanges"),
        "{}",
        user.stdout
    );
}

/// The honest-empties keystone must not lie about what it looked at. A bare
/// `-t harness` PARSES the system lines, so a zero-match run there may not claim they
/// were never scanned; a bare `-t user` never touches them and still must say so.
#[test]
fn the_gated_leaves_note_follows_what_the_scan_actually_parsed() {
    let h = delivery_home();
    let harness = h.run(&["search", "zzznotpresent", &at(SESS), "-t", "harness"]);
    assert!(harness.success, "stderr: {}", harness.stderr);
    assert!(
        harness.stdout.contains("no matching exchanges"),
        "{}",
        harness.stdout
    );
    assert!(
        !harness.stderr.contains("the gated leaves ("),
        "a bare -t harness DID parse the system lines:\n{}",
        harness.stderr
    );
    let hj = h.run(&[
        "search",
        "zzznotpresent",
        &at(SESS),
        "-t",
        "harness",
        "--format",
        "json",
    ]);
    let summary: serde_json::Value = serde_json::from_str(hj.stdout.lines().last().unwrap_or(""))
        .expect("the trailing summary line");
    assert_eq!(summary["gated_leaves_unreached"], false, "{}", hj.stdout);
    assert_eq!(summary["definitive_absence"], true, "{}", hj.stdout);

    // A bare `-t user` reaches no gated leaf at all, so the note stands.
    let user = h.run(&["search", "zzznotpresent", &at(SESS), "-t", "user"]);
    assert!(
        user.stderr.contains("the gated leaves (") && user.stderr.contains("harness.meta.system"),
        "{}",
        user.stderr
    );
    let uj = h.run(&[
        "search",
        "zzznotpresent",
        &at(SESS),
        "-t",
        "user",
        "--format",
        "json",
    ]);
    let summary: serde_json::Value = serde_json::from_str(uj.stdout.lines().last().unwrap_or(""))
        .expect("the trailing summary line");
    assert_eq!(summary["gated_leaves_unreached"], true, "{}", uj.stdout);
}

#[test]
fn the_label_census_counts_what_the_bare_role_would_surface() {
    let h = delivery_home();
    let harness = h.run(&[
        "search",
        "lagoon",
        &at(SESS),
        "-t",
        "harness",
        "--count-by",
        "label",
    ]);
    assert!(harness.success, "stderr: {}", harness.stderr);
    assert!(
        harness.stdout.contains("harness.meta.system"),
        "the delivered record is counted under its leaf:\n{}",
        harness.stdout
    );
    let n: usize = harness
        .stdout
        .lines()
        .find(|l| l.contains("harness.meta.system"))
        .and_then(|l| l.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert_eq!(n, 1, "one delivered record, not both:\n{}", harness.stdout);
    // The explicit leaf counts both.
    let leaf = h.run(&[
        "search",
        "lagoon",
        &at(SESS),
        "-t",
        "harness.meta.system",
        "--count-by",
        "label",
    ]);
    let n: usize = leaf
        .stdout
        .lines()
        .find(|l| l.contains("harness.meta.system"))
        .and_then(|l| l.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert_eq!(n, 2, "{}", leaf.stdout);
    // The agent census loses the placeholder under the bare role.
    let agent = h.run(&[
        "search",
        "lagoon",
        &at(SESS),
        "-t",
        "agent",
        "--count-by",
        "label",
    ]);
    let n: usize = agent
        .stdout
        .lines()
        .find(|l| l.contains("agent.message"))
        .and_then(|l| l.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert_eq!(n, 1, "{}", agent.stdout);
}

#[test]
fn json_carries_delivered_on_every_hit_and_leaves_labels_alone() {
    let h = delivery_home();
    let out = h.run(&[
        "search",
        "lagoon",
        &at(SESS),
        "-t",
        "harness.meta.system",
        "-t",
        "agent.message",
        "-t",
        "user.message",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let mut seen = 0;
    for line in out.stdout.lines() {
        let v: serde_json::Value = serde_json::from_str(line).expect("jsonl");
        for hit in v["hits"].as_array().into_iter().flatten() {
            seen += 1;
            let delivered = hit["delivered"]
                .as_bool()
                .unwrap_or_else(|| panic!("every hit carries `delivered`: {hit}"));
            let excerpt = hit["excerpt"].as_str().unwrap_or_default();
            let label = hit["label"].as_str().unwrap_or_default();
            let expected = !(excerpt.contains("API Error: lagoon")
                || excerpt.contains("placeholder shown only in the UI")
                || excerpt.contains("Remote Control disconnected"));
            assert_eq!(delivered, expected, "{label}: {excerpt}");
            // The label SET is classification, never the delivery verdict.
            if excerpt.contains("lagoon depth 12m") {
                assert_eq!(
                    hit["labels"].as_array().map(Vec::len),
                    Some(1),
                    "the delivered record keeps its one leaf: {hit}"
                );
                assert_eq!(label, "harness.meta.system");
            }
        }
    }
    assert!(seen >= 5, "expected the whole fixture, saw {seen} hit(s)");
}

#[test]
fn a_rejected_tool_call_says_which_denial_kind_the_harness_stamped() {
    // A refused call leaves an errored `tool_result` whose carrier records the harness's
    // own `toolDenialKind`. The classifier's REASON never reaches disk, so this field is
    // the whole on-disk answer to why the call did not run.
    let enc = "-Users-dev-example-project";
    let sess = "7f6e5d4c-3b2a-4190-8f7e-6d5c4b3a2190";
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"11111111-1111-4111-8111-111111111111","parentUuid":null,"timestamp":"2026-06-26T09:00:00.000Z","message":{"role":"user","content":"clear the scratch dir"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"22222222-2222-4222-8222-222222222222","parentUuid":"11111111-1111-4111-8111-111111111111","timestamp":"2026-06-26T09:00:10.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_reject1","name":"Bash","input":{"command":"rm -rf $SCRATCH/*"}}]}}"#,
            "\n",
            r#"{"type":"user","uuid":"33333333-3333-4333-8333-333333333333","parentUuid":"22222222-2222-4222-8222-222222222222","timestamp":"2026-06-26T09:04:00.000Z","toolUseResult":"User rejected tool use","toolDenialKind":"user-rejected","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_reject1","is_error":true,"content":"The user doesn't want to proceed with this tool use."}]}}"#,
            "\n",
            r#"{"type":"user","uuid":"44444444-4444-4444-8444-444444444444","parentUuid":"33333333-3333-4333-8333-333333333333","timestamp":"2026-06-26T09:05:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_plain1","content":"ordinary output"}]}}"#,
            "\n"
        ),
    );

    let txt = h.run(&["search", "doesn't want to proceed", &format!("@{sess}")]);
    assert!(txt.success, "stderr: {}", txt.stderr);
    assert!(
        txt.stdout.contains("[denied: user-rejected]"),
        "the label zone names the denial kind:\n{}",
        txt.stdout
    );

    let js = h.run(&["search", "", &format!("@{sess}"), "--format", "json"]);
    assert!(js.success, "stderr: {}", js.stderr);
    let mut denied = 0;
    let mut plain = 0;
    for line in js.stdout.lines() {
        let v: serde_json::Value = serde_json::from_str(line).expect("jsonl");
        for hit in v["hits"].as_array().into_iter().flatten() {
            match hit["denial_kind"].as_str() {
                Some("user-rejected") => denied += 1,
                Some(other) => panic!("unexpected denial kind {other}: {hit}"),
                None => plain += 1,
            }
        }
    }
    assert_eq!(denied, 1, "exactly the rejected carrier is marked");
    assert!(plain >= 2, "every other hit carries a null denial_kind");

    // A denial is an ERROR result, so the `result` census keeps its two keys.
    let census = h.run(&[
        "search",
        "",
        &format!("@{sess}"),
        "--count-by",
        "result",
        "--format",
        "json",
    ]);
    assert!(census.success, "stderr: {}", census.stderr);
    assert!(
        census.stdout.contains("\"key\":\"error\"") && census.stdout.contains("\"key\":\"ok\""),
        "the result axis is unchanged by the denial field:\n{}",
        census.stdout
    );
}
