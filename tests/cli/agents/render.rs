//! agents rendering: tree text, JSON rows, hygiene, session grouping.

use crate::harness::*;

#[test]
fn agents_text_returned_files_and_tree_render() {
    // Exercise the TEXT-render branches for `--returned-message` / `--with-files` + the
    // always-on tree topology (the print_node `returned`/`files`/workflow-run arms) and the
    // one_line returned-message preview path. A node with no resolvable returned message
    // renders `(unresolved)`; a node with no files renders `files (none)`.
    let h = populated_home();
    let out = h.run(&[
        "agents",
        at(SESS).as_str(),
        "--returned-message",
        "--with-files",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The returned-message line renders (resolved or `(unresolved)`).
    assert!(
        out.stdout.contains("returned"),
        "returned line missing: {}",
        out.stdout
    );
    // The with-files line renders (`files … changed` or `files (none)`).
    assert!(
        out.stdout.contains("files"),
        "files line missing: {}",
        out.stdout
    );
    // Tree topology: the workflow run parents its agent.
    assert!(
        out.stdout.contains("wf_abc") || out.stdout.contains("workflow"),
        "tree topology missing: {}",
        out.stdout
    );
}

#[test]
fn agents_clean_run_text_hygiene() {
    // Mutation pins on the tree renderer: a single-session run has NO leading blank line
    // and no blank-before-first-SESSION; a corpus with no teammates prints NO teammate
    // control hint; a clean lane never prints a zero-count malformed note.
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.starts_with('\n') && !out.stdout.contains("\n\nSESSION"),
        "no blank line ahead of the first session: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("teammate rows are in-process"),
        "teammate control hint must be gated on a teammate being present: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("0 malformed"),
        "a clean lane never prints a zero-count malformed note: {}",
        out.stdout
    );
}

#[test]
fn agents_json_rows() {
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // v0.5 FLAT rows: every node is its own `kind:"agent"` row - the uniform envelope
    // idiom (`jq 'select(.kind=="agent")'`) reaches all shapes directly.
    let kinds: Vec<String> = json_rows(&out.stdout, "agent")
        .iter()
        .filter_map(|n| n.get("shape").and_then(|k| k.as_str()).map(String::from))
        .collect();
    assert!(kinds.iter().any(|k| k == "builtin-task"));
    assert!(kinds.iter().any(|k| k == "workflow"));
}

#[test]
fn agents_reports_skipped_lines_note() {
    // A subagent transcript with a malformed line → the per-row "malformed line(s)
    // skipped" note (agents.rs render skipped_lines arm).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    // The malformed line is the NEWEST (last) record so the TAIL scan reaches and
    // counts it (head stops at the first record; tail walks newest-first).
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-broken1.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"broken1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#, "\n",
            "{ this is a malformed newest line }\n",
        ),
    );
    let out = h.run(&["agents", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("malformed line(s) skipped"),
        "got: {}",
        out.stdout
    );
}

#[test]
fn agents_text_lists_lifecycle_rows() {
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"));
    assert!(out.stdout.contains("builtin-task"));
    assert!(out.stdout.contains("workflow"));
    assert!(out.stdout.contains("completed"));
    assert!(out.stdout.contains("started"));
    assert!(out.stdout.contains("duration"));
    assert!(out.stdout.contains("subagent(s)"));
    // The built-in carries a description line.
    assert!(out.stdout.contains("run the carry task"));
}

#[test]
fn agents_with_files_renders_changed_list_and_summary_json() {
    // A subagent that ACTUALLY changed a file → the `--with-files` text path renders the
    // `files N changed` + per-file create/op tag lines (vs the `(none)` arm), and
    // `--order-by start` exercises the start-axis label. Also covers files `--summary
    // --format json`.
    let h = Home::new();
    let sess = "33333333-4444-5555-6666-777777777777";
    h.write(
        &format!("-Users-x-w/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","cwd":"/Users/x/w","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"tk","name":"Task","input":{"description":"sub task"}}]}}"#, "\n",
        ),
    );
    // A subagent transcript that Writes a new file (so files_changed is non-empty).
    h.write(
        &format!("-Users-x-w/{sess}/subagents/agent-fff999.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"fff999","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"sub: write the file"}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/Users/x/w/new.rs","content":"x"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"sc","parentUuid":"sa","timestamp":"2026-06-07T05:00:04.000Z","toolUseResult":{"type":"create","filePath":"/Users/x/w/new.rs"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("-Users-x-w/{sess}/subagents/agent-fff999.meta.json"),
        r#"{"agentType":"executor","toolUseId":"tk"}"#,
    );
    let out = h.run(&[
        "agents",
        at(sess).as_str(),
        "--with-files",
        "--order-by",
        "start",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("changed") && out.stdout.contains("new.rs"),
        "files-changed list not rendered: {}",
        out.stdout
    );
    // The summary JSON path (the `json_grouped` summary arm + trailing summary object).
    let f = h.run(&[
        "files",
        at(sess).as_str(),
        "--by",
        "summary",
        "--format",
        "json",
    ]);
    assert!(f.success, "stderr: {}", f.stderr);
    let last = f
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .next_back()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(last).unwrap();
    assert_eq!(
        v.get("detail_level").and_then(|d| d.as_str()),
        Some("summary")
    );
}

#[test]
fn agents_groups_multiple_sessions_with_separator() {
    // Two sessions each with a subagent → the render groups rows under per-session
    // headers separated by a blank line (the `last_session.is_some()` separator arm).
    let h = Home::new();
    let sess_a = "aaaaaaaa-0000-0000-0000-000000000001";
    let sess_b = "bbbbbbbb-0000-0000-0000-000000000002";
    for s in [sess_a, sess_b] {
        h.write(
            &format!("{ENC}/{s}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        h.write(
            &format!("{ENC}/{s}/subagents/agent-x{}.jsonl", &s[0..3]),
            &format!(
                "{{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"x{}\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"s\"}}}}\n{{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:00:10.000Z\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"done\"}}]}}}}\n",
                &s[0..3]
            ),
        );
    }
    let out = h.run(&["agents", ENC]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.matches("SESSION").count() >= 2,
        "two session headers: {}",
        out.stdout
    );
}
