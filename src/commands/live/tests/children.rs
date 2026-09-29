//! A workflow lane is settled by its run's own files before its tail is read.

use super::*;

/// One session sidecar holding one workflow run with one lane whose tail is an UNRETURNED
/// tool call written just now - the shape a lane stopped after its structured answer keeps.
/// `journal` is the run's journal text; `manifest` the run manifest, when the run has ended.
fn run_home(journal: &str, manifest: Option<&str>) -> (crate::testfs::Scratch, std::path::PathBuf) {
    let root = crate::testfs::Scratch::dir("live-run");
    let main = root.join("s1.jsonl");
    std::fs::write(&main, "").unwrap();
    let wf = root.join("s1/subagents/workflows/wf_run1");
    std::fs::create_dir_all(&wf).unwrap();
    let now = jiff::Timestamp::now().to_string();
    std::fs::write(
        wf.join("agent-a0123456789abcdef.jsonl"),
        format!(
            r#"{{"type":"assistant","timestamp":"{now}","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t1","name":"StructuredOutput","input":{{}}}}]}}}}"#
        ) + "\n",
    )
    .unwrap();
    std::fs::write(wf.join("journal.jsonl"), journal).unwrap();
    if let Some(m) = manifest {
        let dir = root.join("s1/workflows");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("wf_run1.json"), m).unwrap();
    }
    (root, main)
}

const STARTED: &str = "{\"type\":\"started\",\"agentId\":\"a0123456789abcdef\"}\n";

/// Without a manifest the run is live: the lane is in flight by its tail and the journal's
/// open agent counts.
#[test]
fn a_run_with_no_manifest_leaves_its_lane_to_the_tail() {
    let (_root, main) = run_home(STARTED, None);
    let report = children_report(&main, &std::collections::HashSet::new()).unwrap();
    assert_eq!(report.children[0].state, "in-flight", "{report:?}");
    assert_eq!(report.journal_in_flight, 1);
    assert_eq!(report.live_count, 2);
}

/// The manifest exists only once the run has ended, so it settles the lane whatever its tail
/// shows and the run's journal imbalance counts for nothing.
#[test]
fn an_ended_run_settles_its_lane_and_its_journal() {
    let (_root, main) = run_home(STARTED, Some(r#"{"runId":"wf_run1","status":"completed"}"#));
    let report = children_report(&main, &std::collections::HashSet::new()).unwrap();
    assert_eq!(report.children[0].state, "settled", "{report:?}");
    assert!(
        report.children[0]
            .detail
            .contains("manifest status completed"),
        "{report:?}"
    );
    assert_eq!(report.journal_in_flight, 0);
    assert_eq!(report.live_count, 0);
}

/// A `failed` line closes an agent as a `result` does: the agent is not in flight, and its
/// lane is settled by the journal even while the run is still going.
#[test]
fn a_failed_line_closes_the_agent_like_a_result() {
    let journal = format!("{STARTED}{{\"type\":\"failed\",\"agentId\":\"a0123456789abcdef\"}}\n");
    let (_root, main) = run_home(&journal, None);
    let report = children_report(&main, &std::collections::HashSet::new()).unwrap();
    assert_eq!(report.journal_in_flight, 0, "{report:?}");
    assert_eq!(report.children[0].state, "settled");
    assert!(
        report.children[0].detail.contains("journal closes"),
        "{report:?}"
    );
    assert_eq!(report.live_count, 0);
}
