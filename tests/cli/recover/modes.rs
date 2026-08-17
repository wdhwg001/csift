//! recover mode surface: restore default, salvage, conflicts, honest no-history.

use crate::harness::*;

#[test]
fn recover_subagent_input_fallback_skips_failed_edit() {
    // The DANGER case: a SUBAGENT records results as bare tool_result strings (no
    // toolUseResult), so content comes from the input-side fallback. A failed Edit there
    // (is_error:true) must be skipped, not replayed from its input.
    const PSESS: &str = "cccccccc-9999-9999-9999-999999999999";
    let h = Home::new();
    h.write(
        &format!("{ENC}/{PSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"spawn a worker"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"on it"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-deadbeef.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"make g.md then fix it"}}"#, "\n",
            // Write via input fallback (bare success result).
            r#"{"type":"assistant","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:11.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sw","name":"Write","input":{"file_path":"/p/g.md","content":"aa\nbb\ncc\n"}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:11.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"sw","content":"File created successfully at: /p/g.md"}]}}"#, "\n",
            // FAILED edit (is_error) — must NOT be applied from the input.
            r#"{"type":"assistant","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:12.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sbad","name":"Edit","input":{"file_path":"/p/g.md","old_string":"NOPE","new_string":"GHOST"}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:12.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"sbad","content":"String to replace not found in file.","is_error":true}]}}"#, "\n",
            // SUCCESSFUL edit via input fallback (bare success result).
            r#"{"type":"assistant","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:13.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sok","name":"Edit","input":{"file_path":"/p/g.md","old_string":"bb","new_string":"bb-ok"}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:13.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"sok","content":"The file /p/g.md has been updated successfully."}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-deadbeef.meta.json"),
        r#"{"agentType":"general-purpose","description":"worker","toolUseId":"t0"}"#,
    );
    let out = h.run(&[
        "recover",
        at(PSESS).as_str(),
        "--file",
        "/p/g.md",
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("GHOST"),
        "ghost edit leaked: {}",
        out.stdout
    );
    assert_eq!(
        recon_lines_from_at_json(&out.stdout),
        vec!["aa", "bb-ok", "cc"],
        "subagent: only the good edit lands"
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
fn recover_turn_range_alone_is_accepted() {
    // `--turn` WITHOUT --since/--until is valid (drives the `&&` right operand of the
    // mutual-exclusion guard to its false side: turn_range set, since/until both absent).
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--coverage",
        "--turn",
        "0..0",
    ]);
    assert!(
        out.success,
        "a bare --turn is not a conflict: {}",
        out.stderr
    );
    // Turn 0 only → the first segment's reads/edits are in scope; the turn-1 boundary is not.
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
}

#[test]
fn recover_restore_default_returns_raw_full_content() {
    // Default mode (no --salvage/--patches/--at/--coverage) RESTOREs the file's final content
    // as RAW bytes — no SESSION banner, no line numbers, no mode footer — because this session
    // saw the whole file (the post-drift full Read re-establishes all 6 lines).
    let h = recover_scenario_home();
    let out = h.run(&["recover", at(SESS).as_str(), "--file", RFILE]);
    assert!(out.success, "stderr: {}", out.stderr);
    let expected =
        "import os\nwith open(src) as fh:\n    raw = fh.read()\nuse(raw)\nprint(café🛠)\nEOF\n";
    assert_eq!(out.stdout, expected, "raw restored content");
    // No decoration leaks into the restorable bytes.
    for banned in ["SESSION", "mode=", "  1  "] {
        assert!(
            !out.stdout.contains(banned),
            "no {banned} in raw restore: {}",
            out.stdout
        );
    }
}

#[test]
fn recover_restore_out_writes_raw_file_no_stdout() {
    let h = recover_scenario_home();
    let out_path = h.root.join("restored.py");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.is_empty(),
        "restore --out keeps stdout empty (note goes to stderr): {:?}",
        out.stdout
    );
    assert!(
        out.stderr.contains("recovered"),
        "stderr note: {}",
        out.stderr
    );
    let written = std::fs::read_to_string(&out_path).unwrap();
    assert_eq!(
        written,
        "import os\nwith open(src) as fh:\n    raw = fh.read()\nuse(raw)\nprint(café🛠)\nEOF\n"
    );
}

#[test]
fn recover_real_reconstruction_matches_disk_on_contiguous_prefix() {
    let Some((enc, sess, _)) = real_fixture() else {
        eprintln!("SKIP recover_real_reconstruction_matches_disk: real fixture absent");
        return;
    };
    // Reconstruct the plan file from its Read/Edit stream (NOT the whole-plan anchor) and
    // assert the contiguous-from-line-1 KNOWN prefix matches the live on-disk file
    // byte-for-byte. Gaps + post-drift islands are allowed (partial by design); the
    // trustworthy contiguous prefix must never disagree with disk.
    let disk_plan = PathBuf::from(std::env::var_os("HOME").unwrap())
        .join(".claude")
        .join("plans")
        .join("goofy-finding-kettle.md");
    if !disk_plan.is_file() {
        eprintln!("SKIP: on-disk plan file absent");
        return;
    }
    let out = run_real(&[
        "recover",
        &enc,
        at(sess).as_str(),
        "--file",
        disk_plan.to_str().unwrap(),
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // A leading {kind:"header"} scope record may precede the snapshot when the scope
    // spans subagents — find the first snapshot object (the one carrying `lines`), not just
    // the first non-empty line.
    let snap: serde_json::Value = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| v.get("lines").is_some())
        .expect("a snapshot object with a `lines` array");
    let mut known: std::collections::BTreeMap<usize, String> = std::collections::BTreeMap::new();
    for l in snap.get("lines").and_then(|v| v.as_array()).unwrap() {
        let n = l.get("n").and_then(|v| v.as_u64()).unwrap() as usize;
        let t = l.get("text").and_then(|v| v.as_str()).unwrap().to_string();
        known.insert(n, t);
    }
    let disk = std::fs::read_to_string(&disk_plan).unwrap();
    let disk_lines: Vec<&str> = {
        let mut v: Vec<&str> = disk.split('\n').collect();
        if v.last() == Some(&"") {
            v.pop();
        }
        v
    };
    // Walk the contiguous prefix from line 1 and assert each known line matches disk.
    let mut n = 1usize;
    let mut prefix_len = 0usize;
    while let Some(text) = known.get(&n) {
        assert!(
            n <= disk_lines.len(),
            "reconstructed beyond disk length at {n}"
        );
        assert_eq!(
            text,
            disk_lines[n - 1],
            "contiguous-prefix line {n} must match disk"
        );
        prefix_len = n;
        n += 1;
    }
    assert!(
        prefix_len > 50,
        "expected a substantial clean prefix, got {prefix_len}"
    );
}
