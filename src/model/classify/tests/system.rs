//! The system family: the 11 observed `type:"system"` subtypes, one leaf each.

use super::*;
use crate::model::classify::system;

#[test]
fn every_observed_subtype_maps_to_its_leaf() {
    // The hand-kept table of all 11 subtypes, each with the leaf the map assigns it.
    let table: &[(&str, Class)] = &[
        ("compact_boundary", Class::CompactionBoundary),
        ("turn_duration", Class::TurnDuration),
        ("stop_hook_summary", Class::TurnStopHooks),
        ("away_summary", Class::TurnRecap),
        ("scheduled_task_fire", Class::ScheduleEvent),
        ("model_refusal_fallback", Class::SystemRefusal),
        ("model_refusal_no_fallback", Class::SystemRefusal),
        ("api_error", Class::SystemApiError),
        ("informational", Class::SystemNotice),
        ("agents_killed", Class::SystemKill),
        ("local_command", Class::SystemCommand),
    ];
    assert_eq!(table.len(), 11, "the subtype table is not 11 rows");
    for &(sub, leaf) in table {
        assert_eq!(system::classify(sub), Some(leaf), "{sub}");
        let line = format!(
            r#"{{"type":"system","subtype":"{sub}","uuid":"u","timestamp":"2026-06-07T05:00:01.000Z","content":"x"}}"#
        );
        assert_eq!(labels(&line), vec![leaf], "{line}");
    }
    // An unmodeled subtype yields NO leaf: an address then renders it as an unmodeled
    // unit naming the subtype, which is more use than a catch-all that says nothing.
    assert_eq!(system::classify("some_future_subtype"), None);
    let future = r#"{"type":"system","subtype":"some_future_subtype","uuid":"u","content":"x"}"#;
    assert!(labels(future).is_empty(), "{future}");
    // A system record with NO subtype has nothing to name it by.
    assert!(labels(r#"{"type":"system","uuid":"u"}"#).is_empty());
}

#[test]
fn only_the_local_command_leaf_is_visible_and_the_record_says_so() {
    for c in [
        Class::SystemRefusal,
        Class::SystemApiError,
        Class::SystemNotice,
        Class::SystemKill,
        Class::ScheduleEvent,
        Class::TurnDuration,
        Class::TurnStopHooks,
        Class::TurnRecap,
        Class::CompactionBoundary,
    ] {
        assert!(!c.llm_visible(), "{} must be invisible", c.path());
    }
    assert!(
        Class::SystemCommand.llm_visible(),
        "the re-minted subtype is the conversation"
    );
    // And the RECORD-level verdict agrees: the assembler sends a local_command record
    // and drops every other system subtype.
    let lc = r#"{"type":"system","subtype":"local_command","level":"info","uuid":"lc1","timestamp":"2026-06-07T05:00:02.000Z","content":"<local-command-stdout>ok</local-command-stdout>"}"#;
    assert_eq!(labels(lc), vec![Class::SystemCommand]);
    assert_eq!(parse(lc).delivery_override(), Some(true));
    let info = r#"{"type":"system","subtype":"informational","level":"warning","uuid":"i1","timestamp":"2026-06-07T05:00:01.000Z","content":"Remote Control disconnected - run /remote-control"}"#;
    assert_eq!(labels(info), vec![Class::SystemNotice]);
    assert_eq!(parse(info).level.as_deref(), Some("warning"));
    assert_eq!(parse(info).delivery_override(), None);
}

#[test]
fn the_boundary_keeps_its_own_leaf_and_its_predicate() {
    let boundary = r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","timestamp":"2026-06-07T05:00:03.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"auto"}}"#;
    let rec = parse(boundary);
    assert!(rec.is_compact_boundary());
    assert_eq!(rec.promoted_class(), Some(Class::CompactionBoundary));
    assert_eq!(labels(boundary), vec![Class::CompactionBoundary]);
    // The predicate is type AND subtype: a user record naming the subtype is not one.
    let not =
        r#"{"type":"user","subtype":"compact_boundary","message":{"role":"user","content":"x"}}"#;
    assert!(!parse(not).is_compact_boundary());
}
