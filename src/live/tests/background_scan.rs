//! The background scanner: launches, the three completion carriers, the orphan
//! summary, the agents-stopped notice, async agents, monitors, and the helpers.

use super::*;

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
    assert_eq!(BgState::from_status(Some("weird")), BgState::Completed);
    assert_eq!(BgState::from_status(None), BgState::Completed);
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
