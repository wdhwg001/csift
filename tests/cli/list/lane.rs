//! list background-lane lineage: the `sessionKind` stamp and its whole-file span.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESSION: &str = "11111111-2222-4333-8444-555555555555";

/// A record written by a BACKGROUND lane: the stamp rides the record's top level.
fn bg(uuid: &str, ts: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","sessionKind":"bg","message":{{"role":"user","content":"{text}"}}}}"#
    )
}

/// The same record written by a FOREGROUND lane: no such key at all, because the stamp's
/// value is undefined there and the serializer drops an undefined property.
fn fg(uuid: &str, ts: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","message":{{"role":"user","content":"{text}"}}}}"#
    )
}

/// A foreground ASSISTANT reply. The tail read stops once it has BOTH anchors - the last
/// genuine user AND the last agent message - so a transcript that never answers is walked to
/// the top and no window limit bites. Every fixture below that means to exercise the limit
/// has to give the tail both anchors near the end.
fn fg_agent(uuid: &str, ts: &str, text: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"{ts}","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
    )
}

fn session_row(out: &Output) -> serde_json::Value {
    out.stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("session row")
}

/// The measured shape: a background lane's records, then a foreground resume appending
/// below them, so the stamp covers a PREFIX of the file and no tail window can see its end.
fn handed_back_jsonl() -> String {
    format!(
        "{}\n{}\n{}\n{}\n",
        bg("u1", "2026-06-07T05:00:00.000Z", "run in the background"),
        bg("u2", "2026-06-07T05:01:00.000Z", "still in the background"),
        fg(
            "u3",
            "2026-06-07T06:00:00.000Z",
            "resumed in the foreground"
        ),
        fg("u4", "2026-06-07T06:01:00.000Z", "and again"),
    )
}

#[test]
fn the_lane_row_names_the_values_and_says_the_span_needs_the_flag() {
    let h = Home::new();
    h.write(&format!("{ENC}/{SESSION}.jsonl"), &handed_back_jsonl());

    let out = h.run(&["list", &format!("@{SESSION}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("lane     bg (seen in the head/tail windows; span needs --lineage)"),
        "the default row names the value and points at the flag for the span:\n{}",
        out.stdout
    );

    let row = session_row(&h.run(&["list", &format!("@{SESSION}"), "--format", "json"]));
    assert_eq!(row["session_kind"], serde_json::json!(["bg"]), "{row}");
    assert!(row["session_kind_first_line"].is_null(), "{row}");
    assert!(row["session_kind_last_line"].is_null(), "{row}");
    assert_eq!(row["lineage_scanned"], false, "{row}");
}

#[test]
fn lineage_reports_the_span_the_windows_cannot_see() {
    let h = Home::new();
    h.write(&format!("{ENC}/{SESSION}.jsonl"), &handed_back_jsonl());

    let out = h.run(&["list", &format!("@{SESSION}"), "--lineage"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("lane     bg on L1..L2"),
        "the stamp covers lines 1 and 2 and stops where the foreground resume begins:\n{}",
        out.stdout
    );

    let row = session_row(&h.run(&[
        "list",
        &format!("@{SESSION}"),
        "--lineage",
        "--format",
        "json",
    ]));
    assert_eq!(row["session_kind"], serde_json::json!(["bg"]), "{row}");
    assert_eq!(row["session_kind_first_line"], 1, "{row}");
    assert_eq!(row["session_kind_last_line"], 2, "{row}");
    assert_eq!(row["lineage_scanned"], true, "{row}");
}

#[test]
fn a_foreground_session_carries_no_lane_row_at_all() {
    // An absent key is the foreground answer, so the field is an EMPTY array, not a value.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESSION}.jsonl"),
        &format!("{}\n", fg("u1", "2026-06-07T05:00:00.000Z", "hello")),
    );
    let out = h.run(&["list", &format!("@{SESSION}"), "--lineage"]);
    assert!(
        !out.stdout.contains("lane "),
        "no stamp, no row:\n{}",
        out.stdout
    );
    let row = session_row(&h.run(&[
        "list",
        &format!("@{SESSION}"),
        "--lineage",
        "--format",
        "json",
    ]));
    assert_eq!(
        row["session_kind"].as_array().map(Vec::len),
        Some(0),
        "{row}"
    );
    assert!(row["session_kind_first_line"].is_null(), "{row}");
    assert_eq!(row["lineage_scanned"], true, "{row}");
}

#[test]
fn a_stamp_only_in_the_middle_is_invisible_to_the_windows_and_found_by_the_flag() {
    // The head scan stops at the first genuine user and the tail walks back to the last:
    // a carrier strictly BETWEEN a foreground opener and a foreground tail is outside both,
    // which is the case the flag exists for.
    let h = Home::new();
    let mut body = String::new();
    body.push_str(&format!(
        "{}\n",
        fg("u1", "2026-06-07T05:00:00.000Z", "foreground opener")
    ));
    body.push_str(&format!(
        "{}\n",
        bg("u2", "2026-06-07T05:10:00.000Z", "handed to the background")
    ));
    // A foreground tail carrying BOTH anchors, so the backward walk stops there instead of
    // continuing to the top of the file.
    for i in 0..40 {
        body.push_str(&format!(
            "{}\n",
            fg(
                &format!("t{i}"),
                "2026-06-07T06:00:00.000Z",
                "back in the foreground"
            )
        ));
        body.push_str(&format!(
            "{}\n",
            fg_agent(&format!("a{i}"), "2026-06-07T06:00:01.000Z", "acknowledged")
        ));
    }
    h.write(&format!("{ENC}/{SESSION}.jsonl"), &body);

    let plain = session_row(&h.run(&["list", &format!("@{SESSION}"), "--format", "json"]));
    assert_eq!(
        plain["session_kind"].as_array().map(Vec::len),
        Some(0),
        "the windows never reach the middle carrier: {plain}"
    );

    let scanned = session_row(&h.run(&[
        "list",
        &format!("@{SESSION}"),
        "--lineage",
        "--format",
        "json",
    ]));
    assert_eq!(
        scanned["session_kind"],
        serde_json::json!(["bg"]),
        "{scanned}"
    );
    assert_eq!(scanned["session_kind_first_line"], 2, "{scanned}");
    assert_eq!(scanned["session_kind_last_line"], 2, "{scanned}");
}

#[test]
fn lineage_also_resolves_a_handoff_line_the_tail_window_lost() {
    // Same whole-file pass, so the handoff field stops being window-bounded too: a
    // `continued-in` line with a long foreground tail below it reads null by default.
    let h = Home::new();
    let child = "99999999-2222-4333-8444-555555555555";
    let mut body = String::new();
    body.push_str(&format!(
        "{}\n",
        fg("u1", "2026-06-07T05:00:00.000Z", "before the handoff")
    ));
    body.push_str(&format!(
        r#"{{"type":"continued-in","timestamp":"2026-06-07T05:10:00.000Z","sessionId":"{SESSION}","continuedInSessionId":"{child}"}}"#
    ));
    body.push('\n');
    for i in 0..40 {
        body.push_str(&format!(
            "{}\n",
            fg(
                &format!("t{i}"),
                "2026-06-07T06:00:00.000Z",
                "resumed after the handoff"
            )
        ));
        body.push_str(&format!(
            "{}\n",
            fg_agent(&format!("a{i}"), "2026-06-07T06:00:01.000Z", "acknowledged")
        ));
    }
    h.write(&format!("{ENC}/{SESSION}.jsonl"), &body);

    let plain = session_row(&h.run(&["list", &format!("@{SESSION}"), "--format", "json"]));
    assert!(
        plain["continued_in"].is_null(),
        "the documented window limit: {plain}"
    );
    let scanned = session_row(&h.run(&[
        "list",
        &format!("@{SESSION}"),
        "--lineage",
        "--format",
        "json",
    ]));
    assert_eq!(scanned["continued_in"], child, "{scanned}");
}

#[test]
fn a_nested_key_in_a_payload_is_never_a_lane_stamp() {
    // The pass is a depth-1 walk, so a record whose tool input happens to carry the key
    // reports nothing - the same answer the record model gives.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESSION}.jsonl"),
        &format!(
            "{}\n{}\n",
            fg("u1", "2026-06-07T05:00:00.000Z", "hello"),
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"sessionKind":"bg","command":"true"}}]}}"#
        ),
    );
    let row = session_row(&h.run(&[
        "list",
        &format!("@{SESSION}"),
        "--lineage",
        "--format",
        "json",
    ]));
    assert_eq!(
        row["session_kind"].as_array().map(Vec::len),
        Some(0),
        "a nested key is not a field: {row}"
    );
}
