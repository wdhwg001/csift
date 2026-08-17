//! Task-notification parsing: automation kinds, labels, monitor cadence.

use super::*;

// ── Automation-trigger classification (`<task-notification>`) ──

#[test]
fn automation_trigger_parses_task_notification() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>wh1it9jlj</task-id>\n<tool-use-id>toolu_x</tool-use-id>\n<output-file>/tmp/x.output</output-file>\n<status>completed</status>\n<summary>Dynamic workflow \"READ-ONLY: verify csift files\" completed</summary>\n</task-notification>"}}"#,
    );
    // It STILL opens a turn (it is a real boundary) — but is now classified.
    assert!(
        r.is_genuine_user(),
        "a task-notification passes the genuine-user gate"
    );
    assert!(r.opens_turn());
    let t = r.automation_trigger().expect("classified as automation");
    assert_eq!(t.task_id.as_deref(), Some("wh1it9jlj"));
    assert_eq!(t.status.as_deref(), Some("completed"));
    assert_eq!(
        t.kind,
        AutomationKind::Workflow,
        "Dynamic workflow → workflow"
    );
    assert_eq!(
        t.summary.as_deref(),
        Some("Dynamic workflow \"READ-ONLY: verify csift files\" completed")
    );
    // The rendered ATTRIBUTION label replaces the raw XML blob; a `Dynamic workflow`
    // summary keeps the `workflow` kind.
    let label = r.automation_label().unwrap();
    assert!(
        label.starts_with("[workflow wh1it9jlj completed]"),
        "got: {label}"
    );
    assert!(
        label.contains("Dynamic workflow"),
        "summary in label: {label}"
    );
    assert!(
        !label.contains("<task-id>"),
        "raw XML must not leak: {label}"
    );
}

#[test]
fn automation_trigger_none_for_human_and_partial_graceful() {
    // A plain human message is NOT an automation trigger.
    let human =
        parse(r#"{"type":"user","message":{"role":"user","content":"please fix the bug"}}"#);
    assert!(human.automation_trigger().is_none());
    assert!(human.automation_label().is_none());
    // A partial notification (no summary/status) still labels gracefully with fallbacks.
    let partial = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>abc</task-id>\n</task-notification>"}}"#,
    );
    // No summary → kind falls back to `task` (NOT the old hardcoded `workflow`).
    let label = partial.automation_label().unwrap();
    assert_eq!(label, "[task abc completed]");
}

#[test]
fn automation_kind_classifies_background_command_and_agent() {
    // The mislabel fix: a `Background command "…"` summary renders `background-command`,
    // an `Agent …` summary renders `agent` — NOT the old blanket `workflow`.
    let bg = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>b497m4ncp</task-id>\n<status>completed</status>\n<summary>Background command \"build venvs\" completed (exit code 0)</summary>\n</task-notification>"}}"#,
    );
    let t = bg.automation_trigger().unwrap();
    assert_eq!(t.kind, AutomationKind::BackgroundCommand);
    assert!(
        bg.automation_label()
            .unwrap()
            .starts_with("[background-command b497m4ncp completed]"),
        "got: {:?}",
        bg.automation_label()
    );
    let ag = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>ag1</task-id>\n<status>completed</status>\n<summary>Agent executor finished its task</summary>\n</task-notification>"}}"#,
    );
    assert_eq!(ag.automation_trigger().unwrap().kind, AutomationKind::Agent);
    assert!(ag
        .automation_label()
        .unwrap()
        .starts_with("[agent ag1 completed]"));
    // A failed background command keeps its kind + status.
    let bgf = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>b0855naiz</task-id>\n<status>failed</status>\n<summary>Background command \"Launch the overnight guard\" failed with exit code 1</summary>\n</task-notification>"}}"#,
    );
    assert!(bgf
        .automation_label()
        .unwrap()
        .starts_with("[background-command b0855naiz failed]"));
}

#[test]
fn monitor_cadence_event_replaces_fabricated_completed_status() {
    // A real-captured monitor shape: a Monitor pulse with NO <status> but a real
    // <event> outcome. The label must surface the EVENT (STAGE2_OUTPUT_READY), not fabricate
    // `completed` — which would invert a timed-out monitor's attribution.
    let mon = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>b718g3gqq</task-id>\n<summary>Monitor event: \"full test suite re-run completion\"</summary>\n<event>STAGE2_OUTPUT_READY</event>\n</task-notification>"}}"#,
    );
    let t = mon.automation_trigger().expect("a monitor trigger");
    assert_eq!(t.kind, AutomationKind::Monitor);
    assert_eq!(t.status, None, "this monitor pulse carries no <status>");
    assert_eq!(t.event.as_deref(), Some("STAGE2_OUTPUT_READY"));
    let label = mon.automation_label().unwrap();
    assert!(
        label.starts_with("[monitor b718g3gqq STAGE2_OUTPUT_READY]"),
        "event must replace fabricated `completed`: {label}"
    );
    assert!(
        !label.contains("completed"),
        "no fabricated `completed` when an event is present: {label}"
    );

    // A timed-out monitor carries the timeout notice in <event> — also surfaced, never
    // inverted to `completed`.
    let timeout = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>q9</task-id>\n<summary>Monitor tick</summary>\n<event>[Monitor timed out — re-arm if needed.]</event>\n</task-notification>"}}"#,
    );
    let label2 = timeout.automation_label().unwrap();
    assert!(label2.contains("Monitor timed out"), "got: {label2}");
    assert!(!label2.contains("completed"), "got: {label2}");

    // When BOTH status and event are absent, the label still falls back to `completed`.
    let bare = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>z</task-id>\n<summary>Monitor tick</summary>\n</task-notification>"}}"#,
    );
    assert_eq!(
        bare.automation_label().unwrap(),
        "[monitor z completed] Monitor tick"
    );

    // An explicit <status> still wins over <event> (status is the more authoritative slot).
    let both = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>w</task-id>\n<status>failed</status>\n<summary>Monitor tick</summary>\n<event>SOME_EVENT</event>\n</task-notification>"}}"#,
    );
    assert!(both
        .automation_label()
        .unwrap()
        .starts_with("[monitor w failed]"));
}

#[test]
fn automation_kind_from_summary_direct() {
    use AutomationKind::*;
    assert_eq!(
        AutomationKind::from_summary(Some("Background command \"x\"")),
        BackgroundCommand
    );
    assert_eq!(
        AutomationKind::from_summary(Some("Dynamic workflow \"x\"")),
        Workflow
    );
    assert_eq!(
        AutomationKind::from_summary(Some("workflow run done")),
        Workflow
    );
    assert_eq!(AutomationKind::from_summary(Some("Agent x done")), Agent);
    assert_eq!(
        AutomationKind::from_summary(Some("  background command y")),
        BackgroundCommand
    );
    // A monitor-COMPLETION `<task-notification>` (summary opens Monitor/Scheduled/cron)
    // is its own labeled class — the real `Monitor event: "…"` pulse (seen many times
    // across captures) must NOT fall to `task`. (This is NOT the isMeta ScheduleWakeup tick
    // PROMPT, which never reaches this summary classifier; see AutomationKind::Monitor docs.)
    assert_eq!(
        AutomationKind::from_summary(Some("Monitor event: \"full test suite re-run\"")),
        Monitor
    );
    assert_eq!(AutomationKind::from_summary(Some("Monitor tick")), Monitor);
    assert_eq!(
        AutomationKind::from_summary(Some("Scheduled wakeup fired")),
        Monitor
    );
    assert_eq!(AutomationKind::from_summary(Some("cron run")), Monitor);
    // The captured-monitor shape: a monitor/cron cadence implemented as a `&`-detached
    // `Background command "<monitor-named>"`. The quoted command NAME carrying a
    // monitor-cadence token routes to Monitor (not the generic BackgroundCommand), so the
    // dominant monitor activity is not disguised. (Verified against a captured session, where
    // the monitor loop is `Relaunch monitor timer` / `Re-arm corrected monitor` bg-cmds.)
    assert_eq!(
        AutomationKind::from_summary(Some(
            "Background command \"Relaunch monitor timer (cycle 2)\" completed"
        )),
        Monitor
    );
    assert_eq!(
        AutomationKind::from_summary(Some(
            "Background command \"Re-arm corrected monitor (full-tree liveness)\" completed"
        )),
        Monitor
    );
    assert_eq!(
        AutomationKind::from_summary(Some("Background command \"nightly monitor tick (25min)\"")),
        Monitor
    );
    // PRECISION: a background command that merely mentions monitoring in PROSE (outside the
    // quoted name) or names an unrelated command stays BackgroundCommand — no over-capture.
    assert_eq!(
        AutomationKind::from_summary(Some(
            "Background command \"Run pre-commit gate\" completed (monitor it for failures)"
        )),
        BackgroundCommand
    );
    assert_eq!(
        AutomationKind::from_summary(Some("Background command \"Baseline release build\"")),
        BackgroundCommand
    );
    // The standalone-word guard: `monitoring`/`demonitor` are NOT the word `monitor`.
    assert_eq!(
        AutomationKind::from_summary(Some("Background command \"resource monitoring agent\"")),
        BackgroundCommand
    );
    assert_eq!(AutomationKind::from_summary(Some("something else")), Task);
    assert_eq!(AutomationKind::from_summary(None), Task);
    assert_eq!(AutomationKind::from_summary(Some("")), Task);
    // Slugs round-trip.
    assert_eq!(BackgroundCommand.slug(), "background-command");
    assert_eq!(Workflow.slug(), "workflow");
    assert_eq!(Agent.slug(), "agent");
    assert_eq!(Monitor.slug(), "monitor");
    assert_eq!(Task.slug(), "task");
}

#[test]
fn automation_trigger_multibyte_summary_codepoint_safe() {
    // A multi-byte summary body must not be split mid-codepoint by the tag extractor.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>zh1</task-id>\n<status>completed</status>\n<summary>🤖 batch shipped, please summarize 🎉</summary>\n</task-notification>"}}"#,
    );
    let t = r.automation_trigger().unwrap();
    assert_eq!(
        t.summary.as_deref(),
        Some("🤖 batch shipped, please summarize 🎉")
    );
}

#[test]
fn extract_xml_tag_handles_missing_and_empty() {
    assert_eq!(extract_xml_tag("<a>x</a>", "a").as_deref(), Some("x"));
    assert_eq!(extract_xml_tag("<a></a>", "a"), None); // empty inner → None
    assert_eq!(extract_xml_tag("<a>x", "a"), None); // missing close → None
    assert_eq!(extract_xml_tag("no tags here", "a"), None);
}

#[test]
fn plural_word_pick_pinned() {
    // Mutation pin: `"s"` for anything but exactly one.
    assert_eq!(plural(0), "s");
    assert_eq!(plural(1), "");
    assert_eq!(plural(2), "s");
}

#[test]
fn monitor_cadence_tokens_route_each_disjunct() {
    // Mutation pin: each cadence token ALONE routes a `Background command "…"` pulse to
    // Monitor (the disjuncts must stay independent), and a plain name stays bg-command.
    for s in [
        r#"Background command "nightly monitor tick (25min)" completed"#,
        r#"Background command "liveness probe" completed"#,
        r#"Background command "Re-arm corrected watchdog" completed"#,
        r#"Background command "Relaunch monitor timer (cycle 2)" completed"#,
    ] {
        assert_eq!(
            AutomationKind::from_summary(Some(s)),
            AutomationKind::Monitor,
            "{s}"
        );
    }
    assert_eq!(
        AutomationKind::from_summary(Some(r#"Background command "build project" completed"#)),
        AutomationKind::BackgroundCommand,
        "a plain quoted name stays background-command"
    );
    // The word must be STANDALONE — a substring inside a larger word is not the signal.
    assert_eq!(
        AutomationKind::from_summary(Some(
            r#"Background command "monitoring-dashboard build" completed"#
        )),
        AutomationKind::BackgroundCommand,
        "substring 'monitor' inside a larger word must not route"
    );
}

#[test]
fn automation_label_failed_status_and_no_summary() {
    // A non-`completed` status is rendered verbatim (the status arm is not hardcoded).
    let failed = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>bad7</task-id>\n<status>failed</status>\n<summary>The build broke</summary>\n</task-notification>"}}"#,
    );
    // "The build broke" carries no kind classifier → `task` fallback.
    assert_eq!(
        failed.automation_label().as_deref(),
        Some("[task bad7 failed] The build broke")
    );
    // A trigger with a status but EMPTY summary → the head-only arm (no trailing text).
    let no_sum = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<task-id>q9</task-id>\n<status>running</status>\n<summary></summary>\n</task-notification>"}}"#,
    );
    assert_eq!(
        no_sum.automation_label().as_deref(),
        Some("[task q9 running]")
    );
    // A trigger with NO task-id and NO status → both `?`/`completed` fallbacks fire.
    let bare = parse(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<summary>just a note</summary>\n</task-notification>"}}"#,
    );
    assert_eq!(
        bare.automation_label().as_deref(),
        Some("[task ? completed] just a note")
    );
}
