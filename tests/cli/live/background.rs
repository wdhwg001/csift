//! Background tasks in status/wait (v0.10.0): the seventh verdict, the lens flags, the
//! required timeout, the at-exit activity report, the last-message excerpts, and the
//! harness's agents-stopped notice.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "9f8e7d6c-5b4a-4392-8170-fedcba098765";

const LAUNCH: &str = r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"npm run dev","description":"Serve the harbor app","run_in_background":true}}]}}"#;
const LAUNCH_RESULT: &str = r#"{"type":"user","uuid":"r1","parentUuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"Command running in background with ID: b1a2b3c4d. Output is being written to: /nonexistent/b1a2b3c4d.output. You will be notified when it completes."}]},"toolUseResult":{"stdout":"","stderr":"","interrupted":false,"backgroundTaskId":"b1a2b3c4d"}}"#;
const PROMPT: &str = r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"serve the harbor app in the background"}}"#;
const EOT: &str = r#"{"type":"assistant","uuid":"a2","parentUuid":"r1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"serving; it keeps running"}]}}"#;

/// A settled main lane with one background shell that never returned.
fn open_shell_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{PROMPT}\n{LAUNCH}\n{LAUNCH_RESULT}\n{EOT}\n"),
    );
    h
}

#[test]
fn an_open_background_shell_is_the_seventh_verdict_with_its_row() {
    let h = open_shell_home();
    let out = h.run(&["status", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("verdict  idle-background-open"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("background 1 open; 0 completed, 0 failed, 0 killed, 0 stopped"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("bg        shell   b1a2b3c4d")
            && out.stdout.contains("launched 2026-06-07")
            && out.stdout.contains("\"Serve the harbor app\""),
        "the open task row names kind, id, launch instant and description:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("  lane "),
        "a main-lane launch names no lane (the session is the row's context):\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("have not returned") && out.stdout.contains("csift cannot tell"),
        "the honesty note:\n{}",
        out.stdout
    );
    // The last-message section + its warning.
    assert!(
        out.stdout.contains("last ◂")
            && out
                .stdout
                .contains("serve the harbor app in the background"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("last ▸") && out.stdout.contains("serving; it keeps running"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("never a review of the work")
            && out
                .stdout
                .contains(&format!("csift show @{SESS} --turn -1")),
        "{}",
        out.stdout
    );
    // JSON: verdict, the background object, the last object, the tail state.
    let j = h.run(&["status", &at(SESS), "--format", "json"]);
    let row: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "verdict")
        .expect("verdict row");
    assert_eq!(row["verdict"], "idle-background-open", "{}", j.stdout);
    assert_eq!(row["background"]["open"], 1, "{}", j.stdout);
    assert_eq!(row["background"]["ignored"], 0, "{}", j.stdout);
    let task = &row["background"]["tasks"][0];
    assert_eq!(task["kind"], "shell");
    assert_eq!(task["id"], "b1a2b3c4d");
    assert_eq!(task["tool_use_id"], "t1");
    assert_eq!(task["state"], "open");
    assert_eq!(task["command"], "npm run dev");
    assert_eq!(task["launched_utc"], "2026-06-07T05:00:01.000Z");
    assert!(task["ignored_by"].is_null());
    assert!(task["output_bytes"].is_null(), "no such output file");
    assert_eq!(row["last"]["agent"]["text"], "serving; it keeps running");
    assert_eq!(row["last"]["user"]["truncated"], false);
    assert!(
        row["tail_state"]
            .as_str()
            .unwrap()
            .starts_with("idle (last stop_reason end_turn"),
        "{}",
        j.stdout
    );
}

#[test]
fn the_lens_turns_the_same_session_into_a_clean_stop() {
    let h = open_shell_home();
    // By pattern: the dev server is known never to return.
    let out = h.run(&["status", &at(SESS), "--ignore-background", "npm run dev"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("verdict  idle-eot"), "{}", out.stdout);
    assert!(
        out.stdout.contains("0 open (+1 ignored by the lens)")
            && out
                .stdout
                .contains("[ignored: matches --ignore-background npm run dev]"),
        "the ignored task is still listed, marked with its rule:\n{}",
        out.stdout
    );
    // By time: everything already dangling when this command started.
    let out = h.run(&["status", &at(SESS), "--background-since", "now"]);
    assert!(out.stdout.contains("verdict  idle-eot"), "{}", out.stdout);
    assert!(
        out.stdout
            .contains("[ignored: launched before --background-since now]"),
        "{}",
        out.stdout
    );
    // A cutoff older than the launch keeps it counted.
    let out = h.run(&["status", &at(SESS), "--background-since", "2026-01-01"]);
    assert!(
        out.stdout.contains("verdict  idle-background-open"),
        "{}",
        out.stdout
    );
    // Bad lens inputs fail loud.
    let bad = h.run(&["status", &at(SESS), "--background-since", "someday"]);
    assert!(!bad.success);
    assert!(bad.stderr.contains("--background-since"), "{}", bad.stderr);
    let bad = h.run(&["status", &at(SESS), "--ignore-background", "("]);
    assert!(!bad.success);
    assert!(bad.stderr.contains("--ignore-background"), "{}", bad.stderr);
    // JSON echoes the rule.
    let j = h.run(&[
        "status",
        &at(SESS),
        "--ignore-background",
        "npm",
        "--format",
        "json",
    ]);
    let row: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "verdict")
        .unwrap();
    assert_eq!(row["verdict"], "idle-eot");
    assert_eq!(row["background"]["ignored"], 1);
    assert_eq!(
        row["background"]["tasks"][0]["ignored_by"],
        "matches --ignore-background npm"
    );
}

#[test]
fn a_returned_shell_folds_into_the_counts() {
    let h = Home::new();
    let done = r#"{"type":"user","uuid":"n1","parentUuid":"a2","timestamp":"2026-06-07T05:03:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>b1a2b3c4d</task-id>\n<tool-use-id>t1</tool-use-id>\n<status>completed</status>\n<summary>Background command \"Serve the harbor app\" completed</summary>\n</task-notification>"}}"#;
    let reply = r#"{"type":"assistant","uuid":"a3","parentUuid":"n1","timestamp":"2026-06-07T05:03:05.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"noted"}]}}"#;
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{PROMPT}\n{LAUNCH}\n{LAUNCH_RESULT}\n{EOT}\n{done}\n{reply}\n"),
    );
    let out = h.run(&["status", &at(SESS)]);
    assert!(out.stdout.contains("verdict  idle-eot"), "{}", out.stdout);
    assert!(
        out.stdout
            .contains("background 0 open; 1 completed, 0 failed, 0 killed, 0 stopped"),
        "{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("bg        shell"),
        "closed tasks fold into the counts, no row:\n{}",
        out.stdout
    );
}

#[test]
fn wait_demands_a_timeout_and_reports_what_it_saw() {
    let h = open_shell_home();
    // No --timeout: rejected with the reason.
    let out = h.run(&["wait", &at(SESS), "--until", "stop"]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("wait needs --timeout")
            && out.stderr.contains("never to return")
            && out.stderr.contains("--background-since now"),
        "{}",
        out.stderr
    );
    // With one: `stop` never fires while the shell counts, so the bound elapses (124)
    // and the report says what the session was doing.
    let out = h.run(&[
        "wait",
        &at(SESS),
        "--until",
        "stop",
        "--timeout",
        "1",
        "--interval",
        "50",
    ]);
    assert_eq!(
        out.code,
        Some(124),
        "stdout:\n{}\nstderr:\n{}",
        out.stdout,
        out.stderr
    );
    assert!(out.stdout.contains("fired    timeout"), "{}", out.stdout);
    assert!(
        out.stdout.contains("verdict  idle-background-open"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("at exit  idle (last stop_reason end_turn"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("activity nothing landed after the baseline"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("bg        shell   b1a2b3c4d") && out.stdout.contains("last ▸"),
        "{}",
        out.stdout
    );
    assert!(
        out.stderr.contains("timeout 1s"),
        "the readiness line names the bound:\n{}",
        out.stderr
    );
    // The lens makes the same session a true stop: `stop` fires at once (exit 0).
    let out = h.run(&[
        "wait",
        &at(SESS),
        "--until",
        "stop",
        "--timeout",
        "5",
        "--interval",
        "50",
        "--ignore-background",
        "npm run dev",
    ]);
    assert_eq!(
        out.code,
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        out.stdout,
        out.stderr
    );
    assert!(out.stdout.contains("fired    stop"), "{}", out.stdout);
    assert!(
        out.stderr.contains("lens: since -, 1 ignore pattern(s)"),
        "{}",
        out.stderr
    );
    // The explicit verdict condition reaches the seventh verdict.
    let out = h.run(&[
        "wait",
        &at(SESS),
        "--until",
        "verdict:idle-background-open",
        "--timeout",
        "5",
        "--interval",
        "50",
    ]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert!(
        out.stdout.contains("fired    verdict:idle-background-open"),
        "{}",
        out.stdout
    );
}

#[test]
fn wait_json_carries_activity_background_and_last() {
    let h = open_shell_home();
    let main = h
        .root
        .join(".claude/projects")
        .join(ENC)
        .join(format!("{SESS}.jsonl"));
    let (code, stdout) = drive_wait(
        &h,
        &[
            "wait",
            &at(SESS),
            "--until",
            "tool:Read",
            "--timeout",
            "10",
            "--interval",
            "25",
            "--format",
            "json",
        ],
        || {
            use std::io::Write as _;
            let mut f = std::fs::File::options().append(true).open(&main).unwrap();
            f.write_all(
                concat!(
                    r#"{"type":"user","uuid":"u2","parentUuid":"a2","timestamp":"2026-06-07T05:04:00.000Z","message":{"role":"user","content":"check the dev log"}}"#, "\n",
                    r#"{"type":"assistant","uuid":"a4","parentUuid":"u2","timestamp":"2026-06-07T05:04:01.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"thinking","thinking":"look"},{"type":"tool_use","id":"t2","name":"Read","input":{"file_path":"/x/dev.log"}}]}}"#, "\n",
                )
                .as_bytes(),
            )
            .unwrap();
        },
    );
    assert_eq!(code, Some(0), "{stdout}");
    let obj: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(obj["fired"], "tool:Read");
    assert_eq!(obj["activity"]["records"], 2, "{stdout}");
    assert_eq!(obj["activity"]["tools"]["Read"], 1, "{stdout}");
    assert_eq!(obj["activity"]["thinking"], 1, "{stdout}");
    assert_eq!(obj["activity"]["user_prompts"], 1, "{stdout}");
    assert!(
        obj["at_exit"]
            .as_str()
            .unwrap()
            .starts_with("in a Read call"),
        "{stdout}"
    );
    assert_eq!(obj["background"]["open"], 1, "{stdout}");
    assert_eq!(obj["background"]["tasks"][0]["id"], "b1a2b3c4d");
    assert_eq!(obj["last"]["user"]["text"], "check the dev log");
}

#[test]
fn the_agents_stopped_notice_is_harness_not_human() {
    let h = Home::new();
    let notice = r#"{"type":"user","uuid":"k1","parentUuid":"a2","timestamp":"2026-06-07T05:06:00.000Z","message":{"role":"user","content":"2 background agents were stopped by the user: \"Census the r...\", \"Chart the s...\"."}}"#;
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{PROMPT}\n{EOT}\n{notice}\n"),
    );
    let human = h.run(&["search", "", &at(SESS), "-t", "user.message"]);
    assert!(
        human.stdout.contains("serve the harbor app")
            && !human.stdout.contains("stopped by the user"),
        "{}",
        human.stdout
    );
    let sub = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.notification.subagent",
    ]);
    assert!(
        sub.stdout
            .contains("[subagent stopped] 2 background agents were stopped by the user"),
        "{}",
        sub.stdout
    );
    // A fabricated-prefix match still hits through the whole-file gate.
    let hit = h.run(&["search", "subagent stopped", &at(SESS)]);
    assert!(
        hit.stdout.contains("matched 1 exchange"),
        "{}\n{}",
        hit.stdout,
        hit.stderr
    );
    // `list` shows it as a LABELED harness notice (like any automation pulse), never as
    // the human's own prose.
    let list = h.run(&["list", &at(SESS)]);
    assert!(
        list.stdout
            .contains("[subagent stopped] 2 background agents were stopped by the user"),
        "{}",
        list.stdout
    );
}

#[test]
fn rows_name_a_subagent_lane_and_a_live_output_file() {
    let h = Home::new();
    // The main lane is settled; the launch lives in a subagent lane, its output file exists.
    let out_path = h.root.join("b9z8y7x6w.output");
    std::fs::write(&out_path, "seven!!").unwrap();
    let sub_launch = r#"{"type":"assistant","uuid":"s1","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t9","name":"Bash","input":{"command":"cargo build","description":"Build in the child","run_in_background":true}}]}}"#;
    let sub_result = format!(
        r#"{{"type":"user","uuid":"s2","timestamp":"2026-06-07T05:01:01.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t9","content":"Command running in background with ID: b9z8y7x6w. Output is being written to: {}. You will be notified when it completes."}}]}}}}"#,
        out_path.to_str().unwrap()
    );
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{PROMPT}\n{EOT}\n"),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-abcdef0123456789.jsonl"),
        &format!("{sub_launch}\n{sub_result}\n"),
    );
    let out = h.run(&["status", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("verdict  idle-background-open"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("output 7 B, last write")
            && out.stdout.contains("lane abcdef0123456789"),
        "the row carries the output stat and the launching lane:\n{}",
        out.stdout
    );
    // --no-subagents drops the child's launch: a clean stop.
    let out = h.run(&["status", &at(SESS), "--no-subagents"]);
    assert!(out.stdout.contains("verdict  idle-eot"), "{}", out.stdout);
}

#[test]
fn the_last_section_prints_with_only_a_prompt_on_disk() {
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &format!("{PROMPT}\n"));
    let out = h.run(&["status", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("last ◂") && !out.stdout.contains("last ▸"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("never a review of the work"),
        "the warning rides even a one-sided section:\n{}",
        out.stdout
    );
}
