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
    // the grouped per-op COUNT keys - one on-wire spelling across both files JSON modes.
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

#[test]
fn files_timeline_keeps_every_row_when_a_branch_left_the_conversation() {
    // The GOLDEN PIN on the ROW SET, and it holds either way the chain is built. `files`
    // now splices the structural spine rows of the lines its prefilter drops (via
    // `ChainView`), so its chain sees the whole DAG and CAN name the abandoned branch -
    // and it still lists every row, because disk truth is file order: an Edit on a branch
    // the operator later rewound past really did land. What survival changes is the turn
    // slot and the marker, never the presence of a row. This is the pin that catches the
    // regression a trusted-too-far chain caused before the splice - measured on two real
    // transcripts, 230 rows down to 92 and 4,778 down to 3,054.
    let h = Home::new();
    let enc = "-Users-dev-example-quarry";
    let sess = "6b5a4938-2716-4c05-9d8e-7f6a5b4c3d2e";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"open the quarry survey"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"tool_use","id":"t0","name":"Write","input":{"file_path":"/Users/dev/quarry/survey.md","content":"one"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"cut the north face"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/Users/dev/quarry/north.md"}}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"a1","timestamp":"2026-06-07T05:01:06.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"tool_use","id":"t2","name":"Edit","input":{"file_path":"/Users/dev/quarry/ledge.md"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"a0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"cut the south face instead"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a3","parentUuid":"u2","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","id":"m3","content":[{"type":"tool_use","id":"t3","name":"Edit","input":{"file_path":"/Users/dev/quarry/south.md"}}]}}"#, "\n",
        ),
    );
    let out = h.run(&["files", at(sess).as_str(), "--by", "timeline"]);
    assert!(out.success, "stderr: {}", out.stderr);
    for path in [
        "/Users/dev/quarry/survey.md",
        "/Users/dev/quarry/north.md",
        "/Users/dev/quarry/ledge.md",
        "/Users/dev/quarry/south.md",
    ] {
        assert!(
            out.stdout.contains(path),
            "the timeline lost {path}:\n{}",
            out.stdout
        );
    }
}
