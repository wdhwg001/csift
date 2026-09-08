//! The SURVIVAL AXIS on `files`: disk truth is FILE ORDER, so a mutation made on a branch
//! the operator later rewound past stays a row - marked, stamped, and counted apart in the
//! rollups. Dropping it would answer "did this session write X?" with a lie.

use crate::harness::*;

const FENC: &str = "-Users-dev-example-project";
const FSESS: &str = "5c4b3a29-1807-4655-9443-2e1d0c9b8a77";

fn rows(out: &str) -> Vec<serde_json::Value> {
    out.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// L1 prompt, L2 reply, L3 a hook attachment, L4 the REWOUND prompt with two Edits under
/// it (L5-L8), L9 the resend from the same parent with one Edit (L10-L11).
fn rewind_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{FENC}/{FSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"attachment","uuid":"x0","parentUuid":"a0","timestamp":"2026-06-07T05:00:50.000Z","attachment":{"type":"hook_additional_context","content":["session context"]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"x0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/Users/dev/chart.md","old_string":"a","new_string":"b"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","parentUuid":"a1","timestamp":"2026-06-07T05:01:06.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"r1","timestamp":"2026-06-07T05:01:07.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"tool_use","id":"e2","name":"Edit","input":{"file_path":"/Users/dev/depths.md","old_string":"c","new_string":"d"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r2","parentUuid":"a2","timestamp":"2026-06-07T05:01:08.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e2","content":"ok"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"x0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"survey the southern shoal instead"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a3","parentUuid":"u2","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","id":"m3","content":[{"type":"tool_use","id":"e3","name":"Edit","input":{"file_path":"/Users/dev/chart.md","old_string":"b","new_string":"e"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r3","parentUuid":"a3","timestamp":"2026-06-07T05:02:06.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e3","content":"ok"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn an_abandoned_branch_keeps_its_mutations_and_marks_them() {
    let h = rewind_home();
    let out = h.run(&["files", at(FSESS).as_str(), "--by", "timeline"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Disk truth: all three edits stay. The parity pin - the axis never drops a row.
    let edits = out.stdout.matches("  edit  ").count();
    assert_eq!(edits, 3, "every write is a row:\n{}", out.stdout);
    assert!(
        out.stdout.contains("turn abandoned (root L4)"),
        "the abandoned rows carry the branch head, not a fabricated turn:\n{}",
        out.stdout
    );
    assert_eq!(
        out.stdout.matches("[abandoned]").count(),
        2,
        "both writes on the rewound branch are marked:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("turn 1  edit"),
        "the surviving write keeps its live turn number:\n{}",
        out.stdout
    );
}

#[test]
fn the_timeline_json_nulls_the_turn_and_names_the_branch_head() {
    let h = rewind_home();
    let j = h.run(&[
        "files",
        at(FSESS).as_str(),
        "--by",
        "timeline",
        "--format",
        "json",
    ]);
    let muts: Vec<serde_json::Value> = rows(&j.stdout)
        .into_iter()
        .filter(|v| v["kind"] == "mutation")
        .collect();
    assert_eq!(muts.len(), 3, "{}", j.stdout);
    let ab = muts
        .iter()
        .find(|m| m["line"] == 5)
        .expect("the first abandoned edit");
    assert!(ab["turn_index"].is_null(), "{}", j.stdout);
    assert_eq!(ab["survival"], "abandoned", "{}", j.stdout);
    assert_eq!(ab["abandoned_root_line"], 4, "{}", j.stdout);
    let live = muts
        .iter()
        .find(|m| m["line"] == 10)
        .expect("the surviving edit");
    assert_eq!(live["turn_index"], 1, "{}", j.stdout);
    assert_eq!(live["survival"], "live", "{}", j.stdout);
    assert!(live["abandoned_root_line"].is_null(), "{}", j.stdout);
}

#[test]
fn the_summary_rollup_counts_the_abandoned_share_apart() {
    let h = rewind_home();
    let out = h.run(&["files", at(FSESS).as_str(), "--by", "summary"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("(2 from abandoned branches)"),
        "the rollup states the share without subtracting it:\n{}",
        out.stdout
    );
    let j = h.run(&[
        "files",
        at(FSESS).as_str(),
        "--by",
        "summary",
        "--format",
        "json",
    ]);
    let bucket = rows(&j.stdout)
        .into_iter()
        .find(|v| v["kind"] == "bucket")
        .expect("a bucket row");
    assert_eq!(bucket["abandoned"], 2, "{}", j.stdout);
    assert_eq!(
        bucket["total"], 3,
        "a share, never a subtraction: {}",
        j.stdout
    );
}

#[test]
fn a_transcript_with_no_fork_carries_no_marker_at_all() {
    let h = Home::new();
    h.write(
        &format!("{FENC}/{FSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/Users/dev/chart.md","old_string":"a","new_string":"b"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","parentUuid":"a0","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["files", at(FSESS).as_str(), "--by", "timeline"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("[abandoned]") && !out.stdout.contains("abandoned (root"),
        "a fork-free transcript pays nothing:\n{}",
        out.stdout
    );
    assert!(out.stdout.contains("turn 0  edit"), "{}", out.stdout);
    let j = h.run(&[
        "files",
        at(FSESS).as_str(),
        "--by",
        "summary",
        "--format",
        "json",
    ]);
    let bucket = rows(&j.stdout)
        .into_iter()
        .find(|v| v["kind"] == "bucket")
        .expect("a bucket row");
    assert_eq!(bucket["abandoned"], 0, "{}", j.stdout);
}
