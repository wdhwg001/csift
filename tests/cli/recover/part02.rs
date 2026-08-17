use crate::harness::*;

#[test]
fn recover_at_out_writes_partial_snapshot_with_gaps() {
    // A windowed-read-only file → the --out artifact carries explicit gap markers.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"peek"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/spec.md","content":"l3\nl4","startLine":3,"numLines":2,"totalLines":8}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out_path = h.root.join("snap.txt");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/spec.md",
        "--at",
        "@line:9999",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("wrote partial snapshot"),
        "{}",
        out.stdout
    );
    let body = std::fs::read_to_string(&out_path).expect("snapshot file");
    assert!(
        body.contains("??? lines 1..2 unknown"),
        "leading gap in artifact: {body}"
    );
    assert!(body.contains("l3"), "known content in artifact: {body}");
    assert!(
        body.contains("??? lines 5..8 unknown"),
        "trailing gap in artifact: {body}"
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
fn recover_coverage_reports_unanchorable_holes() {
    // Drives the `if rep.counts.edit_unanchorable > 0` coverage-hole line in the text
    // renderer + the windowed-read count display.
    let h = recover_hole_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/big.rs",
        "--coverage",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("un-anchorable edits (coverage holes): 1"),
        "coverage hole reported: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("windowed"),
        "windowed read counted: {}",
        out.stdout
    );
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
fn recover_at_no_history_says_so() {
    // `--at` against a file with no events → the at-mode `!any` honest-empty arm.
    let h = recover_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/no/such.rs",
        "--at",
        "@line:5",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no recoverable history"),
        "at honest-empty: {}",
        out.stdout
    );
}

/// Cross-session interleaved reconstruction: a file is created by the top-level session,
/// then edited by it AND by two of its subagents, interleaved by wall-clock, with only
/// PARTIAL reads anywhere (no single transcript ever holds the whole file, and each
/// subagent's own buffer is too sparse to anchor its string edits). The union of all
/// transcripts' edits IS the complete file. `--at` must merge the parent+subagents into one
/// timestamp-ordered timeline and reconstruct EVERY line — including the subagents' own
/// edits, which are un-anchorable in their isolated transcripts. NO external-edit
/// attachments are present, so this isolates the merge (not Claude Code's edited_text_file
/// reconciliation). The 8-line file ends in a newline → the trailing-newline normalisation
/// must keep seen_total at 8 (no phantom `line 9 unknown`).
#[test]
fn recover_at_merges_interleaved_cross_session_edits() {
    const MSESS: &str = "cccccccc-5555-5555-5555-555555555555";
    let h = Home::new();
    // ── Top-level: Write the 8-line file, then (after subagent A) one structured-patch edit.
    h.write(
        &format!("{ENC}/{MSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"build doc.md"}}"#, "\n",
            // Write create result carries the full content (as real Claude Code does).
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w0","name":"Write","input":{"file_path":"/p/doc.md","content":"A1\nA2\nA3\nA4\nA5\nA6\nA7\nA8\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:01.500Z","toolUseResult":{"type":"create","filePath":"/p/doc.md","content":"A1\nA2\nA3\nA4\nA5\nA6\nA7\nA8\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w0","content":"ok"}]}}"#, "\n",
            // A PARTIAL read of lines 1-4 (after subagent A's edits land on disk). totalLines
            // is the SEPARATOR count (9) of the newline-terminated 8-line file.
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T05:00:20.000Z","toolUseResult":{"file":{"filePath":"/p/doc.md","content":"A1\nA2\nA3-suba\nA4","startLine":1,"numLines":4,"totalLines":9}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd1","content":"ok"}]}}"#, "\n",
            // Top-level structured-patch edit: A1 → A1-main.
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:21.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e_main","name":"Edit","input":{"file_path":"/p/doc.md","old_string":"A1","new_string":"A1-main"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:00:21.500Z","toolUseResult":{"filePath":"/p/doc.md","oldString":"A1","newString":"A1-main","originalFile":null,"replaceAll":false,"structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-A1","+A1-main"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e_main","content":"ok"}]}}"#, "\n",
        ),
    );
    // ── Subagent A: partial-read lines 3-6, then two STRING edits (bare tool_result, NO
    //    toolUseResult → the input-side fallback supplies content). In isolation its buffer
    //    (lines 3-6) is too sparse to anchor a string edit; only the merge (with the parent's
    //    Write anchor in scope) makes these edits land.
    h.write(
        &format!("{ENC}/{MSESS}/subagents/agent-aaaaaa.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aaaaaa","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"edit lines 3 and 6"}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"aaaaaa","timestamp":"2026-06-07T05:00:10.500Z","toolUseResult":{"file":{"filePath":"/p/doc.md","content":"A3\nA4\nA5\nA6","startLine":3,"numLines":4,"totalLines":9}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rda","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","isSidechain":true,"agentId":"aaaaaa","timestamp":"2026-06-07T05:00:11.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ea1","name":"Edit","input":{"file_path":"/p/doc.md","old_string":"A3","new_string":"A3-suba","replace_all":false}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"aaaaaa","timestamp":"2026-06-07T05:00:11.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ea1","content":"The file /p/doc.md has been updated successfully."}]}}"#, "\n",
            r#"{"type":"assistant","isSidechain":true,"agentId":"aaaaaa","timestamp":"2026-06-07T05:00:12.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ea2","name":"Edit","input":{"file_path":"/p/doc.md","old_string":"A6","new_string":"A6-suba","replace_all":false}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"aaaaaa","timestamp":"2026-06-07T05:00:12.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ea2","content":"The file /p/doc.md has been updated successfully."}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{MSESS}/subagents/agent-aaaaaa.meta.json"),
        r#"{"agentType":"general-purpose","description":"edit A","toolUseId":"t_a"}"#,
    );
    // ── Subagent B: partial-read lines 5-8, then one STRING edit A8 → A8-subb (latest).
    h.write(
        &format!("{ENC}/{MSESS}/subagents/agent-bbbbbb.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"bbbbbb","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"user","content":"edit line 8"}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"bbbbbb","timestamp":"2026-06-07T05:00:30.500Z","toolUseResult":{"file":{"filePath":"/p/doc.md","content":"A5\nA6-suba\nA7\nA8","startLine":5,"numLines":4,"totalLines":9}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rdb","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","isSidechain":true,"agentId":"bbbbbb","timestamp":"2026-06-07T05:00:31.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"eb1","name":"Edit","input":{"file_path":"/p/doc.md","old_string":"A8","new_string":"A8-subb","replace_all":false}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"bbbbbb","timestamp":"2026-06-07T05:00:31.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"eb1","content":"The file /p/doc.md has been updated successfully."}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{MSESS}/subagents/agent-bbbbbb.meta.json"),
        r#"{"agentType":"general-purpose","description":"edit B","toolUseId":"t_b"}"#,
    );

    let out = h.run(&[
        "recover",
        at(MSESS).as_str(),
        "--file",
        "/p/doc.md",
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Exactly ONE merged snapshot object (the parent+subagents folded into one timeline),
    // not three per-transcript fragments.
    let snaps: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .filter(|v| v.get("kind").and_then(|t| t.as_str()) == Some("snapshot"))
        .collect();
    assert_eq!(
        snaps.len(),
        1,
        "one merged snapshot, got {}: {}",
        snaps.len(),
        out.stdout
    );
    let snap = &snaps[0];
    let mut recon: std::collections::BTreeMap<usize, String> = std::collections::BTreeMap::new();
    for l in snap.get("lines").and_then(|v| v.as_array()).unwrap() {
        recon.insert(
            l.get("n").and_then(|v| v.as_u64()).unwrap() as usize,
            l.get("text").and_then(|v| v.as_str()).unwrap().to_string(),
        );
    }
    let got: Vec<&str> = recon.values().map(String::as_str).collect();
    // The COMPLETE interleaved file: parent's Write + parent's edit + BOTH subagents' edits.
    assert_eq!(
        got,
        vec!["A1-main", "A2", "A3-suba", "A4", "A5", "A6-suba", "A7", "A8-subb"],
        "merged reconstruction must carry every cross-session edit: {snap}"
    );
    // No phantom trailing gap (the file ends in a newline; seen_total must be 8, not 9).
    assert_eq!(
        snap.get("seen_total_lines").and_then(|v| v.as_u64()),
        Some(8),
        "trailing-newline normalisation keeps seen_total at 8: {snap}"
    );
    assert!(
        snap.get("gaps")
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(false),
        "a fully-recovered file has no gaps: {snap}"
    );
}

/// `--at <datetime>` is time-travel: the same transcript reconstructs a DIFFERENT state of
/// the file depending on the wall-clock instant asked for. This is the canonical, efficient
/// way to ask "what did this plan/file look like at 8pm last night" — far better than hunting
/// line numbers. Three cutoffs across one Write + two timestamped edits must each land on the
/// exact intervening state: original / after-edit-1 / after-edit-2. Exercises the
/// `resolve_cutoff` datetime branch (TimeWindow until-bound) end-to-end through reconstruction,
/// and accepts both an absolute RFC3339 instant and a bare date.
#[test]
fn recover_at_datetime_time_travels_across_edits() {
    const TSESS: &str = "dddddddd-7777-7777-7777-777777777777";
    let h = Home::new();
    h.write(
        &format!("{ENC}/{TSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"draft plan.md"}}"#, "\n",
            // 05:00 — Write the 3-line original.
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w0","name":"Write","input":{"file_path":"/p/plan.md","content":"L1\nL2\nL3\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:01.500Z","toolUseResult":{"type":"create","filePath":"/p/plan.md","content":"L1\nL2\nL3\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w0","content":"ok"}]}}"#, "\n",
            // 08:00 — edit L2.
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T08:00:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/p/plan.md","old_string":"L2","new_string":"L2-edited"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T08:00:00.500Z","toolUseResult":{"filePath":"/p/plan.md","oldString":"L2","newString":"L2-edited","originalFile":null,"replaceAll":false,"structuredPatch":[{"oldStart":2,"oldLines":1,"newStart":2,"newLines":1,"lines":["-L2","+L2-edited"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#, "\n",
            // 11:00 — edit L3.
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T11:00:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e2","name":"Edit","input":{"file_path":"/p/plan.md","old_string":"L3","new_string":"L3-edited"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c2","timestamp":"2026-06-07T11:00:00.500Z","toolUseResult":{"filePath":"/p/plan.md","oldString":"L3","newString":"L3-edited","originalFile":null,"replaceAll":false,"structuredPatch":[{"oldStart":3,"oldLines":1,"newStart":3,"newLines":1,"lines":["-L3","+L3-edited"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e2","content":"ok"}]}}"#, "\n",
        ),
    );

    // Reconstruct the file as of `when` and return its lines in order.
    let recon_at = |when: &str| -> Vec<String> {
        let out = h.run(&[
            "recover",
            at(TSESS).as_str(),
            "--file",
            "/p/plan.md",
            "--at",
            when,
            "--format",
            "json",
        ]);
        assert!(out.success, "--at {when:?} stderr: {}", out.stderr);
        let snap = out
            .stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
            .find(|v| v.get("kind").and_then(|t| t.as_str()) == Some("snapshot"))
            .unwrap_or_else(|| panic!("--at {when:?} produced no snapshot: {}", out.stdout));
        let mut recon: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        for l in snap.get("lines").and_then(|v| v.as_array()).unwrap() {
            recon.insert(
                l.get("n").and_then(|v| v.as_u64()).unwrap() as usize,
                l.get("text").and_then(|v| v.as_str()).unwrap().to_string(),
            );
        }
        recon.into_values().collect()
    };

    // 06:00 — after the Write, before any edit → the pristine original.
    assert_eq!(
        recon_at("2026-06-07T06:00:00Z"),
        vec!["L1", "L2", "L3"],
        "as of 06:00 only the Write has happened"
    );
    // 09:00 — between the two edits → L2 edited, L3 still original. THE headline time-travel.
    assert_eq!(
        recon_at("2026-06-07T09:00:00Z"),
        vec!["L1", "L2-edited", "L3"],
        "as of 09:00 the L2 edit is applied but the 11:00 L3 edit is not"
    );
    // 12:00 (and a bare date covering the whole day) — after both edits → fully edited.
    assert_eq!(
        recon_at("2026-06-07T12:00:00Z"),
        vec!["L1", "L2-edited", "L3-edited"],
        "as of 12:00 both edits are applied"
    );
    // A bare date is accepted too (resolves to system-local midnight); pick one several days
    // past the edits so no timezone shifts the bound before them → both edits applied.
    assert_eq!(
        recon_at("2026-06-15"),
        vec!["L1", "L2-edited", "L3-edited"],
        "a bare date is a valid --at bound → both edits applied"
    );
}

#[test]
fn recover_at_skips_session_with_no_seen_total() {
    // Two sessions under one project: one reads /p/seen.rs, the other only reads a DIFFERENT
    // file. For the target /p/seen.rs, the second session has empty known + no seen_total →
    // the `known.is_empty() && seen_total_lines.is_none()` continue fires, so only ONE
    // session snapshot is emitted (the other is honestly skipped, not shown as empty).
    let h = Home::new();
    let sess_a = "aaaaaaaa-3333-3333-3333-333333333333";
    let sess_b = "bbbbbbbb-4444-4444-4444-444444444444";
    h.write(
        &format!("{ENC}/{sess_a}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/seen.rs","content":"x\ny","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{sess_b}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/other.rs","content":"q","startLine":1,"numLines":1,"totalLines":1}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["recover", ENC, "--file", "/p/seen.rs", "--at", "@line:9999"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert_eq!(
        out.stdout.matches("SESSION").count(),
        1,
        "only the session that actually touched /p/seen.rs is shown: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("x"),
        "the seen content is rendered: {}",
        out.stdout
    );
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
fn recover_at_text_skips_session_with_events_but_no_known_content() {
    // Text at-mode: the session HAS an event (the edit, so it passes the events-empty
    // guard) but reconstructs to nothing known + no seen_total → the
    // `known.is_empty() && seen_total.is_none()` continue fires → honest "no history".
    let h = recover_empty_reconstruction_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/e.rs",
        "--at",
        "@line:9999",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no recoverable history"),
        "an all-un-anchorable session shows nothing reconstructable: {}",
        out.stdout
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
fn recover_coverage_zero_seen_total_reports_zero_percent() {
    // A coverage run where the reconstruction is empty (un-anchorable edit only) → known 0,
    // seen_total 0 → the `if total > 0` percent guard takes its FALSE side (0%), and the
    // covered-ranges line shows "(none)".
    let h = recover_empty_reconstruction_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/e.rs",
        "--coverage",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("0/0 lines (0%)"),
        "zero-total recoverable reports 0%: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("covered line ranges: (none)"),
        "no covered ranges: {}",
        out.stdout
    );
    // The un-anchorable edit is surfaced as a coverage hole.
    assert!(
        out.stdout
            .contains("un-anchorable edits (coverage holes): 1"),
        "{}",
        out.stdout
    );
}
