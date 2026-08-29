//! status + wait: the six acceptance probes (design section 7).

use crate::harness::*;
use std::io::{BufRead, BufReader, Write as _};

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "4e3d2c1b-0a9f-4876-b543-210fedcba987";

fn eot_main(h: &Home) -> PathBuf {
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the shoals"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"charted; stopping"}]}}"#, "\n",
        ),
    )
}

fn running_main(h: &Home) -> PathBuf {
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"run the long sweep"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"sleep 30"}}]}}"#, "\n",
        ),
    )
}

fn registry_row(pid: u32, status: &str) -> String {
    format!(
        r#"{{"pid":{pid},"sessionId":"{SESS}","status":"{status}","statusUpdatedAt":1767000000000,"kind":"interactive"}}"#
    )
}

#[test]
fn p1_running_from_the_unreturned_tail() {
    let h = Home::new();
    running_main(&h);
    h.write_session_registry(
        std::process::id(),
        &registry_row(std::process::id(), "busy"),
    );
    let out = h.run(&["status", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("verdict  running"),
        "unreturned tail call = running:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("unreturned Bash call"),
        "the evidence names the tool:\n{}",
        out.stdout
    );

    let j = h.run(&["status", &at(SESS), "--format", "json"]);
    let rows: Vec<serde_json::Value> = j
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows[0]["kind"], "header");
    let v = rows.iter().find(|r| r["kind"] == "verdict").expect("row");
    assert_eq!(v["verdict"], "running", "{}", j.stdout);
    assert!(
        v["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["surface"] == "tail"),
        "{}",
        j.stdout
    );
}

#[test]
fn p2_waiting_children_then_idle_eot() {
    // A live child (unreturned tail call) holds the verdict at waiting-children; the
    // settled variant plus an end_turn main = idle-eot with the F7 honesty note.
    let h = Home::new();
    eot_main(&h);
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-c1d2e3f4a5b60718.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"su1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"child task"}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa1","parentUuid":"su1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ct1","name":"Bash","input":{"command":"work"}}]}}"#, "\n",
        ),
    );
    let out = h.run(&["status", &at(SESS)]);
    assert!(
        out.stdout.contains("verdict  waiting-children") && out.stdout.contains("in-flight"),
        "live child lane:\n{}",
        out.stdout
    );

    // Child settles (its result lands): verdict moves to idle-eot.
    let h2 = Home::new();
    eot_main(&h2);
    h2.write(
        &format!("{ENC}/{SESS}/subagents/agent-c1d2e3f4a5b60718.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"su1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"child task"}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa1","parentUuid":"su1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ct1","name":"Bash","input":{"command":"work"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"sr1","parentUuid":"sa1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ct1","content":"done"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa2","parentUuid":"sr1","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"child done"}]}}"#, "\n",
        ),
    );
    // The fixture's mtime is fresh (just written) - the child would read as "active" by
    // recency. Age the file so only the tail shape speaks.
    let sub = h2.projects().join(format!(
        "{ENC}/{SESS}/subagents/agent-c1d2e3f4a5b60718.jsonl"
    ));
    let f = std::fs::File::options().write(true).open(&sub).unwrap();
    f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
    let main = h2.projects().join(format!("{ENC}/{SESS}.jsonl"));
    let f = std::fs::File::options().write(true).open(&main).unwrap();
    f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
    let out2 = h2.run(&["status", &at(SESS)]);
    assert!(
        out2.stdout.contains("verdict  idle-eot"),
        "settled child + end_turn main = idle:\n{}",
        out2.stdout
    );
    assert!(
        out2.stdout.contains("permission prompt"),
        "the F7 honesty note rides idle verdicts:\n{}",
        out2.stdout
    );

    // The journal variant: 2 started, 1 result = a workflow agent in flight.
    let h3 = Home::new();
    eot_main(&h3);
    h3.write(
        &format!("{ENC}/{SESS}/subagents/workflows/wf_test1/journal.jsonl"),
        concat!(
            r#"{"type":"started","key":"k1","agentId":"aaa1"}"#,
            "\n",
            r#"{"type":"started","key":"k2","agentId":"aaa2"}"#,
            "\n",
            r#"{"type":"result","key":"k1","agentId":"aaa1","result":"ok"}"#,
            "\n",
        ),
    );
    let main3 = h3.projects().join(format!("{ENC}/{SESS}.jsonl"));
    let f = std::fs::File::options().write(true).open(&main3).unwrap();
    f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
    let out3 = h3.run(&["status", &at(SESS)]);
    assert!(
        out3.stdout.contains("verdict  waiting-children")
            && out3.stdout.contains("1 workflow agent(s) in flight"),
        "journal imbalance is a live signal:\n{}",
        out3.stdout
    );
}

#[test]
fn p3_waiting_hitl_from_the_sidecar() {
    let h = Home::new();
    eot_main(&h);
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        concat!(
            r#"{"csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"toolu_ask1","type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_ask1","name":"AskUserQuestion","input":{"questions":[{"question":"which harbor?"}]}}]}}"#, "\n",
        ),
    );
    let out = h.run(&["status", &at(SESS)]);
    assert!(
        out.stdout.contains("verdict  waiting-hitl") && out.stdout.contains("pending elicitation"),
        "sidecar pending = blocked on a human:\n{}",
        out.stdout
    );
}

#[cfg(unix)]
#[test]
fn p4_stale_dead_via_pid_and_reuse_guard() {
    // A reliably dead pid: spawn `true`, reap it.
    let dead = std::process::Command::new("true").spawn().unwrap();
    let dead_pid = dead.id();
    let _ = std::process::Command::new("true").spawn().unwrap().wait();
    let mut child = dead;
    let _ = child.wait();

    let h = Home::new();
    running_main(&h); // mid-tool tail
    h.write_session_registry(dead_pid, &registry_row(dead_pid, "busy"));
    let out = h.run(&["status", &at(SESS)]);
    assert!(
        out.stdout.contains("verdict  stale-dead") && out.stdout.contains("MID-TOOL"),
        "dead pid + open call = died mid-tool:\n{}",
        out.stdout
    );

    // Reuse variant: OUR pid (alive) with a procStart that is not our start time.
    let h2 = Home::new();
    eot_main(&h2);
    let me = std::process::id();
    h2.write_session_registry(
        me,
        &format!(
            r#"{{"pid":{me},"sessionId":"{SESS}","status":"idle","procStart":"Sun Aug 16 09:04:23 2026","kind":"interactive"}}"#
        ),
    );
    let out2 = h2.run(&["status", &at(SESS)]);
    assert!(
        out2.stdout.contains("verdict  stale-dead") && out2.stdout.contains("pid reused"),
        "start-time mismatch = reuse, dead:\n{}",
        out2.stdout
    );
}

/// Block on the wait readiness line, then run `act`, then collect the exit.
fn drive_wait(h: &Home, args: &[&str], act: impl FnOnce()) -> (Option<i32>, String) {
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

#[test]
fn p5_wait_fires_only_on_post_start_events() {
    let h = Home::new();
    let main = eot_main(&h);
    let target = at(SESS);
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
    let main = eot_main(&h);
    // The trigger is appended BEFORE wait starts: history, not an event.
    let mut f = std::fs::File::options().append(true).open(&main).unwrap();
    writeln!(
        f,
        r#"{{"type":"assistant","uuid":"a8","timestamp":"2026-06-07T05:00:30.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t8","name":"Read","input":{{"file_path":"/p/handover.md"}}}}]}}}}"#
    )
    .unwrap();
    drop(f);
    let target = at(SESS);
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
