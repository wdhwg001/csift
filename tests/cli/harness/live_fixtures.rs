//! Fixtures for the live (status/wait) probes: a settled main, a running main,
//! a registry row, and the readiness-synchronized wait driver.

use super::home::*;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

pub(crate) const LIVE_ENC: &str = "-Users-dev-example-project";
pub(crate) const LIVE_SESS: &str = "4e3d2c1b-0a9f-4876-b543-210fedcba987";

pub(crate) fn live_eot_main(h: &Home) -> PathBuf {
    h.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the shoals"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"charted; stopping"}]}}"#, "\n",
        ),
    )
}

pub(crate) fn live_running_main(h: &Home) -> PathBuf {
    h.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"run the long sweep"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"sleep 30"}}]}}"#, "\n",
        ),
    )
}

pub(crate) fn live_registry_row(pid: u32, status: &str) -> String {
    format!(
        r#"{{"pid":{pid},"sessionId":"{LIVE_SESS}","status":"{status}","statusUpdatedAt":1767000000000,"kind":"interactive"}}"#
    )
}

/// Block on the wait readiness line, then run `act`, then collect the exit.
pub(crate) fn drive_wait(h: &Home, args: &[&str], act: impl FnOnce()) -> (Option<i32>, String) {
    let mut child = h.spawn(args);
    let stderr = child.stderr.take().unwrap();
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(
        line.contains("csift: watching"),
        "readiness line first: {line}"
    );
    act();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}
