//! C-18 + user.unsent: a draft stays outside turn numbering but is searchable under
//! its own leaf; the collapse count is disclosed; addresses still fetch.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "7a6b5c4d-3e2f-4109-a876-543210fedcba";

/// L1 = a draft opener (parent p0) superseded by L2 (same parent, later); L3 = the reply.
/// L4 = a metadata line no address can render.
fn draft_fixture(h: &Home) {
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"d1","parentUuid":"p0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"the abandoned phrasing of the ask"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"p0","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"user","content":"the sent phrasing of the ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:25.000Z","message":{"role":"assistant","content":[{"type":"text","text":"answered the sent one"}]}}"#, "\n",
            r#"{"type":"file-history-snapshot","messageId":"m1"}"#, "\n",
        ),
    );
}

#[test]
fn search_keeps_drafts_out_of_user_message_and_discloses_the_count() {
    let h = Home::new();
    draft_fixture(&h);
    let out = h.run(&["search", "", at(SESS).as_str(), "-t", "user.message"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("the sent phrasing") && !out.stdout.contains("abandoned phrasing"),
        "user.message stays pure - a draft never rides it:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("1 superseded draft(s) outside turn numbering"),
        "the draft set is DISCLOSED, never silent:\n{}",
        out.stdout
    );
    let outj = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "-t",
        "user.message",
        "--format",
        "json",
    ]);
    let summary: serde_json::Value =
        serde_json::from_str(outj.stdout.lines().last().unwrap()).unwrap();
    assert_eq!(summary["superseded_drafts"], 1, "{}", outj.stdout);

    // A draft-free session prints NO draft note and NO zero-malformed note.
    let clean_sess = "8b7c6d5e-4f30-4219-b987-654321fedcba";
    h.write(
        &format!("{ENC}/{clean_sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"clean lane"}}"#,
            "\n",
        ),
    );
    let clean = h.run(&["search", "", &at(clean_sess), "-t", "user.message"]);
    assert!(
        !clean.stdout.contains("superseded draft") && !clean.stdout.contains("malformed"),
        "clean run prints no zero notes:\n{}",
        clean.stdout
    );
}

#[test]
fn show_fetches_an_addressed_draft_with_an_honest_header() {
    // The refetch law: the draft IS a real record; only turn numbering excludes it.
    let h = Home::new();
    draft_fixture(&h);
    let out = h.run(&["show", at(SESS).as_str(), "--line", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("malformed"),
        "a clean fetch prints no zero note:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("abandoned phrasing")
            && out.stdout.contains("superseded draft")
            && out.stdout.contains("outside turn numbering"),
        "the draft renders with an honest header, never a fabricated t<N>:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("\nt0 ") && !out.stdout.contains("\nt1 "),
        "no fabricated turn header on a draft-only fetch:\n{}",
        out.stdout
    );

    let outj = h.run(&["show", at(SESS).as_str(), "--line", "1", "--format", "json"]);
    let row: serde_json::Value = outj
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .find(|o: &serde_json::Value| o["kind"] == "record")
        .expect("record row");
    assert_eq!(row["superseded_draft"], true, "{}", outj.stdout);
    assert!(
        row["turn_index"].is_null(),
        "no fabricated index: {}",
        outj.stdout
    );

    // A normal record fetched in the same call keeps its numbering and flag=false.
    let both = h.run(&[
        "show",
        at(SESS).as_str(),
        "--line",
        "1..2",
        "--format",
        "json",
    ]);
    let rows: Vec<serde_json::Value> = both
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .filter(|o: &serde_json::Value| o["kind"] == "record")
        .collect();
    assert_eq!(rows.len(), 2, "{}", both.stdout);
    let bsummary: serde_json::Value =
        serde_json::from_str(both.stdout.lines().last().unwrap()).unwrap();
    assert_eq!(
        bsummary["records"], 2,
        "summary counts the units: {}",
        both.stdout
    );
    assert!(
        rows.iter()
            .any(|r| r["superseded_draft"] == false && r["turn_index"] == 0),
        "the sent opener keeps t0: {}",
        both.stdout
    );
}

#[test]
fn show_prints_the_malformed_note_when_a_torn_line_exists() {
    let h = Home::new();
    let sess = "9c8d7e6f-5a4b-4321-8765-fedcba098765";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"solid line"}}"#,
            "\n",
            "not json here",
            "\n",
        ),
    );
    let out = h.run(&["show", &at(sess), "--line", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("1 malformed line(s) skipped"),
        "the torn line is booked on a fetch too:\n{}",
        out.stdout
    );
}

#[test]
fn show_miss_error_states_the_current_render_domain() {
    let h = Home::new();
    draft_fixture(&h);
    let out = h.run(&["show", at(SESS).as_str(), "--line", "4"]);
    assert!(!out.success, "a metadata line is still a miss");
    assert!(
        out.stderr.contains("superseded drafts included")
            && out.stderr.contains("attachment lines")
            && out.stderr.contains("--raw"),
        "the error names what DOES render (no stale metadata/attachment claim):\n{}",
        out.stderr
    );
}
