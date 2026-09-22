//! Text tree rendering: nested runs, node blocks, pending lanes.
//!
//! The tree goes through ONE [`crate::text::Out`]: a session with thousands of subagents
//! prints tens of thousands of rows (18,308 on the section 7 landmark transcript). See the
//! `text::out` module docs for what holding the lock obliges - the duplicate banner stays
//! `println!` because it is already flushed before the writer exists.

use std::io::Write as _;

use super::*;

pub(crate) fn render_text(
    nodes: &[SubagentNode],
    workflow_runs: &[WorkflowRun],
    args: &AgentsArgs,
    view: &View,
    duplicates: &[crate::path::DuplicateGroup],
) -> Result<()> {
    crate::path::emit_duplicate_banner(duplicates);
    let mut w = crate::text::Out::stdout();
    if nodes.is_empty() {
        writeln!(w, "no subagents found")?;
        w.flush()?;
        return Ok(());
    }

    if view.single_node {
        // A single `--agent <hex>` grab: just the matched node, no SESSION/WORKFLOW header.
        for n in nodes {
            print_node_block(&mut w, n, view, 1)?;
        }
    } else {
        render_tree_text(&mut w, nodes, workflow_runs, view)?;
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
    writeln!(w)?;
    writeln!(
        w,
        "{} subagent(s)  ·  kind={kinds}  ·  window-axis={}",
        nodes.len(),
        axis_label(args.order_by)
    )?;

    // When a teammate is in scope, point at the CORRECT control tool (SendMessage by name),
    // since the natural-but-wrong reach (TaskStop / pkill) silently fails on a teammate id.
    if any_teammate(nodes) {
        writeln!(w)?;
        writeln!(w, "{TEAMMATE_CONTROL_HINT_L1}")?;
        writeln!(w, "{TEAMMATE_CONTROL_HINT_L2}")?;
        writeln!(w, "{TEAMMATE_CONTROL_HINT_L3}")?;
    }
    w.flush()?;
    Ok(())
}

/// Tree text: each session → its workflow RUN nodes (with their agents nested) → then the
/// remaining built-in agents. Joins workflow agents to a run by `workflow_id == run_id`.
pub(crate) fn render_tree_text<W: std::io::Write>(
    w: &mut W,
    nodes: &[SubagentNode],
    workflow_runs: &[WorkflowRun],
    view: &View,
) -> Result<()> {
    use std::collections::BTreeMap;

    // Group nodes by parent session.
    let mut by_session: BTreeMap<&str, Vec<&SubagentNode>> = BTreeMap::new();
    for n in nodes {
        by_session
            .entry(n.parent_session_id.as_str())
            .or_default()
            .push(n);
    }
    // A run belongs to the session whose sidecar holds its manifest, and it is SHOWN when at
    // least one of its agents is in scope - both tests, because two sessions can name one
    // workflow id and matching on the id alone printed each of their runs under both.
    let in_scope_wf: std::collections::BTreeSet<&str> = nodes
        .iter()
        .filter_map(|n| n.workflow_id.as_deref())
        .collect();

    let mut first = true;
    for (session, snodes) in &by_session {
        if !first {
            writeln!(w)?;
        }
        first = false;
        writeln!(w, "SESSION  {session}")?;

        // Workflow runs (as parent nodes) + their agents nested under them.
        for run in workflow_runs {
            if run.session_id != **session || !in_scope_wf.contains(run.run_id.as_str()) {
                continue;
            }
            let agents: Vec<&SubagentNode> = snodes
                .iter()
                .filter(|n| n.workflow_id.as_deref() == Some(run.run_id.as_str()))
                .copied()
                .collect();
            if agents.is_empty() {
                continue;
            }
            print_workflow_run(w, run)?;
            // The same walk again, so a run's members are the rows its JSON block emits,
            // in that order. They render FLAT under the run: the run is their parent here,
            // and the agent-to-agent nesting rides `depth` / `parent_agent_id` on the row.
            for (n, _) in preorder_depth(&agents) {
                print_node_block(w, n, view, 2)?;
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
        print_builtin_agents_nested(w, &builtin, view)?;
    }
    Ok(())
}

/// Print built-in agents as a tree by `parent_agent_id`. A root (parent absent, or its parent
/// not an in-scope built-in) prints at indent 1 - identical to the pre-nesting flat layout -
/// and each child one indent deeper.
///
/// The walk is [`preorder_depth`], the one the JSON rows take, so the tree can never print a
/// different set, a different order or a different COUNT from the rows a consumer parses.
pub(crate) fn print_builtin_agents_nested<W: std::io::Write>(
    w: &mut W,
    builtin: &[&SubagentNode],
    view: &View,
) -> Result<()> {
    for (n, depth) in preorder_depth(builtin) {
        print_node_block(w, n, view, 1 + depth)?;
    }
    Ok(())
}

/// Print a workflow RUN header line (the tree parent of its agents).
pub(crate) fn print_workflow_run<W: std::io::Write>(w: &mut W, run: &WorkflowRun) -> Result<()> {
    let mut head = format!("  WORKFLOW  {}", run.run_id);
    if let Some(name) = &run.workflow_name {
        head.push_str(&format!("  [{name}]"));
    }
    if let Some(s) = &run.status {
        head.push_str(&format!("  {s}"));
    }
    writeln!(w, "{head}")?;
    if let Some(n) = run.agent_count {
        writeln!(w, "      agents     {n}")?;
    }
    if let Some(ms) = run.duration_ms {
        writeln!(w, "      duration   {}", fmt_ms(ms))?;
    }
    if let Some(t) = run.total_tokens {
        writeln!(w, "      tokens     {t}")?;
    }
    if let Some(m) = &run.default_model {
        writeln!(w, "      model      {m}")?;
    }
    Ok(())
}

/// Print one node's text block, indented `depth` levels (2 spaces per level).
pub(crate) fn print_node_block<W: std::io::Write>(
    w: &mut W,
    n: &SubagentNode,
    view: &View,
    depth: usize,
) -> Result<()> {
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
    // A teammate answers to TWO ids: the transcript id leading this line and the ROUTING form
    // the official SendMessage takes. Print both together so neither has to be guessed - they
    // are minted apart at spawn, so one cannot be derived from the other.
    if n.kind == SubagentKind::Teammate {
        if let Some(rid) = crate::subagent::routing_id(n.name.as_deref(), n.team_name.as_deref()) {
            head.push_str(&format!("  routing: {rid}"));
        }
    }
    writeln!(w, "{head}")?;

    // TWO lanes can share every id they have: a copied project directory carries the whole
    // sidecar, so a divergent session pair keeps two copies of each agent transcript. The
    // path is the only thing that separates them, so it prints for exactly the ids that
    // occur twice - on an ordinary listing it would be one line of noise per agent, and the
    // JSON row carries it unconditionally because that one is the machine contract.
    if view.ambiguous_ids.contains(&n.agent_id) {
        writeln!(w, "{ind2}transcript {}", n.path.display())?;
    }

    // A `/fork` child: name the fork point (the parent's last record uuid at fork
    // time - feed it to `csift show @<parent-session> --uuid <it>`) + the carried
    // context length. Facts from the head fork-context-ref record, absent otherwise.
    if let Some(fp) = &n.fork_parent_last_uuid {
        let ctx = n
            .fork_context_length
            .map(|c| format!(" (context {c})"))
            .unwrap_or_default();
        writeln!(w, "{ind2}forked-at  {fp}{ctx}")?;
    }

    // A FROZEN lane: the status above says `running`, but it is blocked at an unreturned tool_use.
    // Surface WHY prominently - the disambiguation a flat `running` (or, before the fix, `completed`)
    // hid. escalation-blocked = waiting for a human Yes; awaiting-execution = slow-or-wedged.
    if let Some(class) = n.pending_classification {
        let tool = n.pending_tool_name.as_deref().unwrap_or("?");
        let id = n.pending_tool_use_id.as_deref().unwrap_or("?");
        writeln!(
            w,
            "{ind2}PENDING    {} · {tool} ({id}) · frozen since {}",
            class.label(),
            format_timestamp(n.pending_since_utc.as_deref())
        )?;
        if class == PendingClassification::EscalationBlocked {
            writeln!(w, "{ind2}           ↑ a dangerous-rm Bash CC HOISTS for human approval even under \
                 bypass — almost certainly waiting for a Yes (approve/deny in the main UI), NOT dead."
            )?;
        }
        // The prediction's own evidence: the harness's reason tail (or csift's note
        // where the deciding arm reads the filesystem) and which checker of which
        // Claude Code generation would have decided it.
        if let Some(reason) = &n.pending_reason {
            writeln!(w, "{ind2}           reason: {reason}")?;
        }
        if let Some(checker) = &n.pending_checker {
            writeln!(w, "{ind2}           {checker}")?;
        }
    }

    // A teammate's team + handle (the team-lead addresses it by `@<name>`); shown only when set.
    if let Some(tn) = &n.team_name {
        match &n.name {
            Some(nm) => writeln!(w, "{ind2}team       {tn}  (@{nm})")?,
            None => writeln!(w, "{ind2}team       {tn}")?,
        }
    }
    if let Some(d) = &n.description {
        writeln!(w, "{ind2}desc       {d}")?;
    }
    writeln!(
        w,
        "{ind2}triggered  {}",
        format_timestamp(n.trigger_utc.as_deref())
    )?;
    writeln!(
        w,
        "{ind2}started    {}",
        format_timestamp(n.started_utc.as_deref())
    )?;
    // Truthful terminal line: only a COMPLETED lane prints "completed" (+ duration) -
    // `completed_utc` is status-gated at the node, so presence == completion. A frozen
    // lane's tail instant is already on the PENDING line as "frozen since"; any other
    // non-completed lane (running-not-frozen / unknown) prints the tail ts as
    // "last-seen" so the instant is never lost and never mislabeled.
    if n.completed_utc.is_some() {
        writeln!(
            w,
            "{ind2}completed  {}",
            format_timestamp(n.completed_utc.as_deref())
        )?;
        if let Some(dur) = duration_label(n.trigger_utc.as_deref(), n.completed_utc.as_deref()) {
            writeln!(w, "{ind2}duration   {dur}")?;
        }
    } else if n.pending_classification.is_none() {
        writeln!(
            w,
            "{ind2}last-seen  {}",
            format_timestamp(n.last_activity_utc.as_deref())
        )?;
    }
    if view.want_returned {
        if let (Some(msg), Some(src)) = (&n.returned_message, n.returned_message_source) {
            // On a non-completed lane the newest returned/observed message PREDATES the
            // still-open work - a "work is complete, confirming shutdown" tail reads like
            // an outcome and has misled a real reader (R8). Brand it inline next to the
            // source tag; don't rely on the reader remembering the schema note.
            // `completed_utc` is status-gated, so absence == the lane is not completed.
            if n.completed_utc.is_none() {
                writeln!(w, "{ind2}returned   ({} · history — predates the still-open lane, NOT the outcome) {}",
                    src.label(),
                    one_line(msg)
                )?;
            } else {
                writeln!(w, "{ind2}returned   ({}) {}", src.label(), one_line(msg))?;
            }
        } else {
            writeln!(w, "{ind2}returned   (unresolved)")?;
        }
    }
    if view.want_files {
        if n.files_changed.is_empty() {
            writeln!(w, "{ind2}files      (none)")?;
        } else {
            writeln!(w, "{ind2}files      {} changed", n.files_changed.len())?;
            for (path, op, is_create) in &n.files_changed {
                let tag = if *is_create { "create" } else { op.as_str() };
                writeln!(w, "{ind2}  {tag:<12} {path}")?;
            }
        }
    }
    if n.skipped_lines > 0 {
        // R12: same window census as `list` - lifecycle reads the transcript head/tail only.
        writeln!(
            w,
            "{ind2}note     {} (among the head/tail lines read — full census: csift stats)",
            crate::text::malformed_note(n.skipped_lines)
        )?;
    }
    Ok(())
}
