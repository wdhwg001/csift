//! status end to end: the verdict probes (design section 7) and the JSON envelope.

use crate::harness::*;

#[test]
fn p1_running_from_the_unreturned_tail() {
    let h = Home::new();
    live_running_main(&h);
    h.write_session_registry(
        std::process::id(),
        &live_registry_row(std::process::id(), "busy"),
    );
    let out = h.run(&["status", &at(LIVE_SESS)]);
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

    let j = h.run(&["status", &at(LIVE_SESS), "--format", "json"]);
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
    live_eot_main(&h);
    h.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}/subagents/agent-c1d2e3f4a5b60718.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"su1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"child task"}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa1","parentUuid":"su1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ct1","name":"Bash","input":{"command":"work"}}]}}"#, "\n",
        ),
    );
    let out = h.run(&["status", &at(LIVE_SESS)]);
    assert!(
        out.stdout.contains("verdict  waiting-children") && out.stdout.contains("in-flight"),
        "live child lane:\n{}",
        out.stdout
    );

    // Child settles (its result lands): verdict moves to idle-eot.
    let h2 = Home::new();
    live_eot_main(&h2);
    h2.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}/subagents/agent-c1d2e3f4a5b60718.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"su1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"child task"}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa1","parentUuid":"su1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ct1","name":"Bash","input":{"command":"work"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"sr1","parentUuid":"sa1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ct1","content":"done"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa2","parentUuid":"sr1","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"child done"}]}}"#, "\n",
        ),
    );
    // The child's record timestamps are ancient and its tail ends in an end_turn, so
    // it reads settled on the record-tail law (mtime is not consulted; the aging below
    // is legacy belt from the retired mtime leg, kept as harmless).
    let sub = h2.projects().join(format!(
        "{LIVE_ENC}/{LIVE_SESS}/subagents/agent-c1d2e3f4a5b60718.jsonl"
    ));
    let f = std::fs::File::options().write(true).open(&sub).unwrap();
    f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
    let main = h2.projects().join(format!("{LIVE_ENC}/{LIVE_SESS}.jsonl"));
    let f = std::fs::File::options().write(true).open(&main).unwrap();
    f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
    let out2 = h2.run(&["status", &at(LIVE_SESS)]);
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
    live_eot_main(&h3);
    h3.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}/subagents/workflows/wf_test1/journal.jsonl"),
        concat!(
            r#"{"type":"started","key":"k1","agentId":"aaa1"}"#,
            "\n",
            r#"{"type":"started","key":"k2","agentId":"aaa2"}"#,
            "\n",
            r#"{"type":"result","key":"k1","agentId":"aaa1","result":"ok"}"#,
            "\n",
        ),
    );
    let main3 = h3.projects().join(format!("{LIVE_ENC}/{LIVE_SESS}.jsonl"));
    let f = std::fs::File::options().write(true).open(&main3).unwrap();
    f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
    let out3 = h3.run(&["status", &at(LIVE_SESS)]);
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
    live_eot_main(&h);
    h.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}/elicitations.jsonl"),
        concat!(
            r#"{"csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"toolu_ask1","type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_ask1","name":"AskUserQuestion","input":{"questions":[{"question":"which harbor?"}]}}]}}"#, "\n",
        ),
    );
    let out = h.run(&["status", &at(LIVE_SESS)]);
    assert!(
        out.stdout.contains("verdict  waiting-hitl") && out.stdout.contains("pending elicitation"),
        "sidecar pending = blocked on a human:\n{}",
        out.stdout
    );
}

#[test]
fn p3b_waiting_hitl_from_the_tail_and_the_registry_without_a_sidecar() {
    // A multi-question AskUserQuestion is written at question time: an unreturned one
    // at the tail is hitl, not running (no sidecar installed here).
    let h = Home::new();
    h.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pick the harbor and the tide"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"ask9","name":"AskUserQuestion","input":{"questions":[{"question":"which harbor?","header":"Harbor","options":[{"label":"north"},{"label":"south"}]},{"question":"which tide?","header":"Tide","options":[{"label":"ebb"},{"label":"flood"}]}]}}]}}"#, "\n",
        ),
    );
    let out = h.run(&["status", &at(LIVE_SESS)]);
    assert!(
        out.stdout.contains("verdict  waiting-hitl")
            && out.stdout.contains("unreturned AskUserQuestion call")
            && out.stdout.contains("single-question ask stays buffered"),
        "tail dialog = blocked on a human:\n{}",
        out.stdout
    );
    let js = h.run(&["status", &at(LIVE_SESS), "--format", "json"]);
    assert!(
        js.stdout.contains("\"verdict\":\"waiting-hitl\""),
        "{}",
        js.stdout
    );

    // The registry's `waiting` status alone (a permission prompt, a plan approval, ...)
    // is hitl too, over a settled tail; the note names the dialog kinds.
    let h2 = Home::new();
    live_eot_main(&h2);
    h2.write_session_registry(
        std::process::id(),
        &live_registry_row(std::process::id(), "waiting"),
    );
    let out2 = h2.run(&["status", &at(LIVE_SESS)]);
    assert!(
        out2.stdout.contains("verdict  waiting-hitl")
            && out2.stdout.contains("status waiting")
            && out2.stdout.contains("permission prompt, a plan approval"),
        "registry waiting = blocked on a dialog:\n{}",
        out2.stdout
    );
    // An idle row keeps idle-eot, and the permission-prompt note now names the row.
    let h3 = Home::new();
    live_eot_main(&h3);
    h3.write_session_registry(
        std::process::id(),
        &live_registry_row(std::process::id(), "idle"),
    );
    let out3 = h3.run(&["status", &at(LIVE_SESS)]);
    assert!(
        out3.stdout.contains("verdict  idle-eot")
            && out3
                .stdout
                .contains("registry row (status idle) would read `waiting`"),
        "{}",
        out3.stdout
    );
}

#[test]
fn p3c_foreign_pid_domain_row_is_never_probed() {
    let h = Home::new();
    live_running_main(&h);
    let me = std::process::id();
    h.write_session_registry(
        me,
        &format!(
            r#"{{"pid":{me},"sessionId":"{LIVE_SESS}","status":"busy","procStart":"134328101803820142","pidDomain":"plan9:elsewhere","kind":"interactive"}}"#
        ),
    );
    let out = h.run(&["status", &at(LIVE_SESS)]);
    assert!(
        out.stdout.contains("verdict  running")
            && out
                .stdout
                .contains("row from another pid domain (plan9:elsewhere)")
            && out.stdout.contains("stale-dead is undecidable"),
        "{}",
        out.stdout
    );
}

#[test]
fn p4_stale_dead_via_pid_and_reuse_guard() {
    // A reliably dead pid: spawn a no-op, reap it.
    let spawn_noop = || {
        if cfg!(windows) {
            std::process::Command::new("cmd")
                .args(["/c", "exit"])
                .spawn()
                .unwrap()
        } else {
            std::process::Command::new("true").spawn().unwrap()
        }
    };
    let mut dead = spawn_noop();
    let dead_pid = dead.id();
    let _ = dead.wait();

    let h = Home::new();
    live_running_main(&h); // mid-tool tail
    h.write_session_registry(dead_pid, &live_registry_row(dead_pid, "busy"));
    let out = h.run(&["status", &at(LIVE_SESS)]);
    assert!(
        out.stdout.contains("verdict  stale-dead") && out.stdout.contains("MID-TOOL"),
        "dead pid + open call = died mid-tool:\n{}",
        out.stdout
    );

    // Reuse variant: OUR pid (alive) with a procStart that is not our start time, in
    // the registry's own rendering for this platform (asctime UTC on unix, a FILETIME
    // tick count on Windows - here one from 2022).
    let h2 = Home::new();
    live_eot_main(&h2);
    let me = std::process::id();
    let old_start = if cfg!(windows) {
        "133000000000000000"
    } else {
        "Sun Aug 16 09:04:23 2026"
    };
    h2.write_session_registry(
        me,
        &format!(
            r#"{{"pid":{me},"sessionId":"{LIVE_SESS}","status":"idle","procStart":"{old_start}","kind":"interactive"}}"#
        ),
    );
    let out2 = h2.run(&["status", &at(LIVE_SESS)]);
    // The reuse verdict needs a start time from the host probe; a busybox ps (Alpine)
    // cannot give one, and the honest outcome there is a pid-only probe with the skip
    // disclosed. On Windows the probe is PowerShell's Get-Process StartTime.
    let ps_has_lstart = if cfg!(windows) {
        std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!("(Get-Process -Id {me}).StartTime.ToFileTimeUtc()"),
            ])
            .output()
            .is_ok_and(|o| {
                o.status.success()
                    && String::from_utf8_lossy(&o.stdout)
                        .trim()
                        .bytes()
                        .all(|b| b.is_ascii_digit())
                    && !o.stdout.iter().all(u8::is_ascii_whitespace)
            })
    } else {
        std::process::Command::new("ps")
            .args(["-p", &me.to_string(), "-o", "lstart="])
            .output()
            .is_ok_and(|o| o.status.success() && !o.stdout.iter().all(u8::is_ascii_whitespace))
    };
    if ps_has_lstart {
        assert!(
            out2.stdout.contains("verdict  stale-dead") && out2.stdout.contains("pid reused"),
            "start-time mismatch = reuse, dead:\n{}",
            out2.stdout
        );
    } else {
        assert!(
            out2.stdout.contains("alive (pid only)")
                && out2.stdout.contains("reuse guard was skipped"),
            "no start time on this host = pid-only, disclosed:\n{}",
            out2.stdout
        );
    }
}

#[test]
fn p7_status_json_envelope_and_registry_decoys() {
    let h = Home::new();
    live_running_main(&h);
    let real = h.write_session_registry(
        std::process::id(),
        &live_registry_row(std::process::id(), "busy"),
    );
    // Decoy rows the scan must skip: wrong extension, malformed JSON, another session.
    let dir = real.parent().unwrap().to_path_buf();
    std::fs::write(dir.join("notes.txt"), "not a row").unwrap();
    std::fs::write(dir.join("torn.json"), "{ half").unwrap();
    std::fs::write(
        dir.join("other.json"),
        r#"{"sessionId":"ffffffff-0000-4000-8000-000000000000","pid":1,"status":"idle"}"#,
    )
    .unwrap();
    let j = h.run(&["status", &at(LIVE_SESS), "--format", "json"]);
    assert!(j.success, "stderr: {}", j.stderr);
    let rows: Vec<serde_json::Value> = j
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows[0]["kind"], "header");
    assert_eq!(rows[0]["command"], "status");
    assert_eq!(rows[0]["session_id"], LIVE_SESS);
    assert_eq!(rows[0]["is_subagent"], false);
    let v = rows.iter().find(|r| r["kind"] == "verdict").expect("row");
    assert_eq!(v["verdict"], "running", "{}", j.stdout);
    assert!(
        v["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["surface"] == "registry" && e["value"].as_str().unwrap().contains("busy")),
        "the decoys never displace the real row: {}",
        j.stdout
    );
    assert_eq!(rows.last().unwrap()["kind"], "summary");

    // With a live child lane the JSON verdict row carries the children array.
    h.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}/subagents/agent-1a2b3c4d5e6f7081.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"ka1","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"kt1","name":"Bash","input":{"command":"work"}}]}}"#, "\n",
        ),
    );
    let j2 = h.run(&["status", &at(LIVE_SESS), "--format", "json"]);
    let v2: serde_json::Value = j2
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .find(|r: &serde_json::Value| r["kind"] == "verdict")
        .expect("row");
    let kids = v2["children"].as_array().unwrap();
    assert_eq!(kids.len(), 1, "{}", j2.stdout);
    assert_eq!(kids[0]["session_id"], "1a2b3c4d5e6f7081");
    assert_eq!(kids[0]["state"], "in-flight");

    // Registry dir present but holding only decoys: honest no-row degradation.
    std::fs::remove_file(&real).unwrap();
    let bare = h.run(&["status", &at(LIVE_SESS)]);
    assert!(
        bare.stdout.contains("not currently registered"),
        "{}",
        bare.stdout
    );
}

#[test]
fn p16_a_completion_pulse_settles_its_lane_while_an_open_agent_stays_live() {
    // Two async agents launched from the main lane, both with FRESH paired child tails
    // and no end_turn (the `generating` shape). Agent A's completion notification has
    // landed in the main transcript; agent B's has not. The returned set is built from
    // the background scan (agent kind AND a closed state): A folds as settled, B stays
    // live, and the verdict is waiting-children on B alone.
    let h = Home::new();
    const A: &str = "a0123456789abcdef0";
    const B: &str = "a9876543210fedcba9";
    h.write(
        &format!("{LIVE_ENC}/{LIVE_SESS}.jsonl"),
        &format!(
            concat!(
                r#"{{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"survey both reefs"}}}}"#, "\n",
                r#"{{"type":"user","uuid":"r2","timestamp":"2026-06-07T05:00:03.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t2","content":"Async agent launched successfully."}}]}},"toolUseResult":{{"isAsync":true,"status":"async_launched","agentId":"{A}","description":"Census the reef","outputFile":"/nonexistent/a.output"}}}}"#, "\n",
                r#"{{"type":"user","uuid":"r3","timestamp":"2026-06-07T05:00:04.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t3","content":"Async agent launched successfully."}}]}},"toolUseResult":{{"isAsync":true,"status":"async_launched","agentId":"{B}","description":"Chart the shoal","outputFile":"/nonexistent/b.output"}}}}"#, "\n",
                r#"{{"type":"user","uuid":"n1","timestamp":"2026-06-07T05:09:00.000Z","message":{{"role":"user","content":"<task-notification>\n<task-id>{A}</task-id>\n<tool-use-id>t2</tool-use-id>\n<status>completed</status>\n<summary>Agent \"Census the reef\" finished</summary>\n</task-notification>"}}}}"#, "\n",
                r#"{{"type":"assistant","uuid":"a9","parentUuid":"n1","timestamp":"2026-06-07T05:09:05.000Z","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"one back, one still out"}}]}}}}"#, "\n",
            ),
            A = A,
            B = B
        ),
    );
    let now = jiff::Timestamp::now().to_string();
    for id in [A, B] {
        h.write(
            &format!("{LIVE_ENC}/{LIVE_SESS}/subagents/agent-{id}.jsonl"),
            &format!(
                concat!(
                    r#"{{"type":"assistant","uuid":"ca1","timestamp":"{now}","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"ct1","name":"Bash","input":{{"command":"work"}}}}]}}}}"#, "\n",
                    r#"{{"type":"user","uuid":"cr1","parentUuid":"ca1","timestamp":"{now}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"ct1","content":"ok"}}]}}}}"#, "\n",
                ),
                now = now
            ),
        );
    }
    let j = h.run(&["status", &at(LIVE_SESS), "--format", "json"]);
    assert!(j.success, "stderr: {}", j.stderr);
    let v: serde_json::Value = j
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .find(|r: &serde_json::Value| r["kind"] == "verdict")
        .expect("verdict row");
    assert_eq!(v["verdict"], "waiting-children", "{}", j.stdout);
    let kids = v["children"].as_array().unwrap();
    assert_eq!(kids.len(), 1, "only B is live: {}", j.stdout);
    assert_eq!(kids[0]["session_id"], B);
    assert_eq!(kids[0]["state"], "generating");
    assert_eq!(
        v["settled_children"], 1,
        "A folded on its pulse: {}",
        j.stdout
    );
    // Text: A folds into the count line, B renders as a live row.
    let out = h.run(&["status", &at(LIVE_SESS)]);
    assert!(
        out.stdout.contains("1 settled lane(s) folded"),
        "{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains(&format!("{A}  generating")),
        "A is folded, never a live row:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(B) && out.stdout.contains("generating"),
        "{}",
        out.stdout
    );
}
