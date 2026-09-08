//! The SURVIVAL AXIS across surfaces: where the four commands agree on turn
//! numbering, and the one place they deliberately disagree about a `--turn` window.

use crate::harness::*;

const SURV_ENC: &str = "-Users-dev-example-project";
const SURV_SESS: &str = "8d7c6b5a-4938-4271-8605-4f3e2d1c0b9a";

fn survival_rows(out: &str) -> Vec<serde_json::Value> {
    out.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// A transcript whose history sits ABOVE a compaction cut: L1-L3 write the target, L4 is
/// the `compact_boundary` (its own parentUuid null, `logicalParentUuid` naming L3), L5 the
/// summary, L6-L8 the post-cut turn that edits the same file. Claude Code's loader stops at
/// the boundary; csift steps over it and flags what it then reads `pre-cut`.
fn pre_cut_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{SURV_ENC}/{SURV_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/Users/dev/chart.md","content":"alpha\nbeta\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r0","parentUuid":"a0","timestamp":"2026-06-07T05:00:06.000Z","toolUseResult":{"type":"create","filePath":"/Users/dev/chart.md","content":"alpha\nbeta\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"r0","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preTokens":900}}"#, "\n",
            r#"{"type":"user","uuid":"s1","parentUuid":"b1","isCompactSummary":true,"timestamp":"2026-06-07T05:10:01.000Z","message":{"role":"user","content":"summary of the lagoon work"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"s1","timestamp":"2026-06-07T05:11:00.000Z","message":{"role":"user","content":"survey the southern shoal"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:11:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/Users/dev/chart.md","old_string":"alpha","new_string":"delta"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","parentUuid":"a1","timestamp":"2026-06-07T05:11:06.000Z","toolUseResult":{"filePath":"/Users/dev/chart.md","oldString":"alpha","newString":"delta","structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-alpha","+delta"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#, "\n",
        ),
    );
    h
}

/// GOLDEN PIN: a pre-cut region must not cost `files` a row or `recover` a covered line.
/// The failure this guards against is silent and total - a chain that cannot resolve the
/// region marks its openers abandoned, and a walk that iterates only NUMBERED turns then
/// drops every mutation above the cut. What comes out is a shorter, plausible answer with
/// nothing to indicate the loss.
#[test]
fn a_pre_cut_region_costs_files_no_row_and_recover_no_coverage() {
    let h = pre_cut_home();
    let t = at(SURV_SESS);
    let files = h.run(&["files", t.as_str(), "--by", "timeline"]);
    assert!(files.success, "stderr: {}", files.stderr);
    let muts: Vec<serde_json::Value> = survival_rows(
        &h.run(&["files", t.as_str(), "--by", "timeline", "--format", "json"])
            .stdout,
    )
    .into_iter()
    .filter(|v| v["kind"] == "mutation")
    .collect();
    assert_eq!(
        muts.len(),
        2,
        "the write above the cut and the edit below it:\n{}",
        files.stdout
    );
    let above = muts
        .iter()
        .find(|m| m["line"] == 2)
        .expect("the pre-cut write");
    assert_eq!(above["survival"], "pre-cut", "flagged, never dropped");
    assert_eq!(
        above["turn_index"], 0,
        "a cut is not an abandonment: the turn keeps its number"
    );
    assert!(files.stdout.contains("[pre-cut]"), "{}", files.stdout);

    // recover replays across the cut, so both lines of the file stay recoverable.
    let cov = h.run(&[
        "recover",
        t.as_str(),
        "--file",
        "/Users/dev/chart.md",
        "--coverage",
        "--format",
        "json",
    ]);
    let row = survival_rows(&cov.stdout)
        .into_iter()
        .find(|v| v["kind"] == "coverage")
        .expect("a coverage row");
    assert_eq!(row["recoverable_lines"], 2, "{}", cov.stdout);
    assert_eq!(
        row["abandoned_events"], 0,
        "a pre-cut event is not an abandoned one: {}",
        cov.stdout
    );
}

/// The one place the surfaces disagree, and why. Under the SAME `--turn` window over the
/// live turns, the CONVERSATION surfaces exclude an abandoned record (it belongs to no
/// numbered turn) while the DISK surfaces admit it (the write landed, and a window over a
/// span must not hide a write that happened inside it).
#[test]
fn one_turn_window_excludes_an_abandoned_row_on_search_and_show_and_admits_it_on_files_and_recover()
{
    let h = Home::new();
    h.write(
        &format!("{SURV_ENC}/{SURV_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"attachment","uuid":"x0","parentUuid":"a0","timestamp":"2026-06-07T05:00:50.000Z","attachment":{"type":"hook_additional_context","content":["session context"]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"x0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/Users/dev/chart.md","content":"alpha\nbeta\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","parentUuid":"a1","timestamp":"2026-06-07T05:01:06.000Z","toolUseResult":{"type":"create","filePath":"/Users/dev/chart.md","content":"alpha\nbeta\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"x0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"survey the southern shoal instead"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"surveying"}]}}"#, "\n",
        ),
    );
    let t = at(SURV_SESS);
    const WINDOW: &str = "0..1"; // both live turns; the abandoned branch follows turn 0.

    let found = h.run(&["search", "", t.as_str(), "--turn", WINDOW]);
    assert!(
        !found.stdout.contains("dredge the northern channel"),
        "search asks about the conversation:\n{}",
        found.stdout
    );
    let shown = h.run(&["show", t.as_str(), "--turn", WINDOW]);
    assert!(
        !shown.stdout.contains("dredge the northern channel"),
        "show asks about the conversation:\n{}",
        shown.stdout
    );

    let files = h.run(&["files", t.as_str(), "--by", "timeline", "--turn", WINDOW]);
    assert!(files.success, "stderr: {}", files.stderr);
    assert!(
        files.stdout.contains("turn abandoned (root L4)"),
        "files asks about the disk, and that write landed:\n{}",
        files.stdout
    );
    let cov = h.run(&[
        "recover",
        t.as_str(),
        "--file",
        "/Users/dev/chart.md",
        "--coverage",
        "--turn",
        WINDOW,
        "--format",
        "json",
    ]);
    let row = survival_rows(&cov.stdout)
        .into_iter()
        .find(|v| v["kind"] == "coverage")
        .expect("a coverage row");
    assert_eq!(
        row["abandoned_events"], 1,
        "recover replays it inside the window: {}",
        cov.stdout
    );
    assert_eq!(row["recoverable_lines"], 2, "{}", cov.stdout);
}
