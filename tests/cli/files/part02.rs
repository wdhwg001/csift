use crate::harness::*;

#[test]
fn files_unknown_session_errors() {
    let h = files_scenario_home();
    let out = h.run(&["files", at("00000000-0000-0000-0000-000000000000").as_str()]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("no session file found"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn files_via_project_path_target() {
    let h = files_scenario_home();
    let out = h.run(&["files", ENC, "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("/tmp: 2 write"));
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
fn recover_batch_reconstructs_many_files_in_one_scan() {
    let h = Home::new();
    let read_full = |uid: &str, path: &str, content: &str, total: usize| -> String {
        serde_json::json!({
            "type":"user","uuid":uid,"timestamp":"2026-06-07T05:00:00.000Z",
            "toolUseResult":{"file":{"filePath":path,"content":content,"startLine":1,"numLines":total,"totalLines":total}},
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":uid,"content":"ok"}]}
        }).to_string()
    };
    // Session 1 holds two files; a SECOND session holds a third — all recovered in ONE scan.
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!(
            "{}\n{}\n",
            read_full("r0", "/tmp/alpha.md", "# Alpha\nline two\nline three", 3),
            read_full("r1", "/tmp/beta.md", "beta one\nbeta two", 2)
        ),
    );
    let sess2 = "11112222-3333-4444-5555-666677778888";
    h.write(
        &format!("{ENC}/{sess2}.jsonl"),
        &format!(
            "{}\n",
            read_full("r2", "/tmp/gamma.md", "gamma only line", 1)
        ),
    );

    // Manifest: three real targets + a comment + an absent one.
    let manifest = h.root.join("manifest.txt");
    std::fs::write(
        &manifest,
        "/tmp/alpha.md\n/tmp/beta.md\n# a comment\n/tmp/gamma.md\n/tmp/absent.md\n",
    )
    .unwrap();
    let out_dir = h.root.join("recovered");
    let out = h.run(&[
        "recover",
        "--files-from",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);

    // Each present file is reconstructed to its raw content, mirrored under out-dir.
    assert_eq!(
        std::fs::read_to_string(out_dir.join("tmp/alpha.md")).unwrap(),
        "# Alpha\nline two\nline three\n"
    );
    assert_eq!(
        std::fs::read_to_string(out_dir.join("tmp/beta.md")).unwrap(),
        "beta one\nbeta two\n"
    );
    assert_eq!(
        std::fs::read_to_string(out_dir.join("tmp/gamma.md")).unwrap(),
        "gamma only line\n"
    );
    // The absent target writes no file and is reported as no-history.
    assert!(!out_dir.join("tmp/absent.md").exists());
    let report = std::fs::read_to_string(out_dir.join("recovery-report.tsv")).unwrap();
    assert!(
        report.contains("complete\t3\t3\t/tmp/alpha.md"),
        "report:\n{report}"
    );
    assert!(
        report.contains("no-history\t0\t0\t/tmp/absent.md"),
        "report:\n{report}"
    );
    assert!(out.stdout.contains("3 complete"), "summary: {}", out.stdout);

    // Re-running without --force skips the already-present files.
    let out2 = h.run(&[
        "recover",
        "--files-from",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
    ]);
    assert!(out2.success, "stderr: {}", out2.stderr);
    assert!(
        out2.stdout.contains("3 skipped"),
        "skip summary: {}",
        out2.stdout
    );
}

#[test]
fn recover_coverage_counts_and_boundary() {
    let h = recover_scenario_home();
    let out = h.run(&["recover", at(SESS).as_str(), "--file", RFILE, "--coverage"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
    // Two full reads, two edits, one integrity error, one history snapshot.
    assert!(
        out.stdout.contains("2 read (2 full"),
        "read counts: {}",
        out.stdout
    );
    assert!(out.stdout.contains("edit"), "edit count: {}", out.stdout);
    assert!(out.stdout.contains("integrity-error"), "{}", out.stdout);
    assert!(out.stdout.contains("history-snapshot"), "{}", out.stdout);
    // The modified-since-read boundary is AUTHORITATIVE and carries its jsonl line number.
    assert!(
        out.stdout.contains("modified since read"),
        "boundary text: {}",
        out.stdout
    );
    assert!(out.stdout.contains("AUTHORITATIVE"), "{}", out.stdout);
    // Fragments = boundaries + 1 = 2.
    assert!(
        out.stdout.contains("fragments: 2"),
        "fragments: {}",
        out.stdout
    );
    // The malformed line is counted, never hidden.
    assert!(
        out.stdout.contains("malformed line(s) skipped"),
        "{}",
        out.stdout
    );
}

#[test]
fn recover_patches_segments_split_at_boundary_with_line_numbers() {
    let h = recover_scenario_home();
    let out = h.run(&["recover", at(SESS).as_str(), "--file", RFILE, "--patches"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // At least TWO segments, split by the integrity boundary.
    let segs = out.stdout.matches("─ SEGMENT").count();
    assert!(
        segs >= 2,
        "expected ≥2 segments, got {segs}:\n{}",
        out.stdout
    );
    // The boundary divider carries L<line>, the kind, and AUTHORITATIVE confidence.
    assert!(out.stdout.contains("INTEGRITY BOUNDARY"), "{}", out.stdout);
    assert!(
        out.stdout.contains("modified since read") && out.stdout.contains("AUTHORITATIVE"),
        "boundary line: {}",
        out.stdout
    );
    // Every segment header + boundary carries a jsonl line number (Lnnn).
    assert!(
        out.stdout.contains("L"),
        "line numbers present: {}",
        out.stdout
    );
    // The first segment's diff shows the open().read() → with-block refactor.
    assert!(
        out.stdout.contains("-raw = open(src).read()"),
        "diff removed line: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("+with open(src) as fh:"),
        "diff added line: {}",
        out.stdout
    );
}

#[test]
fn recover_modified_since_read_invalidates_stale_lines() {
    // A full Read (5 lines) → an edit blocked by "modified since read" (the file changed
    // underneath, e.g. prettier) → a re-read of only lines 1-2. The pre-boundary lines 3-5 are
    // now STALE and must be INVALIDATED: salvage shows 1-2 + explicit gaps (never the stale
    // CCC/DDD/EEE), and restore must FAIL rather than confidently hand back stale content.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"read"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/x.txt","content":"AAA\nBBB\nCCC\nDDD\nEEE","startLine":1,"numLines":5,"totalLines":5}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ed1","name":"Edit","input":{"file_path":"/p/x.txt","old_string":"CCC","new_string":"ZZZ"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"err1","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ed1","is_error":true,"content":"<tool_use_error>File has been modified since read, either by the user or by a linter. Read it again before attempting to write it.</tool_use_error>"}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T06:00:02.000Z","toolUseResult":{"file":{"filePath":"/p/x.txt","content":"AAA\nBBB","startLine":1,"numLines":2,"totalLines":5}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd1","content":"ok"}]}}"#, "\n",
        ),
    );
    // Salvage: stale CCC/DDD/EEE are dropped; lines 3-5 are explicit gaps.
    let salv = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/x.txt",
        "--salvage",
    ]);
    assert!(salv.success, "stderr: {}", salv.stderr);
    assert!(
        salv.stdout.contains("    1  AAA"),
        "re-read line kept: {}",
        salv.stdout
    );
    assert!(
        salv.stdout.contains("??? lines 3..5 unknown"),
        "stale region invalidated: {}",
        salv.stdout
    );
    for stale in ["CCC", "DDD", "EEE"] {
        assert!(
            !salv.stdout.contains(stale),
            "stale {stale} must not be shown as current: {}",
            salv.stdout
        );
    }
    // Restore: refuses rather than falsely claim "complete" on the invalidated buffer.
    let rest = h.run(&["recover", at(SESS).as_str(), "--file", "/p/x.txt"]);
    assert!(
        !rest.success,
        "restore must fail on the invalidated file: {}",
        rest.stdout
    );
    assert!(
        rest.stderr.contains("recovered 2/5"),
        "honest partial count: {}",
        rest.stderr
    );
    // Smart failure: it lists the external-change boundary…
    assert!(
        rest.stderr.contains("changed OUTSIDE") && rest.stderr.contains("modified_since_read"),
        "lists the external-change boundary: {}",
        rest.stderr
    );
    // …and recognizes the pre-change state was COMPLETELY recoverable (scenario 1), recommending
    // the pre-change dump + patches-since recipe.
    assert!(
        rest.stderr.contains("COMPLETELY recoverable"),
        "surfaces the complete pre-change state: {}",
        rest.stderr
    );
    assert!(
        rest.stderr.contains("--at @line:") && rest.stderr.contains("--patches"),
        "recommends pre-change dump + patches: {}",
        rest.stderr
    );
}

#[test]
fn recover_patches_json_segments_and_boundary_objects() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--patches",
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
    // At least one segment object (carrying unified_diff + line_no + pre_state_known +
    // anchor_source) and one boundary object (carrying line_no + kind + confidence).
    let seg = objs
        .iter()
        .find(|o| o.get("kind").and_then(|v| v.as_str()) == Some("segment"))
        .expect("a segment object");
    assert!(
        seg.get("unified_diff").and_then(|v| v.as_str()).is_some(),
        "{seg}"
    );
    assert!(
        seg.get("line").and_then(|v| v.as_u64()).is_some(),
        "segment line: {seg}"
    );
    assert!(seg.get("pre_state_known").is_some(), "{seg}");
    assert!(seg.get("anchor_source").is_some(), "{seg}");
    let bnd = objs
        .iter()
        .find(|o| o.get("kind").and_then(|v| v.as_str()) == Some("boundary"))
        .expect("a boundary object");
    assert_eq!(
        bnd.get("cause").and_then(|v| v.as_str()),
        Some("modified_since_read")
    );
    assert_eq!(
        bnd.get("confidence").and_then(|v| v.as_str()),
        Some("authoritative")
    );
    assert!(bnd.get("line").and_then(|v| v.as_u64()).is_some(), "{bnd}");
    // Trailing summary.
    assert_eq!(objs.last().unwrap()["kind"], "summary");
}

#[test]
fn recover_coverage_no_boundaries_says_none() {
    // A clean file with only a full Read (no integrity issues) → no boundaries.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/clean.rs","content":"a\nb","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/clean.rs",
        "--coverage",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("integrity boundaries: (none)"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("fragments: 1"), "{}", out.stdout);
}

#[test]
fn recover_patches_heuristic_bash_boundary_is_flagged() {
    // A full Read then a Bash `sed -i` on the same file → a HEURISTIC (soft) boundary,
    // flagged with HEURISTIC confidence (not AUTHORITATIVE).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/h.rs","content":"a\nb","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b0","name":"Bash","input":{"command":"sed -i 's/a/A/' /p/h.rs"}}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/h.rs",
        "--patches",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("INTEGRITY BOUNDARY"), "{}", out.stdout);
    assert!(
        out.stdout.contains("HEURISTIC"),
        "heuristic confidence: {}",
        out.stdout
    );
    assert!(out.stdout.contains("bash"), "bash detail: {}", out.stdout);
}

#[test]
fn recover_real_multi_patch_segmentation_at_modified_since_read() {
    let Some((enc, sess, _)) = real_fixture() else {
        eprintln!("SKIP recover_real_multi_patch_segmentation: real fixture absent");
        return;
    };
    // engine.py shows a real `File has been modified since read` error at jsonl L22980.
    // A --patches run over it must emit ≥2 segments split by an AUTHORITATIVE boundary
    // carrying that line number, and no single diff may span the boundary.
    let engine = "/Users/testuser/Projects/Acme/widget_factory-worktrees/feature-session-7/app/src/app/engine/engine.py";
    let out = run_real(&[
        "recover",
        &enc,
        at(sess).as_str(),
        "--file",
        engine,
        "--patches",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let segs = out.stdout.matches("─ SEGMENT").count();
    assert!(segs >= 2, "expected ≥2 real segments, got {segs}");
    assert!(
        out.stdout.contains("INTEGRITY BOUNDARY") && out.stdout.contains("L22980"),
        "boundary at the real L22980: {}",
        out.stdout
    );
    assert!(out.stdout.contains("modified since read"), "{}", out.stdout);
    assert!(out.stdout.contains("AUTHORITATIVE"), "{}", out.stdout);
}

#[test]
fn recover_history_snapshot_only_session_emits_no_segment_or_boundary() {
    // A session whose ONLY event for the target is a file-history-snapshot marker. The
    // marker is counted but opens no segment and creates no boundary → the
    // `segments.is_empty() && boundaries.is_empty()` skip fires (both text + JSON patches).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"file-history-snapshot","snapshot":{"timestamp":"2026-06-07T05:00:00.500Z","trackedFileBackups":{"/p/snap.rs":{"backupFileName":null,"version":1,"backupTime":"2026-06-07T05:00:00.500Z"}}}}"#, "\n",
        ),
    );
    // Text patches: the snapshot-only session is skipped → honest empty.
    let text = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/snap.rs",
        "--patches",
    ]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(
        text.stdout.contains("no recoverable history"),
        "snapshot-only session yields no patch segments: {}",
        text.stdout
    );
    // JSON patches: only the trailing summary object, zero segment/boundary objects.
    let js = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/snap.rs",
        "--patches",
        "--format",
        "json",
    ]);
    assert!(js.success, "stderr: {}", js.stderr);
    let objs: Vec<serde_json::Value> = js
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("ndjson"))
        .collect();
    assert!(
        objs.iter().all(|o| o.get("type").is_none()),
        "no segment/boundary objects, only the summary: {}",
        js.stdout
    );
    assert_eq!(objs.last().unwrap()["kind"], "summary");
    assert_eq!(
        objs.last().unwrap()["sessions"].as_u64(),
        Some(0),
        "the snapshot-only session contributed no patch output"
    );
}

#[test]
fn recover_coverage_heuristic_boundary_uses_soft_symbol() {
    // A coverage run over a session with a HEURISTIC (bash) boundary drives the coverage
    // renderer's `~` (soft) boundary symbol arm — distinct from the `⚠` authoritative one.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/sb.rs","content":"a\nb","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b0","name":"Bash","input":{"command":"sed -i 's/a/A/' /p/sb.rs"}}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/sb.rs",
        "--coverage",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("integrity boundaries:") && out.stdout.contains("HEURISTIC"),
        "heuristic boundary listed in coverage: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("~ L"),
        "the soft '~' symbol prefixes a heuristic boundary: {}",
        out.stdout
    );
}
