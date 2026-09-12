//! list background-handoff lineage: the parent-side `continued-in` line.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const PARENT: &str = "11111111-2222-4333-8444-555555555555";
const CHILD: &str = "99999999-2222-4333-8444-555555555555";

/// The handoff line exactly as Claude Code appends it: four keys, no `uuid`.
fn handoff(parent: &str, child: &str) -> String {
    format!(
        r#"{{"type":"continued-in","timestamp":"2026-06-07T05:10:00.000Z","sessionId":"{parent}","continuedInSessionId":"{child}"}}"#
    )
}

fn turn(uuid: &str, ts: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","message":{{"role":"user","content":"{text}"}}}}"#
    )
}

fn session_row(out: &Output) -> serde_json::Value {
    out.stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("session row")
}

#[test]
fn list_names_the_child_a_background_handoff_continued_in() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{PARENT}.jsonl"),
        &format!(
            "{}\n{}\n",
            turn("u1", "2026-06-07T05:00:00.000Z", "do the thing"),
            handoff(PARENT, CHILD)
        ),
    );

    let out = h.run(&["list", &format!("@{PARENT}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(
            "handoff  continued in 99999999 (a background handoff: Claude Code names the \
             child itself, so this one is no inference)"
        ),
        "the row names the child by its first-8 token and says the fact is stated:\n{}",
        out.stdout
    );

    let row = session_row(&h.run(&["list", &format!("@{PARENT}"), "--format", "json"]));
    assert_eq!(row["continued_in"], CHILD, "{row}");
    // The handoff is NOT the clone or the clear lineage: those stay null here.
    assert!(row["minted_by"].is_null(), "{row}");
    assert!(row["cleared_from"].is_null(), "{row}");
    assert_eq!(row["is_clone"], false, "{row}");
}

#[test]
fn a_session_never_handed_over_reports_a_null_continued_in() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{PARENT}.jsonl"),
        &format!("{}\n", turn("u1", "2026-06-07T05:00:00.000Z", "hello")),
    );
    let out = h.run(&["list", &format!("@{PARENT}")]);
    assert!(
        !out.stdout.contains("handoff"),
        "no line, no row:\n{}",
        out.stdout
    );
    let row = session_row(&h.run(&["list", &format!("@{PARENT}"), "--format", "json"]));
    assert!(row["continued_in"].is_null(), "{row}");
}

#[test]
fn a_quoted_field_name_in_a_prompt_is_not_a_handoff() {
    // The field is a TOP-LEVEL key. A transcript that discusses the shape (this repo's own
    // dev sessions do) must not read as having been handed over.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{PARENT}.jsonl"),
        &format!(
            "{}\n",
            turn(
                "u1",
                "2026-06-07T05:00:00.000Z",
                "the line carries \\\"continuedInSessionId\\\" and no uuid"
            )
        ),
    );
    let out = h.run(&["list", &format!("@{PARENT}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("handoff"),
        "prose is not lineage:\n{}",
        out.stdout
    );
    let row = session_row(&h.run(&["list", &format!("@{PARENT}"), "--format", "json"]));
    assert!(row["continued_in"].is_null(), "{row}");
}

#[test]
fn stats_counts_the_handoff_line_type_whole_file() {
    // The documented escape from `list`'s window limit: `stats` validates every line, so
    // its type census reaches a handoff line wherever it sits.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{PARENT}.jsonl"),
        &format!(
            "{}\n{}\n{}\n",
            turn("u1", "2026-06-07T05:00:00.000Z", "first"),
            handoff(PARENT, CHILD),
            turn(
                "u2",
                "2026-06-07T05:20:00.000Z",
                "resumed after the handoff"
            ),
        ),
    );
    let out = h.run(&["stats", &format!("@{PARENT}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("continued-in×1"),
        "the line type is a census key:\n{}",
        out.stdout
    );
}
