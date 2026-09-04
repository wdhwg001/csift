//! recover --coverage: seen totals, holes, boundary symbols.

use crate::harness::*;

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
fn recover_coverage_heuristic_boundary_uses_soft_symbol() {
    // A coverage run over a session with a HEURISTIC (bash) boundary drives the coverage
    // renderer's `~` (soft) boundary symbol arm - distinct from the `⚠` authoritative one.
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
fn recover_coverage_excludes_failed_edit_before_read_after_bash_create() {
    // The user's explicit case: Bash CREATES a file, then a direct Edit (no Read) FAILS with
    // "File has not been read yet" (Bash doesn't satisfy CC's Read gate). When coverage
    // measures "how much can be recovered", that failed Edit must NOT be counted as a
    // recoverable edit - only the (content-less) Bash touch + the integrity boundary show.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"make a config"}}"#, "\n",
            // Bash creates the file - a LITERAL printf, so since v0.9.4 the content
            // is a first-class bash-write anchor (not a mere heuristic touch).
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
        Some(0),
        "the literal create is an anchor, not a heuristic touch: {cov}"
    );
    assert_eq!(
        ev["bash_write_anchor"].as_u64(),
        Some(1),
        "the literal printf create anchors its content: {cov}"
    );
    assert_eq!(
        ev["integrity_error"].as_u64(),
        Some(1),
        "the Edit-before-Read failure surfaces as an integrity annotation, not an edit: {cov}"
    );
    assert_eq!(
        cov["recoverable_lines"].as_u64(),
        Some(3),
        "the literal bash create carries every line; the failed edit adds nothing: {cov}"
    );
}

#[test]
fn recover_coverage_groups_multiple_sessions_with_separator() {
    // Two sessions BOTH touching the same file via a positional project-path target (no
    // --session) → the coverage renderer prints two SESSION headers separated by a blank
    // line (the `if !*first` separator arm).
    let h = Home::new();
    let sess_a = "aaaaaaaa-1111-1111-1111-111111111111";
    let sess_b = "bbbbbbbb-2222-2222-2222-222222222222";
    for s in [sess_a, sess_b] {
        h.write(
            &format!("{ENC}/{s}.jsonl"),
            concat!(
                r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
                r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/shared.rs","content":"a\nb","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
            ),
        );
    }
    let out = h.run(&["recover", ENC, "--file", "/p/shared.rs", "--coverage"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.matches("SESSION").count() >= 2,
        "two session headers with a separator: {}",
        out.stdout
    );
}

#[test]
fn a_blanked_read_echo_is_counted_and_never_replayed_as_content() {
    // A full read, then the harness's blanked echo of a later read (content "" with the
    // counts intact) and a contentless `file_unchanged` echo: neither may turn the known
    // file into an empty one or into three empty lines; both are counted and disclosed.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"read it"}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/bl.md","content":"a\nb\nc","startLine":1,"numLines":3,"totalLines":3}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"r1","content":"ok"}]}}"#, "\n",
            r#"{"type":"user","uuid":"c2","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"file":{"filePath":"/p/bl.md","content":"","startLine":1,"numLines":3,"totalLines":3}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"r2","content":"ok"}]}}"#, "\n",
            r#"{"type":"user","uuid":"c3","timestamp":"2026-06-07T05:00:03.000Z","toolUseResult":{"type":"file_unchanged","file":{"filePath":"/p/bl.md"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"r3","content":"ok"}]}}"#, "\n",
        ),
    );
    let cov = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/bl.md",
        "--coverage",
        "--no-subagents",
    ]);
    assert!(cov.success, "stderr: {}", cov.stderr);
    assert!(
        cov.stdout.contains("1 read (1 full, 0 windowed)") && cov.stdout.contains("2 blanked-read"),
        "the blanked echoes are their own count, never reads: {}",
        cov.stdout
    );
    let at_latest = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/bl.md",
        "--at",
        "@latest",
        "--no-subagents",
    ]);
    assert!(at_latest.success, "stderr: {}", at_latest.stderr);
    assert!(
        ["1  a", "2  b", "3  c"]
            .iter()
            .all(|l| at_latest.stdout.contains(l)),
        "the known content survives the blanked echoes: {}",
        at_latest.stdout
    );
    let json = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/bl.md",
        "--coverage",
        "--no-subagents",
        "--format",
        "json",
    ]);
    let cov_row: serde_json::Value = json
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .find(|o: &serde_json::Value| o["kind"] == "coverage")
        .expect("a coverage row");
    assert_eq!(cov_row["events"]["blanked_read"], 2, "{cov_row}");
    assert_eq!(cov_row["events"]["read_full"], 1, "{cov_row}");
    assert_eq!(cov_row["soft_boundaries"], 2, "{cov_row}");
    assert_eq!(cov_row["hard_boundaries"], 0, "{cov_row}");
    assert!(
        cov_row["boundaries"].as_array().is_some_and(|b| b
            .iter()
            .filter(|x| x["cause"] == "blanked_read")
            .count()
            == 2),
        "both echoes are disclosed as annotation boundaries: {cov_row}"
    );
}

#[test]
fn coverage_splits_full_and_windowed_read_counts() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"read it"}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/rw.md","content":"a\nb\nc","startLine":1,"numLines":3,"totalLines":3}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"r1","content":"ok"}]}}"#, "\n",
            r#"{"type":"user","uuid":"c2","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"file":{"filePath":"/p/rw.md","content":"b","startLine":2,"numLines":1,"totalLines":3}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"r2","content":"ok"}]}}"#, "\n",
            // A Write and a `file` ATTACHMENT read: each has its own count arm.
            r#"{"type":"user","uuid":"c3","timestamp":"2026-06-07T05:00:03.000Z","toolUseResult":{"type":"update","filePath":"/p/rw.md","content":"a\nb\nc"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#, "\n",
            r#"{"type":"attachment","uuid":"att1","timestamp":"2026-06-07T05:00:04.000Z","attachment":{"file":{"filePath":"/p/rw.md","content":"a\nb\nc","startLine":1,"numLines":3,"totalLines":3}}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/rw.md",
        "--coverage",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("3 read (2 full, 1 windowed)"),
        "read split incl. the file attachment: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("1 write"),
        "the write is counted: {}",
        out.stdout
    );

    // The fully-covered snapshot has NO gaps in JSON (no phantom inverted range).
    let json = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/rw.md",
        "--at",
        "@latest",
        "--no-subagents",
        "--format",
        "json",
    ]);
    let snap: serde_json::Value = json
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .find(|o: &serde_json::Value| o["kind"] == "snapshot")
        .expect("snapshot row");
    assert_eq!(snap["gaps"], serde_json::json!([]));
}
