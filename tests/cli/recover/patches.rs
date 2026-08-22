//! recover --patches: diff segments, boundary splits, stale-line invalidation.

use crate::harness::*;

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
fn recover_patches_boundary_only_session_still_renders() {
    // A session whose ONLY event is a Bash mutation on the target (no prior read) produces a
    // boundary but NO segment → `segments.is_empty() && boundaries.is_empty()` is
    // `true && false` (the second operand FALSE side), so the session is NOT skipped and the
    // lone boundary is rendered.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b0","name":"Bash","input":{"command":"sed -i 's/a/A/' /p/bo.rs"}}]}}"#, "\n",
        ),
    );
    // Text patches: the boundary is shown even though no segment exists.
    let text = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/bo.rs",
        "--patches",
    ]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(
        text.stdout.contains("INTEGRITY BOUNDARY") && text.stdout.contains("HEURISTIC"),
        "the lone boundary renders without a segment: {}",
        text.stdout
    );
    // JSON patches: a boundary object is emitted (same non-skip second-operand path).
    let js = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/bo.rs",
        "--patches",
        "--format",
        "json",
    ]);
    assert!(js.success, "stderr: {}", js.stderr);
    let has_boundary = js
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .any(|o| o.get("kind").and_then(|v| v.as_str()) == Some("boundary"));
    assert!(has_boundary, "a boundary object is emitted: {}", js.stdout);
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

#[test]
fn recover_patches_via_project_path_target() {
    // Drive recover by a PROJECT PATH (encoded dir) instead of --session, exercising the
    // multi-session merge + sort path.
    let h = recover_scenario_home();
    let out = h.run(&["recover", ENC, "--file", RFILE, "--coverage"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("recoverable"), "{}", out.stdout);
}

#[test]
fn recover_patches_no_history_says_so() {
    // `--patches` against a file with no recoverable events → the `!any` honest-empty arm
    // of the patches text renderer (distinct from the coverage-mode `!any` already tested).
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/no/such.rs",
        "--patches",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no recoverable history"),
        "patches honest-empty: {}",
        out.stdout
    );
}

#[test]
fn recover_patches_json_out_writes_concatenated_diffs() {
    // The patches-mode JSON renderer's `--out` arm writes the concatenated diff blob.
    let h = recover_scenario_home();
    let out_path = h.root.join("patches-json.diff");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--patches",
        "--format",
        "json",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let blob = std::fs::read_to_string(&out_path).expect("patches JSON --out artifact");
    assert!(
        blob.contains("@@ -") && blob.contains("+with open(src) as fh:"),
        "concatenated diff blob from the JSON renderer: {blob}"
    );
}

#[test]
fn recover_json_patches_skips_empty_event_session() {
    // JSON patches mode skip: a target with no events in the (only) session → no segment or
    // boundary objects, summary.sessions == 0 (the `s.events.is_empty()` JSON-patches arm).
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/no/such.rs",
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
    assert!(
        objs.iter().all(|o| o.get("type").is_none()),
        "no segment/boundary objects for a no-event target: {}",
        out.stdout
    );
    assert_eq!(objs.last().unwrap()["sessions"].as_u64(), Some(0));
}

#[test]
fn patches_interleaves_segments_and_boundaries_in_stream_order() {
    // Write -> bash touch (soft boundary closes segment 1) -> Edit (segment 2):
    // the text stream must read SEGMENT 1, then the boundary, then SEGMENT 2.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"work"}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/p","toolUseResult":{"type":"create","filePath":"/p/seq.md","content":"one\ntwo\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:00:02.000Z","cwd":"/p","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"sed -i 's/one/uno/' seq.md"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c2","timestamp":"2026-06-07T05:00:03.000Z","cwd":"/p","toolUseResult":{"filePath":"/p/seq.md","oldString":"two","newString":"dos"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/seq.md",
        "--patches",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let seg1 = out.stdout.find("SEGMENT 1").expect("segment 1");
    let bound = out
        .stdout
        .find("INTEGRITY BOUNDARY")
        .expect("boundary line");
    let seg2 = out.stdout.find("SEGMENT 2").expect("segment 2");
    assert!(
        seg1 < bound && bound < seg2,
        "stream order seg1 < boundary < seg2: {}",
        out.stdout
    );

    // The JSON twin counts exactly one contributing session in its summary.
    let json = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/seq.md",
        "--patches",
        "--no-subagents",
        "--format",
        "json",
    ]);
    let summary: serde_json::Value =
        serde_json::from_str(json.stdout.lines().last().unwrap()).unwrap();
    assert_eq!(summary["sessions"], 1);
}
