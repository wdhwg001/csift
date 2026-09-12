//! The depth-1 lineage key walk: what counts as a field and what does not.

use super::*;

#[test]
fn a_top_level_key_is_a_field_and_a_nested_one_is_not() {
    // The whole reason the walk is depth-1: a transcript quoting the shape in a payload must
    // read as carrying nothing, the same answer serde gives.
    let top = br#"{"type":"assistant","sessionKind":"bg","message":{"role":"assistant"}}"#;
    let f = lineage_fields(top).expect("an object");
    assert_eq!(f.session_kind.as_deref(), Some("bg"));

    let nested = br#"{"type":"user","message":{"role":"user","content":"sessionKind is bg"},"input":{"sessionKind":"bg"}}"#;
    let f = lineage_fields(nested).expect("an object");
    assert_eq!(
        f.session_kind, None,
        "a key inside a payload is not a top-level field"
    );
    assert!(f.is_empty());
}

#[test]
fn the_handoff_child_id_is_read_from_the_same_walk() {
    let line = br#"{"type":"continued-in","timestamp":"2026-06-07T05:10:00.000Z","sessionId":"11111111-2222-4333-8444-555555555555","continuedInSessionId":"99999999-2222-4333-8444-555555555555"}"#;
    let f = lineage_fields(line).expect("an object");
    assert_eq!(
        f.continued_in_session_id.as_deref(),
        Some("99999999-2222-4333-8444-555555555555")
    );
    assert_eq!(f.session_kind, None);
    assert!(!f.is_empty());
}

#[test]
fn both_fields_ride_one_line_and_whitespace_around_the_colon_is_the_same_record() {
    // A reserialized line (whitespace after the colon) is the same record - the R13 law the
    // prefilter's key-only needles obey, and the walk has to agree with them.
    let line = br#"{ "type" : "assistant" , "sessionKind" : "bg" , "continuedInSessionId" : "99999999-2222-4333-8444-555555555555" }"#;
    let f = lineage_fields(line).expect("an object");
    assert_eq!(f.session_kind.as_deref(), Some("bg"));
    assert_eq!(
        f.continued_in_session_id.as_deref(),
        Some("99999999-2222-4333-8444-555555555555")
    );
}

#[test]
fn a_blank_or_unframed_line_is_not_an_object() {
    assert_eq!(lineage_fields(b""), None);
    assert_eq!(lineage_fields(b"   \n"), None);
    assert_eq!(lineage_fields(b"not json at all"), None);
}

#[test]
fn a_non_string_value_yields_no_field_rather_than_a_fabricated_one() {
    // Tolerance without invention: a shape the harness never writes must not become a value.
    let line = br#"{"type":"assistant","sessionKind":null,"continuedInSessionId":7}"#;
    let f = lineage_fields(line).expect("an object");
    assert_eq!(f.session_kind, None);
    assert_eq!(f.continued_in_session_id, None);
}

#[test]
fn the_prefilter_admits_exactly_the_lines_the_walk_can_answer() {
    // Both needles are quoted KEYS, so a line carrying neither is skipped before the walk.
    assert!(line_has_lineage_key(
        br#"{"type":"assistant","sessionKind":"bg"}"#
    ));
    assert!(line_has_lineage_key(
        br#"{"type":"continued-in","continuedInSessionId":"x"}"#
    ));
    assert!(
        !line_has_lineage_key(br#"{"type":"user","message":{"role":"user","content":"hi"}}"#),
        "an ordinary record carries neither key"
    );
    // PROSE cannot trip a key needle, and the reason is structural rather than lucky: a `"`
    // inside a JSON string is escaped, so a quoted key MENTIONED in a message body is the
    // bytes `\"sessionKind\"` and never the needle.
    assert!(
        !line_has_lineage_key(
            br#"{"type":"user","message":{"role":"user","content":"the \"sessionKind\" key"}}"#
        ),
        "an escaped quote is not the needle's quote"
    );
    // What DOES reach the walk is a NESTED object's own key, which is unescaped in the raw
    // bytes. The prefilter admits it and the depth-1 walk refuses it - a walk that finds
    // nothing, never a wrong answer.
    assert!(line_has_lineage_key(
        br#"{"type":"user","input":{"sessionKind":"bg"}}"#
    ));
    assert_eq!(
        lineage_fields(br#"{"type":"user","input":{"sessionKind":"bg"}}"#)
            .expect("an object")
            .session_kind,
        None
    );
}

#[test]
fn a_torn_line_carries_no_key_instead_of_failing() {
    // There is no parse to fail, which is why the pass reports no malformed lines: a
    // truncated line simply stops answering.
    // Even with the value already past, an object the walk cannot finish yields nothing
    // rather than a partial answer: the unterminated string runs the walk out of input.
    let torn = br#"{"type":"assistant","sessionKind":"bg","message":{"role":"assis"#;
    assert_eq!(lineage_fields(torn), None);
    let torn_early = br#"{"type":"assistant","message":{"role":"assis"#;
    assert_eq!(lineage_fields(torn_early), None);
}
