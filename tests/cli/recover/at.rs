//! recover --at: point-in-time snapshots, gaps, time travel, skips.

use crate::harness::*;

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
fn recover_at_empty_when_spec_omits_cutoff_line() {
    // `--at ""` (an explicit empty cutoff spec) → `resolve_cutoff` returns None → the
    // `if let Some(c) = cutoff` FALSE side: the snapshot renders WITHOUT an "as of:" line.
    let h = recover_scenario_home();
    let out = h.run(&["recover", at(SESS).as_str(), "--file", RFILE, "--at", ""]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("as of: jsonl line"),
        "an empty cutoff omits the 'as of' line: {}",
        out.stdout
    );
    // It still renders the fully-replayed snapshot (no cutoff → everything).
    assert!(out.stdout.contains("import os"), "{}", out.stdout);
}

#[test]
fn recover_at_line_range_outside_known_keeps_seen_total() {
    // A windowed read sets seen_total_lines, but a `--line-range` that selects NO known line
    // leaves `known` empty while seen_total is still Some → the
    // `known.is_empty() && seen_total.is_none()` check has its SECOND operand FALSE (the
    // session is NOT skipped; it renders an all-gap snapshot up to the seen total).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            // A windowed read of lines 5-6 (seen_total 10).
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/lr.rs","content":"l5\nl6","startLine":5,"numLines":2,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    // Restrict to lines 1-2 — OUTSIDE the known 5-6 window → known empties, seen_total stays.
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/lr.rs",
        "--at",
        "@line:9999",
        "--file-lines",
        "1..2",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The session is rendered (not skipped) because seen_total is Some → an explicit gap.
    assert!(
        out.stdout.contains("SESSION") && out.stdout.contains("unknown"),
        "an out-of-range line filter still renders the session as explicit gaps: {}",
        out.stdout
    );
}

#[test]
fn recover_at_skips_failed_string_not_found_edit_top_level() {
    // Top-level: Write 3 lines, a FAILED Edit (is_error:true, no toolUseResult carrier — so
    // its id is absent from ids_with_result and the input-side fallback would otherwise apply
    // the ghost), then a SUCCESSFUL Edit. The ghost must be absent; the good edit applied.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"edit f.md"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w0","name":"Write","input":{"file_path":"/p/f.md","content":"L1\nL2\nL3\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:01.500Z","toolUseResult":{"type":"create","filePath":"/p/f.md","content":"L1\nL2\nL3\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w0","content":"ok"}]}}"#, "\n",
            // FAILED edit — old_string not in the file. No toolUseResult; tool_result is_error.
            r#"{"type":"assistant","uuid":"a_bad","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e_bad","name":"Edit","input":{"file_path":"/p/f.md","old_string":"NONEXISTENT","new_string":"GHOST"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c_bad","timestamp":"2026-06-07T05:00:02.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e_bad","content":"String to replace not found in file.","is_error":true}]}}"#, "\n",
            // SUCCESSFUL edit — carrier with structuredPatch.
            r#"{"type":"assistant","uuid":"a_ok","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e_ok","name":"Edit","input":{"file_path":"/p/f.md","old_string":"L2","new_string":"L2-ok"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c_ok","timestamp":"2026-06-07T05:00:03.500Z","toolUseResult":{"filePath":"/p/f.md","oldString":"L2","newString":"L2-ok","originalFile":null,"replaceAll":false,"structuredPatch":[{"oldStart":2,"oldLines":1,"newStart":2,"newLines":1,"lines":["-L2","+L2-ok"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e_ok","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/f.md",
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("GHOST"),
        "the failed edit's new_string must never appear: {}",
        out.stdout
    );
    assert_eq!(
        recon_lines_from_at_json(&out.stdout),
        vec!["L1", "L2-ok", "L3"],
        "only the successful edit lands"
    );
}
