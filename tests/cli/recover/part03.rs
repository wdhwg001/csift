use crate::harness::*;

#[test]
fn recover_json_coverage_skips_empty_event_session() {
    // JSON coverage mode: one session touches the target, another touches a different file.
    // The non-touching session is skipped (`s.events.is_empty()` true in the JSON branch),
    // so exactly one coverage object precedes the summary, and summary.sessions == 1.
    let h = Home::new();
    let sess_a = "aaaaaaaa-5555-5555-5555-555555555555";
    let sess_b = "bbbbbbbb-6666-6666-6666-666666666666";
    h.write(
        &format!("{ENC}/{sess_a}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/t.rs","content":"a\nb","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{sess_b}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/elsewhere.rs","content":"q","startLine":1,"numLines":1,"totalLines":1}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        ENC,
        "--file",
        "/p/t.rs",
        "--coverage",
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
    let cov_objs = objs
        .iter()
        .filter(|o| o.get("recoverable_lines").is_some())
        .count();
    assert_eq!(
        cov_objs, 1,
        "only the touching session yields a coverage object"
    );
    assert_eq!(
        objs.last().unwrap()["sessions"].as_u64(),
        Some(1),
        "the non-touching session was skipped, not emitted"
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
fn recover_json_at_skips_session_with_no_seen_total() {
    // JSON at-mode skip: same two-session shape as the text variant, but `--format json`,
    // driving the `known.is_empty() && seen_total.is_none()` continue in the JSON renderer.
    let h = Home::new();
    let sess_a = "aaaaaaaa-7777-7777-7777-777777777777";
    let sess_b = "bbbbbbbb-8888-8888-8888-888888888888";
    h.write(
        &format!("{ENC}/{sess_a}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/here.rs","content":"x\ny","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{sess_b}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/elsewhere.rs","content":"q","startLine":1,"numLines":1,"totalLines":1}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        ENC,
        "--file",
        "/p/here.rs",
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
    let snaps = objs
        .iter()
        .filter(|o| o.get("kind").and_then(|v| v.as_str()) == Some("snapshot"))
        .count();
    assert_eq!(
        snaps, 1,
        "only the session that saw /p/here.rs emits a snapshot"
    );
    assert_eq!(objs.last().unwrap()["sessions"].as_u64(), Some(1));
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
fn recover_at_json_line_range_outside_known_keeps_seen_total() {
    // The JSON at-mode twin of the text test: a windowed read sets seen_total, but a
    // `--line-range` selecting no known line empties `known` while seen_total stays Some →
    // the JSON renderer's `known.is_empty() && seen_total.is_none()` second operand is FALSE
    // (the snapshot is emitted, carrying the gap up to the seen total).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/lrj.rs","content":"l5\nl6","startLine":5,"numLines":2,"totalLines":10}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/lrj.rs",
        "--at",
        "@line:9999",
        "--file-lines",
        "1..2",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let snap = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|o| o.get("kind").and_then(|v| v.as_str()) == Some("snapshot"))
        .expect("a snapshot object is still emitted");
    // No known lines survive the range filter, but the seen total + gaps are reported.
    assert_eq!(
        snap.get("lines")
            .and_then(|v| v.as_array())
            .map(|a| a.len()),
        Some(0),
        "no known lines in the 1..2 range: {snap}"
    );
    assert_eq!(
        snap.get("seen_total_lines").and_then(|v| v.as_u64()),
        Some(10),
        "the seen total is preserved: {snap}"
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
fn turns_agent_msgs_rich_restores_middles_and_collapses_declarations() {
    // `--agent-msgs rich` over the long-run turn: the rich first / sudden-rich middle /
    // fused body survive verbatim; the pure-declaration middles collapse into a
    // placeholder carrying a fetchable L{a}–L{b} range. The default (eot-only) shows ONLY
    // the EOT — proving the flag changes behavior.
    let h = turns_home();
    let rich = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "rich",
    ]);
    assert!(rich.success, "stderr: {}", rich.stderr);
    // Rich members survive verbatim.
    assert!(
        rich.stdout.contains("AGENTRICHFIRST"),
        "rich first kept: {}",
        rich.stdout
    );
    assert!(
        rich.stdout.contains("AGENTRICHMID"),
        "sudden rich middle kept"
    );
    assert!(
        rich.stdout.contains("FUSEDTAIL"),
        "fused finding+decl body kept whole"
    );
    assert!(rich.stdout.contains("AGENTEOT"), "the EOT is always kept");
    // The pure declarations are collapsed — their unique token must NOT appear verbatim.
    assert!(
        !rich.stdout.contains("LETMEDECL"),
        "pure declarations must be collapsed, not emitted: {}",
        rich.stdout
    );
    // A placeholder line with a fetchable range is present.
    assert!(
        rich.stdout.contains("agent message") && (rich.stdout.contains("tool call")),
        "a collapsed-agents placeholder is present: {}",
        rich.stdout
    );
    // The `eot-only` ESCAPE keeps only the EOT — the intermediate rich members are absent.
    let eot = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "eot-only",
    ]);
    assert!(eot.stdout.contains("AGENTEOT"), "eot-only keeps the EOT");
    assert!(
        !eot.stdout.contains("AGENTRICHFIRST") && !eot.stdout.contains("AGENTRICHMID"),
        "the eot-only escape must NOT restore intermediate agent messages: {}",
        eot.stdout
    );
}

#[test]
fn turns_default_longest_restores_substance_and_drops_declarations() {
    // The NEW DEFAULT (`longest`, no flag) over the long-run fixture turn. The agent run's
    // char lengths are: AGENTRICHFIRST=43, decls 26–34, AGENTRICHMID=45, FUSEDTAIL=72
    // (the LONGEST), AGENTEOT=35. So the default keeps:
    //   • FUSEDTAIL — the LONGEST (72 chars) → the substantive Rich Response.
    //   • AGENTRICHMID — a RICH middle (file:line + ratio) → a mid-run major finding.
    // and COLLAPSES everything else into placeholders, INCLUDING:
    //   • AGENTRICHFIRST — a SHORT first (43 < 280 rich-min) and not the longest → dropped
    //     (proves the first is kept only when SUBSTANTIVE, not merely rich/present).
    //   • AGENTEOT — a SHORT, non-rich LAST (the ~35-char throwaway wrap-up) → dropped
    //     (THE headline: the last is no longer unconditionally kept; the substance is).
    //   • the pure LETMEDECL declarations.
    // This is exactly the substance the OLD `agents.last()` default silently dropped, plus
    // the deliberate dropping of the throwaway last.
    let h = turns_home();
    let dflt = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(dflt.success, "stderr: {}", dflt.stderr);
    // The LONGEST + the rich middle are restored.
    assert!(
        dflt.stdout.contains("FUSEDTAIL"),
        "default restores the LONGEST agent message: {}",
        dflt.stdout
    );
    assert!(
        dflt.stdout.contains("AGENTRICHMID"),
        "default restores the rich middle finding: {}",
        dflt.stdout
    );
    // The throwaway last (AGENTEOT) and the short first (AGENTRICHFIRST) are NOT kept by
    // the default — they fall below the substantive/rich bar and are not the longest.
    assert!(
        !dflt.stdout.contains("AGENTEOT"),
        "default drops the non-rich throwaway LAST (the headline case): {}",
        dflt.stdout
    );
    assert!(
        !dflt.stdout.contains("AGENTRICHFIRST"),
        "default drops a SHORT (non-substantive) first: {}",
        dflt.stdout
    );
    // The pure declarations still collapse — the default is NOT `all`.
    assert!(
        !dflt.stdout.contains("LETMEDECL"),
        "default collapses pure declarations into a placeholder: {}",
        dflt.stdout
    );
    assert!(
        dflt.stdout.contains("agent message") && dflt.stdout.contains("tool call"),
        "a collapsed-agents placeholder is present under the default: {}",
        dflt.stdout
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

#[test]
fn recover_coverage_excludes_failed_edit_before_read_after_bash_create() {
    // The user's explicit case: Bash CREATES a file, then a direct Edit (no Read) FAILS with
    // "File has not been read yet" (Bash doesn't satisfy CC's Read gate). When coverage
    // measures "how much can be recovered", that failed Edit must NOT be counted as a
    // recoverable edit — only the (content-less) Bash touch + the integrity boundary show.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"make a config"}}"#, "\n",
            // Bash creates the file (heuristic touch, no content captured).
            r#"{"type":"assistant","uuid":"ab","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b0","name":"Bash","input":{"command":"printf 'B1\nB2\nB3\n' > /p/cfg.txt"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"cb","timestamp":"2026-06-07T05:00:01.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b0","content":""}]}}"#, "\n",
            // Direct Edit with no prior Read → fails.
            r#"{"type":"assistant","uuid":"ae","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e_nr","name":"Edit","input":{"file_path":"/p/cfg.txt","old_string":"B2","new_string":"B2-edited"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"ce","timestamp":"2026-06-07T05:00:02.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e_nr","content":"File has not been read yet. Read it first before writing to it.","is_error":true}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/cfg.txt",
        "--coverage",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("B2-edited"),
        "failed edit content must not appear anywhere: {}",
        out.stdout
    );
    let cov: serde_json::Value = serde_json::from_str(
        out.stdout
            .lines()
            .find(|l| l.contains("recoverable_lines"))
            .unwrap(),
    )
    .unwrap();
    let ev = &cov["events"];
    assert_eq!(
        ev["edit"].as_u64(),
        Some(0),
        "failed edit not counted as a recoverable edit: {cov}"
    );
    assert_eq!(
        ev["edit_unanchorable"].as_u64(),
        Some(0),
        "failed edit not even counted as an un-anchorable edit: {cov}"
    );
    assert_eq!(
        ev["bash"].as_u64(),
        Some(1),
        "the Bash create IS a (heuristic) touch: {cov}"
    );
    assert_eq!(
        ev["integrity_error"].as_u64(),
        Some(1),
        "the Edit-before-Read failure surfaces as an integrity annotation, not an edit: {cov}"
    );
    assert_eq!(
        cov["recoverable_lines"].as_u64(),
        Some(0),
        "nothing is recoverable (Bash has no content, the edit failed): {cov}"
    );
}

#[test]
fn recover_file_plan_magic_reconstructs_the_bound_plan() {
    let h = Home::new();
    let plans_dir = h.root.join(".claude").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let bound_abs = plans_dir
        .join("shimmying-spinning-cascade.md")
        .to_string_lossy()
        .into_owned();
    let other_abs = plans_dir.join("decoy.md").to_string_lossy().into_owned();
    write_planning_session(&h, SESS, &bound_abs, &other_abs);

    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "@plan",
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The stderr note announces the resolution.
    assert!(
        out.stderr.contains("@plan resolved to")
            && out.stderr.contains("shimmying-spinning-cascade.md"),
        "missing @plan resolution note: {}",
        out.stderr
    );
    // The bound plan's full Write+Edit history is reconstructed (not the decoy plan).
    assert_eq!(
        recon_lines_from_at_json(&out.stdout),
        vec!["P1", "P2-revised", "P3"],
        "recovered the bound plan's content: {}",
        out.stdout
    );
}
