//! search census NUMBERS: the per-axis record counts, the excluded tally, the turn key's
//! transcript qualifier, and the transcript-id cap. Every assertion here is an exact
//! figure or a conservation law - a census that renders the right keys with the wrong
//! counts is the failure mode a `contains` check cannot see.

use crate::harness::*;

const CENSUS_ENC: &str = "-Users-dev-example-project";
const CENSUS_SESS: &str = "00000000-0000-4000-8000-000000000031";

/// One turn with a paired tool round-trip and one ORPHAN tool_result (its `tool_use` is
/// out of scope), plus a second plain turn. Every record carries a label, so a whole-scope
/// census sees a known set: 5 records in turn 0, 2 in turn 1.
fn census_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{CENSUS_ENC}/{CENSUS_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"first ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","model":"claude-x-1","content":[{"type":"text","text":"reply one"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"a0","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","model":"claude-x-1","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file":"x.rs"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","parentUuid":"a1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"read ok"}]}}"#, "\n",
            r#"{"type":"user","uuid":"c2","parentUuid":"c1","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"sliced-away","content":"orphan result"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"second ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u1","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","model":"claude-x-1","content":[{"type":"text","text":"reply two"}]}}"#, "\n",
        ),
    );
    h
}

/// Run a whole-scope census on one axis and return (key -> count, summary).
fn census(h: &Home, axis: &str) -> (Vec<(String, u64)>, serde_json::Value) {
    let out = h.run(&[
        "search",
        "",
        &at(CENSUS_SESS),
        "--count-by",
        axis,
        "--format",
        "json",
    ]);
    assert!(out.success, "{axis} census failed: {}", out.stderr);
    let rows = json_rows(&out.stdout, "census")
        .iter()
        .map(|r| {
            (
                r["key"].as_str().expect("a census key").to_string(),
                r["records"].as_u64().expect("a census count"),
            )
        })
        .collect();
    (rows, json_summary(&out.stdout))
}

#[test]
fn every_census_axis_files_each_matched_record_exactly_once() {
    let h = census_home();
    // The conservation law every non-label axis obeys: each matched record lands under
    // exactly one key OR in the excluded tally. A count that never increments still
    // renders its key, so only the arithmetic catches it.
    for axis in ["turn", "session", "model", "pairing", "tool", "result"] {
        let (rows, summary) = census(&h, axis);
        let matched = summary["matched_records"]
            .as_u64()
            .expect("matched_records");
        let excluded = summary["excluded_records"]
            .as_u64()
            .expect("excluded_records");
        assert_eq!(matched, 7, "the fixture has 7 labelled records ({axis})");
        let filed: u64 = rows.iter().map(|(_, n)| n).sum();
        assert_eq!(
            filed + excluded,
            matched,
            "{axis}: {filed} filed + {excluded} excluded must equal {matched} matched, rows {rows:?}"
        );
        assert_eq!(
            rows.len() as u64,
            summary["distinct_keys"].as_u64().expect("distinct_keys"),
            "{axis} rows: {rows:?}"
        );
    }
}

#[test]
fn the_turn_axis_is_a_histogram_of_the_records_each_turn_holds() {
    let h = census_home();
    let (rows, _) = census(&h, "turn");
    assert_eq!(
        rows,
        vec![("t0".to_string(), 5), ("t1".to_string(), 2)],
        "turn 0 holds the opener, the reply, the tool round-trip and the orphan"
    );
}

#[test]
fn the_session_axis_counts_every_record_under_its_transcript() {
    let h = census_home();
    let (rows, _) = census(&h, "session");
    assert_eq!(rows, vec![(CENSUS_SESS.to_string(), 7)]);
}

#[test]
fn the_pairing_axis_separates_the_orphan_from_the_paired_round_trip() {
    let h = census_home();
    let (rows, summary) = census(&h, "pairing");
    assert_eq!(
        rows,
        vec![("paired".to_string(), 2), ("orphan".to_string(), 1)],
        "the use and its result are paired; the result whose use is out of scope is not"
    );
    // The four records outside the axis (no tool_use_id) are reported, never dropped.
    assert_eq!(summary["excluded_records"], serde_json::json!(4));
}

#[test]
fn the_model_axis_counts_the_stamped_records_and_reports_the_rest() {
    let h = census_home();
    let (rows, summary) = census(&h, "model");
    assert_eq!(rows, vec![("claude-x-1".to_string(), 3)]);
    assert_eq!(
        summary["excluded_records"],
        serde_json::json!(4),
        "a record with no model stamp is excluded AND tallied"
    );
}

#[test]
fn the_turn_key_names_its_transcript_only_when_the_scope_spans_several() {
    let h = Home::new();
    let one = "00000000-0000-4000-8000-000000000032";
    let two = "00000000-0000-4000-8000-000000000033";
    for (sess, hour) in [(one, "05"), (two, "06")] {
        h.write(
            &format!("{CENSUS_ENC}/{sess}.jsonl"),
            &format!(
                "{}\n",
                format_args!(
                    r#"{{"type":"user","timestamp":"2026-06-07T{hour}:00:00.000Z","message":{{"role":"user","content":"SPANWORD ask"}}}}"#
                )
            ),
        );
    }
    // Two transcripts in scope: a bare `t0` from each would collide into one row, so the
    // key carries the FULL transcript id (a `@` target as-is).
    let out = h.run(&[
        "search",
        "SPANWORD",
        "--count-by",
        "turn",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let keys: Vec<String> = json_rows(&out.stdout, "census")
        .iter()
        .map(|r| r["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        keys,
        vec![format!("{one}\u{b7}t0"), format!("{two}\u{b7}t0")],
        "a multi-transcript turn census qualifies every key"
    );
    // One transcript in scope: the qualifier would be noise, so it is dropped.
    let solo = h.run(&[
        "search",
        "SPANWORD",
        &at(one),
        "--count-by",
        "turn",
        "--format",
        "json",
    ]);
    let solo_keys: Vec<String> = json_rows(&solo.stdout, "census")
        .iter()
        .map(|r| r["key"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(solo_keys, vec!["t0".to_string()]);
}

#[test]
fn the_transcript_id_list_is_capped_at_a_hundred_with_an_explicit_flag() {
    let h = Home::new();
    let write_n = |n: usize| {
        for i in 0..n {
            let sess = format!("00000000-0000-4000-8000-{i:012}");
            h.write(
                &format!("{CENSUS_ENC}/{sess}.jsonl"),
                &format!(
                    "{}\n",
                    format_args!(
                        r#"{{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"CAPWORD ask"}}}}"#
                    )
                ),
            );
        }
    };
    // Exactly the cap: every id is listed and nothing was withheld.
    write_n(100);
    let out = h.run(&["search", "CAPWORD", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let summary = json_summary(&out.stdout);
    assert_eq!(summary["transcript_ids"].as_array().unwrap().len(), 100);
    assert_eq!(
        summary["transcript_ids_truncated"],
        serde_json::json!(false),
        "a hundred ids fit: {summary}"
    );
    // One past it: the list stays a hundred and says so.
    write_n(101);
    let out = h.run(&["search", "CAPWORD", "--format", "json"]);
    let summary = json_summary(&out.stdout);
    assert_eq!(summary["transcript_ids"].as_array().unwrap().len(), 100);
    assert_eq!(
        summary["transcript_ids_truncated"],
        serde_json::json!(true),
        "the hundred-and-first is withheld, never silently: {summary}"
    );
}

#[test]
fn two_merged_sidecar_records_census_as_two_records() {
    // A merged elicitation record has NO physical line, so the census grouper cannot use
    // the line number to tell one record from the next - each line-less hit is its own
    // group. Two pending markers in one turn are two records, not one.
    let h = sidecar_session_home();
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n{}\n",
            mcp_pending_line("el-a", "2026-06-27T01:10:00.000Z", "gdrive", "zzmcp alpha"),
            mcp_pending_line("el-b", "2026-06-27T01:11:00.000Z", "github", "zzmcp beta"),
        ),
    );
    let out = h.run(&[
        "search",
        "zzmcp",
        &at(SESS),
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let summary = json_summary(&out.stdout);
    assert_eq!(
        summary["matched_records"],
        serde_json::json!(2),
        "both pending markers count: {summary}"
    );
    let rows = json_rows(&out.stdout, "census");
    assert_eq!(
        rows.iter()
            .find(|r| r["key"] == "agent.tool.use")
            .map(|r| r["records"].clone()),
        Some(serde_json::json!(2)),
        "census rows:\n{}",
        out.stdout
    );
}
