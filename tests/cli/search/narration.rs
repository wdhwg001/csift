//! The agent.thinking.narration leaf: signature-driven split, census conservation,
//! prefix selection, span, render, and the verbatim exclusion pin (design probes 1-5).
//!
//! Signatures are HAND-EMITTED protobuf (field 2 -> 1 -> 8 tag path) with neutral
//! text - never bytes from a real transcript.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "9c8b7a6d-5e4f-4321-a098-76543210fedc";
const SUB: &str = "0badc0ffee123456";

fn varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    }
    out
}

fn field(num: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = varint((num << 3) | 2);
    out.extend(varint(payload.len() as u64));
    out.extend_from_slice(payload);
    out
}

fn b64(bytes: &[u8]) -> String {
    const AL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(AL[(n >> 18) as usize & 63] as char);
        out.push(AL[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            AL[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            AL[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn sig(tag: &str) -> String {
    let mut outer = vec![0x08, 0x02];
    outer.extend(field(2, &field(1, &field(8, tag.as_bytes()))));
    b64(&outer)
}

/// L1 user turn; L2 reasoning (tag thinking); L3 narration summary; L4 narration with
/// EMPTY text; L5 unparseable signature (degrades to plain thinking); L6 agent prose.
fn narration_home() -> Home {
    let h = Home::new();
    let t = sig("thinking");
    let n = sig("narration");
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!(concat!(
            r#"{{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"chart the reef"}}}}"#, "\n",
            r#"{{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"thinking","thinking":"placeholder reasoning about the reef","signature":"{t}"}}]}}}}"#, "\n",
            r#"{{"type":"assistant","uuid":"a2","parentUuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{{"role":"assistant","content":[{{"type":"thinking","thinking":"placeholder narration summary line","signature":"{n}"}}]}}}}"#, "\n",
            r#"{{"type":"assistant","uuid":"a3","parentUuid":"a2","timestamp":"2026-06-07T05:00:03.000Z","message":{{"role":"assistant","content":[{{"type":"thinking","thinking":"","signature":"{n}"}}]}}}}"#, "\n",
            r#"{{"type":"assistant","uuid":"a4","parentUuid":"a3","timestamp":"2026-06-07T05:00:04.000Z","message":{{"role":"assistant","content":[{{"type":"thinking","thinking":"placeholder torn-signature reasoning","signature":"!!!not-base64!!!"}}]}}}}"#, "\n",
            r#"{{"type":"assistant","uuid":"a5","parentUuid":"a4","timestamp":"2026-06-07T05:00:05.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"the reef is charted"}}]}}}}"#, "\n",
        ), t = t, n = n),
    );
    h
}

#[test]
fn probe1_show_labels_the_discriminating_pair() {
    let h = narration_home();
    let out = h.run(&["show", &at(SESS), "--line", "2..3", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let recs: Vec<&serde_json::Value> = rows.iter().filter(|r| r["kind"] == "record").collect();
    assert_eq!(recs.len(), 2);
    assert_eq!(recs[0]["label"], "agent.thinking", "{}", out.stdout);
    assert_eq!(
        recs[1]["label"], "agent.thinking.narration",
        "{}",
        out.stdout
    );
    // labels[] stays single-element for both: no dual labeling.
    for r in recs {
        assert_eq!(r["labels"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn probe2_census_conservation_and_degradation() {
    let h = narration_home();
    let out = h.run(&["search", "", &at(SESS), "--count-by", "label"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // 2 reasoning (the thinking-tagged + the unparseable-signature record) vs 2 narration
    // (one with EMPTY text): thinking+narration == the 4 a 0.9.1 binary counted as one key.
    assert!(
        out.stdout.contains("2  agent.thinking\n") || out.stdout.contains("2 agent.thinking"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("agent.thinking.narration"),
        "{}",
        out.stdout
    );
    let json = h.run(&[
        "search",
        "",
        &at(SESS),
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    let mut thinking = 0u64;
    let mut narration = 0u64;
    for line in json.stdout.lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        if v["kind"] == "census" {
            match v["key"].as_str() {
                Some("agent.thinking") => thinking = v["records"].as_u64().unwrap(),
                Some("agent.thinking.narration") => narration = v["records"].as_u64().unwrap(),
                _ => {}
            }
        }
    }
    assert_eq!((thinking, narration), (2, 2), "{}", json.stdout);
}

#[test]
fn probe3_prefix_selects_both_and_subtraction_isolates_reasoning() {
    let h = narration_home();
    let both = h.run(&[
        "search",
        "",
        &at(SESS),
        "--count-by",
        "label",
        "-t",
        "agent.thinking",
    ]);
    assert!(both.success, "stderr: {}", both.stderr);
    assert!(
        both.stdout.contains("agent.thinking") && both.stdout.contains("agent.thinking.narration"),
        "prefix selects both leaves:\n{}",
        both.stdout
    );
    let pure = h.run(&[
        "search",
        "",
        &at(SESS),
        "--count-by",
        "label",
        "-t",
        "agent.thinking",
        "-T",
        "agent.thinking.narration",
    ]);
    assert!(pure.success, "stderr: {}", pure.stderr);
    assert!(
        pure.stdout.contains("agent.thinking") && !pure.stdout.contains("agent.thinking.narration"),
        "subtraction isolates pure reasoning:\n{}",
        pure.stdout
    );
    // Direct leaf selection finds the narration text; the marker rides the label zone.
    let hit = h.run(&[
        "search",
        "narration summary line",
        &at(SESS),
        "-t",
        "agent.thinking.narration",
    ]);
    assert!(
        hit.stdout
            .contains("agent.thinking.narration [narration summary]"),
        "label-zone marker:\n{}",
        hit.stdout
    );
}

#[test]
fn probe4_verbatim_never_replays_narration() {
    let h = narration_home();
    let out = h.run(&["verbatim", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("placeholder narration summary line"),
        "narration is a UI affordance, never conversation content:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("the reef is charted"),
        "agent prose still replays:\n{}",
        out.stdout
    );
}

#[test]
fn probe5_span_counts_subagent_narration() {
    let h = narration_home();
    let n = sig("narration");
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{SUB}.jsonl"),
        &format!(concat!(
            r#"{{"type":"user","uuid":"su1","timestamp":"2026-06-07T05:00:06.000Z","message":{{"role":"user","content":"child task"}}}}"#, "\n",
            r#"{{"type":"assistant","uuid":"sa1","parentUuid":"su1","timestamp":"2026-06-07T05:00:07.000Z","message":{{"role":"assistant","content":[{{"type":"thinking","thinking":"child narration placeholder","signature":"{n}"}}]}}}}"#, "\n",
        ), n = n),
    );
    let spanned = h.run(&[
        "search",
        "",
        &at(SESS),
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    let narration_count = |stdout: &str| -> u64 {
        stdout
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|v| v["kind"] == "census" && v["key"] == "agent.thinking.narration")
            .and_then(|v| v["records"].as_u64())
            .unwrap_or(0)
    };
    assert_eq!(narration_count(&spanned.stdout), 3, "{}", spanned.stdout);
    let top = h.run(&[
        "search",
        "",
        &at(SESS),
        "--no-subagents",
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    assert_eq!(narration_count(&top.stdout), 2, "{}", top.stdout);
}

#[test]
fn stats_counts_narration_blocks_per_model_and_flags_unknown_tags() {
    let h = Home::new();
    let t = sig("thinking");
    let n = sig("narration");
    let x = sig("future-tag");
    let stats_sess = "5a4b3c2d-1e0f-4987-b654-3210fedcba98";
    h.write(
        &format!("{ENC}/{stats_sess}.jsonl"),
        &format!(concat!(
            r#"{{"type":"assistant","uuid":"s1","timestamp":"2026-06-07T05:00:01.000Z","message":{{"role":"assistant","id":"msg_s1","model":"claude-test-1","content":[{{"type":"thinking","thinking":"placeholder reasoning","signature":"{t}"}}]}}}}"#, "\n",
            r#"{{"type":"assistant","uuid":"s2","timestamp":"2026-06-07T05:00:02.000Z","message":{{"role":"assistant","id":"msg_s1","model":"claude-test-1","content":[{{"type":"thinking","thinking":"placeholder summary","signature":"{n}"}}]}}}}"#, "\n",
            r#"{{"type":"assistant","uuid":"s3","timestamp":"2026-06-07T05:00:03.000Z","message":{{"role":"assistant","id":"msg_s2","model":"claude-test-2","content":[{{"type":"thinking","thinking":"placeholder summary two","signature":"{n}"}}]}}}}"#, "\n",
            r#"{{"type":"assistant","uuid":"s4","timestamp":"2026-06-07T05:00:04.000Z","message":{{"role":"assistant","id":"msg_s3","model":"claude-test-1","content":[{{"type":"thinking","thinking":"placeholder oddity","signature":"{x}"}}]}}}}"#, "\n",
        ), t = t, n = n, x = x),
    );
    let out = h.run(&["stats", &format!("@{stats_sess}"), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let row: serde_json::Value = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("session row");
    assert_eq!(
        row["narration_blocks"]["claude-test-1"], 1,
        "{}",
        out.stdout
    );
    assert_eq!(
        row["narration_blocks"]["claude-test-2"], 1,
        "{}",
        out.stdout
    );
    assert_eq!(
        row["unknown_thinking_tags"], 1,
        "a future tag value surfaces without a release: {}",
        out.stdout
    );
    let text = h.run(&["stats", &format!("@{stats_sess}")]);
    assert!(
        text.stdout.contains("narration blocks")
            && text.stdout.contains("claude-test-1×1")
            && text.stdout.contains("unknown thinking-signature tags 1"),
        "{}",
        text.stdout
    );
}

#[test]
fn stats_narration_total_merges_across_sessions_and_absence_prints_nothing() {
    let h = Home::new();
    let n = sig("narration");
    let s1 = "7e6f5a4b-3c2d-4109-8765-43210fedcba9";
    let s2 = "8f709a1b-4d3e-4210-9876-543210fedcb0";
    for (sess, model) in [(s1, "claude-test-1"), (s2, "claude-test-1")] {
        h.write(
            &format!("{ENC}/{sess}.jsonl"),
            &format!(concat!(
                r#"{{"type":"assistant","uuid":"t1","timestamp":"2026-06-07T05:00:01.000Z","message":{{"role":"assistant","id":"msg_t1","model":"{model}","content":[{{"type":"thinking","thinking":"placeholder summary","signature":"{n}"}}]}}}}"#, "\n",
            ), model = model, n = n),
        );
    }
    let out = h.run(&["stats", &at(s1), &at(s2)]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The scope TOTAL block merges the per-model narration counts (1 + 1).
    assert!(
        out.stdout.contains("TOTAL") && out.stdout.contains("narration blocks claude-test-1×2"),
        "{}",
        out.stdout
    );
    let j = h.run(&["stats", &at(s1), &at(s2), "--format", "json"]);
    let summary: serde_json::Value =
        serde_json::from_str(j.stdout.lines().last().unwrap()).unwrap();
    assert_eq!(
        summary["narration_blocks"]["claude-test-1"], 2,
        "{}",
        j.stdout
    );
    assert_eq!(summary["unknown_thinking_tags"], 0, "{}", j.stdout);

    // A narration-free session prints NEITHER stats line (absence stays silent).
    let s3 = "9a8b7c6d-5e4f-4321-a987-6543210fedcb";
    h.write(
        &format!("{ENC}/{s3}.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"p1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","id":"msg_p1","model":"claude-test-1","content":[{"type":"thinking","thinking":"plain reasoning, no signature"}]}}"#,
            "\n",
        ),
    );
    let quiet = h.run(&["stats", &at(s3)]);
    assert!(
        !quiet.stdout.contains("narration blocks")
            && !quiet.stdout.contains("unknown thinking-signature"),
        "{}",
        quiet.stdout
    );
}

#[test]
fn stats_empty_string_message_id_counts_individually() {
    // An empty-string id is NOT a dedupe key: two such records with different usage
    // both count (the id-less fallback), never collapse into one map slot.
    let h = Home::new();
    let s = "0b1c2d3e-4f5a-4678-b210-fedcba987654";
    h.write(
        &format!("{ENC}/{s}.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"e1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","id":"","model":"claude-test-1","usage":{"input_tokens":0,"output_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"one"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"e2","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","id":"","model":"claude-test-1","usage":{"input_tokens":0,"output_tokens":31,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"two"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["stats", &at(s), "--format", "json"]);
    let row: serde_json::Value = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("row");
    assert_eq!(
        row["tokens"]["claude-test-1"]["output"], 42,
        "{}",
        out.stdout
    );
}
