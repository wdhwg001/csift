//! The cwd join and the honest per-window disclosure: resolved bash operands match an
//! absolute --file, opaque commands are counted and listed, restore says what it did
//! not replay, and hard error paths keep the JSON envelope whole.

use crate::harness::*;

const DFILE: &str = "/work/proj/notes.md";

/// A session whose record `cwd` is the join base: a structured Write anchors the file,
/// a RELATIVE bash `sed -i` touches it, and a `cargo fmt` runs as an opaque command.
fn join_scenario_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"work on notes"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/work/proj","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/work/proj/notes.md","content":"one\ntwo\nthree\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:00:02.000Z","cwd":"/work/proj","toolUseResult":{"type":"create","filePath":"/work/proj/notes.md","content":"one\ntwo\nthree\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"File created successfully at: /work/proj/notes.md"}]}}"#, "\n",
            // The dominant real shape: a RELATIVE operand, resolved via the record cwd.
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:00:03.000Z","cwd":"/work/proj","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"sed -i 's/one/uno/' notes.md"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c2","timestamp":"2026-06-07T05:00:04.000Z","cwd":"/work/proj","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"ok"}]}}"#, "\n",
            // An opaque mutating-class command: names no files, counted per window.
            r#"{"type":"assistant","uuid":"a3","timestamp":"2026-06-07T05:00:05.000Z","cwd":"/work/proj","message":{"role":"assistant","content":[{"type":"tool_use","id":"b2","name":"Bash","input":{"command":"cargo fmt"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c3","timestamp":"2026-06-07T05:00:06.000Z","cwd":"/work/proj","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b2","content":"ok"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn coverage_joins_relative_bash_to_the_absolute_file() {
    // THE join fix: `--file` is absolute, the bash operand was typed relative. The
    // resolved spelling matches, so the event and its soft boundary appear (before
    // v0.8.0 this printed `events: …` with no bash and `boundaries: (none)`).
    let h = join_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        DFILE,
        "--coverage",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("1 bash (heuristic)"),
        "bash event joined: {}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("integrity boundaries: 1 (0 hard · 1 soft)"),
        "hard/soft split: {}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("bash `sed-i` on /work/proj/notes.md [cwd-joined]"),
        "boundary names the resolved path + class: {}",
        out.stdout
    );
    // The opaque command is disclosed with its transcript line and the ready search.
    assert!(
        out.stdout.contains("opaque in window: 1 mutating-class command(s) whose file set is not in the command text (fmt:cargo)"),
        "opaque note: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("fmt:cargo") && out.stdout.contains("L6"),
        "opaque row lists the real transcript line: {}",
        out.stdout
    );
    let expect_search = format!(
        "inspect the window: csift search 'notes\\.md' @{SESS} -t agent.tool.use \
         --since 2026-06-07T05:00:02.000Z --until 2026-06-07T05:00:03.000Z"
    );
    assert!(
        out.stdout.contains(&expect_search),
        "suggested search is exact + paste-runnable: {}",
        out.stdout
    );

    // JSON carries the same accounting.
    let json = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        DFILE,
        "--coverage",
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(json.success);
    let cov: serde_json::Value = json
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .find(|o: &serde_json::Value| o["kind"] == "coverage")
        .expect("coverage row");
    assert_eq!(cov["hard_boundaries"], 0);
    assert_eq!(cov["soft_boundaries"], 1);
    assert_eq!(cov["opaque_commands"], 1);
    assert_eq!(cov["powershell_commands"], 0);
    assert!(cov["suggested_search"]
        .as_str()
        .unwrap()
        .starts_with("csift search 'notes\\.md'"));
    let b = &cov["boundaries"][0];
    assert_eq!(b["cause"], "bash_mutation");
    assert_eq!(b["source_session_id"], SESS);
    assert_eq!(b["source_line"], b["line"]);
}

#[test]
fn restore_success_disclosure_names_what_was_not_replayed() {
    let h = join_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        DFILE,
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // stdout stays the raw restorable content.
    assert_eq!(out.stdout, "one\ntwo\nthree\n");
    // stderr carries the honest status: complete from the stream, plus the ledger.
    assert!(
        out.stderr
            .contains("complete from the tool stream; NOT verified against disk"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        out.stderr
            .contains("1 change(s) in the window were disclosed as boundaries, not replayed:"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("bash `sed-i` on /work/proj/notes.md"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        out.stderr
            .contains("also in this window: 1 mutating-class command(s)"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        out.stderr
            .contains("inspect the window: csift search 'notes\\.md'"),
        "stderr: {}",
        out.stderr
    );

    // JSON: the restore row carries the full accounting.
    let json = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        DFILE,
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(json.success);
    let row: serde_json::Value = json
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .find(|o: &serde_json::Value| o["kind"] == "restore")
        .expect("restore row");
    assert_eq!(row["complete"], true);
    assert_eq!(row["bash_events"], 1);
    assert_eq!(row["opaque_commands"], 1);
    assert_eq!(row["powershell_commands"], 0);
    assert_eq!(row["boundaries"].as_array().unwrap().len(), 1);
    assert!(row["suggested_search"].as_str().is_some());
}

#[test]
fn restore_clean_window_says_so_positively() {
    // Only a structured Write: the status line states the clean window explicitly.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"write it"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/p/clean.md","content":"only\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"type":"create","filePath":"/p/clean.md","content":"only\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"File created successfully at: /p/clean.md"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/clean.md",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains(
            "complete; no bash mutation of this file and no opaque mutating-class command \
             detected in the window"
        ),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn restore_partial_error_keeps_the_json_envelope_whole() {
    // A windowed Read only (no full anchor): restore is a hard error, but the JSON
    // stream still closes header -> restore row (complete:false) -> summary.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"peek"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"r1","name":"Read","input":{"file_path":"/p/big.md","offset":10,"limit":2}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"file":{"filePath":"/p/big.md","content":"ten\neleven","startLine":10,"numLines":2,"totalLines":40}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"r1","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/big.md",
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(!out.success, "a partial restore stays a hard error");
    let objs: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).expect("valid json line"))
        .collect();
    assert_eq!(
        objs.first().map(|o| o["kind"].clone()),
        Some(serde_json::json!("header"))
    );
    assert_eq!(
        objs.last().map(|o| o["kind"].clone()),
        Some(serde_json::json!("summary"))
    );
    let row = objs.iter().find(|o| o["kind"] == "restore").expect("row");
    assert_eq!(row["complete"], false);
    assert_eq!(row["reason"], "partial");
    assert!(
        out.stderr.contains("cannot fully recover"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn restore_invalidated_history_is_not_reported_as_never_touched() {
    // A full Write anchor followed by a modified-since-read rejection: the buffer is
    // cleared and nothing survives, but the file WAS touched here. The old message
    // claimed "never Read/Written/Edited"; the accurate one names the invalidation.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"edit it"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/p/drift.md","content":"v1\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"type":"create","filePath":"/p/drift.md","content":"v1\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"File created successfully at: /p/drift.md"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/p/drift.md","old_string":"v1","new_string":"v2"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c2","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"<tool_use_error>File has been modified since read, either by the user or by a linter. Read it again before attempting to write it.</tool_use_error>","is_error":true}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/drift.md",
        "--no-subagents",
    ]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("no line content survives the replay"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("invalidated at: modified_since_read"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("never Read/Written/Edited"),
        "the false claim is gone: {}",
        out.stderr
    );
}

#[test]
fn salvage_reports_boundaries_and_a_named_latest_state() {
    let h = join_scenario_home();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        DFILE,
        "--salvage",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("integrity boundaries: 1 (0 hard · 1 soft)"),
        "salvage shows the boundary section: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("opaque in window: 1 mutating-class"),
        "salvage discloses opaque commands: {}",
        out.stdout
    );

    // The absent-file wording names the state instead of a dangling "as of ".
    let miss = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/absent.md",
        "--salvage",
        "--no-subagents",
    ]);
    assert!(miss.success);
    assert!(
        miss.stdout
            .contains("no recoverable history for /p/absent.md at the latest state"),
        "stdout: {}",
        miss.stdout
    );
}

#[test]
fn window_excluding_integrity_events_is_noted() {
    let h = join_scenario_home();
    // Cut the window before the bash mutation: its exclusion must be counted aloud.
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        DFILE,
        "--coverage",
        "--no-subagents",
        "--until",
        "2026-06-07T05:00:02.500Z",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stderr
            .contains("window excluded 1 integrity-relevant event(s)"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        out.stdout.contains("integrity boundaries: (none)"),
        "the excluded boundary is not shown, only noted: {}",
        out.stdout
    );
}
