//! `agents` subcommand — a session's subagent TOPOLOGY, with time-window filters.
//!
//! For each in-scope top-level session, build the toolUseId-linked topology (see
//! [`crate::subagent::build_topology`]): discover its subagent transcripts (built-in
//! Task/Agent-tool + workflow / OMC agents), join each back to the parent tool_use that
//! spawned it, and emit one [`SubagentNode`] per subagent carrying its id, kind,
//! `agentType`, TRUE trigger time (the parent tool_use ts), start + completion
//! timestamps, status, the 3-way-resolved returned message (on demand), and the
//! files-changed list (on demand). `--tree` renders workflow RUN nodes (from the
//! top-level `workflows/wf_*.json` manifests) as parents of their workflow agents.
//!
//! `--since`/`--until` (ISO8601 or relative, system-local) filter by TRIGGER time by
//! default; `--by start|completion` switch axis. Files are processed in parallel across
//! sessions, then sorted for deterministic output.

use std::path::Path;

use anyhow::Result;
use rayon::prelude::*;

use crate::cli::{AgentKindFilter, AgentTimeAxis, AgentsArgs, OutputFormat};
use crate::path;
use crate::subagent::{
    build_topology, discover_workflow_runs, duration_label, SubagentKind, SubagentNode, WorkflowRun,
};
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

/// A session's topology: its linked subagent nodes plus its workflow RUN manifests.
struct SessionTopology {
    nodes: Vec<SubagentNode>,
    workflow_runs: Vec<WorkflowRun>,
}

/// Entry point for `csift agents`.
pub fn run_agents(args: &AgentsArgs) -> Result<()> {
    // Resolve the target session files (PATH(s) + optional --session). With neither,
    // every project is scanned — the same target model as list/search. `agents`
    // discovers each session's subagents itself, so it never spans subagent TRANSCRIPT
    // files here (include_subagents=false).
    let session_files =
        path::resolve_session_files(&args.paths, args.session.as_deref(), false.into())?;

    let time_window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;

    // A single `--agent <hex>` grab implies the returned message + a node-level view.
    let want_returned = args.returned_message || args.agent.is_some();
    // Files are needed when explicitly asked, or for a single-agent grab.
    let want_files = args.with_files || args.agent.is_some();

    // Parallel across sessions: build the linked topology for each.
    let topos: Vec<SessionTopology> = session_files
        .par_iter()
        .map(|sf| topology_for_session(sf, want_files))
        .collect::<Result<Vec<_>>>()?;

    // Flatten nodes; keep workflow runs keyed per session for the tree.
    let mut nodes: Vec<SubagentNode> = Vec::new();
    let mut workflow_runs: Vec<WorkflowRun> = Vec::new();
    for t in topos {
        nodes.extend(t.nodes);
        workflow_runs.extend(t.workflow_runs);
    }

    // Filter: single-agent grab first (exact bare-hex id), then kind, then time window.
    if let Some(want_id) = args.agent.as_deref() {
        nodes.retain(|n| n.agent_id == want_id);
    }
    nodes.retain(|n| kind_allowed(n.kind, &args.kinds));
    nodes.retain(|n| window_admits(n, &time_window, args.by));

    // Deterministic order: by (parent session, trigger time, agent id).
    nodes.sort_by(|a, b| {
        (
            &a.parent_session_id,
            a.trigger_utc.as_deref().unwrap_or(""),
            &a.agent_id,
        )
            .cmp(&(
                &b.parent_session_id,
                b.trigger_utc.as_deref().unwrap_or(""),
                &b.agent_id,
            ))
    });

    // A workflow dir can exist (with a journal + agents) BEFORE its top-level
    // `workflows/wf_*.json` run-manifest is written (an in-flight run) — or after the
    // manifest is pruned. Without a synthesized stand-in, the `--tree` view drops every
    // such agent, because both tree renderers emit a workflow agent ONLY as a child of a
    // matched run. Synthesize a minimal `WorkflowRun` for any in-scope workflow_id that no
    // real manifest covers, so the tree never silently loses a real workflow cluster.
    augment_unmanifested_runs(&nodes, &mut workflow_runs);

    let view = View {
        want_returned,
        want_files,
        tree: args.tree,
    };

    match args.format {
        OutputFormat::Text => render_text(&nodes, &workflow_runs, args, &view),
        OutputFormat::Json => render_json(&nodes, &workflow_runs, &view)?,
    }
    Ok(())
}

/// Add a placeholder [`WorkflowRun`] for any `workflow_id` present on an (already-filtered)
/// node but absent from `workflow_runs` (no top-level manifest — an in-flight or
/// manifest-pruned run). The placeholder carries only the `run_id` (== `workflow_id`); its
/// run-level fields stay `None` so the renderers print just the header + the nested agents.
/// Without this, `--tree` silently drops those agents (they render only as a run's children).
fn augment_unmanifested_runs(nodes: &[SubagentNode], workflow_runs: &mut Vec<WorkflowRun>) {
    use std::collections::BTreeSet;
    let known: BTreeSet<&str> = workflow_runs.iter().map(|r| r.run_id.as_str()).collect();
    // Preserve first-seen (sorted-node) order while de-duplicating.
    let mut missing: Vec<String> = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for n in nodes {
        if let Some(wf) = n.workflow_id.as_deref() {
            if !known.contains(wf) && seen.insert(wf) {
                missing.push(wf.to_string());
            }
        }
    }
    for run_id in missing {
        workflow_runs.push(WorkflowRun {
            run_id,
            task_id: None,
            workflow_name: None,
            status: None,
            agent_count: None,
            duration_ms: None,
            total_tokens: None,
            total_tool_calls: None,
            default_model: None,
            started_utc: None,
        });
    }
}

/// What detail the current invocation wants surfaced.
struct View {
    want_returned: bool,
    want_files: bool,
    tree: bool,
}

/// Build the linked topology + read the workflow-run manifests for one top-level session.
fn topology_for_session(session_jsonl: &Path, with_files: bool) -> Result<SessionTopology> {
    let nodes = build_topology(session_jsonl, with_files)?;
    let workflow_runs = if nodes.is_empty() {
        Vec::new()
    } else {
        discover_workflow_runs(session_jsonl)?
    };
    Ok(SessionTopology {
        nodes,
        workflow_runs,
    })
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

/// True when a node falls inside the time window on the chosen axis. An unbounded window
/// admits everything (incl. nodes missing the axis timestamp). A bounded window NEVER
/// admits a node whose axis timestamp is absent (same rule as `search`/`files` — no
/// fabricated inclusion).
fn window_admits(node: &SubagentNode, window: &TimeWindow, axis: AgentTimeAxis) -> bool {
    if window.is_unbounded() {
        return true;
    }
    let ts = match axis {
        AgentTimeAxis::Trigger => node.trigger_utc.as_deref(),
        AgentTimeAxis::Start => node.started_utc.as_deref(),
        AgentTimeAxis::Completion => node.completed_utc.as_deref(),
    };
    window.contains(ts)
}

// ── Rendering ──

fn axis_label(axis: AgentTimeAxis) -> &'static str {
    match axis {
        AgentTimeAxis::Trigger => "trigger",
        AgentTimeAxis::Start => "start",
        AgentTimeAxis::Completion => "completion",
    }
}

fn render_text(
    nodes: &[SubagentNode],
    workflow_runs: &[WorkflowRun],
    args: &AgentsArgs,
    view: &View,
) {
    if nodes.is_empty() {
        println!("no subagents found");
        return;
    }

    if view.tree {
        render_tree_text(nodes, workflow_runs, view);
    } else {
        render_flat_text(nodes, view);
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
    println!();
    println!(
        "{} subagent(s)  ·  kind={kinds}  ·  window-axis={}",
        nodes.len(),
        axis_label(args.by)
    );
}

/// Flat (non-tree) text: one block per node, grouped under its parent session header.
fn render_flat_text(nodes: &[SubagentNode], view: &View) {
    let mut last_session: Option<&str> = None;
    for n in nodes {
        if last_session != Some(n.parent_session_id.as_str()) {
            if last_session.is_some() {
                println!();
            }
            println!("SESSION  {}", n.parent_session_id);
            last_session = Some(n.parent_session_id.as_str());
        }
        print_node_block(n, view, 1);
    }
}

/// Tree text: each session → its workflow RUN nodes (with their agents nested) → then the
/// remaining built-in agents. Joins workflow agents to a run by `workflow_id == run_id`.
fn render_tree_text(nodes: &[SubagentNode], workflow_runs: &[WorkflowRun], view: &View) {
    use std::collections::BTreeMap;

    // Group nodes by parent session.
    let mut by_session: BTreeMap<&str, Vec<&SubagentNode>> = BTreeMap::new();
    for n in nodes {
        by_session
            .entry(n.parent_session_id.as_str())
            .or_default()
            .push(n);
    }
    // Group workflow runs by session is implicit (runs carry no session id), so we match
    // by the workflow_id present on the in-scope nodes: a run is shown when at least one
    // of its agents is in scope.
    let in_scope_wf: std::collections::BTreeSet<&str> = nodes
        .iter()
        .filter_map(|n| n.workflow_id.as_deref())
        .collect();

    let mut first = true;
    for (session, snodes) in &by_session {
        if !first {
            println!();
        }
        first = false;
        println!("SESSION  {session}");

        // Workflow runs (as parent nodes) + their agents nested under them.
        for run in workflow_runs {
            if !in_scope_wf.contains(run.run_id.as_str()) {
                continue;
            }
            let agents: Vec<&&SubagentNode> = snodes
                .iter()
                .filter(|n| n.workflow_id.as_deref() == Some(run.run_id.as_str()))
                .collect();
            if agents.is_empty() {
                continue;
            }
            print_workflow_run(run);
            for n in agents {
                print_node_block(n, view, 2);
            }
        }

        // Built-in agents (no workflow_id) at the top level of the session.
        for n in snodes.iter().filter(|n| n.workflow_id.is_none()) {
            print_node_block(n, view, 1);
        }
    }
}

/// Print a workflow RUN header line (the tree parent of its agents).
fn print_workflow_run(run: &WorkflowRun) {
    let mut head = format!("  WORKFLOW  {}", run.run_id);
    if let Some(name) = &run.workflow_name {
        head.push_str(&format!("  [{name}]"));
    }
    if let Some(s) = &run.status {
        head.push_str(&format!("  {s}"));
    }
    println!("{head}");
    if let Some(n) = run.agent_count {
        println!("      agents     {n}");
    }
    if let Some(ms) = run.duration_ms {
        println!("      duration   {}", fmt_ms(ms));
    }
    if let Some(t) = run.total_tokens {
        println!("      tokens     {t}");
    }
    if let Some(m) = &run.default_model {
        println!("      model      {m}");
    }
}

/// Print one node's text block, indented `depth` levels (2 spaces per level).
fn print_node_block(n: &SubagentNode, view: &View, depth: usize) {
    let ind = "  ".repeat(depth);
    let ind2 = "  ".repeat(depth + 1);

    let mut head = format!("{ind}{}  {}", n.agent_id, n.kind.label());
    if let Some(wf) = &n.workflow_id {
        head.push_str(&format!("  ({wf})"));
    }
    if let Some(t) = &n.agent_type {
        head.push_str(&format!("  [{t}]"));
    }
    head.push_str(&format!("  {}", n.status.label()));
    println!("{head}");

    if let Some(d) = &n.description {
        println!("{ind2}desc       {d}");
    }
    println!(
        "{ind2}triggered  {}",
        format_timestamp(n.trigger_utc.as_deref())
    );
    println!(
        "{ind2}started    {}",
        format_timestamp(n.started_utc.as_deref())
    );
    println!(
        "{ind2}completed  {}",
        format_timestamp(n.completed_utc.as_deref())
    );
    if let Some(dur) = duration_label(n.trigger_utc.as_deref(), n.completed_utc.as_deref()) {
        println!("{ind2}duration   {dur}");
    }
    if view.want_returned {
        if let (Some(msg), Some(src)) = (&n.returned_message, n.returned_message_source) {
            println!("{ind2}returned   ({}) {}", src.label(), one_line(msg));
        } else {
            println!("{ind2}returned   (unresolved)");
        }
    }
    if view.want_files {
        if n.files_changed.is_empty() {
            println!("{ind2}files      (none)");
        } else {
            println!("{ind2}files      {} changed", n.files_changed.len());
            for (path, op, is_create) in &n.files_changed {
                let tag = if *is_create { "create" } else { op.as_str() };
                println!("{ind2}  {tag:<12} {path}");
            }
        }
    }
    if n.skipped_lines > 0 {
        println!(
            "{ind2}note       {} malformed line(s) skipped",
            n.skipped_lines
        );
    }
}

fn render_json(nodes: &[SubagentNode], workflow_runs: &[WorkflowRun], view: &View) -> Result<()> {
    use std::collections::BTreeSet;

    if view.tree {
        // Tree JSON: one object per workflow run with its agents nested, then the
        // built-in agents at the top level — grouped per session.
        use std::collections::BTreeMap;
        let mut by_session: BTreeMap<&str, Vec<&SubagentNode>> = BTreeMap::new();
        for n in nodes {
            by_session
                .entry(n.parent_session_id.as_str())
                .or_default()
                .push(n);
        }
        let in_scope_wf: BTreeSet<&str> = nodes
            .iter()
            .filter_map(|n| n.workflow_id.as_deref())
            .collect();

        for (session, snodes) in &by_session {
            let mut runs_json = Vec::new();
            for run in workflow_runs {
                if !in_scope_wf.contains(run.run_id.as_str()) {
                    continue;
                }
                let children: Vec<_> = snodes
                    .iter()
                    .filter(|n| n.workflow_id.as_deref() == Some(run.run_id.as_str()))
                    .map(|n| node_json(n, view))
                    .collect();
                if children.is_empty() {
                    continue;
                }
                runs_json.push(workflow_run_json(run, children));
            }
            let builtins: Vec<_> = snodes
                .iter()
                .filter(|n| n.workflow_id.is_none())
                .map(|n| node_json(n, view))
                .collect();
            let obj = serde_json::json!({
                "session_id": session,
                "workflow_runs": runs_json,
                "agents": builtins,
            });
            println!("{}", serde_json::to_string(&obj)?);
        }
        return Ok(());
    }

    // Flat JSON: one node per line.
    for n in nodes {
        println!("{}", serde_json::to_string(&node_json(n, view))?);
    }
    Ok(())
}

/// One node's JSON object. `returned_message` / `files_changed` are included only when the
/// view asked for them (so a plain listing stays compact).
fn node_json(n: &SubagentNode, view: &View) -> serde_json::Value {
    use serde_json::json;
    let mut obj = json!({
        "agent_id": n.agent_id,
        "kind": n.kind.label(),
        "parent_session_id": n.parent_session_id,
        "parent_agent_id": n.parent_agent_id,
        "spawn_tool_use_id": n.spawn_tool_use_id,
        "spawn_tool": n.spawn_tool,
        "workflow_id": n.workflow_id,
        "agent_type": n.agent_type,
        "description": n.description,
        "trigger_utc": n.trigger_utc,
        "trigger_local": n.trigger_utc.as_deref().and_then(local_iso),
        "started_utc": n.started_utc,
        "started_local": n.started_utc.as_deref().and_then(local_iso),
        "completed_utc": n.completed_utc,
        "completed_local": n.completed_utc.as_deref().and_then(local_iso),
        "duration": duration_label(n.trigger_utc.as_deref(), n.completed_utc.as_deref()),
        "status": n.status.label(),
        "depth": n.depth,
        "skipped_lines": n.skipped_lines,
    });
    let map = obj.as_object_mut().expect("json object");
    if view.want_returned {
        map.insert(
            "returned_message".to_string(),
            n.returned_message
                .clone()
                .map_or(serde_json::Value::Null, serde_json::Value::String),
        );
        map.insert(
            "returned_message_source".to_string(),
            n.returned_message_source
                .map_or(serde_json::Value::Null, |s| {
                    serde_json::Value::String(s.label().to_string())
                }),
        );
    }
    if view.want_files {
        let files: Vec<_> = n
            .files_changed
            .iter()
            .map(|(path, op, is_create)| json!({ "path": path, "op": op, "is_create": is_create }))
            .collect();
        map.insert("files_changed".to_string(), serde_json::Value::Array(files));
    }
    if !n.children.is_empty() {
        let kids: Vec<_> = n.children.iter().map(|c| node_json(c, view)).collect();
        map.insert("children".to_string(), serde_json::Value::Array(kids));
    }
    obj
}

/// A workflow RUN's JSON object with its agents nested under `children`.
fn workflow_run_json(run: &WorkflowRun, children: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "run_id": run.run_id,
        "task_id": run.task_id,
        "workflow_name": run.workflow_name,
        "status": run.status,
        "agent_count": run.agent_count,
        "duration_ms": run.duration_ms,
        "total_tokens": run.total_tokens,
        "total_tool_calls": run.total_tool_calls,
        "default_model": run.default_model,
        "started_utc": run.started_utc,
        "children": children,
    })
}

/// Collapse a (possibly multi-line) returned message to a single line for the text view.
/// CODEPOINT-SAFE: replaces whitespace runs without ever slicing into a char.
fn one_line(s: &str) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 200;
    if collapsed.chars().count() <= MAX {
        return collapsed;
    }
    // Truncate on a CHAR boundary (never a byte offset), then mark the elision.
    let truncated: String = collapsed.chars().take(MAX).collect();
    format!("{truncated}…")
}

/// Format a millisecond duration compactly (workflow manifest `durationMs`).
fn fmt_ms(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h{m:02}m")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subagent::{ReturnedMsgSource, SubagentStatus};

    fn node(
        trigger: Option<&str>,
        start: Option<&str>,
        complete: Option<&str>,
        kind: SubagentKind,
    ) -> SubagentNode {
        SubagentNode {
            agent_id: "abc123".to_string(),
            kind,
            parent_session_id: "sess".to_string(),
            parent_agent_id: None,
            spawn_tool_use_id: Some("toolu_x".to_string()),
            spawn_tool: Some("Agent".to_string()),
            workflow_id: None,
            agent_type: None,
            description: None,
            trigger_utc: trigger.map(str::to_string),
            started_utc: start.map(str::to_string),
            completed_utc: complete.map(str::to_string),
            returned_message: None,
            returned_message_source: None,
            status: SubagentStatus::Completed,
            files_changed: Vec::new(),
            depth: 0,
            children: Vec::new(),
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
    fn window_on_trigger_axis_is_the_default() {
        // Trigger at 05:00 → before a 06:00 lower bound → excluded on the TRIGGER axis.
        let w = TimeWindow::from_args(Some("2026-06-07T06:00:00Z"), None).unwrap();
        let n = node(
            Some("2026-06-07T05:00:00Z"),
            Some("2026-06-07T05:00:05Z"),
            Some("2026-06-07T07:00:00Z"),
            SubagentKind::BuiltinTask,
        );
        assert!(!window_admits(&n, &w, AgentTimeAxis::Trigger));
        // …but its COMPLETION (07:00) is inside the window.
        assert!(window_admits(&n, &w, AgentTimeAxis::Completion));
    }

    #[test]
    fn trigger_and_start_can_diverge_across_the_bound() {
        // The trigger LAGS into start by seconds; a bound between them admits on one axis
        // but not the other — proving the axis choice is load-bearing.
        let w = TimeWindow::from_args(Some("2026-06-07T05:00:03Z"), None).unwrap();
        let n = node(
            Some("2026-06-07T05:00:00Z"), // triggered before the bound
            Some("2026-06-07T05:00:05Z"), // started after the bound
            Some("2026-06-07T05:10:00Z"),
            SubagentKind::BuiltinTask,
        );
        assert!(!window_admits(&n, &w, AgentTimeAxis::Trigger));
        assert!(window_admits(&n, &w, AgentTimeAxis::Start));
    }

    #[test]
    fn unbounded_window_admits_even_missing_timestamp() {
        let w = TimeWindow::default();
        let n = node(None, None, None, SubagentKind::Workflow);
        assert!(window_admits(&n, &w, AgentTimeAxis::Trigger));
        assert!(window_admits(&n, &w, AgentTimeAxis::Start));
        assert!(window_admits(&n, &w, AgentTimeAxis::Completion));
    }

    #[test]
    fn bounded_window_excludes_missing_axis_timestamp() {
        let w = TimeWindow::from_args(Some("2026-06-01"), None).unwrap();
        // No trigger timestamp at all → a bounded trigger-window must NOT admit it.
        let n = node(
            None,
            None,
            Some("2026-06-07T05:00:00Z"),
            SubagentKind::Workflow,
        );
        assert!(!window_admits(&n, &w, AgentTimeAxis::Trigger));
        // The completion axis (present + in range) does admit it.
        assert!(window_admits(&n, &w, AgentTimeAxis::Completion));
    }

    #[test]
    fn one_line_collapses_and_is_codepoint_safe() {
        assert_eq!(one_line("a\n  b\tc"), "a b c");
        // A long CJK string truncated on a CHAR boundary never panics.
        let cjk = "x".repeat(100); // 400 chars
        let out = one_line(&cjk);
        assert!(out.chars().count() <= 201); // 200 + the ellipsis
        assert!(out.ends_with('…'));
    }

    #[test]
    fn node_json_omits_returned_and_files_unless_requested() {
        let n = node(
            Some("2026-06-07T05:00:00Z"),
            Some("2026-06-07T05:00:05Z"),
            Some("2026-06-07T05:10:00Z"),
            SubagentKind::BuiltinTask,
        );
        let lean = View {
            want_returned: false,
            want_files: false,
            tree: false,
        };
        let j = node_json(&n, &lean);
        assert!(j.get("returned_message").is_none());
        assert!(j.get("files_changed").is_none());
        // The trigger time IS surfaced and is the default duration anchor.
        assert_eq!(j["trigger_utc"], "2026-06-07T05:00:00Z");

        let rich = View {
            want_returned: true,
            want_files: true,
            tree: false,
        };
        let j2 = node_json(&n, &rich);
        assert!(j2.get("returned_message").is_some());
        assert!(j2.get("files_changed").is_some());
    }

    #[test]
    fn node_json_renders_returned_message_source() {
        let mut n = node(
            Some("2026-06-07T05:00:00Z"),
            None,
            None,
            SubagentKind::BuiltinTask,
        );
        n.returned_message = Some("the answer".to_string());
        n.returned_message_source = Some(ReturnedMsgSource::AsyncChildTail);
        let rich = View {
            want_returned: true,
            want_files: false,
            tree: false,
        };
        let j = node_json(&n, &rich);
        assert_eq!(j["returned_message"], "the answer");
        assert_eq!(j["returned_message_source"], "async-child-tail");
    }

    #[test]
    fn fmt_ms_compact() {
        assert_eq!(fmt_ms(3_000), "3s");
        assert_eq!(fmt_ms(125_000), "2m05s");
        assert_eq!(fmt_ms(3_700_000), "1h01m");
    }
}
