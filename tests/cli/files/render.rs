//! files rendering across detail levels: summary, by-file, by-dir, timeline.

use crate::harness::*;

#[test]
fn files_by_dir_renders_directory_rollup() {
    // Mutation pin: the `--by dir` render path emits the per-directory rollup (a deleted
    // renderer body must not pass by silence).
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["files", at(SESS).as_str(), "--by", "dir"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("/parent") && out.stdout.contains("/sub"),
        "directory rollup must name both dirs: {}",
        out.stdout
    );
}

#[test]
fn files_default_summary_acid_test() {
    let h = files_scenario_home();
    let out = h.run(&["files", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"));
    // The /tmp bucket: two writes (the created docs) + the heuristic bash rm.
    assert!(
        out.stdout.contains("/tmp: 2 write"),
        "/tmp bucket: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("bash (heuristic)"),
        "heuristic bash label: {}",
        out.stdout
    );
    // The gaps bucket: three edits.
    assert!(
        out.stdout.contains("/p/spec/gaps: 3 edit"),
        "gaps bucket: {}",
        out.stdout
    );
    // Footer accounting + heuristic caveat + skipped-line note.
    assert!(out.stdout.contains("detail=summary"));
    assert!(out.stdout.contains("Bash mutations are heuristic"));
    assert!(out.stdout.contains("malformed line(s) skipped"));
}

#[test]
fn files_by_file_distinct_counts_via_json() {
    let h = files_scenario_home();
    let out = h.run(&[
        "files",
        at(SESS).as_str(),
        "--by",
        "file",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    // The trailing summary object reports distinct_files + total_mutations.
    let summary: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    // Distinct files: /tmp/beacon-a.md, /tmp/beacon-b.md, gaps/one,two,three = 5.
    assert_eq!(
        summary.get("distinct_files").and_then(|v| v.as_u64()),
        Some(5),
        "summary: {summary}"
    );
    assert_eq!(
        summary.get("detail_level").and_then(|v| v.as_str()),
        Some("file")
    );
    // Count distinct gap docs (acid test #1): rows whose `file` ends in `/gaps/*.md`.
    let mut gap_docs = 0;
    let mut tmp_creates = 0;
    for l in &lines {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        if v["kind"] != "file" {
            continue;
        }
        if let Some(f) = v.get("path").and_then(|f| f.as_str()) {
            if f.starts_with("/p/spec/gaps/") {
                gap_docs += 1;
            }
            // Acid test #2: /tmp Writes are authoritative creates (write count > 0).
            if f.starts_with("/tmp/") && v.get("write").and_then(|w| w.as_u64()) == Some(1) {
                tmp_creates += 1;
            }
        }
    }
    assert_eq!(gap_docs, 3, "three distinct gap docs touched");
    assert_eq!(tmp_creates, 2, "two /tmp docs created via Write");
}

#[test]
fn files_by_dir_groups_and_counts() {
    let h = files_scenario_home();
    let out = h.run(&[
        "files",
        at(SESS).as_str(),
        "--by",
        "dir",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let mut saw_gaps_dir = false;
    for l in out.stdout.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        if v["kind"] == "dir" && v.get("path").and_then(|d| d.as_str()) == Some("/p/spec/gaps") {
            // Three edits, three distinct files in that dir.
            assert_eq!(v.get("edit").and_then(|e| e.as_u64()), Some(3));
            assert_eq!(v.get("distinct_files").and_then(|d| d.as_u64()), Some(3));
            saw_gaps_dir = true;
        }
    }
    assert!(saw_gaps_dir, "the gaps dir row must appear: {}", out.stdout);
}

#[test]
fn files_timeline_op_uses_underscore_spelling() {
    // The timeline `op` value is UNDERSCORE-delimited (notebook_edit/multi_edit) so it matches
    // the grouped per-op COUNT keys — one on-wire spelling across both files JSON modes.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"m1","name":"MultiEdit","input":{"file_path":"/p/multi.rs","edits":[{"old_string":"a","new_string":"b"}]}}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"n1","name":"NotebookEdit","input":{"notebook_path":"/p/nb.ipynb","new_source":"x"}}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "files",
        at(SESS).as_str(),
        "--by",
        "timeline",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let ops: Vec<&str> = objs.iter().filter_map(|o| o["op"].as_str()).collect();
    assert!(
        ops.contains(&"multi_edit"),
        "expected underscore multi_edit, got: {ops:?}"
    );
    assert!(
        ops.contains(&"notebook_edit"),
        "expected underscore notebook_edit, got: {ops:?}"
    );
    // The hyphenated spelling must NOT appear on the wire.
    assert!(
        !ops.iter().any(|o| o.contains('-')),
        "no hyphenated op token on the wire, got: {ops:?}"
    );
}

#[test]
fn files_help_mentions_detail_levels_and_heuristic() {
    let h = Home::new();
    let out = h.run(&["files", "--help"]);
    assert!(out.success);
    // The detail level is now a single `--by <summary|dir|file|timeline>` value-enum.
    assert!(out.stdout.contains("--by"));
    assert!(out.stdout.contains("summary"));
    assert!(out.stdout.contains("dir"));
    assert!(out.stdout.contains("file"));
    assert!(out.stdout.contains("timeline"));
    // The new full-path filters are documented.
    assert!(
        out.stdout.contains("--regex") && out.stdout.contains("--glob"),
        "help must document the --regex / --glob path filters: {}",
        out.stdout
    );
    // The removed flag must NOT appear.
    assert!(
        !out.stdout.contains("--subagents-only"),
        "help must NOT mention the removed --subagents-only flag: {}",
        out.stdout
    );
    assert!(
        out.stdout.to_lowercase().contains("heuristic"),
        "help must flag the Bash-heuristic caveat: {}",
        out.stdout
    );
}

#[test]
fn files_timeline_is_chronological_with_heuristic_label() {
    let h = files_scenario_home();
    let out = h.run(&["files", at(SESS).as_str(), "--by", "timeline"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("detail=timeline"));
    // The bash rm is the newest mutation (06:00) and carries the heuristic label.
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| l.contains("/tmp/beacon-a.md") || l.contains("/p/spec/gaps"))
        .collect();
    // The first /tmp/beacon-a.md mention (the Write at 05:00) precedes the bash rm.
    let write_pos = out.stdout.find("write  /tmp/beacon-a.md");
    let bash_pos = out.stdout.find("bash (heuristic)  /tmp/beacon-a.md");
    assert!(write_pos.is_some() && bash_pos.is_some(), "{}", out.stdout);
    assert!(
        write_pos < bash_pos,
        "the Write precedes the bash rm chronologically: {}",
        out.stdout
    );
    assert!(!lines.is_empty());
}
