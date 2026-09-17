//! `@<agent-id>` dispatch: the subagent + its topological subtree.

use super::*;

/// Resolve an `@<agent-hex>` target: the subagent's OWN transcript plus (unless
/// `--no-subagents`) its TOPOLOGICAL descendants. Scans `dirs` for the session that owns the
/// agent (its hex is globally unique), builds that session's topology, and emits transcripts
/// per `scope`: `TopLevelOnly` = the agent alone; `WithSubagents` = the agent + descendants.
/// Errors when no such agent exists in scope.
pub(crate) fn resolve_agent_subtree(
    dirs: &[ProjectDir],
    hex: &str,
    scope: SubagentScope,
) -> Result<Vec<PathBuf>> {
    for pd in dirs {
        for top in top_level_jsonls(&pd.dir) {
            let subs = crate::subagent::subagent_transcript_files(&top)?;
            // agent_id (bare hex) → its transcript path, for this session.
            let by_id: std::collections::HashMap<String, PathBuf> = subs
                .iter()
                .map(|p| (crate::subagent::session_id_from_path(p), p.clone()))
                .collect();
            if !by_id.contains_key(hex) {
                continue; // not this session's agent
            }
            // Found the owning session. The descendants come from the agent→agent topology
            // (`parent_agent_id`); on flat real data an agent has none, so the result is just
            // the agent itself - correct, and it nests automatically once CC nests subagents.
            let nodes = crate::subagent::build_topology(&top, false)?;
            let descendants = subtree_agent_ids(&nodes, hex);
            let mut out: Vec<PathBuf> = Vec::new();
            // The agent's OWN transcript is always included (both scopes keep the target itself).
            if let Some(p) = by_id.get(hex) {
                out.push(p.clone());
            }
            if !matches!(scope, SubagentScope::TopLevelOnly) {
                for d in &descendants {
                    if let Some(p) = by_id.get(d) {
                        out.push(p.clone());
                    }
                }
            }
            return Ok(out);
        }
    }
    // Exact id matched nothing. A collision-lengthened `search` header token is a PREFIX of a
    // bare-hex agent id (12 hex chars routes here as an exact-id shape), so before giving up,
    // try a unique literal-prefix match over the in-scope agent ids - fail-loud on ambiguity,
    // a unique hit resolves exactly like the full id.
    if hex.len() >= 12 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        let mut hits: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for pd in dirs {
            for top in top_level_jsonls(&pd.dir) {
                for p in crate::subagent::subagent_transcript_files(&top)? {
                    let sid = crate::subagent::session_id_from_path(&p);
                    if sid.starts_with(hex) {
                        hits.insert(sid);
                    }
                }
            }
        }
        match hits.len() {
            1 => {
                let full = hits.iter().next().map(String::as_str).unwrap_or(hex);
                return resolve_agent_subtree(dirs, full, scope);
            }
            n if n > 1 => {
                let ids: Vec<&str> = hits.iter().map(String::as_str).collect();
                bail!(
                    "`@{hex}` is AMBIGUOUS: {n} agent ids start with it ({}). Use more of the id.",
                    ids.join(", ")
                );
            }
            _ => {}
        }
    }
    bail!(
        "no subagent `{hex}` found under the resolved target(s). List ids with \
         `csift agents <session>`, then pass one as `@<agent-hex>`."
    )
}

/// The bare-hex agent ids in `nodes` that DESCEND from `root` (children, grandchildren, …) via
/// the `parent_agent_id` chain. Excludes `root` itself. Cycle-safe (a `visited` set).
pub(crate) fn subtree_agent_ids(
    nodes: &[crate::subagent::SubagentNode],
    root: &str,
) -> Vec<String> {
    let mut children: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for n in nodes {
        if let Some(p) = n.parent_agent_id.as_deref() {
            children.entry(p).or_default().push(n.agent_id.as_str());
        }
    }
    let mut out = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut stack = vec![root];
    while let Some(cur) = stack.pop() {
        if let Some(kids) = children.get(cur) {
            for &k in kids {
                if visited.insert(k) {
                    out.push(k.to_string());
                    stack.push(k);
                }
            }
        }
    }
    out
}
