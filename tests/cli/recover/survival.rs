//! The SURVIVAL AXIS on `recover`: the replay stays FILE ORDER because an Edit on a branch
//! the operator rewound past still hit the disk, the stamps say which conversation each
//! event belonged to, and the external-write row names the interval it bounds.

use crate::harness::*;

const RENC: &str = "-Users-dev-example-project";
const RSESS: &str = "2b1a0f9e-8d7c-4b6a-9518-4736251a0b9c";
const TARGET: &str = "/Users/dev/chart.md";

fn rows(out: &str) -> Vec<serde_json::Value> {
    out.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// L1 prompt, L2 reply, L3 a hook attachment, L4 the REWOUND prompt, L5-L6 a full Write of
/// the target under it, L7-L8 an Edit of it, L9 the resend from the same parent, L10-L11 a
/// further Edit on the surviving branch. Events are extracted from the RESULT carriers, so
/// the branch contributes two of them (the write at L6, the edit at L8).
fn rewind_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{RENC}/{RSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"attachment","uuid":"x0","parentUuid":"a0","timestamp":"2026-06-07T05:00:50.000Z","attachment":{"type":"hook_additional_context","content":["session context"]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"x0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/Users/dev/chart.md","content":"alpha\nbeta\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","parentUuid":"a1","timestamp":"2026-06-07T05:01:06.000Z","toolUseResult":{"type":"create","filePath":"/Users/dev/chart.md","content":"alpha\nbeta\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"r1","timestamp":"2026-06-07T05:01:07.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/Users/dev/chart.md","old_string":"beta","new_string":"gamma"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r2","parentUuid":"a2","timestamp":"2026-06-07T05:01:08.000Z","toolUseResult":{"filePath":"/Users/dev/chart.md","oldString":"beta","newString":"gamma","structuredPatch":[{"oldStart":2,"oldLines":1,"newStart":2,"newLines":1,"lines":["-beta","+gamma"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"x0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"survey the southern shoal instead"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a3","parentUuid":"u2","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","id":"m3","content":[{"type":"tool_use","id":"e2","name":"Edit","input":{"file_path":"/Users/dev/chart.md","old_string":"alpha","new_string":"delta"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r3","parentUuid":"a3","timestamp":"2026-06-07T05:02:06.000Z","toolUseResult":{"filePath":"/Users/dev/chart.md","oldString":"alpha","newString":"delta","structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-alpha","+delta"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e2","content":"ok"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn an_abandoned_branch_is_replayed_and_the_disclosure_says_so() {
    let h = rewind_home();
    let out = h.run(&[
        "recover",
        at(RSESS).as_str(),
        "--file",
        TARGET,
        "--coverage",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The write and both edits are in the replay: the file really has these bytes.
    assert!(
        out.stdout.contains("1 write") && out.stdout.contains("2 edit"),
        "every disk-touching event is replayed:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("abandoned branches: 2 replayed event(s) come from records off the surviving conversation"),
        "the disclosure line names the count and the reason:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("the write still hit the disk"),
        "and says why they are kept:\n{}",
        out.stdout
    );
}

#[test]
fn the_coverage_json_echoes_the_abandoned_event_count() {
    let h = rewind_home();
    let j = h.run(&[
        "recover",
        at(RSESS).as_str(),
        "--file",
        TARGET,
        "--coverage",
        "--format",
        "json",
    ]);
    let cov = rows(&j.stdout)
        .into_iter()
        .find(|v| v["kind"] == "coverage")
        .expect("a coverage row");
    assert_eq!(cov["abandoned_events"], 2, "{}", j.stdout);
    // The line coverage itself is untouched by the axis - the parity pin.
    assert_eq!(cov["recoverable_lines"], 2, "{}", j.stdout);
}

#[test]
fn a_boundary_on_an_abandoned_record_is_stamped_not_numbered() {
    // An integrity error inside the rewound branch: the boundary is real (the disk drifted)
    // and it belongs to no numbered turn.
    let h = Home::new();
    h.write(
        &format!("{RENC}/{RSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/Users/dev/chart.md","old_string":"a","new_string":"b"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","parentUuid":"a1","timestamp":"2026-06-07T05:01:06.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","is_error":true,"content":"File has been modified since read, either by the user or by a linter."}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"a0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"survey the southern shoal instead"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"surveying"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(RSESS).as_str(),
        "--file",
        TARGET,
        "--coverage",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("turn abandoned (root L3)"),
        "the boundary carries its branch head, not a live turn number:\n{}",
        out.stdout
    );
    let j = h.run(&[
        "recover",
        at(RSESS).as_str(),
        "--file",
        TARGET,
        "--coverage",
        "--format",
        "json",
    ]);
    let cov = rows(&j.stdout)
        .into_iter()
        .find(|v| v["kind"] == "coverage")
        .expect("a coverage row");
    let b = cov["boundaries"]
        .as_array()
        .and_then(|a| a.first().cloned())
        .expect("a boundary");
    assert!(b["turn_index"].is_null(), "{}", j.stdout);
    assert_eq!(b["survival"], "abandoned", "{}", j.stdout);
    assert_eq!(b["abandoned_root_line"], 3, "{}", j.stdout);
}

#[test]
fn a_turn_window_resolves_against_live_numbering() {
    // Two live turns (0 and 1). `--turn -1..` is the LAST live turn, which is the resend -
    // so the surviving branch's edit is in and the first turn's write is out.
    let h = rewind_home();
    let out = h.run(&[
        "recover",
        at(RSESS).as_str(),
        "--file",
        TARGET,
        "--coverage",
        "--turn",
        "-1..",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("1 edit") && !out.stdout.contains("1 write"),
        "the window resolved against the two LIVE turns:\n{}",
        out.stdout
    );
}

#[test]
fn a_target_version_jump_with_no_tool_write_names_the_interval() {
    // The C-24 instrument on the recover TARGET (any path, not just the settings family):
    // a snapshot version jump with nothing replayed in between is an out-of-band write.
    let h = Home::new();
    h.write(
        &format!("{RENC}/{RSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/Users/dev/chart.md","content":"alpha\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","parentUuid":"a0","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"type":"create","filePath":"/Users/dev/chart.md","content":"alpha\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#, "\n",
            r#"{"type":"file-history-snapshot","messageId":"m1","snapshot":{"messageId":"m1","timestamp":"2026-06-07T05:01:00.000Z","trackedFileBackups":{"/Users/dev/chart.md":{"backupFileName":"cc@v1","version":1,"backupTime":"2026-06-07T05:01:00.000Z"}}},"isSnapshotUpdate":false}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"r1","timestamp":"2026-06-07T05:05:00.000Z","message":{"role":"user","content":"carry on"}}"#, "\n",
            r#"{"type":"file-history-delta","messageId":"m2","snapshotMessageId":"m1","trackingPath":"/Users/dev/chart.md","backup":{"backupFileName":"cc@v2","version":2,"backupTime":"2026-06-07T05:06:00.000Z"},"timestamp":"2026-06-07T05:06:00.000Z"}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(RSESS).as_str(),
        "--file",
        TARGET,
        "--coverage",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("external write (inferred, snapshot v1->v2, no tool record since L4)"),
        "the row names the version pair and the interval's low end:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("AUTHORITATIVE"),
        "the instrument is Claude Code's own accounting, not a guess:\n{}",
        out.stdout
    );
}
