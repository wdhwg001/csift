//! verbatim artifacts: --out files, JSON units, determinism, the golden baseline.

use crate::harness::*;

#[test]
fn turns_json_units_carry_id_domain_discriminators() {
    // turns per-unit JSON gains is_subagent + parent_session_id (top-level run here).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"ask a real question"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"a substantive reply"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["verbatim", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let unit = objs
        .iter()
        .find(|o| o["role"] == "user" || o["role"] == "assistant")
        .expect("a per-unit record present");
    assert_eq!(unit["is_subagent"], serde_json::json!(false));
    assert_eq!(unit["session_id"], serde_json::json!(SESS));
    assert_eq!(unit["parent_session_id"], serde_json::json!(SESS));
}

#[test]
fn turns_deterministic_byte_identical() {
    let h = turns_home();
    let a = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "10000",
    ]);
    let b = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "10000",
    ]);
    assert_eq!(
        a.stdout, b.stdout,
        "two identical invocations must be byte-identical"
    );
}

#[test]
fn turns_out_file_holds_full_reconstruction() {
    let h = turns_home();
    let out_path = h.root.join("turns-out.md");
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("wrote full reconstruction"),
        "{}",
        out.stdout
    );
    let body = std::fs::read_to_string(&out_path).expect("out file written");
    assert!(body.contains("▽ L"), "out file carries the rendered turns");
    assert!(
        body.contains("compaction boundary"),
        "out file carries banners"
    );
}

#[test]
fn turns_json_out_file_is_verbatim() {
    let h = turns_home();
    let out_path = h.root.join("turns.json");
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let body = std::fs::read_to_string(&out_path).expect("json out file");
    // The huge user unit's full verbatim text (un-truncated) is in the file.
    assert!(
        body.contains("HEADuser"),
        "out json carries the unit objects"
    );
    // Every non-blank line is valid JSON.
    for line in body.lines().filter(|l| !l.trim().is_empty()) {
        serde_json::from_str::<serde_json::Value>(line).expect("each out line is JSON");
    }
}

#[test]
fn turns_multi_session_text_has_blank_separator_and_both_sessions() {
    // A project-dir target → both sessions in the project are rendered, separated by a
    // blank line (the `if !first { println!() }` arm). Sessions are sorted by id.
    let h = turns_two_sessions_home();
    let out = h.run(&["verbatim", ENC, "--no-subagents", "--budget", "40000"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let session_headers = out
        .stdout
        .lines()
        .filter(|l| l.starts_with("SESSION "))
        .count();
    assert!(
        session_headers >= 2,
        "both sessions rendered: {}",
        out.stdout
    );
    // The clean session's content is present.
    assert!(out.stdout.contains("clean session ask"), "{}", out.stdout);
}

#[test]
fn turns_multi_session_json_runs_both() {
    // JSON over two sessions (a project-dir target) → both sessions' units emitted.
    let h = turns_two_sessions_home();
    let out = h.run(&[
        "verbatim",
        ENC,
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let sessions: std::collections::BTreeSet<&str> = objs
        .iter()
        .filter_map(|o| o.get("session_id").and_then(|v| v.as_str()))
        .collect();
    assert!(
        sessions.len() >= 2,
        "both sessions present in JSON: {sessions:?}"
    );
}

/// The executable re-capture procedure for the baseline above - NOT a behavioral test
/// (ignored by default; the fixture is a temp Home, so no hand-run command can reproduce
/// it). Writes the current eot-only output to tests/turns_pre_feature_baseline.txt.
#[test]
#[ignore = "capture tool — rewrites tests/turns_pre_feature_baseline.txt; run only on an intended output change"]
fn recapture_turns_pre_feature_baseline() {
    let h = turns_home();
    let out = h.run_with_env(
        &[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            "40000",
            "--agent-msgs",
            "eot-only",
        ],
        &[("TZ", "UTC")],
    );
    assert!(out.success, "stderr: {}", out.stderr);
    let dest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/turns_pre_feature_baseline.txt");
    std::fs::write(&dest, &out.stdout).expect("write baseline");
    eprintln!("captured {} bytes to {}", out.stdout.len(), dest.display());
}
