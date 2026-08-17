//! run_agents: topology build, filters, windows, teammate hint.

use super::*;

/// A session's topology: its linked subagent nodes plus its workflow RUN manifests.
pub(crate) struct SessionTopology {
    pub(crate) nodes: Vec<SubagentNode>,
    pub(crate) workflow_runs: Vec<WorkflowRun>,
}

/// Entry point for `csift agents`.
pub fn run_agents(args: &AgentsArgs) -> Result<()> {
    // `agents` has no subagent-span flag - reject the (hidden, no-op) `--no-subagents` with a
    // pointed message instead of letting `allow_hyphen_values` swallow it as a bogus PATH value.
    if let Some(msg) = args.span_flag_error() {
        bail!(msg);
    }

    // Resolve the target session files from the positional target(s). With none, every
    // project is scanned - the same target model as list/search. `agents` discovers each
    // session's subagents itself, so it never spans subagent TRANSCRIPT files here - pass
    // `false` (⇒ `SubagentScope::TopLevelOnly`).
    let session_files = path::resolve_targets_with_session_list(
        &args.paths,
        args.sessions_from.as_deref(),
        false.into(),
        path::Caller::Other,
    )?;

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

    // `--agent <hex>` is a DIRECT id lookup: it BYPASSES the --since/--until/--order-by time
    // window AND the --shape filter (a known id should resolve regardless of when it ran or
    // its shape), and a no-match is a hard error with discovery guidance - never the
    // ambiguous `no subagents found` (which a zero-subagent session also prints). The grab
    // renders a single node (a tree of one), not the whole workflow tree.
    if let Some(want_id) = args.agent.as_deref() {
        nodes.retain(|n| n.agent_id == want_id);
        if nodes.is_empty() {
            bail!(
                "no subagent matched id `{want_id}` in scope. List valid ids first with \
                 `csift agents @<uuid>` (or `csift agents <project-path>`) and read \
                 the `agent_id` column / JSON field, then pass one to `--agent`."
            );
        }
    } else {
        nodes.retain(|n| kind_allowed(n.kind, &args.kinds));
        nodes.retain(|n| window_admits(n, &time_window, args.order_by));
    }

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
    // `workflows/wf_*.json` run-manifest is written (an in-flight run) - or after the
    // manifest is pruned. Without a synthesized stand-in, the tree view drops every
    // such agent, because both tree renderers emit a workflow agent ONLY as a child of a
    // matched run. Synthesize a minimal `WorkflowRun` for any in-scope workflow_id that no
    // real manifest covers, so the tree never silently loses a real workflow cluster.
    augment_unmanifested_runs(&nodes, &mut workflow_runs);

    // The output is ALWAYS the parent->child tree. A single `--agent <hex>` grab renders
    // just that one node (a tree of one): `single_node` suppresses the whole-workflow
    // topology so the grab never dumps every sibling under a WORKFLOW header.
    let view = View {
        want_returned,
        want_files,
        single_node: args.agent.is_some(),
    };

    match args.format {
        OutputFormat::Text => render_text(&nodes, &workflow_runs, args, &view),
        OutputFormat::Json => render_json(&nodes, &workflow_runs, &view)?,
    }
    Ok(())
}

/// Add a placeholder [`WorkflowRun`] for any `workflow_id` present on an (already-filtered)
/// node but absent from `workflow_runs` (no top-level manifest - an in-flight or
/// manifest-pruned run). The placeholder carries only the `run_id` (== `workflow_id`); its
/// run-level fields stay `None` so the renderers print just the header + the nested agents.
/// Without this, the tree silently drops those agents (they render only as a run's children).
pub(crate) fn augment_unmanifested_runs(
    nodes: &[SubagentNode],
    workflow_runs: &mut Vec<WorkflowRun>,
) {
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
pub(crate) struct View {
    pub(crate) want_returned: bool,
    pub(crate) want_files: bool,
    /// A single `--agent <hex>` grab: render JUST the matched node (a tree of one), with no
    /// SESSION/WORKFLOW headers and no nested-topology walk.
    pub(crate) single_node: bool,
}

/// Build the linked topology + read the workflow-run manifests for one top-level session.
pub(crate) fn topology_for_session(
    session_jsonl: &Path,
    with_files: bool,
) -> Result<SessionTopology> {
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

/// True when `kind` passes the `--shape` filter (empty filter ⇒ all kinds).
pub(crate) fn kind_allowed(kind: SubagentKind, want: &[AgentKindFilter]) -> bool {
    if want.is_empty() {
        return true;
    }
    want.iter().any(|w| match w {
        AgentKindFilter::BuiltinTask => kind == SubagentKind::BuiltinTask,
        AgentKindFilter::Workflow => kind == SubagentKind::Workflow,
        AgentKindFilter::Teammate => kind == SubagentKind::Teammate,
    })
}

/// True when a node falls inside the time window on the chosen axis. An unbounded window
/// admits everything (incl. nodes missing the axis timestamp). A bounded window NEVER
/// admits a node whose axis timestamp is absent (same rule as `search`/`files` - no
/// fabricated inclusion).
pub(crate) fn window_admits(node: &SubagentNode, window: &TimeWindow, axis: AgentTimeAxis) -> bool {
    if window.is_unbounded() {
        return true;
    }
    let ts = match axis {
        AgentTimeAxis::Trigger => node.trigger_utc.as_deref(),
        AgentTimeAxis::Start => node.started_utc.as_deref(),
        // The completion axis windows on the lane's TERMINAL instant - the tail
        // newest-record ts (== the completion instant on a completed lane, the
        // freeze/last-activity instant otherwise), exactly the long-documented
        // "--order-by completion = the last record's ts". `completed_utc` itself is
        // status-gated (None unless Completed), which would silently drop every
        // frozen lane from a bounded window - the one shape a monitor asks about.
        AgentTimeAxis::Completion => node.last_activity_utc.as_deref(),
    };
    window.contains(ts)
}

// ── Rendering ──

/// True if any node in the tree (self or any descendant) is a teammate.
pub(crate) fn any_teammate(nodes: &[SubagentNode]) -> bool {
    nodes
        .iter()
        .any(|n| n.kind == SubagentKind::Teammate || any_teammate(&n.children))
}

/// Control-mechanism hint for teammates (`in_process_teammate`), surfaced in `agents` text
/// output whenever the scope holds ≥1 teammate. csift is the LLM's only window into a session's
/// teammates, so it is the natural place to point at the CORRECT control tool - a real session
/// burned ~30 min trying to `TaskStop` / `pkill` a runaway teammate (feeding it the name, the
/// `Name@team` form, AND the exact `aName-<hash>` agentId csift prints) before discovering the
/// mechanism. The ids were not wrong; the TOOL was. Stated from the verified `SendMessage`
/// contract (address by NAME; `message:{type:"shutdown_request"}` terminates), not a guess.
/// Read-only csift cannot act - it only names the tool that can.
pub(crate) const TEAMMATE_CONTROL_HINT_L1: &str = "note: teammate rows are in-process Agent subagents — address one BY NAME (the `(@name)` shown) \
via SendMessage to steer it, and `message:{\"type\":\"shutdown_request\"}` to terminate it.";
pub(crate) const TEAMMATE_CONTROL_HINT_L2: &str =
    "      A teammate is NOT a background task (TaskStop / a `task_id` will not find it) and has no \
separate OS process (it shares the orchestrator PID — `pkill` won't help).";

/// The compact JSON-surface twin of [`TEAMMATE_CONTROL_HINT_L1`]/`_L2` - emitted as a teammate
/// node's `control_hint` field so a `--format json` consumer gets the same pointer.
pub(crate) const TEAMMATE_CONTROL_HINT_JSON: &str =
    "in-process teammate: SendMessage to `name` to steer; \
message {type:\"shutdown_request\"} terminates. Not a TaskStop background task; shares the \
orchestrator PID (no separate process to kill).";

pub(crate) fn axis_label(axis: AgentTimeAxis) -> &'static str {
    match axis {
        AgentTimeAxis::Trigger => "trigger",
        AgentTimeAxis::Start => "start",
        AgentTimeAxis::Completion => "completion",
    }
}
