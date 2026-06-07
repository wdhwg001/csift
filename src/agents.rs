//! `agents` subcommand — a session's subagent lifecycle, with time-window filters.
//!
//! For each in-scope top-level session, discover its subagent transcripts (built-in
//! Task/Agent-tool + workflow / OMC agents — see [`crate::subagent`]) and emit one
//! lifecycle row per subagent: id, kind, `agentType` sub-label, start + completion
//! timestamps (system-local + raw UTC), duration, and a determinable status. The
//! workflow `journal.jsonl` is consulted for completion but never listed as an agent.
//!
//! `--since`/`--until` (ISO8601 or relative, system-local) filter by START time by
//! default; `--by completion` filters on completion instead. Files are processed in
//! parallel across sessions, then sorted for deterministic output.

use std::path::Path;

use anyhow::Result;
use rayon::prelude::*;

use crate::cli::{AgentKindFilter, AgentTimeAxis, AgentsArgs, OutputFormat};
use crate::path;
use crate::subagent::{
    discover_subagents, duration_label, lifecycle, SubagentKind, SubagentLifecycle,
};
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

/// Entry point for `csift agents`.
pub fn run_agents(args: &AgentsArgs) -> Result<()> {
    // Resolve the target session files (PATH(s) + optional --session). With neither,
    // every project is scanned — the same target model as list/search. `agents`
    // discovers each session's subagents itself, so it never spans subagent TRANSCRIPT
    // files here (include_subagents=false).
    let session_files = path::resolve_session_files(&args.paths, args.session.as_deref(), false)?;

    let time_window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;

    // Parallel across sessions: discover + compute lifecycle for each subagent.
    let mut rows: Vec<SubagentLifecycle> = session_files
        .par_iter()
        .map(|sf| lifecycles_for_session(sf))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();

    // Filter by kind, then by the time window on the chosen axis.
    rows.retain(|r| kind_allowed(r.kind, &args.kinds));
    rows.retain(|r| window_admits(r, &time_window, args.by));

    // Deterministic order: by (parent session, start time, agent id).
    rows.sort_by(|a, b| {
        (
            &a.parent_session_id,
            a.started_utc.as_deref().unwrap_or(""),
            &a.agent_id,
        )
            .cmp(&(
                &b.parent_session_id,
                b.started_utc.as_deref().unwrap_or(""),
                &b.agent_id,
            ))
    });

    match args.format {
        OutputFormat::Text => render_text(&rows, args),
        OutputFormat::Json => render_json(&rows)?,
    }
    Ok(())
}

/// All lifecycle rows for one top-level session (empty when it has no subagents).
fn lifecycles_for_session(session_jsonl: &Path) -> Result<Vec<SubagentLifecycle>> {
    let subs = discover_subagents(session_jsonl)?;
    subs.iter().map(lifecycle).collect()
}

/// True when `kind` passes the `--kind` filter (empty filter ⇒ all kinds).
fn kind_allowed(kind: SubagentKind, want: &[AgentKindFilter]) -> bool {
    if want.is_empty() {
        return true;
    }
    want.iter().any(|w| match w {
        AgentKindFilter::BuiltinTask => kind == SubagentKind::BuiltinTask,
        AgentKindFilter::Workflow => kind == SubagentKind::Workflow,
    })
}

/// True when a lifecycle row falls inside the time window on the chosen axis. An
/// unbounded window admits everything (incl. rows missing the axis timestamp). A
/// bounded window NEVER admits a row whose axis timestamp is absent (same rule as
/// `search` time filtering — no fabricated inclusion).
fn window_admits(row: &SubagentLifecycle, window: &TimeWindow, axis: AgentTimeAxis) -> bool {
    if window.is_unbounded() {
        return true;
    }
    let ts = match axis {
        AgentTimeAxis::Start => row.started_utc.as_deref(),
        AgentTimeAxis::Completion => row.completed_utc.as_deref(),
    };
    window.contains(ts)
}

// ── Rendering ──

fn render_text(rows: &[SubagentLifecycle], args: &AgentsArgs) {
    if rows.is_empty() {
        println!("no subagents found");
        return;
    }

    let mut last_session: Option<&str> = None;
    for r in rows {
        // Group rows under their parent session header.
        if last_session != Some(r.parent_session_id.as_str()) {
            if last_session.is_some() {
                println!();
            }
            println!("SESSION  {}", r.parent_session_id);
            last_session = Some(r.parent_session_id.as_str());
        }

        // agent-<hex>  kind  [agentType]  status
        let mut head = format!("  {}  {}", r.agent_id, r.kind.label());
        if let Some(wf) = &r.workflow_id {
            head.push_str(&format!("  ({wf})"));
        }
        if let Some(t) = &r.agent_type {
            head.push_str(&format!("  [{t}]"));
        }
        head.push_str(&format!("  {}", r.status.label()));
        println!("{head}");

        if let Some(d) = &r.description {
            println!("    desc       {d}");
        }
        println!(
            "    started    {}",
            format_timestamp(r.started_utc.as_deref())
        );
        println!(
            "    completed  {}",
            format_timestamp(r.completed_utc.as_deref())
        );
        if let Some(dur) = duration_label(r.started_utc.as_deref(), r.completed_utc.as_deref()) {
            println!("    duration   {dur}");
        }
        if r.skipped_lines > 0 {
            println!(
                "    note       {} malformed line(s) skipped",
                r.skipped_lines
            );
        }
    }

    // Footer with the filter context (so an empty-looking result is explained).
    let kinds = if args.kinds.is_empty() {
        "all".to_string()
    } else {
        args.kinds
            .iter()
            .map(|k| match k {
                AgentKindFilter::BuiltinTask => "builtin-task",
                AgentKindFilter::Workflow => "workflow",
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    let axis = match args.by {
        AgentTimeAxis::Start => "start",
        AgentTimeAxis::Completion => "completion",
    };
    println!();
    println!(
        "{} subagent(s)  ·  kind={kinds}  ·  window-axis={axis}",
        rows.len()
    );
}

fn render_json(rows: &[SubagentLifecycle]) -> Result<()> {
    use serde_json::json;
    for r in rows {
        let obj = json!({
            "agent_id": r.agent_id,
            "kind": r.kind.label(),
            "parent_session_id": r.parent_session_id,
            "workflow_id": r.workflow_id,
            "agent_type": r.agent_type,
            "description": r.description,
            "started_utc": r.started_utc,
            "started_local": r.started_utc.as_deref().and_then(local_iso),
            "completed_utc": r.completed_utc,
            "completed_local": r.completed_utc.as_deref().and_then(local_iso),
            "duration": duration_label(r.started_utc.as_deref(), r.completed_utc.as_deref()),
            "status": r.status.label(),
            "skipped_lines": r.skipped_lines,
        });
        println!("{}", serde_json::to_string(&obj)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subagent::SubagentStatus;

    fn row(start: Option<&str>, complete: Option<&str>, kind: SubagentKind) -> SubagentLifecycle {
        SubagentLifecycle {
            agent_id: "agent-x".to_string(),
            kind,
            parent_session_id: "sess".to_string(),
            workflow_id: None,
            agent_type: None,
            description: None,
            started_utc: start.map(str::to_string),
            completed_utc: complete.map(str::to_string),
            status: SubagentStatus::Completed,
            skipped_lines: 0,
        }
    }

    #[test]
    fn kind_filter_empty_allows_all() {
        assert!(kind_allowed(SubagentKind::BuiltinTask, &[]));
        assert!(kind_allowed(SubagentKind::Workflow, &[]));
    }

    #[test]
    fn kind_filter_restricts() {
        let want = vec![AgentKindFilter::Workflow];
        assert!(!kind_allowed(SubagentKind::BuiltinTask, &want));
        assert!(kind_allowed(SubagentKind::Workflow, &want));
    }

    #[test]
    fn window_on_start_axis() {
        let w = TimeWindow::from_args(Some("2026-06-07T06:00:00Z"), None).unwrap();
        // started at 05:00 → before a 06:00 lower bound → excluded on the start axis.
        let r = row(
            Some("2026-06-07T05:00:00Z"),
            Some("2026-06-07T07:00:00Z"),
            SubagentKind::BuiltinTask,
        );
        assert!(!window_admits(&r, &w, AgentTimeAxis::Start));
        // …but its COMPLETION (07:00) is inside the window.
        assert!(window_admits(&r, &w, AgentTimeAxis::Completion));
    }

    #[test]
    fn unbounded_window_admits_even_missing_timestamp() {
        let w = TimeWindow::default();
        let r = row(None, None, SubagentKind::Workflow);
        assert!(window_admits(&r, &w, AgentTimeAxis::Start));
        assert!(window_admits(&r, &w, AgentTimeAxis::Completion));
    }

    #[test]
    fn bounded_window_excludes_missing_axis_timestamp() {
        let w = TimeWindow::from_args(Some("2026-06-01"), None).unwrap();
        // No start timestamp at all → a bounded start-window must NOT admit it.
        let r = row(None, Some("2026-06-07T05:00:00Z"), SubagentKind::Workflow);
        assert!(!window_admits(&r, &w, AgentTimeAxis::Start));
        // The completion axis (present + in range) does admit it.
        assert!(window_admits(&r, &w, AgentTimeAxis::Completion));
    }
}
