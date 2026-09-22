//! Flat JSON rows: session / run / agent in pre-order.
// The `text::out` obligation, mechanically: a stray print on this thread jumps the
// buffer's queue and one inside the parallel render closure hangs the process, so a
// print outside an `Out` is an error here unless it says why (see `text::out`).
#![deny(clippy::print_stdout, clippy::print_stderr)]

use std::io::Write as _;

use super::*;

pub(crate) fn render_json(
    nodes: &[SubagentNode],
    workflow_runs: &[WorkflowRun],
    view: &View,
    duplicates: &[crate::path::DuplicateGroup],
) -> Result<()> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut out = crate::text::Out::stdout();

    // envelope v2, FLAT rows (v0.5): header → per session a light `kind:"session"` row
    // (counts only) → each workflow run as its own `kind:"run"` row followed by its
    // member `kind:"agent"` rows (tree PRE-ORDER) → the built-in agents (pre-order) →
    // summary. The tree nests in TEXT mode only; JSON consumers reconstruct it from
    // `parent_agent_id`/`depth` - so `jq 'select(.kind=="agent")'` addresses every node,
    // the uniform envelope idiom the old one-giant-session-row shape defeated.
    writeln!(
        out,
        "{}",
        serde_json::to_string(&crate::text::envelope_header(
            "agents",
            crate::path::duplicate_header_fields(duplicates)
        ))?
    )?;
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
        // A run belongs to the session whose sidecar holds its manifest: two sessions can
        // name one workflow id, and without that test each of their runs was emitted under
        // both, so the session count row and the rows under it disagreed.
        let runs: Vec<&WorkflowRun> = workflow_runs
            .iter()
            .filter(|r| {
                r.session_id == **session
                    && in_scope_wf.contains(r.run_id.as_str())
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
        writeln!(out, "{}", serde_json::to_string(&obj)?)?;
        for run in &runs {
            writeln!(
                out,
                "{}",
                serde_json::to_string(&workflow_run_json(run, session))?
            )?;
            let members: Vec<&SubagentNode> = snodes
                .iter()
                .filter(|n| n.workflow_id.as_deref() == Some(run.run_id.as_str()))
                .copied()
                .collect();
            for (n, _) in preorder_depth(&members) {
                writeln!(out, "{}", serde_json::to_string(&agent_row(n, view))?)?;
            }
        }
        let builtin: Vec<&SubagentNode> = snodes
            .iter()
            .filter(|n| n.workflow_id.is_none())
            .copied()
            .collect();
        for (n, _) in preorder_depth(&builtin) {
            writeln!(out, "{}", serde_json::to_string(&agent_row(n, view))?)?;
        }
    }
    let summary = crate::text::envelope_summary(serde_json::json!({
        "sessions": by_session.len(),
        "runs": runs_total,
        "agents": nodes.len(),
    }));
    writeln!(out, "{}", serde_json::to_string(&summary)?)?;
    out.flush()?;
    Ok(())
}

/// The ONE walk both projections take: tree PRE-ORDER over a node set - roots (parent
/// absent or out-of-set) sorted by id, children sorted by id, depth-first - carrying the
/// DEPTH the text tree indents by and the JSON rows ignore. EVERY node comes back EXACTLY
/// ONCE, so the count a session row prints is the number of rows that follow it and the
/// text tree prints the same set in the same order. A node unreachable from any root (a
/// forged parent cycle) is APPENDED at the end rather than dropped - a projection must
/// never lose a node (the old nested shape silently omitted such nodes).
///
/// The visited set keys on each node's POSITION in the set, not on its `agent_id`. Two
/// copies of one session under two project directories whose contents DIVERGE stay in scope
/// by the merge rule (§2.5), and they carry two copies of each agent transcript, which share
/// an id by construction - identical copies merge away, a DIVERGENT pair does not, so the
/// set can still hold two nodes under one id: keying on the id emitted one of the pair while
/// the session row counted both, which is exactly the disagreement between the count and the
/// rows. A position still guards the cycle a `parent_agent_id` chain can name, because a
/// node is reached twice only through an edge and never through itself.
pub(crate) fn preorder_depth<'a>(nodes: &[&'a SubagentNode]) -> Vec<(&'a SubagentNode, usize)> {
    use std::collections::{BTreeMap, HashSet};
    let ids: HashSet<&str> = nodes.iter().map(|n| n.agent_id.as_str()).collect();
    let by_id = |i: &usize| nodes[*i].agent_id.as_str();
    let mut kids: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut roots: Vec<usize> = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        match n.parent_agent_id.as_deref() {
            Some(p) if ids.contains(p) => kids.entry(p).or_default().push(i),
            _ => roots.push(i),
        }
    }
    roots.sort_by_key(by_id);
    let mut out: Vec<(&'a SubagentNode, usize)> = Vec::new();
    let mut seen: HashSet<usize> = HashSet::new();
    let mut stack: Vec<(usize, usize)> = roots.into_iter().rev().map(|i| (i, 0usize)).collect();
    while let Some((i, depth)) = stack.pop() {
        if !seen.insert(i) {
            continue;
        }
        out.push((nodes[i], depth));
        if let Some(cs) = kids.get(nodes[i].agent_id.as_str()) {
            let mut cs = cs.clone();
            cs.sort_by_key(by_id);
            for c in cs.into_iter().rev() {
                stack.push((c, depth + 1));
            }
        }
    }
    for (i, n) in nodes.iter().enumerate() {
        if !seen.contains(&i) {
            out.push((n, 0));
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
        // The node's own transcript. Every other field of two copies of one agent is
        // identical by construction - a copied project directory carries the whole sidecar,
        // so a DIVERGENT session pair keeps two lanes that share even their ids - and this
        // is what tells them apart and what `show <path>` reads to see one alone.
        "path": n.path.display().to_string(),
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
        // WHY the classification reads as it does: the harness's own reason tail
        // for a predicted ask, csift's note when the deciding arm needs the
        // filesystem state at the time, and which checker of which Claude Code
        // generation would have decided. Null on a lane that is not frozen and on a
        // pending call these lexical layers do not model.
        "pending_reason": n.pending_reason,
        "pending_checker": n.pending_checker,
        "pending_since_utc": n.pending_since_utc,
        "pending_since_local": n.pending_since_utc.as_deref().and_then(local_iso),
        "depth": n.depth,
        "skipped_lines": n.skipped_lines,
        // Fork provenance (a /fork child's head fork-context-ref record); null
        // on every non-fork agent. The uuid is the PARENT session's last record
        // at fork time - feed it to `csift show @<parent_session_id> --uuid <it>`.
        "fork_parent_last_uuid": n.fork_parent_last_uuid,
        "fork_context_length": n.fork_context_length,
    });
    let map = obj.as_object_mut().expect("json object");
    // A teammate carries the control-mechanism pointer inline (the JSON twin of the text note)
    // so a `--format json` consumer learns the right tool without the text footer.
    if n.kind == SubagentKind::Teammate {
        map.insert(
            "control_hint".to_string(),
            serde_json::Value::String(TEAMMATE_CONTROL_HINT_JSON.to_string()),
        );
        // The teammate's OTHER id: `agent_id` above is the transcript form, this is the
        // ROUTING form the official SendMessage addresses. Null when the meta carried no
        // team name - the routing id is then unknowable, never fabricated.
        map.insert(
            "routing_id".to_string(),
            crate::subagent::routing_id(n.name.as_deref(), n.team_name.as_deref())
                .map_or(serde_json::Value::Null, serde_json::Value::String),
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
