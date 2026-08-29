//! The --until condition grammar: parse forms, matching arms, verdict duals.

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
fn condition_parse_error_forms_name_the_problem() {
    // Bad regexes fail at parse with the condition family named.
    for (tok, family) in [
        ("notification:(", "notification"),
        ("tool:Read:(", "tool"),
        ("write:(", "write"),
        ("write:ok:(", "write"),
    ] {
        let e = parse_condition(tok).unwrap_err().to_string();
        assert!(e.contains(family) && e.contains("bad regex"), "{tok}: {e}");
    }
    // A notification typo gets the did-you-mean form, not the generic set dump.
    let e = parse_condition("notifications").unwrap_err().to_string();
    assert!(e.contains("did you mean"), "{e}");
    // The generic unknown-head error enumerates the closed set.
    let e = parse_condition("growth").unwrap_err().to_string();
    assert!(e.contains("stop | hitl | auq"), "{e}");
}

#[test]
fn notification_condition_scopes_to_main_and_matches_payload() {
    let pulse: crate::model::Record = serde_json::from_str(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>b497m4ncp</task-id>\n<status>completed</status>\n<summary>Background command \"build venvs\" completed (exit code 0)</summary>\n</task-notification>"}}"#,
    )
    .unwrap();
    let any = parse_condition("notification").unwrap();
    assert!(record_matches(&any, &pulse, true));
    assert!(
        !record_matches(&any, &pulse, false),
        "notifications persist in MAIN only; a child line never fires the condition"
    );
    let hit = parse_condition("notification:build venvs").unwrap();
    assert!(record_matches(&hit, &pulse, true));
    let miss = parse_condition("notification:some other job").unwrap();
    assert!(!record_matches(&miss, &pulse, true));
    // A plain user message is not a notification even on main.
    let plain: crate::model::Record =
        serde_json::from_str(r#"{"type":"user","message":{"role":"user","content":"hello"}}"#)
            .unwrap();
    assert!(!record_matches(&any, &plain, true));
}

#[test]
fn auq_condition_matches_sidecar_pending_and_native_ask() {
    let auq = parse_condition("auq").unwrap();
    let pending: crate::model::Record = serde_json::from_str(
        r#"{"csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"toolu_1","type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"AskUserQuestion","input":{}}]}}"#,
    )
    .unwrap();
    assert!(record_matches(&auq, &pending, true));
    let resolved: crate::model::Record = serde_json::from_str(
        r#"{"csift":"elicitation-marker-v1","csiftPhase":"resolved","csiftKind":"AskUserQuestion","csiftKey":"toolu_1","type":"csift-elicitation-resolved"}"#,
    )
    .unwrap();
    assert!(!record_matches(&auq, &resolved, true));
    // The native ask (an answered AUQ's buffered turn landing) fires too.
    let native: crate::model::Record = serde_json::from_str(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"AskUserQuestion","input":{"questions":[]}}]}}"#,
    )
    .unwrap();
    assert!(record_matches(&auq, &native, true));
    let text_only: crate::model::Record = serde_json::from_str(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"no ask"}]}}"#,
    )
    .unwrap();
    assert!(!record_matches(&auq, &text_only, true));
}

#[test]
fn tool_and_write_condition_edge_arms() {
    // tool: input absent - a name-only condition still fires; an input regex cannot.
    let bare: crate::model::Record = serde_json::from_str(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read"}]}}"#,
    )
    .unwrap();
    assert!(record_matches(
        &parse_condition("tool:Read").unwrap(),
        &bare,
        true
    ));
    assert!(!record_matches(
        &parse_condition("tool:Read:handover").unwrap(),
        &bare,
        true
    ));
    // write: notebook_path is a path source; a path miss never consults the line regex.
    let nb: crate::model::Record = serde_json::from_str(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t2","name":"NotebookEdit","input":{"notebook_path":"/p/lab.ipynb","new_source":"x = 1"}}]}}"#,
    )
    .unwrap();
    assert!(record_matches(
        &parse_condition("write:lab\\.ipynb").unwrap(),
        &nb,
        true
    ));
    assert!(!record_matches(
        &parse_condition("write:other\\.md:x = 1").unwrap(),
        &nb,
        true
    ));
    // A non-write tool never satisfies a write condition.
    let read: crate::model::Record = serde_json::from_str(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t3","name":"Read","input":{"file_path":"/p/lab.ipynb"}}]}}"#,
    )
    .unwrap();
    assert!(!record_matches(
        &parse_condition("write:lab\\.ipynb").unwrap(),
        &read,
        true
    ));
}

#[test]
fn verdict_matches_covers_hitl_and_rejects_record_class() {
    let hitl = parse_condition("hitl").unwrap();
    assert!(verdict_matches(&hitl, Verdict::WaitingHitl));
    assert!(!verdict_matches(&hitl, Verdict::IdleEot));
    assert!(verdict_matches(
        &parse_condition("verdict:running").unwrap(),
        Verdict::Running
    ));
    // Record-class conditions never match at the verdict level.
    assert!(!verdict_matches(
        &parse_condition("auq").unwrap(),
        Verdict::Running
    ));
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

#[test]
fn notification_regex_matches_the_synthesized_label_alone() {
    // `[<kind> ...]` exists ONLY in the synthesized label, never in the raw payload -
    // a label hit must fire on its own (the body match is a fallback, not a partner).
    let pulse: crate::model::Record = serde_json::from_str(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>b497m4ncp</task-id>\n<status>completed</status>\n<summary>Background command \"build venvs\" completed (exit code 0)</summary>\n</task-notification>"}}"#,
    )
    .unwrap();
    let label_only = parse_condition("notification:\\[background-command b497m4ncp").unwrap();
    assert!(record_matches(&label_only, &pulse, true));
}
