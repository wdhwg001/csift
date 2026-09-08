//! The three harness-side entrances into the background (v0.12.0): a shell the MODEL
//! ran in the foreground that ctrl+b, a timeout, or a message delivery moved aside. The
//! launching tool_use carries no `run_in_background` and is never rewritten, so the
//! receipt sentence is the whole instrument.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "7c6b5a49-3827-4160-95e4-a1b2c3d4e5f6";

const PROMPT: &str = r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"run the probe"}}"#;
/// An ORDINARY foreground Bash call: no `run_in_background`, and no background needle
/// anywhere on the line. Only the second pass can reach it.
const FG_LAUNCH: &str = r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"perl -e 'select(undef,undef,undef,150)'","description":"Wait then print the marker","timeout":200000}}]}}"#;
const CTRLB_RESULT: &str = r#"{"type":"user","uuid":"r1","parentUuid":"a1","timestamp":"2026-06-07T05:00:07.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","is_error":false,"content":"Command was manually backgrounded by user with ID: bzz111111. Output is being written to: /nonexistent/bzz111111.output."}]},"toolUseResult":{"stdout":"","stderr":"","interrupted":false,"isImage":false,"noOutputExpected":false,"backgroundTaskId":"bzz111111","backgroundedByUser":true}}"#;
const TO_LAUNCH: &str = r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t2","name":"Bash","input":{"command":"cargo test --all","description":"Run the whole suite"}}]}}"#;
const TO_RESULT: &str = r#"{"type":"user","uuid":"r1","parentUuid":"a1","timestamp":"2026-06-07T05:02:01.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"Command did not complete within its 120s timeout and was moved to the background (ID: btt222222). Output is being written to: /nonexistent/btt222222.output. You will be notified when it completes. To check interim output, use Read on that file path."}]},"toolUseResult":{"stdout":"","stderr":"","interrupted":false,"backgroundTaskId":"btt222222","timedOutAfterMs":120000}}"#;
const EOT: &str = r#"{"type":"assistant","uuid":"a2","parentUuid":"r1","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"backgrounded; carrying on"}]}}"#;

fn home_with(launch: &str, receipt: &str) -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{PROMPT}\n{launch}\n{receipt}\n{EOT}\n"),
    );
    h
}

#[test]
fn a_shell_backgrounded_by_ctrl_b_is_an_open_background_task() {
    let h = home_with(FG_LAUNCH, CTRLB_RESULT);
    let out = h.run(&["status", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("verdict  idle-background-open"),
        "a clean end_turn over an open task is the seventh verdict:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("bg        shell   bzz111111"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("entered by ctrl+b"),
        "the row says the model never asked for this:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("launched 2026-06-07")
            && out.stdout.contains("\"Wait then print the marker\""),
        "the second pass recovered the foreground launch record:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("launched-at unknown"),
        "the launch line is present, so nothing is unknown:\n{}",
        out.stdout
    );

    let json = h.run(&["status", &at(SESS), "--format", "json"]);
    assert!(json.success, "stderr: {}", json.stderr);
    assert!(
        json.stdout.contains(r#""entered_by":"user""#),
        "{}",
        json.stdout
    );
    assert!(
        json.stdout.contains(r#""timed_out_after_ms":null"#)
            && json.stdout.contains(r#""launch_note":null"#),
        "{}",
        json.stdout
    );
}

#[test]
fn a_ctrl_b_task_holds_stop_open_until_the_lens_excuses_it() {
    let h = home_with(FG_LAUNCH, CTRLB_RESULT);
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
    assert!(
        out.stdout.contains("fired    timeout")
            && out.stdout.contains("verdict  idle-background-open"),
        "{}",
        out.stdout
    );
    // The lens runs over the RECOVERED command, which only the second pass supplies.
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
        "select\\(undef",
    ]);
    assert_eq!(
        out.code,
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        out.stdout,
        out.stderr
    );
    assert!(out.stdout.contains("fired    stop"), "{}", out.stdout);
}

#[test]
fn a_timed_out_shell_names_its_timeout_and_holds_stop_open() {
    let h = home_with(TO_LAUNCH, TO_RESULT);
    let out = h.run(&["status", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("verdict  idle-background-open")
            && out.stdout.contains("bg        shell   btt222222"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("entered by timeout after 2m"),
        "the row names the entrance and the timeout it hit:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("\"Run the whole suite\""),
        "the recovered launch supplies the description:\n{}",
        out.stdout
    );

    let json = h.run(&["status", &at(SESS), "--format", "json"]);
    assert!(
        json.stdout.contains(r#""entered_by":"timeout""#)
            && json.stdout.contains(r#""timed_out_after_ms":120000"#),
        "{}",
        json.stdout
    );

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
    assert_eq!(out.code, Some(124), "stdout:\n{}", out.stdout);
}

#[test]
fn a_transcript_holding_the_template_itself_mints_no_task() {
    // The mirror image of the bug this file pins: a session that GREPPED the harness
    // binary carries the template verbatim in a tool_result. Minting from it would flip
    // a genuinely stopped session to idle-background-open and hold `--until stop` open
    // over a task that never existed.
    let dump = r#"{"type":"user","uuid":"r1","parentUuid":"a1","timestamp":"2026-06-07T05:00:07.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"Command was manually backgrounded by user with ID: ${e}. Output is being written to: ${n}."}]}}"#;
    let placeholder = r#"{"type":"user","uuid":"r2","parentUuid":"r1","timestamp":"2026-06-07T05:00:08.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"Command did not complete within its 120s timeout and was moved to the background (ID: <id>). Output is being written to: <path>."}]}}"#;
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{PROMPT}\n{FG_LAUNCH}\n{dump}\n{placeholder}\n{EOT}\n"),
    );
    let out = h.run(&["status", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("verdict  idle-eot") && !out.stdout.contains("bg        shell"),
        "a rendering of the template is not a receipt:\n{}",
        out.stdout
    );
    let out = h.run(&[
        "wait",
        &at(SESS),
        "--until",
        "stop",
        "--timeout",
        "5",
        "--interval",
        "50",
    ]);
    assert_eq!(out.code, Some(0), "stdout:\n{}", out.stdout);
    assert!(out.stdout.contains("fired    stop"), "{}", out.stdout);
}

#[test]
fn a_receipt_without_its_launch_line_discloses_the_unknown_launch_instant() {
    // The launch line is gone (torn, or its tool output was externalised): the task is
    // still real and still counted, but its launch instant is stated as unknown.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{PROMPT}\n{CTRLB_RESULT}\n{EOT}\n"),
    );
    let out = h.run(&["status", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("verdict  idle-background-open")
            && out.stdout.contains("entered by ctrl+b"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("[launched-at unknown; receipt at 2026-06-07T05:00:07]"),
        "the receipt instant is disclosed as such, never passed off as the launch:\n{}",
        out.stdout
    );
    let json = h.run(&["status", &at(SESS), "--format", "json"]);
    assert!(
        json.stdout
            .contains(r#""launch_note":"launched-at unknown; receipt at 2026-06-07T05:00:07""#),
        "{}",
        json.stdout
    );
}
