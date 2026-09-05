//! The registry reader behind `whoami --peers` and the `self` section's state line.
//!
//! Both read the same directory of session rows, which is written by another process while
//! csift reads it: a half-written row, a row for a session whose transcript is elsewhere, and
//! a row stamped by a different operating system all occur, and none of them may be fatal or
//! be turned into a claim about liveness.

use crate::harness::*;

const ENC: &str = "-Users-dev-Projects-registry";
const SESS: &str = "00000000-0000-4000-8000-000000000091";
const ELSEWHERE: &str = "00000000-0000-4000-8000-000000000092";
const CWD: &str = "/Users/dev/Projects/registry";

fn session_transcript() -> String {
    format!(
        "{}\n",
        format_args!(
            r#"{{"type":"user","uuid":"m1","timestamp":"2026-06-07T04:00:00.000Z","sessionId":"{SESS}","cwd":"{CWD}","version":"2.1.258","message":{{"role":"user","content":"hello"}}}}"#
        )
    )
}

fn home() -> Home {
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &session_transcript());
    h
}

/// The sessions directory, for the rows the harness helper cannot name (it keys on a pid).
fn sessions_dir(h: &Home) -> std::path::PathBuf {
    h.root.join(".claude").join("sessions")
}

#[test]
fn unreadable_and_shapeless_registry_rows_are_skipped_not_fatal() {
    // Rows are written by another process. csift reads the directory whatever is in it: a
    // name that is a directory, a half-written row, and a row with no session id are each
    // skipped, and the ones that DO parse are still reported.
    let h = home();
    let dir = sessions_dir(&h);
    std::fs::create_dir_all(dir.join("1001.json")).unwrap();
    h.write_claude("sessions/1002.json", "{\"pid\":1002, this is not json");
    h.write_claude("sessions/1003.json", r#"{"pid":1003,"status":"busy"}"#);
    h.write_claude("sessions/notarow.txt", r#"{"sessionId":"ignored"}"#);
    let me = std::process::id();
    h.write_session_registry(
        me,
        &format!(r#"{{"pid":{me},"sessionId":"{SESS}","status":"busy","entrypoint":"cli"}}"#),
    );
    let out = h.run(&["whoami", "--peers"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains(&format!("{SESS}  top-level session  busy"))
            && out.stdout.contains("1 live lane(s)"),
        "the readable row survives its unreadable neighbours:\n{}",
        out.stdout
    );
}

#[test]
fn a_row_that_cannot_be_probed_is_not_counted_as_live() {
    // Two rows csift must not read as a live lane: one with no pid to probe, and one stamped
    // on another operating system, where this host's pid numbers mean nothing.
    let h = home();
    h.write_claude(
        "sessions/2001.json",
        &format!(r#"{{"sessionId":"{SESS}","status":"busy"}}"#),
    );
    let me = std::process::id();
    h.write_claude(
        "sessions/2002.json",
        &format!(
            r#"{{"pid":{me},"sessionId":"{ELSEWHERE}","status":"busy","pidDomain":"plan9:elsewhere"}}"#
        ),
    );
    let out = h.run(&["whoami", "--peers"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("0 live lane(s)"),
        "neither row proves a live process:\n{}",
        out.stdout
    );
}

#[test]
fn a_live_session_with_no_transcript_here_is_still_reported_by_id() {
    // A session whose transcript sits under another Claude home, or whose project directory
    // was removed, is still a running lane. Dropping it would under-report the census, so the
    // row is printed with the id alone.
    let h = home();
    let me = std::process::id();
    h.write_session_registry(
        me,
        &format!(
            r#"{{"pid":{me},"sessionId":"{ELSEWHERE}","status":"waiting","entrypoint":"cli"}}"#
        ),
    );
    let out = h.run(&["whoami", "--peers"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains(&format!("{ELSEWHERE}  top-level session  waiting")),
        "{}",
        out.stdout
    );
    let json = h.run(&["whoami", "--peers", "--format", "json"]);
    let row = json_rows(&json.stdout, "peer").remove(0);
    assert_eq!(row["lane"], ELSEWHERE);
    assert!(
        row["last_activity_utc"].is_null(),
        "there is no transcript to read an instant from: {row}"
    );
}

#[test]
fn the_self_state_of_a_session_is_unknown_without_a_probeable_row() {
    // The `self` section's state is the registry row plus a pid probe. A row with no pid, and
    // a row from another pid domain, both leave liveness unknown - never asserted either way.
    let h = home();
    h.write_claude(
        "sessions/3001.json",
        &format!(r#"{{"sessionId":"{SESS}","status":"busy"}}"#),
    );
    let out = h.run_with_env(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("state    unknown"),
        "a row with no pid cannot be probed:\n{}",
        out.stdout
    );

    let foreign = home();
    let me = std::process::id();
    foreign.write_session_registry(
        me,
        &format!(
            r#"{{"pid":{me},"sessionId":"{SESS}","status":"busy","pidDomain":"plan9:elsewhere"}}"#
        ),
    );
    let out = foreign.run_with_env(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", SESS)]);
    assert!(
        out.stdout.contains("state    unknown"),
        "a foreign pid domain is never probed:\n{}",
        out.stdout
    );
}

#[test]
fn the_self_state_of_a_session_whose_owner_process_is_gone_is_dead() {
    // A reliably dead pid: spawn a no-op and reap it. The row still says `busy`, and the
    // probe is what refutes it - a transition-written row cannot retract itself.
    let mut noop = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/c", "exit"])
            .spawn()
            .unwrap()
    } else {
        std::process::Command::new("true").spawn().unwrap()
    };
    let dead_pid = noop.id();
    let _ = noop.wait();

    let h = home();
    h.write_session_registry(
        dead_pid,
        &format!(r#"{{"pid":{dead_pid},"sessionId":"{SESS}","status":"busy","entrypoint":"cli"}}"#),
    );
    let out = h.run_with_env(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("state    dead"),
        "the pid probe outranks the row's own word:\n{}",
        out.stdout
    );
}
