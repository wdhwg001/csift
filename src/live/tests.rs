//! Unit tests for the live engine's pure pieces.

use super::*;

#[test]
fn condition_grammar_parses_and_rejects() {
    assert!(matches!(parse_condition("stop").unwrap(), Cond::Stop));
    assert!(matches!(parse_condition("hitl").unwrap(), Cond::Hitl));
    assert!(matches!(parse_condition("auq").unwrap(), Cond::Auq));
    assert!(matches!(
        parse_condition("notification").unwrap(),
        Cond::Notification(None)
    ));
    assert!(matches!(
        parse_condition("notification:done").unwrap(),
        Cond::Notification(Some(_))
    ));
    match parse_condition("tool:Read:handover.md").unwrap() {
        Cond::Tool { name, input_re } => {
            assert_eq!(name, "Read");
            assert!(input_re.is_some());
        }
        other => panic!("{other:?}"),
    }
    match parse_condition("write:plans/.*\\.md:DONE").unwrap() {
        Cond::Write { line_re, .. } => assert!(line_re.is_some()),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        parse_condition("verdict:stale-dead").unwrap(),
        Cond::VerdictIs(Verdict::StaleDead)
    ));
    for bad in ["", "stopp", "tool:", "write:", "verdict:done", "tool"] {
        assert!(parse_condition(bad).is_err(), "must reject `{bad}`");
    }
}

#[test]
fn registry_proc_start_parses_as_utc() {
    let t = parse_registry_proc_start("Sun Aug 16 09:04:23 2026").unwrap();
    assert_eq!(t.to_string(), "2026-08-16T09:04:23Z");
    assert!(parse_registry_proc_start("not a date").is_none());
}

#[test]
fn verdict_slugs_are_the_closed_wire_set() {
    let all = [
        (Verdict::Running, "running"),
        (Verdict::WaitingChildren, "waiting-children"),
        (Verdict::WaitingHitl, "waiting-hitl"),
        (Verdict::IdleEot, "idle-eot"),
        (Verdict::StaleDead, "stale-dead"),
        (Verdict::Unknown, "unknown"),
    ];
    for (v, s) in all {
        assert_eq!(v.slug(), s);
        assert!(matches!(
            parse_condition(&format!("verdict:{s}")).unwrap(),
            Cond::VerdictIs(x) if x == v
        ));
    }
}

#[test]
fn record_conditions_match_the_right_events() {
    let read: crate::model::Record = serde_json::from_str(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/p/handover.md"}}]}}"#,
    )
    .unwrap();
    let tool = parse_condition("tool:Read:handover").unwrap();
    assert!(record_matches(&tool, &read, true));
    let other = parse_condition("tool:Write").unwrap();
    assert!(!record_matches(&other, &read, true));

    let write: crate::model::Record = serde_json::from_str(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t2","name":"Write","input":{"file_path":"/p/notes/final.md","content":"alpha\nDONE beacon\n"}}]}}"#,
    )
    .unwrap();
    let w = parse_condition("write:final\\.md:DONE").unwrap();
    assert!(record_matches(&w, &write, true));
    let w_miss = parse_condition("write:final\\.md:ABSENT").unwrap();
    assert!(!record_matches(&w_miss, &write, true));

    // stop/hitl/verdict are assessment-level, never record-level.
    assert!(!record_matches(
        &parse_condition("stop").unwrap(),
        &read,
        true
    ));
    assert!(verdict_matches(
        &parse_condition("stop").unwrap(),
        Verdict::StaleDead
    ));
    assert!(!verdict_matches(
        &parse_condition("stop").unwrap(),
        Verdict::Running
    ));
}
