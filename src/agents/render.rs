//! Text tree rendering: nested runs, node blocks, pending lanes.

use super::*;

pub(crate) fn render_text(
    nodes: &[SubagentNode],
    workflow_runs: &[WorkflowRun],
    args: &AgentsArgs,
    view: &View,
) {
    if nodes.is_empty() {
        println!("no subagents found");
        return;
    }

    if view.single_node {
        // A single `--agent <hex>` grab: just the matched node, no SESSION/WORKFLOW header.
        for n in nodes {
            print_node_block(n, view, 1);
        }
    } else {
        render_tree_text(nodes, workflow_runs, view);
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
                AgentKindFilter::Teammate => "teammate",
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    println!();
    println!(
        "{} subagent(s)  ·  kind={kinds}  ·  window-axis={}",
        nodes.len(),
        axis_label(args.order_by)
    );

    // When a teammate is in scope, point at the CORRECT control tool (SendMessage by name),
    // since the natural-but-wrong reach (TaskStop / pkill) silently fails on a teammate id.
    if any_teammate(nodes) {
        println!();
        println!("{TEAMMATE_CONTROL_HINT_L1}");
        println!("{TEAMMATE_CONTROL_HINT_L2}");
    }
}

/// Tree text: each session → its workflow RUN nodes (with their agents nested) → then the
/// remaining built-in agents. Joins workflow agents to a run by `workflow_id == run_id`.
pub(crate) fn render_tree_text(nodes: &[SubagentNode], workflow_runs: &[WorkflowRun], view: &View) {
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

        // Built-in agents (no workflow_id), NESTED by the agent→agent topology: a
        // sub-subagent renders UNDER its spawning agent (indent grows with depth), not flat.
        // The on-disk set is flat, so this is reconstructed from `parent_agent_id`.
        let builtin: Vec<&SubagentNode> = snodes
            .iter()
            .filter(|n| n.workflow_id.is_none())
            .copied()
            .collect();
        print_builtin_agents_nested(&builtin, view);
    }
}

/// Print built-in agents as a tree by `parent_agent_id`. A root (parent absent, or its parent
/// not an in-scope built-in) prints at indent 1 - identical to the pre-nesting flat layout -
/// and each child one indent deeper. Pre-order DFS via an explicit stack (no recursion depth
/// risk); siblings in stable `agent_id` order.
pub(crate) fn print_builtin_agents_nested(builtin: &[&SubagentNode], view: &View) {
    use std::collections::{BTreeMap, HashSet};
    let ids: HashSet<&str> = builtin.iter().map(|n| n.agent_id.as_str()).collect();
    let mut kids: BTreeMap<&str, Vec<&SubagentNode>> = BTreeMap::new();
    let mut roots: Vec<&SubagentNode> = Vec::new();
    for &n in builtin {
        match n.parent_agent_id.as_deref() {
            Some(p) if ids.contains(p) => kids.entry(p).or_default().push(n),
            _ => roots.push(n),
        }
    }
    roots.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    // Stack holds (node, indent); push children reversed so they pop in agent_id order.
    let mut stack: Vec<(&SubagentNode, usize)> =
        roots.into_iter().rev().map(|n| (n, 1usize)).collect();
    while let Some((n, indent)) = stack.pop() {
        print_node_block(n, view, indent);
        if let Some(cs) = kids.get(n.agent_id.as_str()) {
            let mut cs = cs.clone();
            cs.sort_by(|a, b| b.agent_id.cmp(&a.agent_id)); // reverse → pop in order
            for c in cs {
                stack.push((c, indent + 1));
            }
        }
    }
}

/// Print a workflow RUN header line (the tree parent of its agents).
pub(crate) fn print_workflow_run(run: &WorkflowRun) {
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
pub(crate) fn print_node_block(n: &SubagentNode, view: &View, depth: usize) {
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

    // A `/fork` child: name the fork point (the parent's last record uuid at fork
    // time - feed it to `csift show @<parent-session> --uuid <it>`) + the carried
    // context length. Facts from the head fork-context-ref record, absent otherwise.
    if let Some(fp) = &n.fork_parent_last_uuid {
        let ctx = n
            .fork_context_length
            .map(|c| format!(" (context {c})"))
            .unwrap_or_default();
        println!("{ind2}forked-at  {fp}{ctx}");
    }

    // A FROZEN lane: the status above says `running`, but it is blocked at an unreturned tool_use.
    // Surface WHY prominently - the disambiguation a flat `running` (or, before the fix, `completed`)
    // hid. escalation-blocked = waiting for a human Yes; awaiting-execution = slow-or-wedged.
    if let Some(class) = n.pending_classification {
        let tool = n.pending_tool_name.as_deref().unwrap_or("?");
        let id = n.pending_tool_use_id.as_deref().unwrap_or("?");
        println!(
            "{ind2}PENDING    {} · {tool} ({id}) · frozen since {}",
            class.label(),
            format_timestamp(n.pending_since_utc.as_deref())
        );
        if class == PendingClassification::EscalationBlocked {
            println!(
                "{ind2}           ↑ a dangerous-rm Bash CC HOISTS for human approval even under \
                 bypass — almost certainly waiting for a Yes (approve/deny in the main UI), NOT dead."
            );
        }
    }

    // A teammate's team + handle (the team-lead addresses it by `@<name>`); shown only when set.
    if let Some(tn) = &n.team_name {
        match &n.name {
            Some(nm) => println!("{ind2}team       {tn}  (@{nm})"),
            None => println!("{ind2}team       {tn}"),
        }
    }
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
    // Truthful terminal line: only a COMPLETED lane prints "completed" (+ duration) -
    // `completed_utc` is status-gated at the node, so presence == completion. A frozen
    // lane's tail instant is already on the PENDING line as "frozen since"; any other
    // non-completed lane (running-not-frozen / unknown) prints the tail ts as
    // "last-seen" so the instant is never lost and never mislabeled.
    if n.completed_utc.is_some() {
        println!(
            "{ind2}completed  {}",
            format_timestamp(n.completed_utc.as_deref())
        );
        if let Some(dur) = duration_label(n.trigger_utc.as_deref(), n.completed_utc.as_deref()) {
            println!("{ind2}duration   {dur}");
        }
    } else if n.pending_classification.is_none() {
        println!(
            "{ind2}last-seen  {}",
            format_timestamp(n.last_activity_utc.as_deref())
        );
    }
    if view.want_returned {
        if let (Some(msg), Some(src)) = (&n.returned_message, n.returned_message_source) {
            // On a non-completed lane the newest returned/observed message PREDATES the
            // still-open work - a "work is complete, confirming shutdown" tail reads like
            // an outcome and has misled a real reader (R8). Brand it inline next to the
            // source tag; don't rely on the reader remembering the schema note.
            // `completed_utc` is status-gated, so absence == the lane is not completed.
            if n.completed_utc.is_none() {
                println!(
                    "{ind2}returned   ({} · history — predates the still-open lane, NOT the outcome) {}",
                    src.label(),
                    one_line(msg)
                );
            } else {
                println!("{ind2}returned   ({}) {}", src.label(), one_line(msg));
            }
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
        // R12: same window census as `list` - lifecycle reads the transcript head/tail only.
        println!(
            "{ind2}note     {} (among the head/tail lines read — full census: csift stats)",
            crate::text::malformed_note(n.skipped_lines)
        );
    }
}
