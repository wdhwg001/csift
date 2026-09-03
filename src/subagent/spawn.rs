//! Spawn indexing: ParentSpawnIndex, the global spawn index, returned-message sources.

use super::*;

/// One spawn fact indexed off a parent `Task`/`Agent`/`Workflow` `tool_use` (§2): the
/// spawning tool name, the TRUE trigger timestamp (the tool_use record's ts), the
/// human-readable description, and the requested subagent type. Built once per session by
/// [`index_parent_spawns`] and joined to a [`Subagent`] by `spawn_tool_use_id`.
#[derive(Debug, Clone, Default)]
pub struct SpawnMeta {
    /// The spawning tool name as spelled in the transcript (`Agent` / `Task` / `Workflow`).
    /// Surfaced on the node as `spawn_tool` so a consumer can distinguish an `Agent`-tool
    /// launch from a `Workflow`-tool launch (the kind is path-derived; this is the
    /// transcript-side corroboration).
    pub name: Option<String>,
    /// The parent tool_use record's timestamp - the TRUE trigger instant (§4).
    pub trigger_utc: Option<String>,
    /// `input.description` on the spawning tool_use.
    pub description: Option<String>,
    /// `input.subagent_type` on the spawning tool_use - the richer agent-type label used
    /// as a fallback when the built-in meta.json's `agentType` is absent.
    pub subagent_type: Option<String>,
}

/// A parent-session spawn index (§2): `tool_use_id → SpawnMeta` for every
/// `Task`/`Agent`/`Workflow` tool_use, PLUS `tool_use_id → tool_result_text` for the
/// paired result (the sync returned-message source). Built once per session by a single
/// forward scan of the parent transcript, then joined to each subagent.
#[derive(Debug, Clone, Default)]
pub struct ParentSpawnIndex {
    pub(crate) spawns: std::collections::HashMap<String, SpawnMeta>,
    pub(crate) tool_results: std::collections::HashMap<String, String>,
    /// `spawn tool_use_id → issuing agent` - `Some(agent_id)` when the spawn was recorded
    /// in a SUBAGENT's transcript (so the spawned child is a sub-subagent of that agent),
    /// `None` when issued by the main session itself. Populated only when the index is built
    /// GLOBALLY ([`build_global_spawn_index`]); a single-transcript build leaves it empty.
    pub(crate) issuer: std::collections::HashMap<String, Option<String>>,
    /// `spawn input.name → [(trigger_utc, tool_use_id)]` for every spawn tool_use that named
    /// its agent. The NAME-join fallback for a TEAMMATE, whose meta carries no `toolUseId` (so
    /// the usual id-join can't reach its spawning `Agent` tool_use). Keyed by the `Agent` tool's
    /// `name` param (== the teammate's meta `name`). A name may recur across a session, so the
    /// values are a list disambiguated by trigger time in [`Self::spawn_id_for_name`].
    pub(crate) by_name: std::collections::HashMap<String, Vec<(Option<String>, String)>>,
}

impl ParentSpawnIndex {
    /// The [`SpawnMeta`] for a spawning tool_use id, if indexed.
    #[must_use]
    pub fn spawn(&self, tool_use_id: &str) -> Option<&SpawnMeta> {
        self.spawns.get(tool_use_id)
    }

    /// The paired tool_result text for a tool_use id, if present (the sync returned
    /// message - may be the `Async agent launched …` sentinel).
    #[must_use]
    pub fn tool_result_text(&self, tool_use_id: &str) -> Option<&str> {
        self.tool_results.get(tool_use_id).map(String::as_str)
    }

    /// The PARENT agent id for a spawn tool_use id: `Some(agent_id)` when that spawn was
    /// issued from a subagent transcript (⇒ the spawned child nests under that agent),
    /// `None` when main-issued or unknown. The agent→agent topology link.
    #[must_use]
    pub fn parent_agent_for(&self, spawn_tool_use_id: &str) -> Option<String> {
        self.issuer.get(spawn_tool_use_id).cloned().flatten()
    }

    /// The spawning tool_use id for a NAMED spawn (the teammate name-join, §FIX3). Among the
    /// spawns that share `name`, prefer the LATEST whose trigger ≤ `at_or_before` (the child's
    /// head ts - the spawn always precedes the child), so a recurring name binds to the right
    /// launch; fall back to the first recorded spawn when none qualifies (or no bound given).
    /// `None` when the name was never used to spawn. ISO8601-UTC strings compare chronologically.
    #[must_use]
    pub fn spawn_id_for_name(&self, name: &str, at_or_before: Option<&str>) -> Option<String> {
        let cands = self.by_name.get(name)?;
        if let Some(bound) = at_or_before {
            if let Some(best) = cands
                .iter()
                .filter(|(ts, _)| ts.as_deref().is_some_and(|ts| ts <= bound))
                .max_by(|a, b| {
                    a.0.as_deref()
                        .unwrap_or("")
                        .cmp(b.0.as_deref().unwrap_or(""))
                })
            {
                return Some(best.1.clone());
            }
        }
        cands.first().map(|(_, id)| id.clone())
    }

    /// Fold `other` INTO `self` - used by [`build_global_spawn_index`] to merge the per-subagent
    /// LOCAL indexes built in parallel back into the global one. The id-keyed maps
    /// (`spawns`/`tool_results`/`issuer`) are FIRST-wins (v0.10.2): a tool_use id is unique
    /// per SPAWN, but a `/fork` child is a clone of its parent's transcript and repeats every
    /// spawn record the parent had issued before the fork, so a later-wins fold would
    /// re-parent those siblings onto the clone. The main transcript is folded first and the
    /// subs follow in the deterministic discovery order, so the original issuer keeps the
    /// entry and a clone's copy never displaces it. The `by_name` lists are APPENDED (self's
    /// entries first), keeping the per-name order byte-identical to a serial scan.
    pub(crate) fn merge(&mut self, other: ParentSpawnIndex) {
        for (k, v) in other.spawns {
            self.spawns.entry(k).or_insert(v);
        }
        for (k, v) in other.tool_results {
            self.tool_results.entry(k).or_insert(v);
        }
        for (k, v) in other.issuer {
            self.issuer.entry(k).or_insert(v);
        }
        for (name, mut vals) in other.by_name {
            self.by_name.entry(name).or_default().append(&mut vals);
        }
    }
}

/// Build the [`ParentSpawnIndex`] for a session by a single forward scan of its parent
/// transcript (§2). For each `Task`/`Agent`/`Workflow` tool_use, record its id → spawn
/// facts (name, trigger ts, description, subagent_type). For each tool_result, record its
/// `tool_use_id → rendered text`. A missing / unreadable parent jsonl yields an empty
/// index (degrade, never error).
pub fn index_parent_spawns(parent_jsonl: &Path) -> Result<ParentSpawnIndex> {
    let mut idx = ParentSpawnIndex::default();
    scan_spawns_into(parent_jsonl, None, &mut idx)?;
    Ok(idx)
}

/// Build the GLOBAL spawn index: the main transcript (issuer `None`) PLUS every subagent
/// transcript (issuer = that agent's id). This is what makes agent→agent nesting resolvable -
/// a sub-subagent's spawn `Task`/`Agent` tool_use is recorded in its SPAWNING agent's
/// transcript, NOT the main one, so a main-only scan ([`index_parent_spawns`]) can't see it.
/// The union also recovers a nested agent's trigger ts / description / subagent_type (which
/// likewise live in the spawning agent's transcript). On-disk layout is flat (every agent
/// under `<main>/subagents/`), so the children are already discovered; this only adds the
/// LOGICAL parent linkage the flat layout drops.
pub fn build_global_spawn_index(main_jsonl: &Path, subs: &[Subagent]) -> Result<ParentSpawnIndex> {
    // The main scan (issuer `None`) is exactly `index_parent_spawns`; then union each
    // subagent transcript tagged with its own agent id as the issuer.
    let mut idx = index_parent_spawns(main_jsonl)?;
    // Scan each subagent transcript into its OWN local index IN PARALLEL, then fold the locals in.
    // Parallelizing ACROSS subs is what rescues the single-session target (`agents @<uuid>`), where
    // the caller's across-sessions `par_iter` (agents.rs) degenerates to one thread and would
    // otherwise leave all 3000+ subs to a single core. `rayon`'s ordered collect preserves the
    // deterministic `subs` order, so the subsequent in-order merge yields a byte-identical index.
    let locals: Vec<ParentSpawnIndex> = subs
        .par_iter()
        .map(|s| {
            let mut local = ParentSpawnIndex::default();
            scan_spawns_into(&s.path, Some(s.agent_id.as_str()), &mut local)?;
            Ok(local)
        })
        .collect::<Result<Vec<_>>>()?;
    for local in locals {
        idx.merge(local);
    }
    Ok(idx)
}

/// Scan one transcript for spawn tool_uses + tool_results, accumulating into `idx`. `issuer`
/// tags every spawn id with the agent that issued it (`None` = the main session). A missing /
/// unreadable jsonl is a no-op (degrade, never error).
pub(crate) fn scan_spawns_into(
    jsonl: &Path,
    issuer: Option<&str>,
    idx: &mut ParentSpawnIndex,
) -> Result<()> {
    let Some(mmap) = mmap_bytes(jsonl)? else {
        return Ok(());
    };
    let bytes: &[u8] = &mmap;
    // Byte-prefilter + parallel parse (mirrors search's stage-1 / `files`' candidate gate): only a
    // line carrying a `tool_use` (a spawn) or a paired `tool_result` (the sync returned-message
    // source) can contribute to the index; every other line (thinking/text/genuine-user/summary/
    // system) is skipped BEFORE the full serde parse. This also within-file-parallelizes the big
    // 398 MB main scan. Malformed candidate lines are silently dropped (as the old scan did - the
    // spawn index is best-effort), so the returned skip count is intentionally ignored.
    let (records, _skipped) = parse_candidates_parallel(bytes, spawn_line_candidate);
    for (_line_no, rec) in &records {
        accumulate_spawns(rec, issuer, idx);
    }
    Ok(())
}

/// Byte prefilter for [`scan_spawns_into`]: keep a raw line only when it can carry a spawn
/// tool_use or a paired tool_result. A tool_result block always carries a `tool_use_id`
/// (⊇ the `tool_use` literal), so `tool_use` alone is already a complete superset; the explicit
/// `tool_result` disjunct documents intent and stays conservative. Cheap SIMD `memmem`, no parse.
pub(crate) fn spawn_line_candidate(line: &[u8]) -> bool {
    memchr::memmem::find(line, b"tool_use").is_some()
        || memchr::memmem::find(line, b"tool_result").is_some()
}

/// Fold one candidate record's spawn tool_uses + paired tool_results into `idx`. `issuer` tags
/// every spawn id with the agent that issued it (`None` = the main session). Split out of
/// [`scan_spawns_into`] so the (prefiltered, parallel) parse and this serial accumulation are
/// separate concerns; the body is identical to the old inline closure.
pub(crate) fn accumulate_spawns(rec: &Record, issuer: Option<&str>, idx: &mut ParentSpawnIndex) {
    let Some(blocks) = rec.blocks() else {
        return;
    };
    for b in blocks {
        match b {
            Block::ToolUse {
                id: Some(id),
                name: Some(name),
                input,
            } if is_spawn_tool(name) => {
                let input = input.as_ref();
                let str_in = |k: &str| {
                    input
                        .and_then(|v| v.get(k))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                };
                idx.spawns.insert(
                    id.clone(),
                    SpawnMeta {
                        name: Some(name.clone()),
                        trigger_utc: rec.timestamp.clone(),
                        description: str_in("description"),
                        subagent_type: str_in("subagent_type"),
                    },
                );
                idx.issuer.insert(id.clone(), issuer.map(str::to_string));
                // Index by the spawn's `input.name` (the `Agent` tool's `name` param) so a
                // teammate - whose meta has no `toolUseId` - can name-join to its launch.
                if let Some(spawn_name) = str_in("name") {
                    idx.by_name
                        .entry(spawn_name)
                        .or_default()
                        .push((rec.timestamp.clone(), id.clone()));
                }
            }
            Block::ToolResult {
                tool_use_id: Some(id),
                content: Some(c),
                ..
            } => {
                idx.tool_results
                    .insert(id.clone(), tool_result_content_text(c));
            }
            _ => {}
        }
    }
}

/// True for a tool name that SPAWNS a subagent. The real transcript spelling is `Agent`
/// (151× in session 0a1b2c3d) and `Workflow` (22×); `Task` is matched defensively (the
/// canonical built-in Task-tool name, present in other corpora).
pub(crate) fn is_spawn_tool(name: &str) -> bool {
    matches!(name, "Agent" | "Task" | "Workflow")
}

/// The synthesized prefix Claude Code writes into a tool_result when a subagent is
/// launched ASYNCHRONOUSLY (run_in_background) - the real returned message is then NOT in
/// the parent tool_result but in the child transcript tail. Verified 17× in session
/// 0a1b2c3d.
pub(crate) const ASYNC_LAUNCH_SENTINEL: &str = "Async agent launched";

/// Where a subagent's returned message was resolved FROM (§3) - surfaced so a consumer
/// knows whether it read the parent tool_result, the child transcript tail, or the
/// workflow journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnedMsgSource {
    /// A synchronous built-in: the parent tool_result text IS the returned message.
    SyncToolResult,
    /// An async built-in (`Async agent launched …` sentinel): the message is the child
    /// transcript's tail assistant text.
    AsyncChildTail,
    /// A workflow agent: the message is the `journal.jsonl` `result` event payload.
    WorkflowJournal,
}

impl ReturnedMsgSource {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            ReturnedMsgSource::SyncToolResult => "sync-tool-result",
            ReturnedMsgSource::AsyncChildTail => "async-child-tail",
            ReturnedMsgSource::WorkflowJournal => "workflow-journal",
        }
    }
}

/// The `result`-event payload for a workflow subagent's `journal.jsonl` (§3), rendered to
/// text. Extends [`journal_reports_completion`]'s scan to capture the payload instead of a
/// bool. `None` for a built-in (no journal), an absent / unreadable journal, or no
/// matching `result` event. The payload is usually a string (the agent's final message);
/// a non-string payload is JSON-rendered so it is never lost.
#[must_use]
pub fn journal_result(subagent: &Subagent, journals: &JournalCache) -> Option<String> {
    journals
        .events_for(subagent)?
        .results
        .get(&subagent.agent_id)?
        .clone()
}
