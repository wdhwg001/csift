//! Settings-family external writes on the files timeline (the snapshot instrument).

use crate::harness::*;

const XSESS: &str = "beef5678-1234-4abc-8def-998877665544";

#[test]
fn settings_family_external_write_surfaces_and_others_do_not() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{XSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"tune the config"}}"#, "\n",
            // Tool-edit BOTH files so both are tracked.
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/Users/dev/.claude/settings.json","old_string":"a","new_string":"b"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#, "\n",
            r#"{"type":"file-history-snapshot","messageId":"m1","snapshot":{"messageId":"m1","timestamp":"2026-06-07T05:01:00.000Z","trackedFileBackups":{"/Users/dev/.claude/settings.json":{"backupFileName":"aa@v1","version":1,"backupTime":"2026-06-07T05:01:00.000Z"},"/Users/dev/proj/notes.md":{"backupFileName":"bb@v1","version":1,"backupTime":"2026-06-07T05:01:00.000Z"}}},"isSnapshotUpdate":false}"#, "\n",
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:05:00.000Z","message":{"role":"user","content":"carry on"}}"#, "\n",
            // BOTH versions jump with no tool record in the interval: only the
            // settings-family path may surface (the scope law).
            r#"{"type":"file-history-snapshot","messageId":"m2","snapshot":{"messageId":"m2","timestamp":"2026-06-07T05:06:00.000Z","trackedFileBackups":{"/Users/dev/.claude/settings.json":{"backupFileName":"aa@v2","version":2,"backupTime":"2026-06-07T05:06:00.000Z"},"/Users/dev/proj/notes.md":{"backupFileName":"bb@v2","version":2,"backupTime":"2026-06-07T05:06:00.000Z"}}},"isSnapshotUpdate":false}"#, "\n",
        ),
    );
    let out = h.run(&["files", at(XSESS).as_str(), "--by", "timeline"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("external write  /Users/dev/.claude/settings.json"),
        "the settings silent write surfaces:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("file-history v1->v2") && out.stdout.contains("no tool record"),
        "the instrument detail rides the row:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout
            .contains("external write  /Users/dev/proj/notes.md"),
        "a non-settings tracked path NEVER surfaces (the scope law):\n{}",
        out.stdout
    );
    // JSON: op external_write + the detail field; heuristic stays false.
    let j = h.run(&[
        "files",
        at(XSESS).as_str(),
        "--by",
        "timeline",
        "--format",
        "json",
    ]);
    let row = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["op"] == "external_write")
        .expect("external_write row");
    assert_eq!(
        row["path"], "/Users/dev/.claude/settings.json",
        "{}",
        j.stdout
    );
    assert_eq!(row["heuristic"], false, "{}", j.stdout);
    assert!(
        row["detail"].as_str().unwrap().contains("v1->v2"),
        "{}",
        j.stdout
    );
}

#[test]
fn a_tool_write_in_the_interval_explains_the_jump() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{XSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"tune"}}"#, "\n",
            r#"{"type":"file-history-snapshot","messageId":"m1","snapshot":{"messageId":"m1","timestamp":"2026-06-07T05:01:00.000Z","trackedFileBackups":{"/Users/dev/.claude/settings.json":{"backupFileName":"aa@v1","version":1,"backupTime":"2026-06-07T05:01:00.000Z"}}},"isSnapshotUpdate":false}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/Users/dev/.claude/settings.json","old_string":"a","new_string":"b"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T05:02:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#, "\n",
            r#"{"type":"file-history-snapshot","messageId":"m2","snapshot":{"messageId":"m2","timestamp":"2026-06-07T05:03:00.000Z","trackedFileBackups":{"/Users/dev/.claude/settings.json":{"backupFileName":"aa@v2","version":2,"backupTime":"2026-06-07T05:03:00.000Z"}}},"isSnapshotUpdate":false}"#, "\n",
        ),
    );
    let out = h.run(&["files", at(XSESS).as_str(), "--by", "timeline"]);
    assert!(
        !out.stdout.contains("external write"),
        "an explained jump is not external:\n{}",
        out.stdout
    );
}

#[test]
fn family_gate_same_version_and_cross_spelling_pins() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{XSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"tune"}}"#, "\n",
            // The EDIT names the absolute spelling; the snapshot tracks the RELATIVE
            // one - the interval join must match across spellings.
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/Users/dev/.claude/settings.json","old_string":"a","new_string":"b"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T05:00:31.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#, "\n",
            r#"{"type":"file-history-snapshot","messageId":"m1","snapshot":{"messageId":"m1","timestamp":"2026-06-07T05:01:00.000Z","trackedFileBackups":{".claude/settings.json":{"backupFileName":"aa@v1","version":1,"backupTime":"2026-06-07T05:01:00.000Z"},".claude/CLAUDE.md":{"backupFileName":"cc@v1","version":1,"backupTime":"2026-06-07T05:01:00.000Z"}}},"isSnapshotUpdate":false}"#, "\n",
            // Edit again (absolute), snapshot bumps the RELATIVE settings entry: the
            // cross-spelling join must mark this jump EXPLAINED. CLAUDE.md jumps
            // WITHOUT a tool record - and must stay invisible (not settings-family).
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e2","name":"Edit","input":{"file_path":"/Users/dev/.claude/settings.json","old_string":"b","new_string":"c"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r2","timestamp":"2026-06-07T05:02:01.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e2","content":"ok"}]}}"#, "\n",
            r#"{"type":"file-history-snapshot","messageId":"m2","snapshot":{"messageId":"m2","timestamp":"2026-06-07T05:03:00.000Z","trackedFileBackups":{".claude/settings.json":{"backupFileName":"aa@v2","version":2,"backupTime":"2026-06-07T05:03:00.000Z"},".claude/CLAUDE.md":{"backupFileName":"cc@v2","version":2,"backupTime":"2026-06-07T05:03:00.000Z"}}},"isSnapshotUpdate":false}"#, "\n",
            // A SAME-version snapshot repeat must add no second row.
            r#"{"type":"file-history-snapshot","messageId":"m3","snapshot":{"messageId":"m3","timestamp":"2026-06-07T05:04:00.000Z","trackedFileBackups":{".claude/settings.json":{"backupFileName":"aa@v2","version":2,"backupTime":"2026-06-07T05:03:00.000Z"}}},"isSnapshotUpdate":false}"#, "\n",
        ),
    );
    let out = h.run(&["files", at(XSESS).as_str(), "--by", "timeline"]);
    assert!(
        !out.stdout.contains("external write"),
        "explained cross-spelling jump + non-settings path + same-version repeat all \
         stay silent:\n{}",
        out.stdout
    );
}

#[test]
fn rollup_counts_and_labels_carry_the_external_write() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{XSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"tune"}}"#, "\n",
            r#"{"type":"file-history-snapshot","messageId":"m1","snapshot":{"messageId":"m1","timestamp":"2026-06-07T05:01:00.000Z","trackedFileBackups":{"/Users/dev/.claude/settings.json":{"backupFileName":"aa@v1","version":1,"backupTime":"2026-06-07T05:01:00.000Z"}}},"isSnapshotUpdate":false}"#, "\n",
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:05:00.000Z","message":{"role":"user","content":"more"}}"#, "\n",
            r#"{"type":"file-history-snapshot","messageId":"m2","snapshot":{"messageId":"m2","timestamp":"2026-06-07T05:06:00.000Z","trackedFileBackups":{"/Users/dev/.claude/settings.json":{"backupFileName":"aa@v2","version":2,"backupTime":"2026-06-07T05:06:00.000Z"}}},"isSnapshotUpdate":false}"#, "\n",
        ),
    );
    let out = h.run(&["files", at(XSESS).as_str(), "--by", "file"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("1 external write (inferred)"),
        "the per-file op label counts the external write exactly once:\n{}",
        out.stdout
    );
}
