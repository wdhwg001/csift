//! On-disk discovery: sidecar dir, flat/workflow layouts, defensive nesting.

use super::*;

/// The sidecar directory `<ENCODED>/<session-uuid>/` for a top-level session jsonl,
/// or `None` if the session has no sidecar. The sidecar is named after the session
/// uuid (the jsonl basename without `.jsonl`).
#[must_use]
pub fn sidecar_dir_for_session(session_jsonl: &Path) -> Option<PathBuf> {
    let stem = session_jsonl.file_stem()?.to_str()?;
    let parent = session_jsonl.parent()?;
    let dir = parent.join(stem);
    dir.is_dir().then_some(dir)
}

/// Discover every subagent transcript under a top-level session's sidecar dir.
///
/// Walks `<session-uuid>/subagents/` for built-in `agent-<hex>.jsonl` and
/// `<session-uuid>/subagents/workflows/wf_*/agent-<hex>.jsonl` for workflow agents.
/// **`journal.jsonl` is excluded** (it is an event log, not a transcript), as is any
/// non-`agent-*.jsonl` or `.meta.json` file. Returns an empty vec when the session
/// has no sidecar / no subagents (never an error for the common no-subagent case).
pub fn discover_subagents(session_jsonl: &Path) -> Result<Vec<Subagent>> {
    let Some(sidecar) = sidecar_dir_for_session(session_jsonl) else {
        return Ok(Vec::new());
    };
    let parent_session_id = session_jsonl
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();

    let subagents_dir = sidecar.join("subagents");
    if !subagents_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();

    // (A) built-in: direct children `agent-<hex>.jsonl` of `subagents/`.
    for p in agent_jsonls_in(&subagents_dir)? {
        out.push(make_subagent(
            p,
            SubagentKind::BuiltinTask,
            &parent_session_id,
            None,
        ));
    }

    // (B) workflow: `subagents/workflows/wf_*/agent-<hex>.jsonl`.
    let workflows_dir = subagents_dir.join("workflows");
    if workflows_dir.is_dir() {
        for wf_dir in subdirs_in(&workflows_dir)? {
            let workflow_id = wf_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string);
            for p in agent_jsonls_in(&wf_dir)? {
                out.push(make_subagent(
                    p,
                    SubagentKind::Workflow,
                    &parent_session_id,
                    workflow_id.clone(),
                ));
            }
        }
    }

    // (C) DEFENSIVE recursion (insurance, not a present-data fix). Verified 2026-06-07:
    // across all 2348 subagent transcripts on disk there are ZERO sub-sub-agents — the
    // real layout is single-level FLAT (a child of a general-purpose subagent would land
    // flat in this SAME `subagents/` dir, already covered by (A)). But if a FUTURE Claude
    // Code layout nests a child under `subagents/agent-<hex>/subagents/agent-<hex>.jsonl`,
    // (A)/(B) would silently drop it. This bounded walk descends ONLY into directories
    // literally named `subagents` (skipping the already-handled top-level one + the
    // `workflows/` subtree), depth-capped to prevent symlink-cycle blowups, deduping by
    // absolute path so nothing already found is double-counted. Kept cheap: read_dir-only
    // (O(entries), no transcript-content read), same envelope as (A)/(B).
    let already: std::collections::HashSet<PathBuf> = out.iter().map(|s| s.path.clone()).collect();
    discover_nested_defensive(
        &subagents_dir,
        &parent_session_id,
        &already,
        MAX_NESTED_DEPTH,
        &mut out,
    )?;

    // Deterministic order: by (kind, agent_id) so output is stable across runs.
    out.sort_by(|a, b| (a.kind.label(), &a.agent_id).cmp(&(b.kind.label(), &b.agent_id)));
    Ok(out)
}

/// Depth cap for the defensive nested-subagents walk. 3 bounds cost + breaks any
/// symlink cycle (symlinks are skipped too, so this is belt-and-suspenders). The real
/// data is FLAT (depth 0), so this only ever fires on a hypothetical future layout.
pub(crate) const MAX_NESTED_DEPTH: usize = 3;

/// Defensive bounded walk: descend into any directory literally named `subagents` nested
/// under `dir` (a hypothetical future `subagents/agent-<hex>/subagents/…` layout),
/// collecting flat `agent-<hex>.jsonl` transcripts there as built-in subagents. Skips
/// symlinks (no follow), excludes the `workflows/` subtree (handled by (B)) and any path
/// already discovered, and stops at `depth == 0`. Kind is classified by path location
/// (these nested ones sit under a `subagents/` dir ⇒ `BuiltinTask`).
pub(crate) fn discover_nested_defensive(
    dir: &Path,
    parent_session_id: &str,
    already: &std::collections::HashSet<PathBuf>,
    depth: usize,
    out: &mut Vec<Subagent>,
) -> Result<()> {
    if depth == 0 {
        return Ok(());
    }
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return Ok(()), // unreadable dir → degrade silently (insurance path)
    };
    for entry in read {
        let Ok(entry) = entry else { continue };
        let p = entry.path();
        // Skip symlinks entirely (no follow) — cycle + escape safety. `symlink_metadata`
        // does NOT traverse the link, so a symlinked dir is classified as a symlink here.
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            continue;
        }
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if name == "workflows" {
            // The workflow subtree is owned by (B); do not re-walk it here.
            continue;
        }
        if name == "subagents" {
            // A NESTED subagents dir: collect its flat agent transcripts (deduped), then
            // keep descending (its children may themselves nest a `subagents/`).
            for jp in agent_jsonls_in(&p)? {
                if !already.contains(&jp) {
                    out.push(make_subagent(
                        jp,
                        SubagentKind::BuiltinTask,
                        parent_session_id,
                        None,
                    ));
                }
            }
        }
        // Recurse into every non-symlink subdir (agent-<hex>/, a nested subagents/, …).
        discover_nested_defensive(&p, parent_session_id, already, depth - 1, out)?;
    }
    Ok(())
}

/// Just the subagent transcript file paths for a session — the surface `list` /
/// `search` need to span subagent work (no lifecycle parse). Excludes journals.
pub fn subagent_transcript_files(session_jsonl: &Path) -> Result<Vec<PathBuf>> {
    Ok(discover_subagents(session_jsonl)?
        .into_iter()
        .map(|s| s.path)
        .collect())
}

/// `agent-<hex>.jsonl` files directly inside `dir` (NOT recursing into `workflows/`,
/// and explicitly NOT `journal.jsonl` — only the `agent-` prefix qualifies).
pub(crate) fn agent_jsonls_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let read = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read subagents dir {}", dir.display()))?;
    for entry in read {
        let entry =
            entry.with_context(|| format!("error reading an entry in {}", dir.display()))?;
        let p = entry.path();
        let is_file = entry.file_type().map(|ft| ft.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }
        // Must be `agent-<...>.jsonl`. This naming rule is what keeps `journal.jsonl`
        // (no `agent-` prefix) out, and keeps `.meta.json` companions out (wrong ext).
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if name.starts_with("agent-") && p.extension().is_some_and(|e| e == "jsonl") {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// Immediate subdirectories of `dir` (the `wf_*` workflow dirs). Sorted.
pub(crate) fn subdirs_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let read = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read workflows dir {}", dir.display()))?;
    for entry in read {
        let entry =
            entry.with_context(|| format!("error reading an entry in {}", dir.display()))?;
        let p = entry.path();
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        if is_dir {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}
