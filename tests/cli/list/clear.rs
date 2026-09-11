//! list `/clear` lineage: the mint signature and the cost-ledger checkpoint join.

use crate::harness::*;

/// `2026-06-07T05:10:00.000Z` in epoch ms - the `/clear` wrapper instant every fixture
/// here anchors on.
const WRAPPER_MS: i64 = 1_780_809_000_000;
const WRAPPER_TS: &str = "2026-06-07T05:10:00.000Z";
const ENC: &str = "-Users-dev-example-project";

/// A predecessor transcript whose cost-ledger checkpoint closes `close_ms` (epoch ms).
/// `trailing` is appended after the checkpoint - a resumed session appends below it.
fn origin_jsonl(session: &str, close_ms: i64, trailing: &str) -> String {
    let start = close_ms - 60_000;
    format!(
        concat!(
            r#"{{"type":"user","uuid":"o1","timestamp":"2026-06-07T05:08:00.000Z","message":{{"role":"user","content":"work before the clear"}}}}"#,
            "\n",
            r#"{{"type":"cost-state","sessionId":"{s}","totalCostUSD":0.5,"totalAPIDuration":100,"totalAPIDurationWithoutRetries":90,"totalToolDuration":10,"totalLinesAdded":3,"totalLinesRemoved":1,"totalDuration":60000,"startTime":{st},"modelUsage":{{}},"hasUnknownModelCost":false}}"#,
            "\n",
            "{tr}",
        ),
        s = session,
        st = start,
        tr = trailing
    )
}

/// The transcript a `/clear` minted: bookkeeping, the isMeta caveat, the wrapper.
fn cleared_jsonl() -> String {
    concat!(
        r#"{"type":"mode","mode":"default"}"#, "\n",
        r#"{"type":"user","uuid":"c1","isMeta":true,"timestamp":"2026-06-07T05:09:59.900Z","message":{"role":"user","content":"<local-command-caveat>Caveat: local commands</local-command-caveat>"}}"#, "\n",
        r#"{"type":"user","uuid":"c2","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"<command-name>/clear</command-name>\n<command-message>clear</command-message>\n<command-args></command-args>"}}"#, "\n",
        r#"{"type":"user","uuid":"c3","timestamp":"2026-06-07T05:10:30.000Z","message":{"role":"user","content":"first prompt after the clear"}}"#, "\n",
    )
    .to_string()
}

fn session_row(out: &Output) -> serde_json::Value {
    out.stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("session row")
}

#[test]
fn list_joins_a_cleared_session_to_its_predecessor() {
    let h = Home::new();
    let old = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let new = "bbbbbbbb-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    // The checkpoint closes 3 ms BEFORE the wrapper. The measured WRAPPER distance on a
    // live clear is 164 ms (3 ms was the caveat record's own offset, not this one); the
    // fixture keeps a small number because what it pins is the rendered distance and its
    // direction, and both sit far inside the 2000 ms window either way.
    h.write(
        &format!("{ENC}/{old}.jsonl"),
        &origin_jsonl(old, WRAPPER_MS - 3, ""),
    );
    h.write(&format!("{ENC}/{new}.jsonl"), &cleared_jsonl());

    let out = h.run(&["list", &format!("@{new}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(
            "cleared  from aaaaaaaa (an inference: the checkpoint the clear wrote closes 3 ms \
             before this file opens)"
        ),
        "the join names the predecessor, the distance and its direction:\n{}",
        out.stdout
    );
    let row = session_row(&h.run(&["list", &format!("@{new}"), "--format", "json"]));
    assert_eq!(row["minted_by"], "clear", "{row}");
    assert_eq!(row["cleared_from"], old, "{row}");
    assert_eq!(row["cleared_from_distance_ms"], 3, "{row}");
    assert_eq!(
        row["cleared_from_candidates"].as_array().map(Vec::len),
        Some(0)
    );

    // The predecessor is not itself clear-minted.
    let prow = session_row(&h.run(&["list", &format!("@{old}"), "--format", "json"]));
    assert!(prow["minted_by"].is_null(), "{prow}");
    assert!(prow["cleared_from"].is_null(), "{prow}");
}

#[test]
fn list_refuses_a_checkpoint_one_millisecond_outside_the_window() {
    let h = Home::new();
    let old = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeef";
    let new = "bbbbbbbb-bbbb-4ccc-8ddd-eeeeeeeeeeef";
    // 2001 ms: one past the join window, so the mint stands and the join does not.
    h.write(
        &format!("{ENC}/{old}.jsonl"),
        &origin_jsonl(old, WRAPPER_MS - 2001, ""),
    );
    h.write(&format!("{ENC}/{new}.jsonl"), &cleared_jsonl());

    let out = h.run(&["list", &format!("@{new}")]);
    assert!(
        out.stdout
            .contains("minted by /clear; no cost-ledger checkpoint within 2000 ms"),
        "an out-of-window checkpoint never joins:\n{}",
        out.stdout
    );
    let row = session_row(&h.run(&["list", &format!("@{new}"), "--format", "json"]));
    assert_eq!(row["minted_by"], "clear", "{row}");
    assert!(row["cleared_from"].is_null(), "{row}");
    assert!(row["cleared_from_distance_ms"].is_null(), "{row}");
}

#[test]
fn list_reports_a_tie_and_joins_neither() {
    let h = Home::new();
    let a = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeee01";
    let b = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeee02";
    let new = "bbbbbbbb-bbbb-4ccc-8ddd-eeeeeeeeee03";
    // Two siblings whose checkpoints close at the SAME instant: the ambiguity is real.
    h.write(
        &format!("{ENC}/{a}.jsonl"),
        &origin_jsonl(a, WRAPPER_MS - 10, ""),
    );
    h.write(
        &format!("{ENC}/{b}.jsonl"),
        &origin_jsonl(b, WRAPPER_MS - 10, ""),
    );
    h.write(&format!("{ENC}/{new}.jsonl"), &cleared_jsonl());

    let out = h.run(&["list", &format!("@{new}")]);
    assert!(
        out.stdout
            .contains("2 siblings tie at 10 ms (aaaaaaaa, aaaaaaaa) - joined to neither"),
        "a tie is reported, never resolved:\n{}",
        out.stdout
    );
    let row = session_row(&h.run(&["list", &format!("@{new}"), "--format", "json"]));
    assert!(row["cleared_from"].is_null(), "no join on a tie: {row}");
    let cands = row["cleared_from_candidates"]
        .as_array()
        .expect("candidates array");
    assert_eq!(cands.len(), 2, "{row}");
    assert!(
        cands.iter().any(|c| c == a) && cands.iter().any(|c| c == b),
        "{row}"
    );
}

#[test]
fn list_joins_through_a_checkpoint_that_is_no_longer_last() {
    let h = Home::new();
    let old = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeee04";
    let new = "bbbbbbbb-bbbb-4ccc-8ddd-eeeeeeeeee05";
    // The predecessor was resumed after the clear, so records follow its checkpoint.
    // The join reads the checkpoint, not the file's tail, so it still holds.
    h.write(
        &format!("{ENC}/{old}.jsonl"),
        &origin_jsonl(
            old,
            WRAPPER_MS - 3,
            concat!(
                r#"{"type":"user","uuid":"o2","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"resumed by id"}}"#, "\n",
            ),
        ),
    );
    h.write(&format!("{ENC}/{new}.jsonl"), &cleared_jsonl());

    let row = session_row(&h.run(&["list", &format!("@{new}"), "--format", "json"]));
    assert_eq!(row["cleared_from"], old, "{row}");
    assert_eq!(row["cleared_from_distance_ms"], 3, "{row}");
}

#[test]
fn list_leaves_an_ordinary_session_untouched() {
    let h = Home::new();
    let plain = "cccccccc-bbbb-4ccc-8ddd-eeeeeeeeee06";
    let neighbour = "cccccccc-bbbb-4ccc-8ddd-eeeeeeeeee07";
    // A checkpoint sits right beside it, but this transcript opens with a human turn.
    h.write(
        &format!("{ENC}/{neighbour}.jsonl"),
        &origin_jsonl(neighbour, WRAPPER_MS, ""),
    );
    h.write(
        &format!("{ENC}/{plain}.jsonl"),
        &format!(
            "{}\n",
            format_args!(
                r#"{{"type":"user","uuid":"p1","timestamp":"{WRAPPER_TS}","message":{{"role":"user","content":"an ordinary opener"}}}}"#
            )
        ),
    );
    let out = h.run(&["list", &format!("@{plain}")]);
    assert!(
        !out.stdout.contains("cleared"),
        "no cleared row without the wrapper:\n{}",
        out.stdout
    );
    let row = session_row(&h.run(&["list", &format!("@{plain}"), "--format", "json"]));
    assert!(row["minted_by"].is_null(), "{row}");
    assert!(row["cleared_from"].is_null(), "{row}");
}

#[test]
fn list_needs_the_wrapper_first_not_merely_present() {
    let h = Home::new();
    let old = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeee08";
    let mid = "bbbbbbbb-bbbb-4ccc-8ddd-eeeeeeeeee09";
    h.write(
        &format!("{ENC}/{old}.jsonl"),
        &origin_jsonl(old, WRAPPER_MS - 3, ""),
    );
    // A `/clear` wrapper MID-conversation is a clear leaving this file, not minting it.
    h.write(
        &format!("{ENC}/{mid}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"m1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"a human opener"}}"#, "\n",
            r#"{"type":"user","uuid":"m2","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"<command-name>/clear</command-name>\n<command-args></command-args>"}}"#, "\n",
        ),
    );
    let row = session_row(&h.run(&["list", &format!("@{mid}"), "--format", "json"]));
    assert!(
        row["minted_by"].is_null(),
        "only the FIRST non-isMeta user record decides the mint: {row}"
    );
}
