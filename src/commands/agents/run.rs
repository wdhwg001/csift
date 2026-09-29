//! run_agents: topology build, filters, windows, teammate hint.
// The `text::out` obligation, mechanically: a stray print on this thread jumps the
// buffer's queue and one inside the parallel render closure hangs the process, so a
// print outside an `Out` is an error here unless it says why (see `text::out`).
#![deny(clippy::print_stdout, clippy::print_stderr)]

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
    // `false` (=> `SubagentScope::TopLevelOnly`).
    let resolved = path::resolve_targets_with_session_list_reported(
        &args.paths,
        args.sessions_from.as_deref(),
        false.into(),
        path::Caller::Other,
    )?;
    let session_files = resolved.files;

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
    // A copied project directory carries the whole sidecar, its workflow MANIFESTS included,
    // so the same run is discovered once per session COPY. One run is one row, and a run's
    // identity is the PAIR (session, run): a run id is unique within the session that minted
    // it and nothing makes it unique across sessions, so keying on the id alone would drop
    // another session's run as though it were a copy. The renderers join a run to a session
    // by that same pair, so such a drop shows up as a session missing its run, never as a
    // duplicate. The first occurrence stays, which keeps the tree's run order exactly as
    // the scan produced it.
    let mut seen_runs: std::collections::BTreeSet<(String, String)> =
        std::collections::BTreeSet::new();
    for t in topos {
        nodes.extend(t.nodes);
        for r in t.workflow_runs {
            if seen_runs.insert((r.session_id.clone(), r.run_id.clone())) {
                workflow_runs.push(r);
            }
        }
    }
    let mut duplicates = resolved.duplicates;
    // The sidecar copies `agents` gathers take the SAME merge as every other surface.
    // `agents` resolves TopLevelOnly and walks the sidecars ITSELF, so the resolver's union
    // merge never sees them - and without this the one command whose whole job is counting
    // agents was the one that double-counted them: a divergent session pair whose two
    // sidecars hold ONE byte-identical agent transcript printed two lanes under a header
    // saying nothing was duplicated, while `list` showed one row and `search -c` counted the
    // records once.
    let (kept, agent_groups) = path::dedup_copies(nodes.iter().map(|n| n.path.clone()).collect());
    if !agent_groups.is_empty() {
        let keep: std::collections::BTreeSet<std::path::PathBuf> = kept.into_iter().collect();
        nodes.retain(|n| keep.contains(&n.path));
        path::emit_duplicate_notes(&agent_groups);
        duplicates.extend(agent_groups);
    }
    // `--agent <hex>` is a DIRECT id lookup: it BYPASSES the --since/--until/--order-by time
    // window AND the --shape filter (a known id should resolve regardless of when it ran or
    // its shape), and a no-match is a hard error with discovery guidance - never the
    // ambiguous `no subagents found` (which a zero-subagent session also prints). The grab
    // renders a single node (a tree of one), not the whole workflow tree.
    if let Some(given) = args.agent.as_deref() {
        // Every other id position in csift takes the `@` sigil, and this error's own remedy
        // prints ids beside `@<uuid>`, so the sigil is stripped rather than matched: an id
        // carries no `@` of its own on any shape (bare hex, `a<Name>-<16 hex>`).
        let want_id = given.trim().strip_prefix('@').unwrap_or(given.trim());
        // ONE leading sigil, never more: an id that still carries an `@` after that strip is
        // no agent id, and searching for it only reported a sigiled id as unmatched.
        if want_id.contains('@') {
            bail!(
                "`--agent {}` is not an agent id: `--agent` takes one id as `csift agents` \
                 prints it - a bare hex or a teammate's `a<Name>-<16 hex>` - with at most one \
                 leading `@`, and an id carries no `@` of its own on any shape.",
                given.trim()
            );
        }
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
        nodes.retain(|n| agent_type_allowed(n.agent_type.as_deref(), &args.agent_types));
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
    // Which ids more than one node in this set carries: the copies of one agent transcript,
    // which are identical in every field but their path.
    let mut id_counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for n in &nodes {
        *id_counts.entry(n.agent_id.as_str()).or_default() += 1;
    }
    let view = View {
        want_returned,
        want_files,
        single_node: args.agent.is_some(),
        ambiguous_ids: id_counts
            .into_iter()
            .filter(|&(_, c)| c > 1)
            .map(|(id, _)| id.to_string())
            .collect(),
    };

    match args.format {
        OutputFormat::Text => {
            render_text(&nodes, &workflow_runs, args, &view, &duplicates)?;
        }
        OutputFormat::Json => render_json(&nodes, &workflow_runs, &view, &duplicates)?,
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
    // Keyed by the PAIR, like the fold above: a manifest under session A must not stand in
    // for session B's run of the same id, or B's agents lose the run header they render
    // under and vanish from the tree.
    let known: BTreeSet<(&str, &str)> = workflow_runs
        .iter()
        .map(|r| (r.session_id.as_str(), r.run_id.as_str()))
        .collect();
    // Preserve first-seen (sorted-node) order while de-duplicating.
    let mut missing: Vec<(String, String)> = Vec::new();
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    for n in nodes {
        if let Some(wf) = n.workflow_id.as_deref() {
            let key = (n.parent_session_id.as_str(), wf);
            if !known.contains(&key) && seen.insert(key) {
                missing.push((n.parent_session_id.clone(), wf.to_string()));
            }
        }
    }
    for (session_id, run_id) in missing {
        workflow_runs.push(WorkflowRun {
            session_id,
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
    /// The agent ids that occur MORE THAN ONCE in the emitted set - two copies of one
    /// session under two project directories carry two copies of each agent transcript, and
    /// those nodes share every id they have. The text tree prints the transcript path for
    /// exactly these, because that is where two blocks would otherwise read identically;
    /// the JSON row carries `path` unconditionally, being the machine contract.
    pub(crate) ambiguous_ids: std::collections::BTreeSet<String>,
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

/// True when the node's `agent_type` passes the repeatable `--agent-type` filter
/// (exact match, empty filter => all). A node with NO agent_type only passes an
/// empty filter - an explicit filter never matches the unknown.
fn agent_type_allowed(agent_type: Option<&str>, wanted: &[String]) -> bool {
    wanted.is_empty() || agent_type.is_some_and(|t| wanted.iter().any(|w| w == t))
}

/// True when `kind` passes the `--shape` filter (empty filter => all kinds).
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

// -- Rendering --

/// True if any node in the tree (self or any descendant) is a teammate.
pub(crate) fn any_teammate(nodes: &[SubagentNode]) -> bool {
    nodes
        .iter()
        .any(|n| n.kind == SubagentKind::Teammate || any_teammate(&n.children))
}

/// Control-mechanism hint for teammates (`in_process_teammate`), surfaced in `agents` text
/// output whenever the scope holds >=1 teammate. csift is the LLM's only window into a session's
/// teammates, so it is the natural place to point at the CORRECT control tool and at the id
/// each tool takes - a real session burned ~30 min trying to `TaskStop` / `pkill` a runaway
/// teammate (feeding it the name, the `Name@team` form, AND the exact `aName-<hash>` agentId
/// csift prints) on a build where TaskStop rejected every teammate form. The two tools resolve
/// different id forms. SendMessage takes the BARE name as `to` and refuses any `to` carrying
/// `@`, so the `Name@team` routing form is never a SendMessage address; its agent-id arm also
/// takes the `aName-<hex>` transcript id, delivering to the running teammate or resuming a
/// stopped one (ledger SUB-035). TaskStop resolves the name and the `name@team` identity from
/// Claude Code 2.1.198 (absent in 2.1.197; ledger SUB-020) and never the transcript id.
/// `message:{type:"shutdown_request"}` terminates. Read-only csift cannot act - it only names
/// the tools that can.
pub(crate) const TEAMMATE_CONTROL_HINT_L1: &str = "note: teammate rows are in-process Agent subagents — steer one with SendMessage addressed by its bare NAME \
(the `(@name)` shown, without the `@`), and terminate it with `message:{\"type\":\"shutdown_request\"}`.";
pub(crate) const TEAMMATE_CONTROL_HINT_L2: &str =
    "      TaskStop also stops a teammate by its name or `name@team` from Claude Code 2.1.198 (earlier \
builds reject every form); the `aName-<hex>` id above is a transcript id, never a task id, and a \
teammate has no separate OS process (it shares the orchestrator PID — `pkill` won't help).";

/// The two-id rule, stated where the wrong id is about to be pasted into the wrong tool. A
/// teammate's routing id and its transcript id are minted apart at spawn, so neither can be
/// derived from the other; SendMessage refuses the routing id (any `to` carrying `@`) and
/// takes the transcript id, and only the routing form can collide - which is why csift keys
/// its own targeting on the transcript id and prints both forms on the node line.
pub(crate) const TEAMMATE_CONTROL_HINT_L3: &str =
    "      SendMessage refuses any `to` carrying `@`, so the routing id Name@Team (the `routing:` field \
above) is what TaskStop takes and never a SendMessage `to`; SendMessage also takes a<Name>-<hex>, the id \
the hook payload and the transcript file carry, and resumes a stopped teammate by it; the routing id \
can collide (two same-named teammates share it), the transcript id never does.";

/// The compact JSON-surface twin of [`TEAMMATE_CONTROL_HINT_L1`]/`_L2`/`_L3` - emitted as a
/// teammate node's `control_hint` field so a `--format json` consumer gets the same pointer.
pub(crate) const TEAMMATE_CONTROL_HINT_JSON: &str =
    "in-process teammate: SendMessage to its bare `name` to steer (it refuses a `to` carrying \
`@`, so never `routing_id`), or to `agent_id`, which also resumes it when it is not running; \
message {type:\"shutdown_request\"} terminates; TaskStop by `name` or `routing_id` (Name@Team) \
also stops it from Claude Code 2.1.198 (never by the `agent_id` transcript id); shares the \
orchestrator PID (no separate process to kill). The two ids are minted apart: `routing_id` \
collides when two teammates share a name, while `agent_id` (a<Name>-<hex>), the id the hook \
payload and the transcript file carry, never does.";

pub(crate) fn axis_label(axis: AgentTimeAxis) -> &'static str {
    match axis {
        AgentTimeAxis::Trigger => "trigger",
        AgentTimeAxis::Start => "start",
        AgentTimeAxis::Completion => "completion",
    }
}
