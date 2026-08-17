//! Flat JSON rows: session / run / agent in pre-order.

use super::*;

pub(crate) fn render_json(
    nodes: &[SubagentNode],
    workflow_runs: &[WorkflowRun],
    view: &View,
) -> Result<()> {
    use std::collections::{BTreeMap, BTreeSet};

    // envelope v2, FLAT rows (v0.5): header → per session a light `kind:"session"` row
    // (counts only) → each workflow run as its own `kind:"run"` row followed by its
    // member `kind:"agent"` rows (tree PRE-ORDER) → the built-in agents (pre-order) →
    // summary. The tree nests in TEXT mode only; JSON consumers reconstruct it from
    // `parent_agent_id`/`depth` - so `jq 'select(.kind=="agent")'` addresses every node,
    // the uniform envelope idiom the old one-giant-session-row shape defeated.
    println!(
        "{}",
        serde_json::to_string(&crate::text::envelope_header(
            "agents",
            serde_json::json!({})
        ))?
    );
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

    let mut runs_total = 0usize;
    for (session, snodes) in &by_session {
        let runs: Vec<&WorkflowRun> = workflow_runs
            .iter()
            .filter(|r| {
                in_scope_wf.contains(r.run_id.as_str())
                    && snodes
                        .iter()
                        .any(|n| n.workflow_id.as_deref() == Some(r.run_id.as_str()))
            })
            .collect();
        runs_total += runs.len();
        let obj = serde_json::json!({
            "kind": "session",
            "session_id": session,
            "runs": runs.len(),
            "agents": snodes.len(),
        });
        println!("{}", serde_json::to_string(&obj)?);
        for run in &runs {
            println!(
                "{}",
                serde_json::to_string(&workflow_run_json(run, session))?
            );
            let members: Vec<&SubagentNode> = snodes
                .iter()
                .filter(|n| n.workflow_id.as_deref() == Some(run.run_id.as_str()))
                .copied()
                .collect();
            for n in preorder(&members) {
                println!("{}", serde_json::to_string(&agent_row(n, view))?);
            }
        }
        let builtin: Vec<&SubagentNode> = snodes
            .iter()
            .filter(|n| n.workflow_id.is_none())
            .copied()
            .collect();
        for n in preorder(&builtin) {
            println!("{}", serde_json::to_string(&agent_row(n, view))?);
        }
    }
    let summary = crate::text::envelope_summary(serde_json::json!({
        "sessions": by_session.len(),
        "runs": runs_total,
        "agents": nodes.len(),
    }));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

/// Tree PRE-ORDER over a node set: roots (parent absent or out-of-set) sorted by id,
/// children sorted by id, depth-first. A node unreachable from any root (a forged
/// parent cycle) is APPENDED at the end rather than dropped - flat rows must never
/// lose a node (the old nested shape silently omitted such nodes).
pub(crate) fn preorder<'a>(nodes: &[&'a SubagentNode]) -> Vec<&'a SubagentNode> {
    use std::collections::{BTreeMap, HashSet};
    let ids: HashSet<&str> = nodes.iter().map(|n| n.agent_id.as_str()).collect();
    let mut kids: BTreeMap<&str, Vec<&'a SubagentNode>> = BTreeMap::new();
    let mut roots: Vec<&'a SubagentNode> = Vec::new();
    for &n in nodes {
        match n.parent_agent_id.as_deref() {
            Some(p) if ids.contains(p) => kids.entry(p).or_default().push(n),
            _ => roots.push(n),
        }
    }
    roots.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    let mut out: Vec<&'a SubagentNode> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&'a SubagentNode> = roots.into_iter().rev().collect();
    while let Some(n) = stack.pop() {
        if !seen.insert(n.agent_id.as_str()) {
            continue;
        }
        out.push(n);
        if let Some(cs) = kids.get(n.agent_id.as_str()) {
            let mut cs = cs.clone();
            cs.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
            for c in cs.into_iter().rev() {
                stack.push(c);
            }
        }
    }
    for &n in nodes {
        if !seen.contains(n.agent_id.as_str()) {
            out.push(n);
        }
    }
    out
}

/// One flat `kind:"agent"` row - [`node_json`] plus the envelope discriminator.
pub(crate) fn agent_row(n: &SubagentNode, view: &View) -> serde_json::Value {
    let mut v = node_json(n, view);
    if let Some(map) = v.as_object_mut() {
        map.insert("kind".to_string(), serde_json::json!("agent"));
    }
    v
}

/// One node's JSON object. `returned_message` / `files_changed` are included only when the
/// view asked for them (so a plain listing stays compact).
pub(crate) fn node_json(n: &SubagentNode, view: &View) -> serde_json::Value {
    use serde_json::json;
    let mut obj = json!({
        "agent_id": n.agent_id,
        // The TRANSCRIPT-SHAPE discriminator (builtin-task | workflow | teammate) -
        // named `shape` so `kind` stays the envelope discriminator exclusively.
        "shape": n.kind.label(),
        "parent_session_id": n.parent_session_id,
        "parent_agent_id": n.parent_agent_id,
        "spawn_tool_use_id": n.spawn_tool_use_id,
        "spawn_tool": n.spawn_tool,
        "workflow_id": n.workflow_id,
        "agent_type": n.agent_type,
        "name": n.name,
        "team_name": n.team_name,
        "description": n.description,
        "trigger_utc": n.trigger_utc,
        "trigger_local": n.trigger_utc.as_deref().and_then(local_iso),
        "started_utc": n.started_utc,
        "started_local": n.started_utc.as_deref().and_then(local_iso),
        // Status-gated: non-null ONLY when `status` is `completed` (a frozen/running
        // lane's tail ts is NOT a completion - it lives in `last_activity_*` below,
        // and on a frozen lane also in `pending_since_*`).
        "completed_utc": n.completed_utc,
        "completed_local": n.completed_utc.as_deref().and_then(local_iso),
        "last_activity_utc": n.last_activity_utc,
        "last_activity_local": n.last_activity_utc.as_deref().and_then(local_iso),
        "duration": duration_label(n.trigger_utc.as_deref(), n.completed_utc.as_deref()),
        "status": n.status.label(),
        "pending_tool_use_id": n.pending_tool_use_id,
        "pending_tool_name": n.pending_tool_name,
        "pending_classification": n.pending_classification.map(PendingClassification::label),
        "pending_since_utc": n.pending_since_utc,
        "pending_since_local": n.pending_since_utc.as_deref().and_then(local_iso),
        "depth": n.depth,
        "skipped_lines": n.skipped_lines,
    });
    let map = obj.as_object_mut().expect("json object");
    // A teammate carries the control-mechanism pointer inline (the JSON twin of the text note)
    // so a `--format json` consumer learns the right tool without the text footer.
    if n.kind == SubagentKind::Teammate {
        map.insert(
            "control_hint".to_string(),
            serde_json::Value::String(TEAMMATE_CONTROL_HINT_JSON.to_string()),
        );
    }
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

/// A workflow RUN's flat `kind:"run"` row - its member agents follow as their own
/// `kind:"agent"` rows (no nesting in JSON; `workflow_id` joins them back to the run).
pub(crate) fn workflow_run_json(run: &WorkflowRun, session_id: &str) -> serde_json::Value {
    serde_json::json!({
        "kind": "run",
        "session_id": session_id,
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
        // Pair every `_utc` with its system-local companion, matching node_json and every
        // other ts-emission site (the run object was the lone exception with a bare _utc).
        "started_local": run.started_utc.as_deref().and_then(local_iso),
    })
}

/// Max chars of a returned-message preview shown inline in the `agents` text view, matching
/// the scannable-preview cap `list` uses (vs the 400-char context-rich `search`/`recover`).
pub(crate) const ONE_LINE_MAX: usize = 200;

/// Collapse a (possibly multi-line) returned message to a single line for the text view, via
/// the SHARED excerpt helper - so the elision is marked with the same explicit `… (+N
/// chars)` count every other content-excerpt path emits (the never-silent-truncation
/// contract, SPEC §0/§8.1). This previously emitted a BARE `…` with no count, the lone
/// silent-truncation violation in the tree.
pub(crate) fn one_line(s: &str) -> String {
    crate::text::collapse_and_truncate(s, ONE_LINE_MAX)
}

/// Format a millisecond duration compactly (workflow manifest `durationMs`).
pub(crate) fn fmt_ms(ms: u64) -> String {
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
