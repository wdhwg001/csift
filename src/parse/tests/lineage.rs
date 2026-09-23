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
    assert!(f.no_session_lineage());
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
    assert!(!f.no_session_lineage());
}

#[test]
fn both_fields_ride_one_line_and_whitespace_around_the_colon_is_the_same_record() {
    // A reserialized line (whitespace after the colon) is the same record - the needle law the
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

#[test]
fn a_malformed_object_interior_stops_the_walk_rather_than_guessing() {
    // Two shapes the walk cannot continue past, both yielding nothing: a token at a KEY
    // position that is not a string, and a key with no colon after it. Neither can be a
    // field, and inventing one from the bytes already read would be worse than saying so.
    assert_eq!(
        lineage_fields(br#"{"sessionKind":"bg", 7:"x"}"#),
        None,
        "a bare number at a key position ends the walk"
    );
    assert_eq!(
        lineage_fields(br#"{"slug" "quiet-harbor-relay"}"#),
        None,
        "a key with no colon ends the walk"
    );
}

#[test]
fn the_slug_and_the_timestamp_ride_the_same_walk() {
    // `plan --audit` reads these two; an EMPTY slug is read as absent, because an empty slug
    // binds nothing and would otherwise register as a change point of its own.
    let f = lineage_fields(
        br#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","slug":"quiet-harbor-relay"}"#,
    )
    .expect("an object");
    assert_eq!(f.slug.as_deref(), Some("quiet-harbor-relay"));
    assert_eq!(f.timestamp.as_deref(), Some("2026-06-07T05:00:00.000Z"));
    // A slug-only line carries no SESSION lineage, which is what the narrower predicate says.
    assert!(f.no_session_lineage());

    let empty = lineage_fields(br#"{"type":"user","slug":""}"#).expect("an object");
    assert_eq!(empty.slug, None, "an empty slug binds nothing");
    assert!(line_has_slug_key(br#"{"type":"user","slug":"x"}"#));
    assert!(!line_has_slug_key(br#"{"type":"user","sessionKind":"bg"}"#));
}

/// The MINTED-IN pair: a record names the session its object came from under the snake
/// spelling and the session whose file it now sits in under the camel one. The walk reads
/// both so the comparison can be made on the line itself.
#[test]
fn a_minted_id_is_a_link_only_where_it_points_somewhere_else() {
    let own = "1a2b3c4d-5e6f-4708-9a1b-2c3d4e5f6071";
    let other = "22222222-2222-4333-8444-555555555511";

    let hydrated = lineage_fields(
        format!(r#"{{"type":"user","session_id":"{other}","sessionId":"{own}"}}"#).as_bytes(),
    )
    .expect("an object");
    assert_eq!(hydrated.minted_elsewhere(), Some(other));
    assert!(
        !hydrated.no_session_lineage(),
        "a disagreeing pair IS a session-lineage answer"
    );

    let agreeing = lineage_fields(
        format!(r#"{{"type":"user","session_id":"{own}","sessionId":"{own}"}}"#).as_bytes(),
    )
    .expect("an object");
    assert_eq!(
        agreeing.minted_elsewhere(),
        None,
        "a pointer to this very file points nowhere"
    );
    assert!(agreeing.no_session_lineage());

    // Either half alone says nothing: with no camel id there is nothing to compare against.
    let snake_only =
        lineage_fields(format!(r#"{{"type":"user","session_id":"{other}"}}"#).as_bytes())
            .expect("an object");
    assert_eq!(snake_only.minted_elsewhere(), None);
    assert!(snake_only.no_session_lineage());
}

/// The prefilter admits the snake key, and refuses a line carrying none of the three.
#[test]
fn the_lineage_prefilter_admits_the_minted_key() {
    assert!(line_has_lineage_key(
        br#"{"type":"user","session_id":"x","sessionId":"y"}"#
    ));
    assert!(
        !line_has_lineage_key(br#"{"type":"user","sessionId":"y","slug":"quiet-harbor-relay"}"#),
        "the camel key alone is not a lineage needle: every record carries it"
    );
}
