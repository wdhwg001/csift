//! Background reports under the lens, the sort order, the last messages, the activity
//! census, the tail-state words, and the seventh verdict (the scanner itself is tested
//! beside it in `background_scan.rs`).

use super::*;

#[test]
fn the_lens_ignores_by_time_and_pattern_but_still_lists() {
    let t = TempSession::new(&lines(&[LAUNCH, LAUNCH_RESULT, EOT]), None);
    // A pattern over the command.
    let lens = BackgroundLens::from_args(None, &["npm run dev".to_string()]).unwrap();
    assert!(lens.is_active());
    let r = report(&t, &lens);
    assert_eq!(r.open_counted(), 0);
    assert_eq!(r.open_ignored(), 1);
    assert_eq!(
        r.tasks[0].ignored_by.as_deref(),
        Some("matches --ignore-background npm run dev")
    );
    assert!(r.summary_line().contains("0 open (+1 ignored by the lens)"));
    // A pattern over the description.
    let lens = BackgroundLens::from_args(None, &["harbor".to_string()]).unwrap();
    assert_eq!(report(&t, &lens).open_ignored(), 1);
    // A non-matching pattern keeps it counted.
    let lens = BackgroundLens::from_args(None, &["reef".to_string()]).unwrap();
    assert_eq!(report(&t, &lens).open_counted(), 1);
    // `now` ignores everything already launched (the fixture is dated 2026-06-07).
    let lens = BackgroundLens::from_args(Some("now"), &[]).unwrap();
    let r = report(&t, &lens);
    assert_eq!(r.open_counted(), 0);
    assert_eq!(
        r.tasks[0].ignored_by.as_deref(),
        Some("launched before --background-since now")
    );
    // An absolute cutoff before the launch keeps it.
    let lens = BackgroundLens::from_args(Some("2026-06-01T00:00:00Z"), &[]).unwrap();
    assert_eq!(report(&t, &lens).open_counted(), 1);
    // Bad inputs fail loud.
    assert!(BackgroundLens::from_args(Some("yesterday-ish"), &[]).is_err());
    assert!(BackgroundLens::from_args(None, &["(".to_string()]).is_err());
    assert!(!BackgroundLens::default().is_active());
}

#[test]
fn open_tasks_sort_counted_then_ignored_then_closed_newest_first() {
    let l2 = LAUNCH
        .replace("\"t1\"", "\"t2\"")
        .replace("05:00:01", "05:00:03")
        .replace("npm run dev", "tail -f log")
        .replace("Serve the harbor app", "Follow the log");
    let r2 = LAUNCH_RESULT
        .replace("\"t1\"", "\"t2\"")
        .replace("b1a2b3c4d", "b2b2b2b2b")
        .replace("05:00:02", "05:00:04");
    let done1 = r#"{"type":"user","uuid":"n1","timestamp":"2026-06-07T05:03:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>b1a2b3c4d</task-id>\n<status>completed</status>\n<summary>done</summary>\n</task-notification>"}}"#;
    let l3 = LAUNCH
        .replace("\"t1\"", "\"t3\"")
        .replace("05:00:01", "05:00:06")
        .replace("npm run dev", "cargo watch");
    let t = TempSession::new(
        &lines(&[LAUNCH, LAUNCH_RESULT, &l2, &r2, &l3, EOT, done1]),
        None,
    );
    let lens = BackgroundLens::from_args(None, &["tail -f".to_string()]).unwrap();
    let r = report(&t, &lens);
    let order: Vec<(&str, bool, bool)> = r
        .tasks
        .iter()
        .map(|t| (t.tool_use_id.as_str(), t.is_open(), t.ignored_by.is_some()))
        .collect();
    assert_eq!(
        order,
        vec![
            ("t3", true, false),
            ("t2", true, true),
            ("t1", false, false)
        ],
        "{order:?}"
    );
}

#[test]
fn last_messages_take_the_newest_prompt_and_reply_as_excerpts() {
    let long = "x".repeat(900);
    let main = format!(
        concat!(
            r#"{{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"first prompt"}}}}"#,
            "\n",
            r#"{{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:05.000Z","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"first reply"}}]}}}}"#,
            "\n",
            r#"{{"type":"user","uuid":"u2","timestamp":"2026-06-07T05:01:00.000Z","message":{{"role":"user","content":"<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n<summary>Background command finished: lint</summary>\n</task-notification>"}}}}"#,
            "\n",
            r#"{{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:01:05.000Z","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"{long}"}}]}}}}"#,
            "\n",
        ),
        long = long
    );
    let t = TempSession::new(&main, None);
    let l = last_messages(&t.main).unwrap();
    let u = l.user.unwrap();
    assert!(
        u.text.starts_with("[background-command b1 completed]"),
        "{}",
        u.text
    );
    assert_eq!(u.ts_utc.as_deref(), Some("2026-06-07T05:01:00.000Z"));
    assert!(!u.truncated);
    let a = l.agent.unwrap();
    assert!(a.truncated);
    assert!(a.text.ends_with("(+500 chars)"), "{}", a.text);
    assert_eq!(a.ts_utc.as_deref(), Some("2026-06-07T05:01:05.000Z"));
    // An empty file yields nothing (never an error).
    let e = TempSession::new("", None);
    let l = last_messages(&e.main).unwrap();
    assert!(l.user.is_none() && l.agent.is_none());
}

#[test]
fn activity_census_and_tail_state_words() {
    let mut act = Activity::default();
    let parse = |s: &str| crate::parse::parse_line(s.as_bytes()).unwrap().unwrap();
    act.fold(&parse(LAUNCH), "main");
    act.fold(&parse(EOT), "main");
    act.fold(
        &parse(
            r#"{"type":"assistant","uuid":"a3","timestamp":"t","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hm"},{"type":"tool_use","id":"t5","name":"Read","input":{"file_path":"/x"}}]}}"#,
        ),
        "abcdef0123456789",
    );
    act.fold(
        &parse(r#"{"type":"user","uuid":"u9","timestamp":"t","message":{"role":"user","content":"go on"}}"#),
        "main",
    );
    act.fold(
        &parse(
            r#"{"type":"user","uuid":"n9","timestamp":"t","message":{"role":"user","content":"<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n<summary>Background command finished</summary>\n</task-notification>"}}"#,
        ),
        "main",
    );
    assert_eq!(act.records, 5);
    assert_eq!(act.lanes.len(), 2);
    assert_eq!(act.tools.get("Bash"), Some(&1));
    assert_eq!(act.tools.get("Read"), Some(&1));
    assert_eq!(
        (
            act.thinking,
            act.agent_messages,
            act.user_prompts,
            act.notifications
        ),
        (1, 1, 1, 1)
    );
    assert_eq!(
        act.summary_line(),
        "5 record(s) in 2 lane(s): tools Bash x1 Read x1 · thinking 1 · messages 1 · prompts 1 · notifications 1"
    );
    assert_eq!(
        Activity::default().summary_line(),
        "nothing landed after the baseline"
    );
    let j = act.json();
    assert_eq!(j["records"], 5);
    assert_eq!(j["tools"]["Bash"], 1);

    // Tail-state words.
    let in_call = shape(Some(("Bash", "2000-01-01T00:00:00Z")), Some("tool_use"), 3);
    assert!(
        tail_state_words(&in_call).starts_with("in a Bash call for "),
        "{}",
        tail_state_words(&in_call)
    );
    let now = jiff::Timestamp::now().to_string();
    let generating = TailShape {
        unreturned_use: None,
        last_stop_reason: Some("tool_use".to_string()),
        last_ts_utc: Some(now.clone()),
        records_seen: 4,
    };
    assert!(
        tail_state_words(&generating).starts_with("generating (last record"),
        "{}",
        tail_state_words(&generating)
    );
    let idle = TailShape {
        unreturned_use: None,
        last_stop_reason: Some("end_turn".to_string()),
        last_ts_utc: Some(now),
        records_seen: 4,
    };
    assert!(
        tail_state_words(&idle).starts_with("idle (last stop_reason end_turn, last record"),
        "{}",
        tail_state_words(&idle)
    );
    let stale = shape(None, Some("tool_use"), 4); // 2026-06-07: far older than 300s
    assert!(
        tail_state_words(&stale).starts_with("idle (last stop_reason tool_use"),
        "{}",
        tail_state_words(&stale)
    );
    assert_eq!(
        tail_state_words(&TailShape::default()),
        "no records readable at the tail"
    );
}

#[test]
fn the_seventh_verdict_and_stop_semantics() {
    let bg_open = {
        let t = TempSession::new(&lines(&[LAUNCH, LAUNCH_RESULT, EOT]), None);
        report(&t, &BackgroundLens::default())
    };
    let eot = shape(None, Some("end_turn"), 4);
    let a = assess_full(
        Some(&row("idle", None)),
        None,
        &eot,
        &ChildrenReport::default(),
        &[],
        false,
        &bg_open,
    );
    assert_eq!(a.verdict, Verdict::IdleBackgroundOpen);
    assert_eq!(a.verdict.slug(), "idle-background-open");
    assert!(a
        .evidence
        .iter()
        .any(|e| e.surface == "background" && e.value.starts_with("1 open;")));
    assert!(note_text(&a).contains("have not returned"));
    assert!(
        note_text(&a).contains("permission prompt"),
        "the F7 note applies here too"
    );
    // Never a stop; the explicit verdict condition reaches it.
    assert!(!verdict_matches(
        &parse_condition("stop").unwrap(),
        Verdict::IdleBackgroundOpen
    ));
    assert!(verdict_matches(
        &parse_condition("verdict:idle-background-open").unwrap(),
        Verdict::IdleBackgroundOpen
    ));
    // Under a lens that ignores it, the same shape is a clean stop.
    let lens = BackgroundLens::from_args(None, &["npm".to_string()]).unwrap();
    let bg_ignored = {
        let t = TempSession::new(&lines(&[LAUNCH, LAUNCH_RESULT, EOT]), None);
        report(&t, &lens)
    };
    let a = assess_full(
        Some(&row("idle", None)),
        None,
        &eot,
        &ChildrenReport::default(),
        &[],
        false,
        &bg_ignored,
    );
    assert_eq!(a.verdict, Verdict::IdleEot);
    assert!(a
        .evidence
        .iter()
        .any(|e| e.surface == "background" && e.value.contains("(+1 ignored by the lens)")));
    // Stronger evidence still outranks it: a live child, a running tail, a dead pid.
    let children = ChildrenReport {
        children: vec![],
        journal_in_flight: 0,
        live_count: 1,
    };
    assert_eq!(
        assess_full(None, None, &eot, &children, &[], false, &bg_open).verdict,
        Verdict::WaitingChildren
    );
    let running = shape(Some(("Bash", "2026-06-07T05:00:05Z")), Some("tool_use"), 4);
    assert_eq!(
        assess_full(
            None,
            None,
            &running,
            &ChildrenReport::default(),
            &[],
            false,
            &bg_open
        )
        .verdict,
        Verdict::Running
    );
    assert_eq!(
        assess_full(
            Some(&row("idle", Some(1))),
            Some(&PidLiveness::Dead),
            &eot,
            &ChildrenReport::default(),
            &[],
            false,
            &bg_open
        )
        .verdict,
        Verdict::StaleDead
    );
    // No background at all: the classic idle-eot, no background evidence row.
    let a = assess_full(
        None,
        None,
        &eot,
        &ChildrenReport::default(),
        &[],
        false,
        &BackgroundReport::default(),
    );
    assert_eq!(a.verdict, Verdict::IdleEot);
    assert!(!a.evidence.iter().any(|e| e.surface == "background"));
}
