//! Child-lane liveness: subagent transcripts + the incremental workflow journal.
//!
//! `subagents/*.meta.json` has NO status field (child liveness can never come from
//! meta), and workflow RESULT files are terminal-only - but `journal.jsonl` inside each
//! `wf_*` dir is written INCREMENTALLY (`{type:"started",agentId}` at spawn,
//! `{type:"result",agentId}` at return), so `started - result` = workflow agents in
//! flight right now. A non-workflow child's liveness comes from its own transcript tail
//! (an unreturned tool call) plus recent growth (child transcripts grow only from the
//! child's own message flow - notifications land in MAIN only, so recency is a real
//! signal here, unlike the main lane's F9 trap).

use super::*;

/// A child transcript's mtime younger than this counts as "recently active" evidence
/// beside the tail shape (a subagent flushes per content block, so a working child's
/// transcript moves constantly).
const CHILD_RECENT_SECS: i64 = 15;

#[derive(Debug, Clone)]
pub(crate) struct ChildState {
    pub(crate) session_id: String,
    /// `in-flight` (unreturned tail call) | `active` (recent growth, no pending call) |
    /// `settled`.
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

/// Inspect every child lane of `main_jsonl`.
pub(crate) fn children_report(main_jsonl: &Path) -> Result<ChildrenReport> {
    let mut report = ChildrenReport::default();
    let subs = crate::subagent::subagent_transcript_files(main_jsonl).unwrap_or_default();
    for sub in &subs {
        let sid = crate::subagent::session_id_from_path(sub);
        let shape = tail_shape(sub)?;
        let mtime_age = std::fs::metadata(sub)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        let (state, detail) = if let Some((tool, ts)) = &shape.unreturned_use {
            (
                "in-flight",
                format!(
                    "unreturned {tool} call{}",
                    age_secs(ts.as_deref())
                        .map(|a| format!(" ({a}s ago)"))
                        .unwrap_or_default()
                ),
            )
        } else if mtime_age.is_some_and(|a| a <= CHILD_RECENT_SECS) {
            (
                "active",
                format!("transcript grew {}s ago", mtime_age.unwrap_or(0)),
            )
        } else {
            (
                "settled",
                shape
                    .last_ts_utc
                    .as_deref()
                    .and_then(|t| age_secs(Some(t)))
                    .map_or_else(
                        || "no timestamped tail".to_string(),
                        |a| format!("last record {a}s ago"),
                    ),
            )
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

    // Workflow journals: started - result per wf dir (incremental, so an imbalance is a
    // LIVE signal - a terminal dump would always balance).
    let sub_root = main_jsonl
        .with_extension("")
        .join("subagents")
        .join("workflows");
    if sub_root.is_dir() {
        for wf in std::fs::read_dir(&sub_root)? {
            let wf = wf?;
            let journal = wf.path().join("journal.jsonl");
            if !journal.is_file() {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&journal) else {
                continue;
            };
            let mut started = 0usize;
            let mut resulted = 0usize;
            for line in raw.lines() {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };
                match v.get("type").and_then(serde_json::Value::as_str) {
                    Some("started") => started += 1,
                    Some("result") => resulted += 1,
                    _ => {}
                }
            }
            report.journal_in_flight += started.saturating_sub(resulted);
        }
    }
    if report.journal_in_flight > 0 {
        report.live_count += report.journal_in_flight;
    }
    Ok(report)
}
