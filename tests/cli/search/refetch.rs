//! The ADDRESSES a search hit prints: the per-hit `refetch` command (line-addressed for a
//! native record, uuid-addressed when there is no line) and the sibling-remainder range.
//! An address that merely "looks like a csift show command" is not enough - the wrong
//! address fetches the wrong record silently, so these pin the exact string.

use crate::harness::*;

/// The first hit object of the first exchange row.
fn first_hit(stdout: &str) -> serde_json::Value {
    let rows = json_rows(stdout, "exchange");
    assert!(!rows.is_empty(), "no exchange rows in:\n{stdout}");
    rows[0]["hits"][0].clone()
}

#[test]
fn a_native_hit_refetches_by_its_own_line_number() {
    let h = populated_home();
    let out = h.run(&[
        "search",
        "carry",
        &at(SESS),
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hit = first_hit(&out.stdout);
    let line = hit["line"].as_u64().expect("a native hit carries its line");
    assert_eq!(
        hit["refetch"],
        serde_json::json!(format!("csift show @{SESS} --line {line}")),
        "a native hit addresses itself by LINE - the uuid form is the fallback: {hit}"
    );
    // And the address round-trips against the same transcript.
    let back = h.run(&["show", &at(SESS), "--line", &line.to_string()]);
    assert!(back.success, "refetch must resolve: {}", back.stderr);
}

#[test]
fn a_merged_sidecar_hit_has_no_line_and_refetches_by_uuid() {
    let h = sidecar_session_home();
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n",
            mcp_pending_line(
                "el-r",
                "2026-06-27T01:10:00.000Z",
                "gdrive",
                "zzmcp confirm"
            )
        ),
    );
    let out = h.run(&["search", "zzmcp", &at(SESS), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hit = first_hit(&out.stdout);
    assert!(hit["line"].is_null(), "no physical line: {hit}");
    assert_eq!(
        hit["refetch"],
        serde_json::json!(format!("csift show @{SESS} --uuid m-el-r")),
        "a line-less record can only be addressed by uuid: {hit}"
    );
}

#[test]
fn the_sibling_remainder_range_skips_the_line_less_records_of_its_turn() {
    // `turn_lines` is the span the `(+N more)` pointer hands back to `csift show --line`.
    // A merged sidecar record sits in the turn with no physical line, and a zero would
    // make the printed range start below the file.
    let h = sidecar_session_home();
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n",
            auq_pending_line("auq-r", "2026-06-27T01:10:00.000Z", "which way?")
        ),
    );
    let out = h.run(&[
        "search",
        "start the work",
        &at(SESS),
        "-t",
        "user.message",
        "--siblings",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows = json_rows(&out.stdout, "exchange");
    assert_eq!(
        rows[0]["turn_lines"],
        serde_json::json!([1, 2]),
        "the range spans the turn's REAL lines only: {}",
        out.stdout
    );
}
