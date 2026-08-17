//! recover --at JSON projections: artifacts, provenance rows, skip parity.

use crate::harness::*;

#[test]
fn recover_at_json_lines_carry_provenance_and_gaps() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--at",
        "@turn:0",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let snap = json_rows(&out.stdout, "snapshot").remove(0);
    let lines = snap
        .get("lines")
        .and_then(|v| v.as_array())
        .expect("lines array");
    // Every emitted line carries n + text + set_at_line provenance (the jsonl line that set it).
    for l in lines {
        assert!(
            l.get("n").is_some() && l.get("text").is_some(),
            "line shape: {l}"
        );
        assert!(l.get("set_at_line").is_some(), "provenance: {l}");
    }
    assert!(snap.get("gaps").is_some(), "gaps array present: {snap}");
}

#[test]
fn recover_at_json_out_writes_artifact() {
    let h = recover_scenario_home();
    let out_path = h.root.join("snap.json.txt");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--at",
        "@turn:0",
        "--format",
        "json",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // stdout is NDJSON; the --out file is the verbatim reconstructed body.
    let body = std::fs::read_to_string(&out_path).expect("at --out artifact");
    assert!(body.contains("import os"), "verbatim known content: {body}");
}

#[test]
fn recover_at_json_out_writes_partial_snapshot_artifact() {
    // The at-mode JSON renderer's `--out` arm: it writes the partial-snapshot blob (known
    // lines + explicit gap markers) to disk while still emitting NDJSON to stdout.
    let h = recover_scenario_home();
    let out_path = h.root.join("at-artifact.txt");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--at",
        "@turn:0",
        "--format",
        "json",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let blob = std::fs::read_to_string(&out_path).expect("at JSON --out artifact");
    assert!(
        blob.contains("import os"),
        "the snapshot artifact carries known content: {blob}"
    );
}

#[test]
fn recover_at_json_skips_session_with_events_but_no_known_content() {
    // JSON at-mode: same shape → no snapshot object, only the summary with sessions == 0.
    let h = recover_empty_reconstruction_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/e.rs",
        "--at",
        "@line:9999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("ndjson"))
        .collect();
    assert!(
        objs.iter()
            .all(|o| o.get("kind").and_then(|v| v.as_str()) != Some("snapshot")),
        "no snapshot emitted for an empty reconstruction: {}",
        out.stdout
    );
    assert_eq!(objs.last().unwrap()["sessions"].as_u64(), Some(0));
}

#[test]
fn recover_json_at_skips_session_with_no_seen_total() {
    // JSON at-mode skip: same two-session shape as the text variant, but `--format json`,
    // driving the `known.is_empty() && seen_total.is_none()` continue in the JSON renderer.
    let h = Home::new();
    let sess_a = "aaaaaaaa-7777-7777-7777-777777777777";
    let sess_b = "bbbbbbbb-8888-8888-8888-888888888888";
    h.write(
        &format!("{ENC}/{sess_a}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/here.rs","content":"x\ny","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{sess_b}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/elsewhere.rs","content":"q","startLine":1,"numLines":1,"totalLines":1}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        ENC,
        "--file",
        "/p/here.rs",
        "--at",
        "@line:9999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("ndjson"))
        .collect();
    let snaps = objs
        .iter()
        .filter(|o| o.get("kind").and_then(|v| v.as_str()) == Some("snapshot"))
        .count();
    assert_eq!(
        snaps, 1,
        "only the session that saw /p/here.rs emits a snapshot"
    );
    assert_eq!(objs.last().unwrap()["sessions"].as_u64(), Some(1));
}

#[test]
fn recover_at_json_line_range_outside_known_keeps_seen_total() {
    // The JSON at-mode twin of the text test: a windowed read sets seen_total, but a
    // `--line-range` selecting no known line empties `known` while seen_total stays Some →
    // the JSON renderer's `known.is_empty() && seen_total.is_none()` second operand is FALSE
    // (the snapshot is emitted, carrying the gap up to the seen total).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/lrj.rs","content":"l5\nl6","startLine":5,"numLines":2,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/lrj.rs",
        "--at",
        "@line:9999",
        "--file-lines",
        "1..2",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let snap = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|o| o.get("kind").and_then(|v| v.as_str()) == Some("snapshot"))
        .expect("a snapshot object is still emitted");
    // No known lines survive the range filter, but the seen total + gaps are reported.
    assert_eq!(
        snap.get("lines")
            .and_then(|v| v.as_array())
            .map(|a| a.len()),
        Some(0),
        "no known lines in the 1..2 range: {snap}"
    );
    assert_eq!(
        snap.get("seen_total_lines").and_then(|v| v.as_u64()),
        Some(10),
        "the seen total is preserved: {snap}"
    );
}
