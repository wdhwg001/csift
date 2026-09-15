//! The background scanner: launches, the three completion carriers, the orphan
//! summary, the agents-stopped notice, async agents, monitors, and the helpers.

use super::*;

// The three harness-side entrances. Each launch is an ORDINARY foreground call - no
// `run_in_background` key, and no background needle on the line at all - so only the
// second pass can reach it.
const FG_LAUNCH: &str = r#"{"type":"assistant","uuid":"g1","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t5","name":"Bash","input":{"command":"perl -e 'select(undef,undef,undef,150)'","description":"Wait then print the marker","timeout":200000}}]}}"#;
const CTRLB_RESULT: &str = r#"{"type":"user","uuid":"g2","timestamp":"2026-06-07T05:10:06.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t5","is_error":false,"content":"Command was manually backgrounded by user with ID: bzz111111. Output is being written to: /nonexistent/bzz111111.output."}]},"toolUseResult":{"stdout":"","stderr":"","interrupted":false,"isImage":false,"noOutputExpected":false,"backgroundTaskId":"bzz111111","backgroundedByUser":true}}"#;
const TO_LAUNCH: &str = r#"{"type":"assistant","uuid":"o1","timestamp":"2026-06-07T05:12:00.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t6","name":"Bash","input":{"command":"cargo test --all","description":"Run the suite"}}]}}"#;
const TO_RESULT: &str = r#"{"type":"user","uuid":"o2","timestamp":"2026-06-07T05:14:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t6","content":"Command did not complete within its 120s timeout and was moved to the background (ID: btt222222). Output is being written to: /nonexistent/btt222222.output. You will be notified when it completes. To check interim output, use Read on that file path."}]},"toolUseResult":{"backgroundTaskId":"btt222222","timedOutAfterMs":120000}}"#;
const DM_RESULT: &str = r#"{"type":"user","uuid":"d2","timestamp":"2026-06-07T05:16:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t4","content":"Command was moved to the background (ID: bdd333333) so that a message that arrived while it was running can reach you; it was not interrupted. Output is being written to: /nonexistent/bdd333333.output."}]}}"#;

const ARM: &str = r#"{"type":"assistant","uuid":"m1","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t7","name":"Monitor","input":{"command":"tail -f build.log","description":"Watch the build","timeout_ms":300000}}]}}"#;
const ARM_RESULT: &str = r#"{"type":"user","uuid":"m2","timestamp":"2026-06-07T05:02:01.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t7","content":"Monitor started (task b7m7m7m7m, timeout 300000ms). You will be notified on each event."}]},"toolUseResult":{"taskId":"b7m7m7m7m","timeoutMs":300000}}"#;

#[test]
fn a_backgrounded_shell_is_open_until_a_carrier_names_it() {
    let t = TempSession::new(&lines(&[LAUNCH, LAUNCH_RESULT, EOT]), None);
    let r = report(&t, &BackgroundLens::default());
    assert_eq!(r.tasks.len(), 1, "{:?}", r.tasks);
    let task = &r.tasks[0];
    assert_eq!(task.kind, BgKind::Shell);
    assert_eq!(task.id.as_deref(), Some("b1a2b3c4d"));
    assert_eq!(task.tool_use_id, "t1");
    assert_eq!(task.description.as_deref(), Some("Serve the harbor app"));
    assert_eq!(task.command.as_deref(), Some("npm run dev"));
    assert_eq!(
        task.output_file.as_deref(),
        Some("/nonexistent/b1a2b3c4d.output")
    );
    assert!(task.is_open());
    assert_eq!(
        task.output_bytes, None,
        "a missing output file stats to nothing"
    );
    assert_eq!(r.open_counted(), 1);
    assert_eq!(
        r.summary_line(),
        "1 open; 0 completed, 0 failed, 0 killed, 0 stopped"
    );

    // A user-record notification closes it by tool-use-id (exact join).
    let done = r#"{"type":"user","uuid":"n1","timestamp":"2026-06-07T05:03:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>b1a2b3c4d</task-id>\n<tool-use-id>t1</tool-use-id>\n<status>completed</status>\n<summary>Background command \"Serve the harbor app\" completed</summary>\n</task-notification>"}}"#;
    let t = TempSession::new(&lines(&[LAUNCH, LAUNCH_RESULT, EOT, done]), None);
    let r = report(&t, &BackgroundLens::default());
    assert_eq!(r.tasks[0].state, BgState::Completed);
    assert_eq!(
        r.tasks[0].returned_utc.as_deref(),
        Some("2026-06-07T05:03:00.000Z")
    );
    assert_eq!(r.open_counted(), 0);
    assert_eq!(r.closed_counts(), (1, 0, 0, 0, 0));
}

#[test]
fn mid_turn_carriers_and_statuses_close_by_task_id() {
    // A queue-operation carrier (never a user record) with a killed status, joined by
    // the task id alone (no tool-use-id tag).
    let queued = r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-06-07T05:03:00.000Z","sessionId":"s","content":"<task-notification>\n<task-id>b1a2b3c4d</task-id>\n<status>killed</status>\n<summary>Background command \"Serve the harbor app\" was stopped</summary>\n</task-notification>"}"#;
    let t = TempSession::new(&lines(&[LAUNCH, LAUNCH_RESULT, EOT, queued]), None);
    assert_eq!(
        report(&t, &BackgroundLens::default()).tasks[0].state,
        BgState::Killed
    );
    // A queued_command attachment carrier with a failed status.
    let att = r#"{"type":"attachment","timestamp":"2026-06-07T05:03:00.000Z","attachment":{"type":"queued_command","commandMode":"task-notification","prompt":"<task-notification>\n<task-id>b1a2b3c4d</task-id>\n<tool-use-id>t1</tool-use-id>\n<status>failed</status>\n<summary>Background command failed with exit code 2</summary>\n</task-notification>"}}"#;
    let t = TempSession::new(&lines(&[LAUNCH, LAUNCH_RESULT, EOT, att]), None);
    assert_eq!(
        report(&t, &BackgroundLens::default()).tasks[0].state,
        BgState::Failed
    );
    // Claude Code's orphan reconciliation: several ids in one notice + the sentinel.
    let orphan = r#"{"type":"user","uuid":"n2","timestamp":"2026-06-08T05:00:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>b1a2b3c4d</task-id>\n<task-id>__orphan_summary__:shell</task-id>\n<status>stopped</status>\n<summary>1 background shell command task(s) from the previous session have no completion record.</summary>\n</task-notification>"}}"#;
    let t = TempSession::new(&lines(&[LAUNCH, LAUNCH_RESULT, EOT, orphan]), None);
    let r = report(&t, &BackgroundLens::default());
    assert_eq!(r.tasks[0].state, BgState::Stopped);
    assert!(
        r.notes.iter().any(|n| n.contains("reconciled as stopped")),
        "{:?}",
        r.notes
    );
    // A notice naming an unknown id changes nothing.
    let other = r#"{"type":"user","uuid":"n3","timestamp":"2026-06-07T05:03:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>zzzzzzzzz</task-id>\n<status>completed</status>\n<summary>Background command other</summary>\n</task-notification>"}}"#;
    let t = TempSession::new(&lines(&[LAUNCH, LAUNCH_RESULT, EOT, other]), None);
    assert!(report(&t, &BackgroundLens::default()).tasks[0].is_open());
}

#[test]
fn subagent_launches_complete_in_the_parent_main_transcript() {
    let sub_launch = r#"{"type":"assistant","uuid":"s1","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t9","name":"Bash","input":{"command":"cargo build","run_in_background":true}}]}}"#;
    let sub_result = r#"{"type":"user","uuid":"s2","timestamp":"2026-06-07T05:01:01.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t9","content":"Command running in background with ID: b9z8y7x6w. Output is being written to: /nonexistent/b9.output."}]}}"#;
    let main_done = r#"{"type":"user","uuid":"n1","timestamp":"2026-06-07T05:04:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>b9z8y7x6w</task-id>\n<tool-use-id>t9</tool-use-id>\n<status>completed</status>\n<summary>Background command completed</summary>\n</task-notification>"}}"#;
    let t = TempSession::new(
        &lines(&[EOT, main_done]),
        Some(&lines(&[sub_launch, sub_result])),
    );
    // Spanning the children finds the launch; the parent main's carrier closes it.
    let r = report(&t, &BackgroundLens::default());
    assert_eq!(r.tasks.len(), 1);
    assert_eq!(r.tasks[0].lane, "abcdef0123456789");
    assert_eq!(r.tasks[0].state, BgState::Completed);
    assert_eq!(r.scanned_files, 2);
    // --no-subagents: the child's launch is out of scope.
    let r = background_report(&t.main, false, &BackgroundLens::default()).unwrap();
    assert!(r.tasks.is_empty());
    // A SUBAGENT target still reads its parent's main for the completion.
    let r = background_report(&t.sub_path(), true, &BackgroundLens::default()).unwrap();
    assert_eq!(r.tasks.len(), 1);
    assert_eq!(r.tasks[0].state, BgState::Completed);
}

#[test]
fn async_agent_launches_and_the_stopped_notice() {
    let agent = r#"{"type":"user","uuid":"r2","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"Async agent launched successfully."}]},"toolUseResult":{"isAsync":true,"status":"async_launched","agentId":"a0123456789abcdef0","description":"Census the reef","outputFile":"/nonexistent/a0.output"}}"#;
    let t = TempSession::new(&lines(&[agent, EOT]), None);
    let r = report(&t, &BackgroundLens::default());
    assert_eq!(r.tasks.len(), 1);
    let a = &r.tasks[0];
    assert_eq!(a.kind, BgKind::Agent);
    assert_eq!(a.id.as_deref(), Some("a0123456789abcdef0"));
    assert_eq!(a.description.as_deref(), Some("Census the reef"));
    assert!(a.is_open());
    // Its completion names the agent id as the task id.
    let done = r#"{"type":"user","uuid":"n1","timestamp":"2026-06-07T05:09:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>a0123456789abcdef0</task-id>\n<tool-use-id>t2</tool-use-id>\n<status>completed</status>\n<summary>Agent \"Census the reef\" finished</summary>\n</task-notification>"}}"#;
    let t = TempSession::new(&lines(&[agent, EOT, done]), None);
    assert_eq!(
        report(&t, &BackgroundLens::default()).tasks[0].state,
        BgState::Completed
    );
    // The stopped notice (queue line + user record) yields ONE note and changes no state.
    let q = r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-06-07T05:10:00.000Z","sessionId":"s","content":"2 background agents were stopped by the user: \"Census the r...\", \"Chart the s...\"."}"#;
    let u = r#"{"type":"user","uuid":"k1","timestamp":"2026-06-07T05:10:00.200Z","message":{"role":"user","content":"2 background agents were stopped by the user: \"Census the r...\", \"Chart the s...\"."}}"#;
    let t = TempSession::new(&lines(&[agent, EOT, q, u]), None);
    let r = report(&t, &BackgroundLens::default());
    assert!(r.tasks[0].is_open());
    assert_eq!(r.notes.len(), 1, "{:?}", r.notes);
    assert!(r.notes[0]
        .starts_with("2 background agent(s) were stopped by the user at 2026-06-07T05:10:00"));
}

#[test]
fn helpers_parse_the_result_text_and_locate_the_main_transcript() {
    assert_eq!(
        after_marker(
            "Command running in background with ID: b1a2b3c4d. Output is being written to: /x/y.output. You will",
            "with ID: "
        )
        .as_deref(),
        Some("b1a2b3c4d")
    );
    assert_eq!(
        after_marker("... written to: /x/y.output. You", "written to: ").as_deref(),
        Some("/x/y.output")
    );
    assert_eq!(
        after_marker("... written to: /x/y.output", "written to: ").as_deref(),
        Some("/x/y.output")
    );
    assert_eq!(after_marker("no marker here", "with ID: "), None);
    assert_eq!(after_marker("with ID: ", "with ID: "), None);
    assert_eq!(
        all_xml_tags(
            "<task-id>a</task-id> x <task-id> b </task-id><task-id></task-id><task-id>c",
            "task-id"
        ),
        vec!["a".to_string(), "b".to_string()]
    );
    let main = std::path::Path::new("/p/-enc/1111-2222.jsonl");
    assert_eq!(main_transcript_for(main), main);
    let sub = std::path::Path::new("/p/-enc/1111-2222/subagents/workflows/wf_1/agent-ab.jsonl");
    assert_eq!(
        main_transcript_for(sub),
        std::path::PathBuf::from("/p/-enc/1111-2222.jsonl")
    );
    assert_eq!(BgState::from_status(Some("stopped")), BgState::Stopped);
    // v0.10.3: a fifth harness value and an unknown literal are their own buckets, never
    // booked as completed (the remote-agent notifier writes `blocked`).
    assert_eq!(BgState::from_status(Some("blocked")), BgState::Blocked);
    assert_eq!(BgState::from_status(Some("weird")), BgState::Other);
    assert_eq!(BgState::from_status(Some("completed")), BgState::Completed);
    assert_eq!(BgState::from_status(Some(" ")), BgState::Completed);
    assert_eq!(BgState::from_status(None), BgState::Completed);
    assert_eq!(BgState::Blocked.slug(), "blocked");
    assert_eq!(BgState::Other.slug(), "other");
    for (st, slug) in [
        (BgState::Open, "open"),
        (BgState::Completed, "completed"),
        (BgState::Failed, "failed"),
        (BgState::Killed, "killed"),
        (BgState::Stopped, "stopped"),
        (BgState::TimedOut, "timed-out"),
    ] {
        assert_eq!(st.slug(), slug);
    }
    assert_eq!(BgKind::Shell.slug(), "shell");
    assert_eq!(BgKind::Agent.slug(), "agent");
    assert_eq!(BgKind::Monitor.slug(), "monitor");
    assert_eq!(
        after_marker(
            "Monitor started (task b7m7m7m7m, timeout 300000ms). You will",
            "(task "
        )
        .as_deref(),
        Some("b7m7m7m7m")
    );
    assert_eq!(
        after_marker("Monitor started (task b7m7m7m7m)", "(task ").as_deref(),
        Some("b7m7m7m7m")
    );
}

#[test]
fn a_monitor_is_open_through_event_pulses_until_it_ends_or_times_out() {
    let t = TempSession::new(&lines(&[ARM, ARM_RESULT, EOT]), None);
    let r = report(&t, &BackgroundLens::default());
    assert_eq!(r.tasks.len(), 1);
    let m = &r.tasks[0];
    assert_eq!(m.kind, BgKind::Monitor);
    assert_eq!(m.id.as_deref(), Some("b7m7m7m7m"));
    assert_eq!(m.description.as_deref(), Some("Watch the build"));
    assert_eq!(m.command.as_deref(), Some("tail -f build.log"));
    assert!(m.is_open());
    // An event pulse (no <status>) keeps it armed.
    let pulse = r#"{"type":"user","uuid":"p1","timestamp":"2026-06-07T05:03:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>b7m7m7m7m</task-id>\n<tool-use-id>t7</tool-use-id>\n<event>Compiling csift v0.10.0</event>\n<summary>Monitor event: \"Watch the build\"</summary>\n</task-notification>"}}"#;
    let t = TempSession::new(&lines(&[ARM, ARM_RESULT, EOT, pulse]), None);
    assert!(report(&t, &BackgroundLens::default()).tasks[0].is_open());
    // The termination notice closes it.
    let ended = r#"{"type":"user","uuid":"p2","timestamp":"2026-06-07T05:04:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>b7m7m7m7m</task-id>\n<tool-use-id>t7</tool-use-id>\n<output-file>/nonexistent/b7m7m7m7m.output</output-file>\n<status>completed</status>\n<summary>Monitor \"Watch the build\" ended: command exited</summary>\n</task-notification>"}}"#;
    let t = TempSession::new(&lines(&[ARM, ARM_RESULT, EOT, pulse, ended]), None);
    assert_eq!(
        report(&t, &BackgroundLens::default()).tasks[0].state,
        BgState::Completed
    );
    // A timeout event closes it as timed-out, counted in the summary line.
    let timeout = r#"{"type":"user","uuid":"p3","timestamp":"2026-06-07T05:07:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>b7m7m7m7m</task-id>\n<tool-use-id>t7</tool-use-id>\n<event>[Monitor timed out - re-arm if needed.]</event>\n<summary>Monitor event: \"Watch the build\"</summary>\n</task-notification>"}}"#;
    let t = TempSession::new(&lines(&[ARM, ARM_RESULT, EOT, pulse, timeout]), None);
    let r = report(&t, &BackgroundLens::default());
    assert_eq!(r.tasks[0].state, BgState::TimedOut);
    assert_eq!(r.closed_counts(), (0, 0, 0, 0, 1));
    assert!(
        r.summary_line().ends_with("0 stopped, 1 timed out"),
        "{}",
        r.summary_line()
    );
    // A websocket monitor names its url as the command; a persistent one is the lens's job.
    let ws = r#"{"type":"assistant","uuid":"m3","timestamp":"2026-06-07T05:08:00.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t8","name":"Monitor","input":{"ws":{"url":"wss://relay.example/feed"},"description":"Relay feed","persistent":true}}]}}"#;
    let ws_result = r#"{"type":"user","uuid":"m4","timestamp":"2026-06-07T05:08:01.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t8","content":"Monitor started (task s8s8s8s8s, persistent - runs until TaskStop or session end)."}]},"toolUseResult":{"taskId":"s8s8s8s8s","timeoutMs":0,"persistent":true}}"#;
    let t = TempSession::new(&lines(&[ws, ws_result, EOT]), None);
    let lens = BackgroundLens::from_args(None, &["relay\\.example".to_string()]).unwrap();
    let r = report(&t, &lens);
    assert_eq!(r.tasks[0].kind, BgKind::Monitor);
    assert_eq!(
        r.tasks[0].command.as_deref(),
        Some("wss://relay.example/feed")
    );
    assert_eq!(r.tasks[0].id.as_deref(), Some("s8s8s8s8s"));
    assert_eq!(r.open_ignored(), 1);
}

#[test]
fn ctrl_b_mints_a_task_from_its_receipt_and_the_second_pass_recovers_the_launch() {
    let t = TempSession::new(&lines(&[FG_LAUNCH, CTRLB_RESULT, EOT]), None);
    let r = report(&t, &BackgroundLens::default());
    assert_eq!(r.tasks.len(), 1, "{:?}", r.tasks);
    let task = &r.tasks[0];
    assert_eq!(task.kind, BgKind::Shell);
    assert_eq!(task.entered_by, Some(BgEntrance::User));
    assert_eq!(task.id.as_deref(), Some("bzz111111"));
    assert_eq!(task.tool_use_id, "t5");
    assert_eq!(
        task.output_file.as_deref(),
        Some("/nonexistent/bzz111111.output"),
        "every arm of the template carries the output path"
    );
    assert_eq!(task.timed_out_after_ms, None);
    assert!(task.is_open());
    // The second pass reached the launch line, which carries no background needle.
    assert_eq!(
        task.launched_utc.as_deref(),
        Some("2026-06-07T05:10:00.000Z")
    );
    assert_eq!(
        task.command.as_deref(),
        Some("perl -e 'select(undef,undef,undef,150)'")
    );
    assert_eq!(
        task.description.as_deref(),
        Some("Wait then print the marker")
    );
    assert_eq!(task.launch_note, None);
    assert_eq!(r.open_counted(), 1);

    // The completion pulse is the ORDINARY carrier - the entrance is invisible there,
    // which is why the receipt is the only discriminator - and it closes the row.
    let done = r#"{"type":"user","uuid":"g3","timestamp":"2026-06-07T05:12:30.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>bzz111111</task-id>\n<tool-use-id>t5</tool-use-id>\n<output-file>/nonexistent/bzz111111.output</output-file>\n<status>completed</status>\n<summary>Background command \"Wait then print the marker\" completed (exit code 0)</summary>\n</task-notification>"}}"#;
    let t = TempSession::new(&lines(&[FG_LAUNCH, CTRLB_RESULT, EOT, done]), None);
    let r = report(&t, &BackgroundLens::default());
    assert_eq!(r.tasks[0].state, BgState::Completed);
    assert_eq!(r.tasks[0].entered_by, Some(BgEntrance::User));
    assert_eq!(r.open_counted(), 0);
}

#[test]
fn a_receipt_whose_launch_line_is_gone_keeps_the_receipt_instant_and_says_so() {
    // No launch line at all (torn, or externalised): the row is still real, but its
    // launch instant is unknown and is disclosed rather than fabricated.
    let t = TempSession::new(&lines(&[CTRLB_RESULT, EOT]), None);
    let r = report(&t, &BackgroundLens::default());
    assert_eq!(r.tasks.len(), 1);
    let task = &r.tasks[0];
    assert_eq!(task.entered_by, Some(BgEntrance::User));
    assert_eq!(task.command, None);
    assert_eq!(task.description, None);
    assert_eq!(
        task.launched_utc.as_deref(),
        Some("2026-06-07T05:10:06.000Z")
    );
    assert_eq!(
        task.launch_note.as_deref(),
        Some("launched-at unknown; receipt at 2026-06-07T05:10:06")
    );
}

#[test]
fn the_timeout_and_message_delivery_entrances_are_their_own_labels() {
    let t = TempSession::new(&lines(&[TO_LAUNCH, TO_RESULT, EOT]), None);
    let r = report(&t, &BackgroundLens::default());
    assert_eq!(r.tasks.len(), 1);
    let task = &r.tasks[0];
    assert_eq!(task.entered_by, Some(BgEntrance::Timeout));
    assert_eq!(task.id.as_deref(), Some("btt222222"));
    assert_eq!(task.timed_out_after_ms, Some(120_000));
    assert_eq!(task.command.as_deref(), Some("cargo test --all"));
    assert_eq!(
        task.launched_utc.as_deref(),
        Some("2026-06-07T05:12:00.000Z")
    );
    assert!(task.is_open());

    let t = TempSession::new(&lines(&[DM_RESULT, EOT]), None);
    let r = report(&t, &BackgroundLens::default());
    assert_eq!(r.tasks.len(), 1);
    assert_eq!(r.tasks[0].entered_by, Some(BgEntrance::DeliverMessage));
    assert_eq!(r.tasks[0].id.as_deref(), Some("bdd333333"));
    assert_eq!(r.tasks[0].timed_out_after_ms, None);

    // Both arms of the ordinary launch keep their own answer.
    let t = TempSession::new(&lines(&[LAUNCH, LAUNCH_RESULT, ARM, ARM_RESULT, EOT]), None);
    let r = report(&t, &BackgroundLens::default());
    let entrances: Vec<_> = r.tasks.iter().map(|t| (t.kind, t.entered_by)).collect();
    assert!(
        entrances.contains(&(BgKind::Shell, Some(BgEntrance::Model))),
        "{entrances:?}"
    );
    assert!(
        entrances.contains(&(BgKind::Monitor, None)),
        "{entrances:?}"
    );
    for (slug, e) in [
        ("model", BgEntrance::Model),
        ("user", BgEntrance::User),
        ("timeout", BgEntrance::Timeout),
        ("deliver-message", BgEntrance::DeliverMessage),
    ] {
        assert_eq!(e.slug(), slug);
    }
    assert_eq!(BgEntrance::Model.label(), None);
    assert_eq!(BgEntrance::User.label(), Some("entered by ctrl+b"));
}

#[test]
fn a_flagged_launch_is_never_minted_twice_and_a_quoted_template_mints_nothing() {
    // Pathological but the guard that matters: a row that already exists from a flagged
    // launch is UPDATED by its receipt, never duplicated, and the launch stays the
    // authority on how it got there.
    let manual_for_t1 = r#"{"type":"user","uuid":"r9","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"Command was manually backgrounded by user with ID: b1a2b3c4d. Output is being written to: /nonexistent/b1a2b3c4d.output."}]}}"#;
    let t = TempSession::new(&lines(&[LAUNCH, manual_for_t1, EOT]), None);
    let r = report(&t, &BackgroundLens::default());
    assert_eq!(r.tasks.len(), 1, "{:?}", r.tasks);
    assert_eq!(r.tasks[0].entered_by, Some(BgEntrance::Model));
    assert_eq!(r.tasks[0].id.as_deref(), Some("b1a2b3c4d"));
    assert_eq!(r.tasks[0].launch_note, None);

    // A transcript that RENDERS the template mints nothing - minting from one would be
    // this fix's own mirror image. Three shapes, all real: the binary's own source with
    // its `${e}`/`${n}` interpolations, a documentation `<id>` placeholder, and the
    // truncated quote a grep line leaves behind.
    for rendered in [
        "Command was manually backgrounded by user with ID: ${e}. Output is being written to: ${n}.",
        "Command was moved to the background (ID: ${e}) so that a message that arrived while it was running can reach you; it was not interrupted.",
        "Command did not complete within its ${Math.max(1,Math.round(d/1000))}s timeout and was moved to the background (ID: ${e}). Output is being written to: ${n}.",
        "Command was manually backgrounded by user with ID: <id>. Output is being written to: <path>.",
        "Command was moved to the background (ID: <id>) so that a message that arrived ...",
        "Command did not complete within its 120s timeout and was moved to the background (ID: <id>).",
        "Command was manually backgrounded by user with ID: \nnext line",
        "Command was moved to the background (ID: \ns timeout and was moved",
        "Command did not complete within its \ns timeout and was moved to the background (ID: x",
    ] {
        assert_eq!(receipt_entrance(rendered), None, "{rendered}");
    }
    // Each arm's own closing clause is required, and so is the id grammar (BG-009:
    // `b` + 8 base36). A well-formed id with the wrong tail, or the right tail with a
    // malformed id, is not a receipt.
    for near_miss in [
        // the manual arm without its output clause
        "Command was manually backgrounded by user with ID: bzz111111.",
        // the manual arm whose path names a DIFFERENT task's file
        "Command was manually backgrounded by user with ID: bzz111111. Output is being written to: /t/tasks/bqq999999.output.",
        // the deliver arm without its message clause
        "Command was moved to the background (ID: bdd333333). Output is being written to: /t/x.output.",
        // an id that is not the local_bash grammar
        "Command was manually backgrounded by user with ID: b7. Output is being written to: /t/b7.output.",
        "Command did not complete within its 120s timeout and was moved to the background (ID: TASKID)",
        // the ordinary launch ack is not an entrance receipt at all
        "Command running in background with ID: b1a2b3c4d. Output is being written to: /t/b1a2b3c4d.output.",
    ] {
        assert_eq!(receipt_entrance(near_miss), None, "{near_miss}");
    }
    assert_eq!(
        receipt_entrance(
            "  Command was manually backgrounded by user with ID: bzz111111. Output is \
             being written to: /t/tasks/bzz111111.output. "
        ),
        Some((BgEntrance::User, "bzz111111".to_string())),
        "the text is matched left-trimmed"
    );
}

#[test]
fn foreground_tools_are_never_launches_and_a_real_output_file_stats() {
    // A foreground Bash (no run_in_background) and a Read are not launches.
    let fg = r#"{"type":"assistant","uuid":"f1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"f1","name":"Bash","input":{"command":"grep run_in_background notes.md","description":"List"}},{"type":"tool_use","id":"f2","name":"Read","input":{"file_path":"/x"}}]}}"#;
    let fg_res = r#"{"type":"user","uuid":"f2","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"f1","content":"a b"},{"type":"tool_result","tool_use_id":"f2","content":"x"}]}}"#;
    let t = TempSession::new(&lines(&[fg, fg_res, EOT]), None);
    assert!(report(&t, &BackgroundLens::default()).tasks.is_empty());
    // A background shell whose output file EXISTS reports its size and last write.
    let t = TempSession::new("", None);
    let out_path = t.root.join("b1a2b3c4d.output");
    std::fs::write(&out_path, "twelve bytes").unwrap();
    // The path is embedded in a JSON string: JSON-escape it (Windows backslashes).
    let escaped = serde_json::to_string(out_path.to_str().unwrap()).unwrap();
    let result = LAUNCH_RESULT.replace("/nonexistent/b1a2b3c4d.output", escaped.trim_matches('"'));
    std::fs::write(&t.main, lines(&[LAUNCH, &result, EOT])).unwrap();
    let r = report(&t, &BackgroundLens::default());
    assert_eq!(r.tasks[0].output_bytes, Some(12));
    assert!(r.tasks[0].output_age_secs.is_some_and(|a| a < 60));
}
