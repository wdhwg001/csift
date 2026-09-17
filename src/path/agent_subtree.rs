//! `@<agent-id>` dispatch: the subagent + its topological subtree.

use super::*;

/// Resolve an `@<agent-hex>` target: the subagent's OWN transcript plus (unless
/// `--no-subagents`) its TOPOLOGICAL descendants. Scans `dirs` for the session that owns the
/// agent (its hex is globally unique), builds that session's topology, and emits transcripts
/// per `scope`: `TopLevelOnly` = the agent alone; `WithSubagents` = the agent + descendants.
/// Errors when no such agent exists in scope.
///
/// EVERY copy is returned, not the first one found: a copied project directory carries the
/// whole sidecar with it, so one agent hex can be owned by two copies of one session, and
/// returning the first hit picked a copy by `read_dir` order - possibly the very copy the
/// session path would have dropped - with nothing said about it. The copies are handed back
/// UNMERGED, because the ONE merge runs over the resolver's whole union: two rules deciding
/// two halves of the same basename is what put both copies of one transcript in scope.
pub(crate) fn resolve_agent_subtree(
    dirs: &[ProjectDir],
    hex: &str,
    scope: SubagentScope,
) -> Result<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut owner: Option<PathBuf> = None;
    'scan: for pd in dirs {
        for top in top_level_jsonls(&pd.dir) {
            if let Some(files) = agent_files_under(&top, hex, scope)? {
                found.extend(files);
                owner = Some(top);
                break 'scan;
            }
        }
    }
    // The owning session is known, and an agent hex is globally unique, so the ONLY other
    // file that can hold this agent is a COPY OF THAT SESSION - the same basename under
    // another project directory. That is one `is_file` probe per directory, never a second
    // walk of every sidecar in the corpus, and the difference is measurable: on a live root
    // of 30 project directories, interleaved seven times against the walk-everything shape
    // in one binary, the medians were 0.31 s probing against 0.54 s walking (debug build,
    // warm cache). Probing is also indistinguishable from the pre-merge early return, which
    // did the same scan and then stopped.
    if let (Some(owner), Some(name)) = (&owner, owner.as_ref().and_then(|o| o.file_name())) {
        let mut probed: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
        for pd in dirs {
            let sibling = pd.dir.join(name);
            // A directory can be named twice in one invocation, and probing the same file
            // twice would hand `dedup_copies` a path duplicated against itself - which is
            // not two copies of a session, it is one file counted twice.
            if sibling == *owner || !probed.insert(sibling.clone()) || !sibling.is_file() {
                continue;
            }
            if let Some(files) = agent_files_under(&sibling, hex, scope)? {
                found.extend(files);
            }
        }
    }
    if !found.is_empty() {
        return Ok(found);
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

/// The transcripts ONE top-level session contributes for an `@<agent-hex>` target: the
/// agent's own transcript plus, unless `--no-subagents`, its topological descendants.
/// `None` when this session does not own the agent, which is the common answer.
fn agent_files_under(top: &Path, hex: &str, scope: SubagentScope) -> Result<Option<Vec<PathBuf>>> {
    let subs = crate::subagent::subagent_transcript_files(top)?;
    // agent_id (bare hex) → its transcript path, for this session.
    let by_id: std::collections::HashMap<String, PathBuf> = subs
        .iter()
        .map(|p| (crate::subagent::session_id_from_path(p), p.clone()))
        .collect();
    let Some(own) = by_id.get(hex) else {
        return Ok(None);
    };
    // The descendants come from the agent→agent topology (`parent_agent_id`); on flat real
    // data an agent has none, so the result is just the agent itself - correct, and it
    // nests automatically once CC nests subagents.
    let mut out = vec![own.clone()];
    if !matches!(scope, SubagentScope::TopLevelOnly) {
        let nodes = crate::subagent::build_topology(top, false)?;
        for d in subtree_agent_ids(&nodes, hex) {
            if let Some(p) = by_id.get(&d) {
                out.push(p.clone());
            }
        }
    }
    Ok(Some(out))
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
