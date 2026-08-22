//! Claude Code freshness signals through the CLI surface: the modified-file hint as a
//! hard boundary, staleRecovered as an annotation, and the formatter clue on an
//! external edit.

use crate::harness::*;

#[test]
fn stale_read_hint_is_an_authoritative_hard_boundary() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"work"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/work/proj","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/work/proj/src/api.rs","content":"fn a() {}\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:00:02.000Z","cwd":"/work/proj","toolUseResult":{"type":"create","filePath":"/work/proj/src/api.rs","content":"fn a() {}\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"File created successfully at: /work/proj/src/api.rs"}]}}"#, "\n",
            // A generator script modifies the read set; CC's hint names the file
            // RELATIVE to the shell cwd. The command text itself names no path.
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:00:03.000Z","cwd":"/work/proj","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"./gen.sh"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c2","timestamp":"2026-06-07T05:00:04.000Z","cwd":"/work/proj","toolUseResult":{"stdout":"done","stderr":"","staleReadFileStateHint":"[This command modified 1 file you've previously read: src/api.rs. Call Read before editing.]"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"done"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/work/proj/src/api.rs",
        "--coverage",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("1 modified-file-hint"),
        "hint counted: {}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("integrity boundaries: 1 (1 hard · 0 soft)"),
        "the hint is a HARD boundary: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains(
            "Claude Code reported /work/proj/src/api.rs modified by this shell command \
             (staleReadFileStateHint)"
        ),
        "detail names the resolved path: {}",
        out.stdout
    );
}

#[test]
fn stale_recovered_annotation_reaches_the_coverage_view() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"edit"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/p/a.rs","content":"x\ny\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"type":"create","filePath":"/p/a.rs","content":"x\ny\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"File created successfully at: /p/a.rs"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/p/a.rs","old_string":"x","new_string":"z"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c2","timestamp":"2026-06-07T05:00:04.000Z","toolUseResult":{"filePath":"/p/a.rs","oldString":"x","newString":"z","staleRecovered":true},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/a.rs",
        "--coverage",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("1 stale-recovered"),
        "counted: {}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("integrity boundaries: 1 (0 hard · 1 soft)"),
        "an annotation, never hard: {}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("the edit applied cleanly, but the file had been modified"),
        "detail: {}",
        out.stdout
    );
    // The edit still applied: restore stays complete (with the disclosure).
    let restore = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/a.rs",
        "--no-subagents",
    ]);
    assert!(restore.success, "stderr: {}", restore.stderr);
    assert_eq!(restore.stdout, "z\ny\n");
    assert!(
        restore
            .stderr
            .contains("complete from the tool stream; NOT verified against disk"),
        "stderr: {}",
        restore.stderr
    );
}

#[test]
fn external_edit_carries_the_formatter_clue_when_one_ran() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"work"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/p","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/p/lib.rs","content":"fn x() {}\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:00:02.000Z","cwd":"/p","toolUseResult":{"type":"create","filePath":"/p/lib.rs","content":"fn x() {}\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"File created successfully at: /p/lib.rs"}]}}"#, "\n",
            // The formatter runs (names no files), then CC attaches the external edit.
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:00:03.000Z","cwd":"/p","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"cargo fmt"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c2","timestamp":"2026-06-07T05:00:04.000Z","cwd":"/p","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"ok"}]}}"#, "\n",
            r#"{"type":"attachment","uuid":"att1","timestamp":"2026-06-07T05:00:05.000Z","cwd":"/p","attachment":{"type":"edited_text_file","filename":"/p/lib.rs","snippet":"1: fn x() {}"}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/p/lib.rs",
        "--coverage",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("a formatter-class command (fmt:cargo) ran at L4 in this window"),
        "clue in the boundary detail: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("check the project's conventions"),
        "clue closes with the pointer: {}",
        out.stdout
    );
}
