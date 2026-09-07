//! The record-level delivery override: the three shapes where Claude Code's request
//! assembler disagrees with the leaf default, and the shapes it must NOT fire on.

use super::*;

/// A `system`/`local_command` record - message-less, so the leaf default calls it
/// invisible, yet the assembler re-mints it as a user message.
#[test]
fn local_command_is_delivered_although_its_leaf_is_not() {
    let line = r#"{"type":"system","subtype":"local_command","level":"info","uuid":"s1","timestamp":"2026-06-07T05:00:01.000Z","content":"<local-command-stdout>done</local-command-stdout>","isMeta":false}"#;
    let rec = parse(line);
    assert_eq!(rec.delivery_override(), Some(true));
    // The LABEL is unchanged: delivery is a selection fact, not a classification one.
    assert_eq!(rec.promoted_class(), Some(Class::MetaSystem));
    assert!(!Class::MetaSystem.llm_visible());
}

/// Every OTHER system subtype keeps the leaf default (no override), so the two agree.
#[test]
fn other_system_subtypes_carry_no_override() {
    for sub in [
        "informational",
        "api_error",
        "agents_killed",
        "scheduled_task_fire",
        "model_refusal_fallback",
        "turn_duration",
        "away_summary",
        "stop_hook_summary",
        "compact_boundary",
        "some_future_subtype",
    ] {
        let line = format!(
            r#"{{"type":"system","subtype":"{sub}","uuid":"x","timestamp":"2026-06-07T05:00:01.000Z","content":"x"}}"#
        );
        assert_eq!(parse(&line).delivery_override(), None, "{sub}");
    }
    // A system record with no subtype at all is not the delivered one either.
    let bare =
        r#"{"type":"system","uuid":"z","timestamp":"2026-06-07T05:00:02.000Z","content":"x"}"#;
    assert_eq!(parse(bare).delivery_override(), None);
}

/// `isVirtual` - rendered in the UI, filtered before the API send - on either role.
#[test]
fn is_virtual_records_are_not_delivered() {
    let user = r#"{"type":"user","isVirtual":true,"uuid":"u1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":"typed"}}"#;
    assert_eq!(parse(user).delivery_override(), Some(false));
    let asst = r#"{"type":"assistant","isVirtual":true,"uuid":"a1","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"assistant","model":"claude-x","content":[{"type":"text","text":"hi"}]}}"#;
    assert_eq!(parse(asst).delivery_override(), Some(false));
    // `false` is not `true`: an explicitly non-virtual record carries no override.
    let plain = r#"{"type":"user","isVirtual":false,"uuid":"u2","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"user","content":"typed"}}"#;
    assert_eq!(parse(plain).delivery_override(), None);
}

/// The API-error placeholder needs BOTH halves: the marker and the `<synthetic>` model.
#[test]
fn the_api_error_placeholder_needs_both_the_marker_and_the_sentinel_model() {
    let placeholder = r#"{"type":"assistant","isApiErrorMessage":true,"uuid":"a2","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"assistant","model":"<synthetic>","content":[{"type":"text","text":"API Error: 500"}]}}"#;
    assert_eq!(parse(placeholder).delivery_override(), Some(false));
    // A real assistant turn that merely errored keeps its real model - and is sent.
    let real_model = r#"{"type":"assistant","isApiErrorMessage":true,"uuid":"a3","timestamp":"2026-06-07T05:00:07.000Z","message":{"role":"assistant","model":"claude-x","content":[{"type":"text","text":"API Error: 500"}]}}"#;
    assert_eq!(parse(real_model).delivery_override(), None);
    // The sentinel model alone is not the marker.
    let no_marker = r#"{"type":"assistant","uuid":"a4","timestamp":"2026-06-07T05:00:08.000Z","message":{"role":"assistant","model":"<synthetic>","content":[{"type":"text","text":"x"}]}}"#;
    assert_eq!(parse(no_marker).delivery_override(), None);
    // The rule is assistant-only: a user record with both fields is not the placeholder.
    let user_side = r#"{"type":"user","isApiErrorMessage":true,"uuid":"u3","timestamp":"2026-06-07T05:00:09.000Z","message":{"role":"user","model":"<synthetic>","content":"x"}}"#;
    assert_eq!(parse(user_side).delivery_override(), None);
}

/// The api-error rule must not fire on a SYSTEM record that happens to carry a model.
#[test]
fn a_local_command_with_a_model_field_still_reads_as_delivered() {
    let line = r#"{"type":"system","subtype":"local_command","uuid":"s2","timestamp":"2026-06-07T05:00:10.000Z","content":"echo","isApiErrorMessage":true,"message":{"role":"assistant","model":"<synthetic>"}}"#;
    assert_eq!(
        parse(line).delivery_override(),
        Some(true),
        "the system arm returns before the assistant arms are reached"
    );
}

/// An ordinary record - the overwhelming majority - carries no override at all, so the
/// leaf default decides exactly as it did before.
#[test]
fn ordinary_records_carry_no_override() {
    let human = r#"{"type":"user","uuid":"u4","timestamp":"2026-06-07T05:00:11.000Z","message":{"role":"user","content":"do the thing"}}"#;
    assert_eq!(parse(human).delivery_override(), None);
    let asst = r#"{"type":"assistant","uuid":"a5","timestamp":"2026-06-07T05:00:12.000Z","message":{"role":"assistant","model":"claude-x","content":[{"type":"text","text":"ok"}]}}"#;
    assert_eq!(parse(asst).delivery_override(), None);
    let attach = r#"{"type":"attachment","uuid":"t1","attachment":{"type":"file","x":1}}"#;
    assert_eq!(parse(attach).delivery_override(), None);
    let typeless = r#"{"uuid":"n1","timestamp":"2026-06-07T05:00:13.000Z"}"#;
    assert_eq!(parse(typeless).delivery_override(), None);
}
