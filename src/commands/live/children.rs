//! Child-lane liveness: subagent transcripts + the incremental workflow journal.
//!
//! `subagents/*.meta.json` has NO status field (child liveness can never come from
//! meta), and workflow RESULT files are terminal-only - but `journal.jsonl` inside each
//! `wf_*` dir is written INCREMENTALLY (`{type:"started",agentId}` at spawn,
//! `{type:"result",agentId}` at return), so `started - result` = workflow agents in
//! flight right now, counting `{type:"failed"}` as a close too, and only while the run's
//! terminal-only manifest is absent. A non-workflow child's liveness comes from its own transcript tail
//! (an unreturned tool call) plus recent growth (child transcripts grow only from the
//! child's own message flow - notifications land in MAIN only, so recency is a real
//! signal here, unlike the main lane's F9 trap).

use super::*;

/// Tail-record age (seconds) under which a settled-looking child is treated as still
/// GENERATING when its last assistant record is not an end_turn. Measured: intra-lane
/// record gaps reach p99.9 = 295s during genuine work (a long generation writes
/// nothing for minutes), while dead lanes sit >= 31h out - four orders of magnitude of
/// separation, so 300s misses almost no real work and resurrects no dead lane. A 15s
/// window reads a lane mid-generation as settled 1 time in 17.
pub(crate) const CHILD_RECENT_SECS: i64 = 300;

#[derive(Debug, Clone)]
pub(crate) struct ChildState {
    pub(crate) session_id: String,
    /// `in-flight` (unreturned tail call) | `generating` (recent tail record and the
    /// last assistant record is not an end_turn - the model is mid-generation; a
    /// paired tail alone proves nothing) | `settled`.
    pub(crate) state: &'static str,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ChildrenReport {
    pub(crate) children: Vec<ChildState>,
    /// Workflow agents started minus returned across this session's journals.
    pub(crate) journal_in_flight: usize,
    pub(crate) live_count: usize,
}

/// Inspect every child lane of `main_jsonl`. `returned` names the agent ids whose
/// completion notification already landed in the main transcript (an async agent's
/// `<task-notification>`): such a lane is SETTLED whatever its tail looks like - the
/// harness's own word outranks the recency heuristic (a lane that finished seconds
/// ago still carries a fresh non-end_turn tail and would read `generating` for up to
/// 300s otherwise). A WORKFLOW lane is settled the same way by its run's own files - the
/// manifest an ended run writes, or a journal line closing the agent - before its tail is read.
pub(crate) fn children_report(
    main_jsonl: &Path,
    returned: &std::collections::HashSet<String>,
) -> Result<ChildrenReport> {
    let mut report = ChildrenReport::default();
    // The DISCOVERED lanes, because discovery already knows which run a workflow lane came
    // from (`workflow_id`, the run directory it was found in) and that a built-in lane has
    // none. Re-deriving the run from the lane's path tested directory NAMES, which a project
    // directory that is itself named like a sidecar component can satisfy.
    let subs = crate::subagent::discover_subagents(main_jsonl).unwrap_or_default();
    // Every run's journal and manifest, read once, and the one settle rule `agents` reads too.
    let mut journals = crate::subagent::JournalCache::build(&subs);
    journals.add_session_runs(main_jsonl);
    for lane in &subs {
        let sub = &lane.path;
        let sid = crate::subagent::session_id_from_path(sub);
        let by_run = journals.settled_by_run(lane);
        let (state, detail) = if returned.contains(&sid) {
            (
                "settled",
                "completion notification received in the main transcript".to_string(),
            )
        } else if let Some(why) = by_run {
            ("settled", why)
        } else {
            tail_state(sub)?
        };
        if state != "settled" {
            report.live_count += 1;
        }
        report.children.push(ChildState {
            session_id: sid,
            state,
            detail,
        });
    }

    // Workflow journals: started - closed per run that has NOT ended (incremental, so an
    // imbalance there is a LIVE signal).
    report.journal_in_flight = journals.journal_in_flight();
    report.live_count += report.journal_in_flight;
    Ok(report)
}

/// A lane's state read off its own transcript tail: `in-flight` on an unreturned call,
/// `generating` on a fresh tail whose last assistant record is not an end_turn, else `settled`.
fn tail_state(sub: &Path) -> Result<(&'static str, String)> {
    let shape = tail_shape(sub)?;
    // The RECORD-tail instant is the semantic fact (mtime can lag or lead it).
    let tail_age = shape.last_ts_utc.as_deref().and_then(|t| age_secs(Some(t)));
    Ok(if let Some((tool, ts)) = &shape.unreturned_use {
        (
            "in-flight",
            format!(
                "unreturned {tool} call{}",
                age_secs(ts.as_deref())
                    .map(|a| format!(" ({a}s ago)"))
                    .unwrap_or_default()
            ),
        )
    } else if tail_age.is_some_and(|a| a <= CHILD_RECENT_SECS)
        && shape.last_stop_reason.as_deref() != Some("end_turn")
    {
        // A paired tail with a fresh record and no end_turn = mid-generation (the
        // model is thinking/writing between tool calls; stop_reason alone would
        // mark 73% of DEAD lanes live, so recency is the load-bearing conjunct).
        (
            "generating",
            format!(
                "last record {}s ago, no end_turn yet",
                tail_age.unwrap_or(0)
            ),
        )
    } else {
        (
            "settled",
            tail_age.map_or_else(
                || "no timestamped tail".to_string(),
                |a| format!("last record {a}s ago"),
            ),
        )
    })
}
