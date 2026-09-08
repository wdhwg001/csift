//! Token usage: Claude Code repeats the identical usage object on every per-block
//! record of one API message, so the sums dedupe by message.id per transcript.

use crate::harness::*;

const DEDUP_ENC: &str = "-Users-dev-example-project";
const DEDUP_SESS: &str = "3f2e1d0c-9b8a-4765-b432-10fedcba9876";

#[test]
fn stats_usage_counts_one_api_message_once() {
    // Two records share one message.id with the IDENTICAL usage echo (the normal CC
    // per-block shape); a third record carries its own id.
    let h = Home::new();
    h.write(
        &format!("{DEDUP_ENC}/{DEDUP_SESS}.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","id":"msg_p1","model":"claude-test-1","usage":{"input_tokens":3,"output_tokens":40,"cache_read_input_tokens":100,"cache_creation_input_tokens":7},"content":[{"type":"thinking","thinking":"placeholder reasoning"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","id":"msg_p1","model":"claude-test-1","usage":{"input_tokens":3,"output_tokens":40,"cache_read_input_tokens":100,"cache_creation_input_tokens":7},"content":[{"type":"text","text":"charted"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a3","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","id":"msg_p2","model":"claude-test-1","usage":{"input_tokens":1,"output_tokens":2,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"done"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["stats", &at(DEDUP_SESS), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let row: serde_json::Value = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("session row");
    let t = &row["tokens"]["claude-test-1"];
    assert_eq!(t["output"], 42, "40 once + 2, never 82: {}", out.stdout);
    assert_eq!(t["input"], 4, "{}", out.stdout);
    assert_eq!(t["cache_read"], 100, "{}", out.stdout);
    assert_eq!(t["cache_creation"], 7, "{}", out.stdout);
}

#[test]
fn stats_usage_zeroed_replay_never_wins() {
    // The compaction-replay shape: the same message.id written again with ZEROED
    // usage. The per-field MAX keeps the real numbers regardless of order.
    let h = Home::new();
    h.write(
        &format!("{DEDUP_ENC}/{DEDUP_SESS}.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"z1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","id":"msg_r1","model":"claude-test-1","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"replayed copy"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"z2","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","id":"msg_r1","model":"claude-test-1","usage":{"input_tokens":2,"output_tokens":708,"cache_read_input_tokens":474834,"cache_creation_input_tokens":3997},"content":[{"type":"text","text":"original"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["stats", &at(DEDUP_SESS), "--format", "json"]);
    let row: serde_json::Value = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("session row");
    let t = &row["tokens"]["claude-test-1"];
    assert_eq!(
        t["output"], 708,
        "the zeroed replay never wins: {}",
        out.stdout
    );
    assert_eq!(t["cache_read"], 474834, "{}", out.stdout);
}

#[test]
fn stats_usage_dedupe_is_per_file_and_total_sums_files() {
    // The spawn message is copied into a child transcript with DIFFERENT usage: each
    // file keeps its own value (a genuine per-transcript fact); TOTAL sums both.
    let h = Home::new();
    h.write(
        &format!("{DEDUP_ENC}/{DEDUP_SESS}.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"m1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","id":"msg_x1","model":"claude-test-1","usage":{"input_tokens":0,"output_tokens":6945,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"spawning"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{DEDUP_ENC}/{DEDUP_SESS}/subagents/agent-1234567890abcdef.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"c1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","id":"msg_x1","model":"claude-test-1","usage":{"input_tokens":0,"output_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"child copy"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["stats", &at(DEDUP_SESS), "--format", "json"]);
    let outputs: Vec<u64> = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "session")
        .filter_map(|v| v["tokens"]["claude-test-1"]["output"].as_u64())
        .collect();
    let mut sorted = outputs.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec![10, 6945],
        "per-file values kept: {}",
        out.stdout
    );
    let summary: serde_json::Value = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "summary")
        .expect("summary");
    assert_eq!(
        summary["tokens"]["claude-test-1"]["output"], 6955,
        "TOTAL sums files, never dedupes across them: {}",
        out.stdout
    );
}

#[test]
fn stats_usage_idless_records_count_individually() {
    // A record with no message.id keeps the old per-record semantics beside an
    // id-bearing pair (every pre-id transcript takes this path).
    let h = Home::new();
    h.write(
        &format!("{DEDUP_ENC}/{DEDUP_SESS}.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"n1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","model":"claude-test-1","usage":{"input_tokens":0,"output_tokens":5,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"old format"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"n2","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","model":"claude-test-1","usage":{"input_tokens":0,"output_tokens":5,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"old format two"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"n3","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","id":"msg_q1","model":"claude-test-1","usage":{"input_tokens":0,"output_tokens":30,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"new format"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["stats", &at(DEDUP_SESS), "--format", "json"]);
    let row: serde_json::Value = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("session row");
    assert_eq!(
        row["tokens"]["claude-test-1"]["output"], 40,
        "5 + 5 individually + 30 once: {}",
        out.stdout
    );
}
