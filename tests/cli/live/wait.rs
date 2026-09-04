//! wait end to end: baseline law, conditions, discovery, scoping, exit 124.

use crate::harness::*;
use std::io::Write as _;

#[test]
fn p5_wait_fires_only_on_post_start_events() {
    let h = Home::new();
    let main = live_eot_main(&h);
    let target = at(LIVE_SESS);
    let (code, stdout) = drive_wait(
        &h,
        &[
            "wait",
            &target,
            "--until",
            "tool:Read:handover",
            "--interval",
            "25",
            "--timeout",
            "30",
        ],
        || {
            let mut f = std::fs::File::options().append(true).open(&main).unwrap();
            writeln!(
                f,
                r#"{{"type":"assistant","uuid":"a8","timestamp":"2026-06-07T05:00:59.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"about to read"}}]}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"assistant","uuid":"a9","timestamp":"2026-06-07T05:01:00.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t9","name":"Read","input":{{"file_path":"/p/handover.md"}}}}]}}}}"#
            )
            .unwrap();
        },
    );
    assert_eq!(code, Some(0), "condition fired cleanly: {stdout}");
    assert!(
        stdout.contains("fired    tool:Read:handover"),
        "names the condition: {stdout}"
    );
}

#[test]
fn p6_history_never_fires_and_timeout_is_124() {
    let h = Home::new();
    let main = live_eot_main(&h);
    // The trigger is appended BEFORE wait starts: history, not an event.
    let mut f = std::fs::File::options().append(true).open(&main).unwrap();
    writeln!(
        f,
        r#"{{"type":"assistant","uuid":"a8","timestamp":"2026-06-07T05:00:30.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t8","name":"Read","input":{{"file_path":"/p/handover.md"}}}}]}}}}"#
    )
    .unwrap();
    drop(f);
    let target = at(LIVE_SESS);
    let (code, stdout) = drive_wait(
        &h,
        &[
            "wait",
            &target,
            "--until",
            "tool:Read:handover",
            "--interval",
            "25",
            "--timeout",
            "2",
            "--format",
            "json",
        ],
        || {},
    );
    assert_eq!(code, Some(124), "timeout exits 124: {stdout}");
    let obj: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(obj["fired"], "timeout", "{stdout}");
}

#[test]
fn p8_wait_verdict_condition_and_subagent_target_and_exactly_one() {
    let h = Home::new();
    live_eot_main(&h);
    h.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}/subagents/agent-c1d2e3f4a5b60718.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"su1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"child task"}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa2","parentUuid":"su1","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"child done"}]}}"#, "\n",
        ),
    );
    // A verdict-class condition on a SUBAGENT target: fires on the first assessment,
    // and the output carries the lane-honesty registry note.
    let sub_target = "@c1d2e3f4a5b60718".to_string();
    let (code, stdout) = drive_wait(
        &h,
        &[
            "wait",
            &sub_target,
            "--until",
            "stop",
            "--interval",
            "25",
            "--timeout",
            "30",
        ],
        || {},
    );
    assert_eq!(code, Some(0), "verdict condition fired: {stdout}");
    assert!(
        stdout.contains("fired    stop") && stdout.contains("verdict  idle-eot"),
        "{stdout}"
    );

    // status on the subagent target says the registry cannot cover it.
    let s = h.run(&["status", &sub_target]);
    assert!(s.success, "stderr: {}", s.stderr);
    assert!(
        s.stdout.contains("top-level interactive sessions only"),
        "honest degradation note: {}",
        s.stdout
    );

    // Both commands watch exactly ONE session per call.
    let t = at(LIVE_SESS);
    let two_status = h.run(&["status", &t, &sub_target]);
    assert!(!two_status.success);
    assert!(
        two_status.stderr.contains("exactly ONE session"),
        "{}",
        two_status.stderr
    );
    let two_wait = h.run(&["wait", &t, &sub_target, "--until", "stop"]);
    assert!(!two_wait.success);
    assert!(
        two_wait.stderr.contains("exactly ONE session"),
        "{}",
        two_wait.stderr
    );
}

#[test]
fn p9_wait_discovers_children_spawned_mid_wait() {
    // No --interval: the adaptive cadence drives the poll. The child transcript does
    // not exist when the watch starts - discovery must pick it up with a zero baseline
    // (its whole content is post-start).
    let h = Home::new();
    live_eot_main(&h);
    let target = at(LIVE_SESS);
    let child_rel = format!("{LIVE_ENC}/{LIVE_SESS}/subagents/agent-9f8e7d6c5b4a3210.jsonl");
    let (code, stdout) = drive_wait(
        &h,
        &[
            "wait",
            &target,
            "--until",
            "write:handoff\\.md:READY",
            "--timeout",
            "20",
        ],
        || {
            h.write(
                &child_rel,
                concat!(
                    r#"{"type":"assistant","uuid":"wa1","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"wt1","name":"Write","input":{"file_path":"/p/handoff.md","content":"READY for pickup\n"}}]}}"#, "\n",
                ),
            );
        },
    );
    assert_eq!(code, Some(0), "child-lane write fired: {stdout}");
    assert!(
        stdout.contains("fired    write:handoff\\.md:READY"),
        "{stdout}"
    );
}

#[test]
fn p10_wait_sees_a_sidecar_ask_that_lands_mid_wait() {
    let h = Home::new();
    live_eot_main(&h);
    let target = at(LIVE_SESS);
    let sidecar_rel = format!("{LIVE_ENC}/{LIVE_SESS}/elicitations.jsonl");
    let (code, stdout) = drive_wait(
        &h,
        &[
            "wait",
            &target,
            "--until",
            "auq",
            "--interval",
            "25",
            "--timeout",
            "20",
        ],
        || {
            h.write(
                &sidecar_rel,
                concat!(
                    r#"{"csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"toolu_mid1","type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_mid1","name":"AskUserQuestion","input":{"questions":[{"question":"proceed?"}]}}]}}"#, "\n",
                ),
            );
        },
    );
    assert_eq!(code, Some(0), "sidecar ask fired: {stdout}");
    assert!(stdout.contains("fired    auq"), "{stdout}");
}

#[test]
fn p11_history_in_every_lane_never_fires() {
    // Pre-existing child transcript AND sidecar, both carrying would-match events:
    // they are HISTORY, and no lane's history may fire a wait.
    let h = Home::new();
    live_eot_main(&h);
    h.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}/subagents/agent-3c4d5e6f70819a2b.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"ha1","timestamp":"2026-06-07T04:59:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ht1","name":"Read","input":{"file_path":"/p/handover.md"}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}/elicitations.jsonl"),
        concat!(
            r#"{"csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"toolu_old1","type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_old1","name":"AskUserQuestion","input":{"questions":[{"question":"old ask?"}]}}]}}"#, "\n",
        ),
    );
    let target = at(LIVE_SESS);
    let (code, stdout) = drive_wait(
        &h,
        &[
            "wait",
            &target,
            "--until",
            "auq",
            "--until",
            "tool:Read:handover",
            "--interval",
            "25",
            "--timeout",
            "2",
        ],
        || {},
    );
    assert_eq!(code, Some(124), "history never fires: {stdout}");
}

#[test]
fn p12_no_subagents_scopes_the_watch_and_the_verdict() {
    let h = Home::new();
    live_eot_main(&h);
    // An in-flight child exists, but --no-subagents must not consult it.
    h.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}/subagents/agent-5e6f70819a2b3c4d.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"na1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"nt1","name":"Bash","input":{"command":"work"}}]}}"#, "\n",
        ),
    );
    let s = h.run(&["status", &at(LIVE_SESS), "--no-subagents"]);
    assert!(s.success, "stderr: {}", s.stderr);
    assert!(
        s.stdout.contains("verdict  idle-eot") && !s.stdout.contains("in-flight"),
        "child lanes out of scope:\n{}",
        s.stdout
    );

    // A child EXISTING at start must not even be baselined under --no-subagents: an
    // event appended to it mid-wait stays invisible.
    let target = at(LIVE_SESS);
    let pre_rel = format!("{LIVE_ENC}/{LIVE_SESS}/subagents/agent-5e6f70819a2b3c4d.jsonl");
    let pre_abs = h.projects().join(&pre_rel);
    let (code_pre, stdout_pre) = drive_wait(
        &h,
        &[
            "wait",
            &target,
            "--no-subagents",
            "--until",
            "tool:Grep",
            "--interval",
            "25",
            "--timeout",
            "2",
        ],
        || {
            let mut f = std::fs::File::options()
                .append(true)
                .open(&pre_abs)
                .unwrap();
            writeln!(
                f,
                r#"{{"type":"assistant","uuid":"pa1","timestamp":"2026-06-07T05:04:00.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"pt1","name":"Grep","input":{{"pattern":"x"}}}}]}}}}"#
            )
            .unwrap();
        },
    );
    assert_eq!(
        code_pre,
        Some(124),
        "a pre-existing child lane is out of watch scope: {stdout_pre}"
    );

    // A child born mid-wait with a matching event must NOT fire under --no-subagents.
    let born_rel = format!("{LIVE_ENC}/{LIVE_SESS}/subagents/agent-70819a2b3c4d5e6f.jsonl");
    let (code, stdout) = drive_wait(
        &h,
        &[
            "wait",
            &target,
            "--no-subagents",
            "--until",
            "write:handoff\\.md",
            "--interval",
            "25",
            "--timeout",
            "2",
        ],
        || {
            h.write(
                &born_rel,
                concat!(
                    r#"{"type":"assistant","uuid":"ba1","timestamp":"2026-06-07T05:03:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"bt1","name":"Write","input":{"file_path":"/p/handoff.md","content":"x"}}]}}"#, "\n",
                ),
            );
        },
    );
    assert_eq!(code, Some(124), "child lanes out of watch scope: {stdout}");
}

#[test]
fn p12_notification_fires_on_a_queue_enqueue_line_not_on_its_remove() {
    // A pulse absorbed mid-turn never becomes a user record: it lives on a
    // queue-operation ENQUEUE line (and a queued_command attachment). `--until
    // notification` fires on that carrier (v0.10.2); the later REMOVE line for the
    // same pulse is not a second delivery.
    let h = Home::new();
    let main = live_eot_main(&h);
    let target = at(LIVE_SESS);
    let pulse = r#"<task-notification>\n<task-id>b7</task-id>\n<status>completed</status>\n<summary>Background command finished</summary>\n</task-notification>"#;
    let (code, stdout) = drive_wait(
        &h,
        &[
            "wait",
            &target,
            "--until",
            "notification",
            "--interval",
            "25",
            "--timeout",
            "2",
        ],
        || {
            let mut f = std::fs::File::options().append(true).open(&main).unwrap();
            writeln!(
                f,
                r#"{{"type":"queue-operation","operation":"remove","reason":"absorbed_mid_turn","timestamp":"2026-06-07T05:05:00.000Z","sessionId":"{LIVE_SESS}","content":"{pulse}"}}"#
            )
            .unwrap();
        },
    );
    assert_eq!(code, Some(124), "a remove line alone never fires: {stdout}");
    let (code, stdout) = drive_wait(
        &h,
        &[
            "wait",
            &target,
            "--until",
            // A regex only the RAW carrier text satisfies (the rendered label carries no
            // XML tag): the regex is tried against the carrier text too, not only
            // against the fabricated label.
            "notification:<status>completed</status>",
            "--interval",
            "25",
            "--timeout",
            "30",
        ],
        || {
            let mut f = std::fs::File::options().append(true).open(&main).unwrap();
            writeln!(
                f,
                r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"2026-06-07T05:06:00.000Z","sessionId":"{LIVE_SESS}","content":"{pulse}"}}"#
            )
            .unwrap();
        },
    );
    assert_eq!(code, Some(0), "the enqueue carrier fires: {stdout}");
    assert!(
        stdout.contains("notifications 1"),
        "the activity census counts the queue-carried pulse once:\n{stdout}"
    );
}

#[test]
fn p17_notification_delivered_in_a_child_lane_fires() {
    // The harness normally delivers a pulse to the main transcript, but one addressed to
    // the owning agent lands in that agent's lane (2 of 2906 delivered records in the
    // reference corpus at Claude Code 2.1.258). `--until notification` watches every
    // lane (v0.10.3); a child born mid-wait starts at baseline 0.
    let h = Home::new();
    let _main = live_eot_main(&h);
    let target = at(LIVE_SESS);
    let child = h
        .root
        .join(".claude/projects")
        .join(LIVE_ENC)
        .join(LIVE_SESS)
        .join("subagents")
        .join("agent-a1b2c3d4e5f6a7b8c.jsonl");
    let (code, stdout) = drive_wait(
        &h,
        &[
            "wait",
            &target,
            "--until",
            "notification:child-only job",
            "--interval",
            "25",
            "--timeout",
            "30",
        ],
        || {
            std::fs::create_dir_all(child.parent().unwrap()).unwrap();
            std::fs::write(
                &child,
                format!(
                    "{}\n",
                    r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:07:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>b9</task-id>\n<status>completed</status>\n<summary>Background command \"child-only job\" completed (exit code 0)</summary>\n</task-notification>"}}"#
                ),
            )
            .unwrap();
        },
    );
    assert_eq!(code, Some(0), "a child-lane delivery fires: {stdout}");
    assert!(stdout.contains("fired"), "{stdout}");
}

#[test]
fn p18_a_transcript_that_shrinks_mid_wait_is_reported_and_keeps_being_watched() {
    // Claude Code rewrites a transcript in place (a rewind tombstone truncates and
    // rewrites the tail). A memory map of the file would fault with SIGBUS on the
    // first page touched past the new end; `wait` reads its increments with plain
    // positional reads instead, moves the baseline to the new end, says so, and still
    // fires on what lands afterwards (v0.10.4).
    let h = Home::new();
    let main = live_eot_main(&h);
    let target = at(LIVE_SESS);
    let before = std::fs::metadata(&main).unwrap().len();
    let pulse = r#"<task-notification>\n<task-id>b8</task-id>\n<status>completed</status>\n<summary>Background command \"after the rewrite\" completed (exit code 0)</summary>\n</task-notification>"#;
    let (code, stdout) = drive_wait(
        &h,
        &[
            "wait",
            &target,
            "--until",
            "notification:after the rewrite",
            "--interval",
            "25",
            "--timeout",
            "30",
        ],
        || {
            // the in-place rewrite: cut the file below the baseline, then append
            let f = std::fs::File::options().write(true).open(&main).unwrap();
            f.set_len(before / 2).unwrap();
            drop(f);
            std::thread::sleep(std::time::Duration::from_millis(200));
            let mut f = std::fs::File::options().append(true).open(&main).unwrap();
            writeln!(
                f,
                r#"{{"type":"user","uuid":"n9","timestamp":"2026-06-07T05:09:00.000Z","message":{{"role":"user","content":"{pulse}"}}}}"#
            )
            .unwrap();
        },
    );
    assert_eq!(
        code,
        Some(0),
        "the pulse appended after the shrink fires: {stdout}"
    );
    assert!(
        stdout.contains("transcript shrank 1 time(s)") && stdout.contains("baseline moved"),
        "the shrink is disclosed in the activity line:\n{stdout}"
    );
}
