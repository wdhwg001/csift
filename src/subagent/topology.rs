//! SubagentNode + build_topology: flat-on-disk to nested tree, workflow runs.

use super::*;

/// One fully-linked subagent node in the topology (§new-model). Carries the flat
/// lifecycle facts PLUS the toolUseId-linked spawn linkage (trigger time, parent agent,
/// returned message) and the per-node files-changed list. `children` is the tool_use-graph
/// nesting (empty on all current data — depth is uniformly 1, a platform constraint).
#[derive(Debug, Clone)]
pub struct SubagentNode {
    /// Bare-hex canonical agent id (== record `agentId`).
    pub agent_id: String,
    pub kind: SubagentKind,
    pub parent_session_id: String,
    /// The bare-hex id of the PARENT subagent, when this node nests under another (future
    /// depth>1). `None` for a top-level (depth-1) subagent of the session.
    pub parent_agent_id: Option<String>,
    /// The parent `Task`/`Agent` tool_use id that spawned this subagent (the join key).
    pub spawn_tool_use_id: Option<String>,
    /// The spawning tool name (`Agent` / `Task` / `Workflow`) from the parent tool_use,
    /// when the spawn was located in the parent transcript. `None` for an UNLINKED node.
    pub spawn_tool: Option<String>,
    pub workflow_id: Option<String>,
    pub agent_type: Option<String>,
    /// The agent's `name` (meta.json `name` = the `Agent` tool's `name` param) — a teammate
    /// handle (`VSRepro`) or an OMC lane name (`LaneDONE`). `None` when unnamed.
    pub name: Option<String>,
    /// The `teamName` for a teammate (`kind == Teammate`); `None` otherwise.
    pub team_name: Option<String>,
    pub description: Option<String>,
    /// TRUE trigger time = the parent tool_use ts (§4); falls back to the child-head ts
    /// (`started_utc`) when the spawn index has no entry.
    pub trigger_utc: Option<String>,
    /// Child transcript HEAD ts (the lagging secondary "when").
    pub started_utc: Option<String>,
    /// The COMPLETION instant — populated ONLY when `status == Completed`. A frozen /
    /// running / unknown lane carries `None` here: its tail ts is a freeze or
    /// last-activity instant, NOT a completion, and a consumer doing the name-driven
    /// thing (`if completed_utc: treat as done`) must not get a false positive (the
    /// text tree suppressed the misleading "completed" line long before the JSON did).
    pub completed_utc: Option<String>,
    /// Tail newest-record ts — the lane's LAST-ACTIVITY instant, present whenever the
    /// transcript has any timestamp regardless of status. Equals `completed_utc` on a
    /// completed lane; on a frozen lane it equals `pending_since_utc`.
    pub last_activity_utc: Option<String>,
    /// The subagent's returned message (§3), resolved 3-ways. `None` when unresolved.
    pub returned_message: Option<String>,
    pub returned_message_source: Option<ReturnedMsgSource>,
    pub status: SubagentStatus,
    /// FROZEN-lane disambiguation (all `None` for a normal lane). When the newest meaningful
    /// record is an unreturned tool_use, `status` is `Running` and these carry: the pending
    /// tool_use id/name, its [`PendingClassification`] (escalation-blocked vs awaiting-execution),
    /// and when it froze. Lets a monitor tell "waiting for a human Yes" from "about to die".
    pub pending_tool_use_id: Option<String>,
    pub pending_tool_name: Option<String>,
    pub pending_classification: Option<PendingClassification>,
    pub pending_since_utc: Option<String>,
    /// Files this subagent mutated (reuses the `files`/`bash_mutations` extractors over the
    /// node's own transcript). Each is `(path, op_label, is_create)`.
    pub files_changed: Vec<(String, String, bool)>,
    /// tool_use-graph nesting depth (0 = a direct subagent of the parent session).
    pub depth: usize,
    /// Nested sub-subagents (empty on all current data).
    pub children: Vec<SubagentNode>,
    pub skipped_lines: usize,
}

/// Resolve a subagent's returned message 3 ways (§3):
/// - **workflow** → the `journal.jsonl` `result` payload ([`journal_result`]);
/// - **sync built-in** → the parent tool_result text for its spawn id;
/// - **async built-in** (`Async agent launched …` sentinel) → the child transcript tail's
///   assistant text.
///
/// Returns `(message, source)`; `(None, None)` when nothing resolves.
pub(crate) fn resolve_returned_message(
    subagent: &Subagent,
    index: &ParentSpawnIndex,
    journals: &JournalCache,
) -> (Option<String>, Option<ReturnedMsgSource>) {
    // Workflow agents always resolve through the journal (their parent tool_result is the
    // Workflow-tool launch echo, not the per-agent message).
    if subagent.kind == SubagentKind::Workflow {
        if let Some(msg) = journal_result(subagent, journals) {
            return (Some(msg), Some(ReturnedMsgSource::WorkflowJournal));
        }
        return (None, None);
    }
    // Built-in: try the parent tool_result for the spawn id.
    if let Some(id) = subagent.spawn_tool_use_id.as_deref() {
        if let Some(text) = index.tool_result_text(id) {
            if text.contains(ASYNC_LAUNCH_SENTINEL) {
                // Async launch → the real message is the child transcript tail.
                if let Some(tail) = child_tail_text(&subagent.path) {
                    return (Some(tail), Some(ReturnedMsgSource::AsyncChildTail));
                }
                // Tail unavailable → honestly report the sentinel as what we have.
                return (
                    Some(text.to_string()),
                    Some(ReturnedMsgSource::SyncToolResult),
                );
            }
            return (
                Some(text.to_string()),
                Some(ReturnedMsgSource::SyncToolResult),
            );
        }
    }
    // No spawn id / no parent result (e.g. parent transcript absent) → fall back to the
    // child tail so a returned message is still surfaced when possible.
    if let Some(tail) = child_tail_text(&subagent.path) {
        return (Some(tail), Some(ReturnedMsgSource::AsyncChildTail));
    }
    (None, None)
}

/// The newest visible assistant text in a subagent transcript (the async returned
/// message). Reads only the tail (newest-first), stopping at the first assistant record
/// carrying visible text.
pub(crate) fn child_tail_text(path: &Path) -> Option<String> {
    let mut found: Option<String> = None;
    let _ = tail_records(path, 0, |rec| {
        if let Some(t) = rec.agent_text() {
            found = Some(t);
            return false; // newest visible assistant text is enough
        }
        true
    });
    found
}

/// A workflow RUN node (§5), parsed from a top-level `<session>/workflows/wf_*.json`
/// manifest (NOT `subagents/workflows/`). Surfaced as the parent of its workflow agents in
/// the `--tree` view. Fields are best-effort (a key absent in an older manifest → `None`).
#[derive(Debug, Clone)]
pub struct WorkflowRun {
    /// `runId` (== the `wf_<id>` stem, which matches the `subagents/workflows/wf_<id>/`
    /// dir name — the join key to the workflow agents).
    pub run_id: String,
    pub task_id: Option<String>,
    pub workflow_name: Option<String>,
    pub status: Option<String>,
    pub agent_count: Option<u64>,
    pub duration_ms: Option<u64>,
    pub total_tokens: Option<u64>,
    pub total_tool_calls: Option<u64>,
    pub default_model: Option<String>,
    pub started_utc: Option<String>,
}

/// Read every top-level `<session>/workflows/wf_*.json` manifest (§5) as a [`WorkflowRun`].
/// Returns an empty vec when the session has no sidecar / no `workflows/` dir (never an
/// error for the common no-workflow case). The `workflows/scripts/` subdir and any
/// non-`wf_*.json` entry are ignored.
pub fn discover_workflow_runs(session_jsonl: &Path) -> Result<Vec<WorkflowRun>> {
    let Some(sidecar) = sidecar_dir_for_session(session_jsonl) else {
        return Ok(Vec::new());
    };
    let wf_dir = sidecar.join("workflows");
    if !wf_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let read = std::fs::read_dir(&wf_dir)
        .with_context(|| format!("cannot read workflows dir {}", wf_dir.display()))?;
    for entry in read {
        let Ok(entry) = entry else { continue };
        let p = entry.path();
        let is_file = entry.file_type().map(|ft| ft.is_file()).unwrap_or(false);
        if !is_file {
            continue; // skip the `scripts/` subdir etc.
        }
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if !(name.starts_with("wf_") && name.ends_with(".json")) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue; // malformed manifest → skip (never crash)
        };
        let str_f = |k: &str| {
            v.get(k)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        };
        let u64_f = |k: &str| v.get(k).and_then(serde_json::Value::as_u64);
        // run_id: prefer the manifest field, fall back to the filename stem.
        let run_id = str_f("runId").unwrap_or_else(|| name.trim_end_matches(".json").to_string());
        out.push(WorkflowRun {
            run_id,
            task_id: str_f("taskId"),
            workflow_name: str_f("workflowName"),
            status: str_f("status"),
            agent_count: u64_f("agentCount"),
            duration_ms: u64_f("durationMs"),
            total_tokens: u64_f("totalTokens"),
            total_tool_calls: u64_f("totalToolCalls"),
            default_model: str_f("defaultModel"),
            started_utc: str_f("startTime").or_else(|| str_f("timestamp")),
        });
    }
    out.sort_by(|a, b| a.run_id.cmp(&b.run_id));
    Ok(out)
}

/// Build the fully-linked [`SubagentNode`] list for one session (§Part A): discover the
/// subagents, index the parent spawns, then for each subagent join the spawn meta (true
/// trigger time + returned message) and, when `with_files`, extract its files-changed.
///
/// `with_files` gates the (heavier) per-node transcript re-scan for mutations — off by
/// default so a plain `agents` listing stays cheap. The nodes come back flat (depth 0);
/// the tool_use-graph nesting is a no-op on current data (depth uniformly 1).
pub fn build_topology(session_jsonl: &Path, with_files: bool) -> Result<Vec<SubagentNode>> {
    let subs = discover_subagents(session_jsonl)?;
    if subs.is_empty() {
        return Ok(Vec::new());
    }
    // GLOBAL spawn index (main + every subagent transcript) so a nested agent's spawn —
    // recorded in its spawning agent's transcript — links the child to that agent. On-disk
    // layout is flat, so `subs` already holds every agent at any depth; this recovers the
    // LOGICAL parent + the nested agent's spawn metadata the flat layout drops.
    let index = build_global_spawn_index(session_jsonl, &subs)?;
    // Every distinct workflow journal read+parsed ONCE for the whole build (see JournalCache).
    let journals = JournalCache::build(&subs);
    // Build each node IN PARALLEL: `node_for` is pure (reads only its own transcript + the shared
    // `&index`), so this is a drop-in `par_iter`. `rayon`'s ordered collect preserves the `subs`
    // order, so the resulting `nodes` vec is byte-identical to the old serial `.iter().map()`.
    // Like the spawn-index parallelism, this is what rescues the single-session target where the
    // caller's outer across-sessions `par_iter` runs on one thread.
    let mut nodes: Vec<SubagentNode> = subs
        .par_iter()
        .map(|s| node_for(s, &index, &journals, with_files))
        .collect::<Result<_>>()?;
    assign_depths(&mut nodes);
    Ok(nodes)
}

/// Set each node's `depth` = its number of AGENT ancestors (0 = a direct subagent of the
/// session). Walks the `parent_agent_id` chain via an id→parent map, with a cycle guard (a
/// corrupt/forged chain can never hang the walk). The on-disk set is flat, so this is the
/// only place depth>0 is established.
pub(crate) fn assign_depths(nodes: &mut [SubagentNode]) {
    let parent: std::collections::HashMap<String, Option<String>> = nodes
        .iter()
        .map(|n| (n.agent_id.clone(), n.parent_agent_id.clone()))
        .collect();
    for n in nodes.iter_mut() {
        let mut depth = 0usize;
        let mut cur = n.parent_agent_id.clone();
        let mut guard = 0usize;
        while let Some(pid) = cur {
            depth += 1;
            guard += 1;
            if guard > 64 {
                break; // defensive: a cycle in a forged chain never hangs
            }
            cur = parent.get(&pid).cloned().flatten();
        }
        n.depth = depth;
    }
}

/// Build one [`SubagentNode`] from a discovered [`Subagent`] + the session spawn index.
pub(crate) fn node_for(
    subagent: &Subagent,
    index: &ParentSpawnIndex,
    journals: &JournalCache,
    with_files: bool,
) -> Result<SubagentNode> {
    let lc = lifecycle(subagent, journals)?;
    // Effective spawn id: the meta `toolUseId` for a built-in/workflow agent, OR — for a
    // TEAMMATE, whose meta carries none — the NAME-join to its spawning `Agent` tool_use. This
    // single resolution lights up the whole spawn linkage below (trigger, parent, tool, type).
    let effective_spawn_id = subagent.spawn_tool_use_id.clone().or_else(|| {
        if subagent.kind == SubagentKind::Teammate {
            subagent
                .name
                .as_deref()
                .and_then(|nm| index.spawn_id_for_name(nm, lc.started_utc.as_deref()))
        } else {
            None
        }
    });
    let spawn = effective_spawn_id.as_deref().and_then(|id| index.spawn(id));
    // True trigger time = parent tool_use ts; fall back to the child-head ts.
    let trigger_utc = spawn
        .and_then(|s| s.trigger_utc.clone())
        .or_else(|| lc.started_utc.clone());
    // Description: prefer the built-in meta's, fall back to the spawn input's.
    let description = lc
        .description
        .clone()
        .or_else(|| spawn.and_then(|s| s.description.clone()));
    // agentType: a TEAMMATE meta overloads `agentType` with the teammate NAME (e.g. `VSRepro`),
    // so prefer the spawn's real `subagent_type` (`oh-my-claudecode:qa-tester`) and keep the
    // meta name only as a fallback. For built-in/workflow, prefer the meta then fall back to the
    // spawn's `subagent_type` (richer than the bare `workflow-subagent` for an unlabeled meta).
    let agent_type = if subagent.kind == SubagentKind::Teammate {
        spawn
            .and_then(|s| s.subagent_type.clone())
            .or_else(|| lc.agent_type.clone())
    } else {
        lc.agent_type
            .clone()
            .or_else(|| spawn.and_then(|s| s.subagent_type.clone()))
    };
    let spawn_tool = spawn.and_then(|s| s.name.clone());
    let (returned_message, returned_message_source) =
        resolve_returned_message(subagent, index, journals);
    // Classify a frozen lane (if any): a pending Bash whose command CC would hoist (dangerous rm)
    // is escalation-blocked (waiting for a human); anything else pending is awaiting-execution.
    let (pending_tool_use_id, pending_tool_name, pending_classification, pending_since_utc) =
        match &lc.pending {
            Some(p) => {
                let class = if p.tool_name == "Bash"
                    && p.command
                        .as_deref()
                        .is_some_and(crate::bash_danger::is_dangerous_rm)
                {
                    PendingClassification::EscalationBlocked
                } else {
                    PendingClassification::AwaitingExecution
                };
                (
                    Some(p.tool_use_id.clone()),
                    Some(p.tool_name.clone()),
                    Some(class),
                    p.since_utc.clone(),
                )
            }
            None => (None, None, None, None),
        };
    let files_changed = if with_files {
        node_files_changed(&subagent.path)?
    } else {
        Vec::new()
    };
    Ok(SubagentNode {
        agent_id: subagent.agent_id.clone(),
        kind: subagent.kind,
        parent_session_id: subagent.parent_session_id.clone(),
        parent_agent_id: effective_spawn_id
            .as_deref()
            .and_then(|id| index.parent_agent_for(id)),
        spawn_tool_use_id: effective_spawn_id.clone(),
        spawn_tool,
        workflow_id: subagent.workflow_id.clone(),
        agent_type,
        name: subagent.name.clone(),
        team_name: subagent.team_name.clone(),
        description,
        trigger_utc,
        started_utc: lc.started_utc.clone(),
        // The lifecycle's `completed_utc` is the raw tail ts; it is a COMPLETION only
        // when the status resolved Completed — otherwise it is last-activity.
        completed_utc: (lc.status == SubagentStatus::Completed)
            .then(|| lc.completed_utc.clone())
            .flatten(),
        last_activity_utc: lc.completed_utc.clone(),
        returned_message,
        returned_message_source,
        status: lc.status,
        pending_tool_use_id,
        pending_tool_name,
        pending_classification,
        pending_since_utc,
        files_changed,
        depth: 0,
        children: Vec::new(),
        skipped_lines: lc.skipped_lines,
    })
}

/// Extract a subagent's files-changed by running the SAME structured + Bash mutation
/// extractors `files` uses, over the node's own transcript (§query-5). Returns
/// `(path, op_label, is_create)` per mutation, de-duplicated to one row per (path, op).
pub(crate) fn node_files_changed(path: &Path) -> Result<Vec<(String, String, bool)>> {
    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(Vec::new());
    };
    let bytes: &[u8] = &mmap;
    let mut records: Vec<Record> = Vec::new();
    scan_lines_bytes(bytes, |line| {
        if let Ok(Some(rec)) = parse_line(line) {
            records.push(rec);
        }
    })?;
    let muts = crate::files::mutations_in_records(&records);
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for m in muts {
        let key = (m.path.clone(), m.op.label());
        if seen.insert(key) {
            out.push((m.path, m.op.label().to_string(), m.is_create));
        }
    }
    Ok(out)
}
