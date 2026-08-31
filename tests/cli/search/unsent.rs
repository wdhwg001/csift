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
