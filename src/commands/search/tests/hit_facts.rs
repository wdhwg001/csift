//! `unmodeled_shape`: which VALUE the 1:1 law names when csift models no leaf for a
//! line. The three axes it splits on, and the answers it must refuse to give.

use super::*;

/// A shape csift already models answers NOTHING here, on every axis. That is not a
/// formality: the caller reaches this helper only for a record classify left unlabeled,
/// so a helper that named a modeled value anyway would put `? unmodeled: system/api_error`
/// on a reader's screen for a leaf csift renders perfectly well - the one failure the
/// unit exists to prevent.
#[test]
fn a_modeled_shape_has_no_unmodeled_value_on_any_axis() {
    let modeled_subtype =
        rec(r#"{"type":"system","subtype":"api_error","uuid":"s1","content":"boom"}"#);
    assert_eq!(unmodeled_shape(&modeled_subtype), None);
    let modeled_payload = rec(
        r#"{"type":"attachment","uuid":"a1","attachment":{"type":"hook_additional_context","content":["x"]}}"#,
    );
    assert_eq!(unmodeled_shape(&modeled_payload), None);
    let modeled_line = rec(r#"{"type":"cost-state","sessionId":"s","totalCostUSD":1.0}"#);
    assert_eq!(unmodeled_shape(&modeled_line), None);
    // The two transcript types are never unmodeled either: their leaf is decided deeper.
    let user = rec(r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"hi"}}"#);
    assert_eq!(unmodeled_shape(&user), None);
}

/// And an UNmodeled shape names the value on the axis that decides it - the deeper one
/// wherever the top-level type is itself modeled, because the type is not what csift
/// failed to catch up with.
#[test]
fn an_unmodeled_shape_names_the_axis_that_decides_it() {
    let subtype = rec(r#"{"type":"system","subtype":"some_future_subtype","uuid":"s2"}"#);
    assert_eq!(
        unmodeled_shape(&subtype).as_deref(),
        Some("system/some_future_subtype")
    );
    let payload =
        rec(r#"{"type":"attachment","uuid":"a2","attachment":{"type":"some_future_payload"}}"#);
    assert_eq!(
        unmodeled_shape(&payload).as_deref(),
        Some("attachment/some_future_payload")
    );
    let line = rec(r#"{"type":"future-thing","uuid":"f1","note":"x"}"#);
    assert_eq!(unmodeled_shape(&line).as_deref(), Some("future-thing"));
    // A system line with no subtype at all, and a typeless line: no value to name.
    assert_eq!(
        unmodeled_shape(&rec(r#"{"type":"system","uuid":"s3"}"#)),
        None
    );
    assert_eq!(unmodeled_shape(&rec(r#"{"uuid":"n1"}"#)), None);
}
