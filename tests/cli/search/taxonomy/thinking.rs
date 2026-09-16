//! The THINKING family end to end: the four `agent.thinking.*` leaves, the two markers
//! that name text the CLIENT wrote into the assistant's role, the two per-hit JSON keys,
//! and the reachability consequence of an empty body.
//!
//! Every line is built from the on-disk shape the inventory names, never copied from a
//! corpus. The two SIGNATURES are hand-emitted protobuf (the field 2 -> 1 -> 8 tag path
//! `model/narration.rs` decodes) frozen here as base64 constants, so a reader can see the
//! whole fixture on one screen: `CAISDAoKQgh0aGlua2luZw==` decodes to the tag `thinking`
//! and `CAISDQoLQgluYXJyYXRpb24=` to `narration`. Both are 24 characters, which is what
//! `signature_len` reports for them.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "7f6e5d4c-3b2a-4109-8765-43210fedcba9";
/// The tag path field 2 -> 1 -> 8 carrying the word `thinking`.
const SIG_THINKING: &str = "CAISDAoKQgh0aGlua2luZw==";
/// The same path carrying `narration`.
const SIG_NARRATION: &str = "CAISDQoLQgluYXJyYXRpb24=";

/// One transcript holding every shape the family owns, one per line:
///
/// L1 the human turn - L2 `agent.thinking.summary` (a signed block with text) -
/// L3 `agent.thinking.narration` (the narration-tagged signature) -
/// L4 `agent.thinking.omitted` (signed, EMPTY text) - L5 `agent.thinking.redacted` (the
/// documented ciphertext shape) - L6 the cancel SALVAGE block (the `<synthetic>` sentinel
/// beside a present-and-empty signature) - L7 the strip path's `[Thinking removed]`
/// stand-in - L8 an assistant turn that QUOTES that phrase.
fn family_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!(
            concat!(
                r#"{{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"chart the reef"}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"thinking","thinking":"placeholder reasoning about the reef","signature":"{t}"}}]}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"a2","parentUuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{{"role":"assistant","content":[{{"type":"thinking","thinking":"placeholder narration summary line","signature":"{n}"}}]}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"a3","parentUuid":"a2","timestamp":"2026-06-07T05:00:03.000Z","message":{{"role":"assistant","content":[{{"type":"thinking","thinking":"","signature":"{t}"}}]}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"a4","parentUuid":"a3","timestamp":"2026-06-07T05:00:04.000Z","message":{{"role":"assistant","content":[{{"type":"redacted_thinking","data":"EncryptedOpaqueBlob=="}}]}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"a5","parentUuid":"a4","timestamp":"2026-06-07T05:00:05.000Z","message":{{"role":"assistant","model":"<synthetic>","id":"11111111-2222-4333-8444-555555555555","stop_reason":"stop_sequence","content":[{{"type":"thinking","thinking":"placeholder partial reasoning the person cancelled","signature":""}}]}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"a6","parentUuid":"a5","timestamp":"2026-06-07T05:00:06.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"[Thinking removed]"}}]}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"a7","parentUuid":"a6","timestamp":"2026-06-07T05:00:07.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"the client pushes [Thinking removed] when it strips every block"}}]}}}}"#, "\n",
            ),
            t = SIG_THINKING,
            n = SIG_NARRATION
        ),
    );
    h
}

/// The `label` census keys the four leaves produce, and the two bare-face selectors that
/// split them. The counts are derived from the fixture, not from a run: one line per leaf,
/// and the summary leaf is carried twice - the ordinary block and the salvage one, which
/// keeps that leaf because the block IS a summary slot in the message.
#[test]
fn the_four_leaves_are_census_keys_and_the_bare_face_shows_two() {
    let h = family_home();
    let all = h.run(&[
        "search",
        "",
        "-t",
        "agent.thinking.*",
        &at(SESS),
        "--count-by",
        "label",
        "--format",
        "json",
        "--no-subagents",
    ]);
    assert!(all.success, "stderr: {}", all.stderr);
    let mut by: Vec<(String, u64)> = all
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["axis"] == "label" && v["key"].is_string())
        .map(|v| {
            (
                v["key"].as_str().unwrap_or_default().to_string(),
                v["records"].as_u64().unwrap_or_default(),
            )
        })
        .collect();
    by.sort();
    assert_eq!(
        by,
        vec![
            ("agent.thinking.narration".to_string(), 1),
            ("agent.thinking.omitted".to_string(), 1),
            ("agent.thinking.redacted".to_string(), 1),
            ("agent.thinking.summary".to_string(), 2),
        ]
    );
    // The BARE category is the LLM-visible face, and the two textless leaves are not on
    // it: a reader asking for "the reasoning" gets the blocks that carry some.
    let bare = h.run(&[
        "search",
        "",
        "-t",
        "agent.thinking",
        &at(SESS),
        "--count-by",
        "label",
        "--format",
        "json",
        "--no-subagents",
    ]);
    assert!(bare.success, "stderr: {}", bare.stderr);
    let keys: Vec<String> = bare
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["axis"] == "label" && v["key"].is_string())
        .map(|v| v["key"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(
        keys.contains(&"agent.thinking.summary".to_string())
            && keys.contains(&"agent.thinking.narration".to_string()),
        "the bare face carries the two leaves that hold text; got {keys:?}"
    );
    assert!(
        !keys
            .iter()
            .any(|k| k.ends_with(".omitted") || k.ends_with(".redacted")),
        "the bare face must not carry a textless leaf; got {keys:?}"
    );
}

/// Each leaf's JSON spelling, its body, and the two per-hit keys - read off the ADDRESS,
/// which is where a consumer lands after following a hit's `refetch`.
#[test]
fn a_show_address_renders_each_leafs_body_and_keys() {
    let h = family_home();
    let out = h.run(&["show", &at(SESS), "--line", "2..6", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let recs: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "record")
        .collect();
    assert_eq!(recs.len(), 5, "five addressed units; got {}", recs.len());
    // L2 the ordinary summary: its own text is the body, and the signature length is the
    // CHARACTER count of the fixture's own 24-character base64 signature.
    assert_eq!(recs[0]["label"], "agent.thinking.summary");
    assert_eq!(recs[0]["body"], "placeholder reasoning about the reef");
    assert_eq!(recs[0]["signature_len"], 24);
    assert!(recs[0]["thinking_text_absent"].is_null());
    // L3 the narration summary: same shape, the tag in the signature is the only
    // difference on the wire.
    assert_eq!(recs[1]["label"], "agent.thinking.narration");
    assert_eq!(recs[1]["signature_len"], 24);
    // L4 the omitted leaf: an EMPTY body, the absence stated as a key, and the marker in
    // the label zone rather than in the text.
    assert_eq!(recs[2]["label"], "agent.thinking.omitted");
    assert_eq!(recs[2]["body"], "");
    assert_eq!(recs[2]["thinking_text_absent"], true);
    assert_eq!(recs[2]["signature_len"], 24);
    // L5 the redacted leaf: an empty body, no signature field in its shape and so no key,
    // and the ciphertext on no path but --raw.
    assert_eq!(recs[3]["label"], "agent.thinking.redacted");
    assert_eq!(recs[3]["body"], "");
    assert!(recs[3]["signature_len"].is_null());
    assert!(!out.stdout.contains("EncryptedOpaqueBlob"));
    // L6 the salvage block: the summary leaf, its text kept, and a signature that is
    // PRESENT and empty - which is 0 and never null.
    assert_eq!(recs[4]["label"], "agent.thinking.summary");
    assert_eq!(recs[4]["signature_len"], 0);
    assert_eq!(
        recs[4]["body"],
        "placeholder partial reasoning the person cancelled"
    );
}

/// The label-zone renders, in the text surface a person reads.
#[test]
fn the_text_render_names_each_shape_in_the_label_zone() {
    let h = family_home();
    let out = h.run(&["show", &at(SESS), "--line", "2..8"]);
    assert!(out.success, "stderr: {}", out.stderr);
    for want in [
        "[no summary text]",
        "[redacted thinking]",
        "[cancelled stream salvage]",
        "[thinking removed by the client]",
    ] {
        assert!(
            out.stdout.contains(want),
            "missing {want} in: {}",
            out.stdout
        );
    }
    // The stand-in marker is keyed on the WHOLE trimmed text, so L8 - an assistant turn
    // that quotes the phrase inside a longer message - is the assistant speaking.
    assert_eq!(
        out.stdout
            .matches("[thinking removed by the client]")
            .count(),
        1,
        "only the stand-in itself takes the marker: {}",
        out.stdout
    );
}

/// The salvage block is DELIVERED, so a bare `agent` reaches it. The flag that kept it out
/// of the cancelling session's own request (`isVirtual`) is stripped on the persist path
/// and reaches no line, so csift reports what the transcript carries rather than a verdict
/// read off a field that is not there.
#[test]
fn a_bare_agent_reaches_the_salvage_block() {
    let h = family_home();
    let out = h.run(&[
        "search",
        "",
        "-t",
        "agent",
        &at(SESS),
        "--format",
        "json",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let salvaged: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["hits"].as_array().cloned())
        .flatten()
        .filter(|hit| hit["signature_len"] == 0)
        .collect();
    assert_eq!(salvaged.len(), 1, "the salvage unit is on the bare face");
    assert_eq!(salvaged[0]["label"], "agent.thinking.summary");
    assert_eq!(salvaged[0]["delivered"], true);
}

/// The reachability consequence of an empty body, pinned rather than left to be
/// discovered: a pattern scan cannot find either textless leaf, and the ciphertext is not
/// matchable either - so `search` says a definitive absence and the ways in are an empty
/// pattern, the census, or an address.
#[test]
fn a_textless_leaf_is_unreachable_by_a_pattern_scan() {
    let h = family_home();
    for pattern in ["redacted", "EncryptedOpaqueBlob"] {
        let out = h.run(&[
            "search",
            pattern,
            "-t",
            "agent.thinking.*",
            &at(SESS),
            "--no-subagents",
        ]);
        assert!(out.success, "stderr: {}", out.stderr);
        assert!(
            out.stdout.contains("no matching exchanges"),
            "pattern {pattern} must find nothing rendered; got: {}",
            out.stdout
        );
    }
    // And the escape hatch works: --raw on the address hands back the bytes.
    let raw = h.run(&["show", &at(SESS), "--line", "5", "--raw"]);
    assert!(raw.success, "stderr: {}", raw.stderr);
    assert!(raw.stdout.contains("EncryptedOpaqueBlob"));
}
