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

/// Without a manifest the run is live: the lane is in flight by its tail, and the journal's
/// open agent IS that lane, so it is one live lane rather than a lane and a journal agent.
#[test]
fn a_run_with_no_manifest_leaves_its_lane_to_the_tail() {
    let (_root, main) = run_home(STARTED, None);
    let report = children_report(&main, &std::collections::HashSet::new()).unwrap();
    assert_eq!(report.children[0].state, "in-flight", "{report:?}");
    assert_eq!(report.journal_in_flight, 0);
    assert_eq!(report.live_count, 1);
}

/// An agent the journal started whose transcript has not reached the disk is in flight by the
/// journal alone, and counts there beside the lane that is live by its tail.
#[test]
fn a_started_agent_with_no_lane_yet_counts_through_the_journal() {
    let journal = format!("{STARTED}{{\"type\":\"started\",\"agentId\":\"a1111111111111111\"}}\n");
    let (_root, main) = run_home(&journal, None);
    let report = children_report(&main, &std::collections::HashSet::new()).unwrap();
    assert_eq!(report.journal_in_flight, 1, "{report:?}");
    assert_eq!(report.live_count, 2, "{report:?}");
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

/// Which run a lane belongs to is where discovery FOUND it, never a reading of directory names:
/// a built-in lane in a session whose project directory is itself named `subagents`, beside an
/// ended run that is also named `subagents`, is no lane of that run and stays in flight.
#[test]
fn a_built_in_lane_is_settled_by_no_run_whatever_its_directories_are_named() {
    let root = crate::testfs::Scratch::dir("live-run-names");
    let project = root.join("subagents");
    let main = project.join("s1.jsonl");
    let sidecar = project.join("s1/subagents");
    std::fs::create_dir_all(sidecar.join("workflows/subagents")).unwrap();
    std::fs::write(&main, "").unwrap();
    let now = jiff::Timestamp::now().to_string();
    std::fs::write(
        sidecar.join("agent-a0123456789abcdef.jsonl"),
        format!(
            r#"{{"type":"assistant","timestamp":"{now}","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{}}}}]}}}}"#
        ) + "\n",
    )
    .unwrap();
    std::fs::write(sidecar.join("workflows/subagents/journal.jsonl"), "").unwrap();
    let manifests = project.join("s1/workflows");
    std::fs::create_dir_all(&manifests).unwrap();
    std::fs::write(
        manifests.join("subagents.json"),
        r#"{"runId":"subagents","status":"completed"}"#,
    )
    .unwrap();
    let report = children_report(&main, &std::collections::HashSet::new()).unwrap();
    assert_eq!(report.children.len(), 1, "{report:?}");
    assert_eq!(report.children[0].state, "in-flight", "{report:?}");
    assert_eq!(report.live_count, 1, "{report:?}");
}
