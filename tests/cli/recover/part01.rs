use crate::harness::*;

#[test]
fn recover_coverage_out_is_noop_with_stderr_note() {
    // `--out` is a no-op in --coverage mode: no file is written, and a stderr note makes the
    // no-op visible (the help truth-up for r6).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/p/app.rs","content":"line\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","toolUseResult":{"type":"create","filePath":"/p/app.rs"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#, "\n",
        ),
    );
    let out_path = h.root.join("cov-out.md");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/app.rs",
        "--coverage",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("--out is ignored in --coverage mode"),
        "missing the no-op note: {}",
        out.stderr
    );
    assert!(
        !out_path.exists(),
        "coverage --out must not create a file, but it did"
    );
}

#[test]
fn recover_batch_requires_out_dir_and_excludes_file() {
    let h = recover_scenario_home();
    let manifest = h.root.join("m.txt");
    std::fs::write(&manifest, "/tmp/x.md\n").unwrap();
    let no_out = h.run(&["recover", "--files-from", manifest.to_str().unwrap()]);
    assert!(!no_out.success);
    assert!(no_out.stderr.contains("--out-dir"), "{}", no_out.stderr);
    let both = h.run(&[
        "recover",
        "--files-from",
        manifest.to_str().unwrap(),
        "--out-dir",
        h.root.join("o").to_str().unwrap(),
        "--file",
        "/tmp/x.md",
    ]);
    assert!(!both.success);
    assert!(
        both.stderr.contains("mutually exclusive"),
        "{}",
        both.stderr
    );
}

#[test]
fn recover_at_snapshot_has_line_numbers_and_no_fabrication() {
    let h = recover_scenario_home();
    // As of @turn:0 (before the post-drift re-read), the file is the 4-line original with
    // the line-2 edit applied → 5 lines, all known, line-numbered.
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--at",
        "@turn:0",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Line-numbered known content.
    assert!(out.stdout.contains("import os"), "{}", out.stdout);
    assert!(
        out.stdout.contains("with open(src) as fh:"),
        "edit applied: {}",
        out.stdout
    );
    // The café🛠 line round-trips UTF-8 verbatim (locale-neutral multi-byte).
    assert!(
        out.stdout.contains("café🛠"),
        "utf-8 verbatim: {}",
        out.stdout
    );
}

#[test]
fn recover_at_partial_read_marks_explicit_gaps() {
    // A separate session that ONLY windowed-reads a slice of a file → explicit gaps.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"look at the spec"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/spec.md","content":"line5\nline6\nline7","startLine":5,"numLines":3,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/spec.md",
        "--at",
        "@line:9999",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Known lines 5-7 are numbered; lines 1-4 and 8-10 are EXPLICIT gaps, never fabricated.
    assert!(
        out.stdout.contains("??? lines 1..4 unknown"),
        "leading gap: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("    5  line5"),
        "numbered known line: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("??? lines 8..10 unknown"),
        "trailing gap: {}",
        out.stdout
    );
}

#[test]
fn recover_json_every_object_has_line_no_and_local_ts() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--coverage",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    // First object is the coverage object; it carries covered_ranges + boundaries (each
    // boundary has line_no + ts_utc + ts_local).
    let cov: serde_json::Value = serde_json::from_str(lines[1]).expect("ndjson parses");
    assert!(cov.get("covered_ranges").is_some(), "{cov}");
    let bounds = cov
        .get("boundaries")
        .and_then(|b| b.as_array())
        .expect("boundaries array");
    assert!(!bounds.is_empty(), "≥1 boundary");
    let b0 = &bounds[0];
    assert!(
        b0.get("line").and_then(|v| v.as_u64()).is_some(),
        "boundary line: {b0}"
    );
    assert!(
        b0.get("ts_utc").is_some() && b0.get("ts_local").is_some(),
        "boundary ts: {b0}"
    );
    assert_eq!(
        b0.get("cause").and_then(|v| v.as_str()),
        Some("modified_since_read")
    );
    // Trailing summary line.
    let summary: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert_eq!(summary["kind"], "summary", "trailing summary: {summary}");
}

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
fn recover_two_modes_conflict() {
    let h = Home::new();
    let out = h.run(&["recover", ".", "--file", RFILE, "--coverage", "--patches"]);
    assert!(
        !out.success,
        "two modes must be a clap conflict: {}",
        out.stdout
    );
}

#[test]
fn recover_turn_range_intersects_with_time_window() {
    // --turn ∧ --since/--until intersect (both filters AND); a window that
    // excludes everything still succeeds with an honest empty reconstruction.
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--coverage",
        "--turn",
        "0..99",
        "--until",
        "2027-01-01",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("recoverable"),
        "coverage renders under the intersection: {}",
        out.stdout
    );
}

#[test]
fn recover_file_required_for_all_modes() {
    let h = recover_scenario_home();
    // Every mode (patches / at / coverage) requires --file → each bails without it.
    let at_sess = at(SESS);
    for mode in [
        vec!["recover", at_sess.as_str(), "--patches"],
        vec!["recover", at_sess.as_str(), "--at", "@turn:0"],
        vec!["recover", at_sess.as_str(), "--coverage"],
    ] {
        let no_file = h.run(&mode);
        assert!(!no_file.success, "{mode:?} must bail without --file");
        assert!(
            no_file.stderr.contains("--file") && no_file.stderr.contains("required"),
            "{mode:?} file-required bail: {}",
            no_file.stderr
        );
    }
}

#[test]
fn recover_dry_run_alias_works() {
    let h = recover_scenario_home();
    let out = h.run(&["recover", at(SESS).as_str(), "--file", RFILE, "--dry-run"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("recoverable"),
        "coverage via --dry-run alias: {}",
        out.stdout
    );
}

#[test]
fn recover_line_range_restricts_output() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--at",
        "@line:9999",
        "--file-lines",
        "1..2",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Only lines 1-2 are shown; line 5 (EOF) is outside the range and absent.
    assert!(out.stdout.contains("import os"), "{}", out.stdout);
    assert!(
        !out.stdout.contains("    6  EOF"),
        "line 6 outside range: {}",
        out.stdout
    );
}

#[test]
fn recover_help_mentions_modes() {
    let h = Home::new();
    let out = h.run(&["recover", "--help"]);
    assert!(out.success);
    // All five modes (default restore + the four explicit flags) and their semantics are
    // documented, including the salvage fallback the restore-fail message points at.
    for needle in [
        "--salvage",
        "--patches",
        "--at",
        "--coverage",
        "restore",
        "Segmented unified-diff",
        "Best-effort",
        "partial snapshot",
    ] {
        assert!(
            out.stdout.contains(needle),
            "help missing {needle}:\n{}",
            out.stdout
        );
    }
}

#[test]
fn recover_no_history_says_so() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/no/such/file.rs",
        "--coverage",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no recoverable history"),
        "honest empty result: {}",
        out.stdout
    );
}

#[test]
fn recover_restore_partial_file_errors_pointing_to_salvage() {
    // The session only WINDOW-read lines 5-7 of a 10-line file. Default restore must FAIL
    // (never a holey file), name what it can/can't recover, and point at --salvage.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"look"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/spec.md","content":"line5\nline6\nline7","startLine":5,"numLines":3,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["recover", at(SESS).as_str(), "--file", "/p/spec.md"]);
    assert!(
        !out.success,
        "partial restore must fail: stdout={}",
        out.stdout
    );
    assert!(
        out.stdout.is_empty(),
        "no holey file on stdout: {:?}",
        out.stdout
    );
    assert!(
        out.stderr.contains("recovered 3/10"),
        "names recoverable count: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("[5-7]"),
        "covered range: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("1-4") && out.stderr.contains("8-10"),
        "missing ranges: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("--salvage"),
        "points at --salvage: {}",
        out.stderr
    );
    // No external-change boundary here (just an incomplete read) — so no boundary list.
    assert!(
        !out.stderr.contains("changed OUTSIDE"),
        "no boundary list when there was no external change: {}",
        out.stderr
    );
    // …but the hidden-change caveat fires even without a boundary.
    assert!(
        out.stderr.contains("does not hunt for hidden changes"),
        "caveat present even with no boundary: {}",
        out.stderr
    );
}

#[test]
fn recover_restore_json_emits_single_complete_object_no_trailer() {
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    // envelope v2: header + the single kind:"restore" row + summary.
    assert_eq!(lines.len(), 3, "header + restore + summary: {}", out.stdout);
    let v = json_rows(&out.stdout, "restore").remove(0);
    assert_eq!(v["file"], RFILE);
    assert_eq!(v["complete"], serde_json::Value::Bool(true));
    assert_eq!(v["lines"], serde_json::json!(6));
    assert!(
        v["content"]
            .as_str()
            .unwrap()
            .contains("with open(src) as fh:"),
        "content carries the edited line: {}",
        out.stdout
    );
    json_summary(&out.stdout);
}

#[test]
fn recover_salvage_dumps_surviving_fragment_with_gaps() {
    // --salvage is restore's never-fails sibling: a windowed-only session yields the surviving
    // lines (numbered) with the rest as explicit gaps, never an error.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"look"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/spec.md","content":"line5\nline6\nline7","startLine":5,"numLines":3,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/spec.md",
        "--salvage",
    ]);
    assert!(out.success, "salvage never fails: {}", out.stderr);
    assert!(
        out.stdout.contains("??? lines 1..4 unknown"),
        "leading gap: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("    5  line5"),
        "numbered survivor: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("??? lines 8..10 unknown"),
        "trailing gap: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("mode=salvage"),
        "salvage footer: {}",
        out.stdout
    );
}

#[test]
fn recover_restore_surfaces_fuller_pre_change_partial_state() {
    // Scenario 2: a file NOT authored here — windowed-read lines 1-8 of a 10-line file, then a
    // modified-since-read boundary, then re-read only lines 1-2. Latest is 2/10; but BEFORE the
    // change 8/10 survives (fuller, still partial). Restore surfaces that + a snapshot-as-of recipe.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"read"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/big.txt","content":"L1\nL2\nL3\nL4\nL5\nL6\nL7\nL8","startLine":1,"numLines":8,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ed1","name":"Edit","input":{"file_path":"/p/big.txt","old_string":"L1","new_string":"X1"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"err1","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ed1","is_error":true,"content":"<tool_use_error>File has been modified since read, either by the user or by a linter. Read it again before attempting to write it.</tool_use_error>"}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T06:00:02.000Z","toolUseResult":{"file":{"filePath":"/p/big.txt","content":"L1\nL2","startLine":1,"numLines":2,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd1","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["recover", at(SESS).as_str(), "--file", "/p/big.txt"]);
    assert!(!out.success, "partial restore fails: {}", out.stdout);
    assert!(
        out.stderr.contains("recovered 2/10"),
        "latest count: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("MORE survives") && out.stderr.contains("8/10"),
        "fuller (still partial) pre-change state surfaced: {}",
        out.stderr
    );
    // The recommended pre-change dump is `--at @line:N` (NOT `--salvage --at`, which would be a
    // mutually-exclusive-mode parse error).
    assert!(
        out.stderr.contains("--at @line:") && !out.stderr.contains("--salvage --at"),
        "recommends a valid snapshot-as-of command: {}",
        out.stderr
    );
}

#[test]
fn recover_patches_out_writes_concatenated_diffs() {
    let h = recover_scenario_home();
    let out_path = h.root.join("patches.diff");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--patches",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("wrote concatenated patches"),
        "{}",
        out.stdout
    );
    let blob = std::fs::read_to_string(&out_path).expect("patches file");
    assert!(
        blob.contains("@@ -") && blob.contains("+with open(src) as fh:"),
        "diff blob: {blob}"
    );
}
