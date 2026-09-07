//! user.unsent: superseded esc-recall drafts are searchable under their own leaf,
//! outside turn numbering, never riding user.message.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "6d5e4f30-2a1b-4cde-9876-fedcba543210";

/// L1 draft (parent p0, esc-recalled), L2 resend (same parent), L3 reply, L4 next turn.
fn unsent_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"d1","parentUuid":"p0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef before the tide turns"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"p0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"chart the reef and the harbor before the tide turns"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"charted both"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"a1","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"now the shoals"}}"#, "\n",
        ),
    );
    h
}

#[test]
fn draft_text_is_searchable_under_its_own_leaf() {
    let h = unsent_home();
    // The draft-only wording ("before the tide" appears in both; the draft lacks "harbor").
    let out = h.run(&["search", "reef before the tide", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("user.unsent") && out.stdout.contains("draft (superseded"),
        "the draft surfaces, labeled and annotated:\n{}",
        out.stdout
    );
    // JSON: null turn_index + superseded_draft + single label.
    let j = h.run(&[
        "search",
        "reef before the tide",
        &at(SESS),
        "--format",
        "json",
    ]);
    let row: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "exchange")
        .expect("exchange row");
    assert_eq!(row["superseded_draft"], true, "{}", j.stdout);
    assert!(row["turn_index"].is_null(), "{}", j.stdout);
    assert_eq!(row["hits"][0]["label"], "user.unsent", "{}", j.stdout);
    assert_eq!(
        row["hits"][0]["labels"],
        serde_json::json!(["user.unsent"]),
        "{}",
        j.stdout
    );
}

#[test]
fn selectors_split_unsent_from_message_and_census_agrees() {
    let h = unsent_home();
    let unsent = h.run(&["search", "", &at(SESS), "-t", "user.unsent"]);
    assert!(
        unsent.stdout.contains("chart the reef before the tide")
            && !unsent.stdout.contains("harbor")
            && !unsent.stdout.contains("now the shoals"),
        "-t user.unsent selects only the draft:\n{}",
        unsent.stdout
    );
    let msg = h.run(&["search", "", &at(SESS), "-t", "user.message"]);
    assert!(
        msg.stdout.contains("harbor") && msg.stdout.contains("now the shoals"),
        "{}",
        msg.stdout
    );
    assert!(
        !msg.stdout.contains("reef before the tide turns\n"),
        "user.message never carries a draft:\n{}",
        msg.stdout
    );
    let census = h.run(&[
        "search",
        "",
        &at(SESS),
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    let count = |key: &str| -> u64 {
        census
            .stdout
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|v| v["kind"] == "census" && v["key"] == key)
            .and_then(|v| v["records"].as_u64())
            .unwrap_or(0)
    };
    assert_eq!(count("user.unsent"), 1, "{}", census.stdout);
    assert_eq!(count("user.message"), 2, "{}", census.stdout);
}

#[test]
fn turn_numbering_and_windows_ignore_drafts() {
    let h = unsent_home();
    // The resend opens t0 (the draft never consumes a number) and the footer discloses.
    let out = h.run(&["search", "harbor", &at(SESS)]);
    assert!(out.stdout.contains("·t0"), "{}", out.stdout);
    assert!(
        out.stdout
            .contains("1 superseded draft(s) outside turn numbering")
            && out.stdout.contains("-t user.unsent"),
        "disclosure names the leaf:\n{}",
        out.stdout
    );
    // A --turn window asks about NUMBERED turns: the draft never emits under one.
    let windowed = h.run(&["search", "", &at(SESS), "--turn", "0..5"]);
    assert!(
        !windowed.stdout.contains("draft (superseded"),
        "a turn window excludes draft units:\n{}",
        windowed.stdout
    );
}

#[test]
fn show_labels_an_addressed_draft_unsent() {
    let h = unsent_home();
    let out = h.run(&["show", &at(SESS), "--line", "1", "--format", "json"]);
    let rec: serde_json::Value = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "record")
        .expect("record row");
    assert_eq!(rec["label"], "user.unsent", "{}", out.stdout);
    assert_eq!(rec["superseded_draft"], true, "{}", out.stdout);
}

#[test]
fn role_selector_excludes_drafts_and_the_glob_reaches_them() {
    let h = unsent_home();
    // The 0.7-era contract restored: `-t user` = what the human actually sent.
    let role = h.run(&["search", "", &at(SESS), "-t", "user"]);
    assert!(
        !role.stdout.contains("draft (superseded"),
        "-t user never surfaces a draft:\n{}",
        role.stdout
    );
    assert!(
        role.stdout.contains("harbor") && role.stdout.contains("now the shoals"),
        "{}",
        role.stdout
    );
    // The explicit everything form reaches it.
    let glob = h.run(&["search", "", &at(SESS), "-t", "user.*"]);
    assert!(
        glob.stdout.contains("draft (superseded"),
        "-t 'user.*' includes the draft:\n{}",
        glob.stdout
    );
    // The census keys follow the same law.
    let census = h.run(&["search", "", &at(SESS), "-t", "user", "--count-by", "label"]);
    assert!(
        !census.stdout.contains("user.unsent"),
        "role-scoped census keys are visible-only:\n{}",
        census.stdout
    );
}

/// C-27: the draft states its distance from the message that replaced it.
#[test]
fn a_rendered_draft_carries_the_diff_against_the_sent_message() {
    let h = unsent_home();
    // The draft is 36 chars, the resend 51: the edit inserted "and the harbor "
    // (15 chars), 29.4% of the final. A pure insertion, so the count here happens to
    // equal the length difference; the replacement shapes that separate the two are
    // the chardiff unit tests.
    let out = h.run(&["search", "", &at(SESS), "-t", "user.unsent"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(
            "differs from the sent message in 15 chars (29.4% of the final): may carry an \
             addition or a correction"
        ),
        "the diff line renders verbatim:\n{}",
        out.stdout
    );
    let j = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "user.unsent",
        "--format",
        "json",
    ]);
    let row: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "exchange")
        .expect("exchange row");
    assert_eq!(row["superseding_line"], 2, "{}", j.stdout);
    assert_eq!(row["superseding_uuid"], "u1", "{}", j.stdout);
    assert_eq!(row["diff_chars"], 15, "{}", j.stdout);
    assert_eq!(row["diff_pct"], 29.4, "{}", j.stdout);
    assert_eq!(row["diff_exact"], true, "{}", j.stdout);
}

/// A NON-draft exchange carries none of the five keys (search's lean envelope), and a
/// non-draft `show` row carries them as explicit nulls. A draft's superseding sibling is
/// always in the same transcript - it is what makes the record a draft - so "the sibling
/// is absent" is exactly the non-draft case.
#[test]
fn a_non_draft_row_carries_no_diff_fields() {
    let h = unsent_home();
    let j = h.run(&["search", "harbor", &at(SESS), "--format", "json"]);
    let row: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "exchange")
        .expect("exchange row");
    assert_eq!(row["superseded_draft"], false, "{}", j.stdout);
    for key in ["superseding_line", "diff_chars", "diff_pct", "diff_exact"] {
        assert!(row.get(key).is_none(), "{key} is absent:\n{}", j.stdout);
    }
    assert!(
        !j.stdout.contains("differs from the sent message"),
        "no diff line on a sent message:\n{}",
        j.stdout
    );
    let show = h.run(&["show", &at(SESS), "--line", "2", "--format", "json"]);
    let rec: serde_json::Value = show
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "record")
        .expect("record row");
    for key in [
        "superseding_line",
        "superseding_uuid",
        "diff_chars",
        "diff_pct",
        "diff_exact",
    ] {
        assert!(rec[key].is_null(), "{key} is null:\n{}", show.stdout);
    }
}

#[test]
fn show_renders_the_diff_line_for_an_addressed_draft() {
    let h = unsent_home();
    let out = h.run(&["show", &at(SESS), "--line", "1"]);
    assert!(
        out.stdout
            .contains("differs from the sent message in 15 chars (29.4% of the final)"),
        "an addressed draft states its distance:\n{}",
        out.stdout
    );
    let j = h.run(&["show", &at(SESS), "--line", "1", "--format", "json"]);
    let rec: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "record")
        .expect("record row");
    assert_eq!(rec["superseding_line"], 2, "{}", j.stdout);
    assert_eq!(rec["superseding_uuid"], "u1", "{}", j.stdout);
    assert_eq!(rec["diff_chars"], 15, "{}", j.stdout);
    assert_eq!(rec["diff_pct"], 29.4, "{}", j.stdout);
    assert_eq!(rec["diff_exact"], true, "{}", j.stdout);
}

/// The share is of the FINAL message, so a draft the user cut down reads above 100%.
#[test]
fn the_share_exceeds_100_when_the_draft_was_the_longer_text() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"d1","parentUuid":"p0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef and the harbor and the shoals"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"p0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"chart it"}}"#, "\n",
        ),
    );
    // 44 chars down to 8: the script deletes far more than the final message is long,
    // so the share of the final is above 100.
    let j = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "user.unsent",
        "--format",
        "json",
    ]);
    let row: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "exchange")
        .expect("exchange row");
    let pct = row["diff_pct"].as_f64().expect("diff_pct is a number");
    assert!(pct > 100.0, "a shrunk draft reads above 100%: {}", j.stdout);
    assert_eq!(row["diff_exact"], true, "{}", j.stdout);
}

/// No denominator, no percentage: an empty final message drops the parenthetical
/// rather than fabricating a 0 or a 100, and the wire says null.
#[test]
fn an_empty_sent_message_renders_the_line_without_a_share() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"d1","parentUuid":"p0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"p0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":""}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"charted"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["search", "", &at(SESS), "-t", "user.unsent"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(
            "differs from the sent message in 14 chars: may carry an addition or a correction"
        ),
        "no share when the final message is empty:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("% of the final"),
        "the parenthetical is dropped, never fabricated:\n{}",
        out.stdout
    );
    let j = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "user.unsent",
        "--format",
        "json",
    ]);
    let row: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "exchange")
        .expect("exchange row");
    assert_eq!(row["diff_chars"], 14, "{}", j.stdout);
    assert!(row["diff_pct"].is_null(), "{}", j.stdout);
    assert_eq!(row["diff_exact"], true, "{}", j.stdout);
}

#[test]
fn a_sectioned_draft_keeps_the_single_unsent_label() {
    // A superseded draft shaped like a sectioned text (a pulse) must not fan out into
    // per-section classes its own labels[] does not carry (v0.10.2): one hit, class
    // and labels both `user.unsent`, and the notification selector never sees it.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"d1","parentUuid":"p0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n<summary>Background command finished the reef census</summary>\n</task-notification>"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"p0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"chart the reef and the harbor"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"charted both"}]}}"#, "\n",
        ),
    );
    let j = h.run(&[
        "search",
        "reef census",
        &at(SESS),
        "-t",
        "user.unsent",
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let hits: Vec<serde_json::Value> = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|r| r["kind"] == "exchange")
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(hits.len(), 1, "{}", j.stdout);
    assert_eq!(hits[0]["label"], "user.unsent", "{}", j.stdout);
    assert_eq!(
        hits[0]["labels"],
        serde_json::json!(["user.unsent"]),
        "{}",
        j.stdout
    );
    let none = h.run(&[
        "search",
        "reef census",
        &at(SESS),
        "-t",
        "harness.notification",
        "-c",
    ]);
    assert_eq!(none.stdout.trim(), "0", "{}", none.stdout);
}
