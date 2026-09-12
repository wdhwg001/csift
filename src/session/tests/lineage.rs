//! Background-handoff lineage: the `continued-in` line the parent transcript receives.

use super::*;

/// The line as Claude Code writes it: four keys, no `uuid`, no `message{}`, no `role`.
fn handoff(parent: &str, child: &str) -> String {
    format!(
        r#"{{"type":"continued-in","timestamp":"2026-06-07T05:10:00.000Z","sessionId":"{parent}","continuedInSessionId":"{child}"}}"#
    )
}

#[test]
fn the_list_prefilter_keeps_a_handoff_line_on_its_key_alone() {
    // The line carries neither `"role"` nor `message{}`, so the role keep alone drops it
    // unparsed: without the second needle the field could never be read.
    let line = handoff(
        "11111111-2222-4333-8444-555555555555",
        "99999999-2222-4333-8444-555555555555",
    );
    assert!(
        !crate::parse::line_has_role_marker(line.as_bytes()),
        "a handoff line carries no role marker: {line}"
    );
    assert!(
        line_is_list_candidate(line.as_bytes()),
        "the list prefilter keeps it anyway: {line}"
    );
}

#[test]
fn the_needle_is_the_key_not_the_type_value() {
    // A payload that merely SPELLS the type value is not a handoff line, and admitting it
    // would cost a parse on every such line. The key-only form is what separates them.
    let prose = br#"{"type":"user","message":{"role":"user","content":"the continued-in line"}}"#;
    assert!(
        line_is_list_candidate(prose),
        "it is a user record, so the ROLE keep admits it regardless"
    );
    let quiet = br#"{"type":"attachment","attachment":{"note":"a continued-in line"}}"#;
    assert!(
        !line_is_list_candidate(quiet),
        "an attachment that only spells the type value stays unparsed"
    );
}

#[test]
fn summarize_reads_the_child_id_off_a_tail_written_handoff_line() {
    // The handoff line is appended AFTER the last conversation record, which is the
    // order the backward tail walk relies on.
    let parent = "11111111-2222-4333-8444-555555555555";
    let child = "99999999-2222-4333-8444-555555555555";
    let p = tmp_session(
        "handoff-tail",
        &[
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"do the thing"}}"#,
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#,
            &handoff(parent, child),
        ],
    );
    let s = summarize_session(&p).unwrap();
    std::fs::remove_file(&p).ok();
    assert_eq!(
        s.continued_in.as_deref(),
        Some(child),
        "the child id is read verbatim from the line"
    );
}

#[test]
fn a_key_inside_a_payload_is_not_a_handoff() {
    // Only a TOP-LEVEL key is the field: a transcript quoting the shape (csift's own dev
    // sessions do) must not be read as having been handed over.
    let p = tmp_session(
        "handoff-quoted",
        &[
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"the line is {\"continuedInSessionId\":\"99999999-2222-4333-8444-555555555555\"}"}}"#,
        ],
    );
    let s = summarize_session(&p).unwrap();
    std::fs::remove_file(&p).ok();
    assert_eq!(
        s.continued_in, None,
        "a quoted key is prose, not a lineage field"
    );
}

#[test]
fn the_newest_handoff_line_wins() {
    // A parent handed over twice names the LAST child; the tail walk is newest-first, so
    // the first line it reaches is the answer.
    let parent = "11111111-2222-4333-8444-555555555555";
    let first = "aaaaaaaa-2222-4333-8444-555555555555";
    let second = "bbbbbbbb-2222-4333-8444-555555555555";
    let p = tmp_session(
        "handoff-twice",
        &[
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"first prompt"}}"#,
            &handoff(parent, first),
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"resumed"}]}}"#,
            &handoff(parent, second),
        ],
    );
    let s = summarize_session(&p).unwrap();
    std::fs::remove_file(&p).ok();
    assert_eq!(
        s.continued_in.as_deref(),
        Some(second),
        "the newest line names the current child"
    );
}

#[test]
fn a_transcript_with_no_handoff_line_reports_none() {
    let p = tmp_session(
        "handoff-absent",
        &[
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"hello"}}"#,
        ],
    );
    let s = summarize_session(&p).unwrap();
    std::fs::remove_file(&p).ok();
    assert_eq!(s.continued_in, None);
}
