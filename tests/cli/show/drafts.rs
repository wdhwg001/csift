//! C-18 superseded-draft honesty: scans disclose the collapse; addresses still fetch.

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
fn search_collapses_the_draft_and_discloses_the_count() {
    let h = Home::new();
    draft_fixture(&h);
    let out = h.run(&["search", "", at(SESS).as_str(), "-t", "user.message"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("the sent phrasing") && !out.stdout.contains("abandoned phrasing"),
        "the scan hides the draft (turn hygiene):\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("1 superseded draft(s) collapsed"),
        "the collapse is DISCLOSED, never silent:\n{}",
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
}

#[test]
fn show_fetches_an_addressed_draft_with_an_honest_header() {
    // The refetch law: the draft IS a real record; only turn numbering excludes it.
    let h = Home::new();
    draft_fixture(&h);
    let out = h.run(&["show", at(SESS).as_str(), "--line", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
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
    assert!(
        rows.iter()
            .any(|r| r["superseded_draft"] == false && r["turn_index"] == 0),
        "the sent opener keeps t0: {}",
        both.stdout
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
