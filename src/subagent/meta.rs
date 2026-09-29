//! meta.json fields, subagent construction, the per-run JournalCache.

use super::*;

/// Build a [`Subagent`], locating its `.meta.json` companion if present and reading the
/// `toolUseId` + `name` from it (the spawn linkage for the topology, section 1).
pub(crate) fn make_subagent(
    path: PathBuf,
    kind: SubagentKind,
    parent_session_id: &str,
    workflow_id: Option<String>,
) -> Subagent {
    // The on-disk filename stem is `agent-<hex>`, but the CANONICAL agent id - the
    // value in the transcript record's `agentId` field AND in the workflow journal's
    // `agentId` - is the bare `<hex>` WITHOUT the `agent-` prefix (verified against
    // real data). We store the bare hex so journal-completion lookup matches and the
    // id we print equals the record's own `agentId`.
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let agent_id = bare_agent_id(stem).to_string();
    // Read the companion `agent-<hex>.meta.json` ONCE here (its located path is used only for this
    // read - `lifecycle` takes agent_type/description off the struct, so the path isn't stored).
    let meta_path = path.with_extension("meta.json");
    let meta_path = meta_path.is_file().then_some(meta_path);
    let meta = read_meta(meta_path.as_deref());
    // A built-in-LOCATION agent whose meta declares `taskKind:"in_process_teammate"` is a
    // teammate, not a plain Task subagent - the only way to tell them apart (both sit at
    // `subagents/agent-<id>.jsonl`). Workflow agents never carry this taskKind, so the upgrade
    // only ever fires from BuiltinTask.
    let kind = if kind == SubagentKind::BuiltinTask
        && meta.task_kind.as_deref() == Some("in_process_teammate")
    {
        SubagentKind::Teammate
    } else {
        kind
    };
    Subagent {
        agent_id,
        kind,
        path,
        parent_session_id: parent_session_id.to_string(),
        workflow_id,
        spawn_tool_use_id: meta.tool_use_id,
        name: meta.name,
        team_name: meta.team_name,
        agent_type: meta.agent_type,
        description: meta.description,
        meta_parent_agent_id: meta.parent_agent_id,
    }
}

/// The fields csift reads from a subagent's `meta.json`. A built-in meta carries
/// `{agentType, description, toolUseId}` (+ often `name`); a workflow agent meta carries only
/// `{agentType}`; a TEAMMATE meta carries `{agentType, description, name, taskKind, teamName,
/// color, model, ...}` and NO `toolUseId`. All are optional - a malformed / missing / key-absent
/// meta yields all-`None` (never an error; the lifecycle still resolves from the transcript).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetaFields {
    pub agent_type: Option<String>,
    pub description: Option<String>,
    /// The spawning parent `Task`/`Agent` tool_use id (built-in only; the topology join
    /// key). Read here because the topology join has no other source for it.
    pub tool_use_id: Option<String>,
    pub name: Option<String>,
    /// `taskKind` - `"in_process_teammate"` marks a teammate (the only way to distinguish it
    /// from a built-in Task subagent, since both share the on-disk location). `None`/other for
    /// a plain built-in or workflow agent.
    pub task_kind: Option<String>,
    /// `teamName` - the team a teammate belongs to (teammate metas only).
    pub team_name: Option<String>,
    /// `parentAgentId` - the harness's own word on the spawning agent (written since CC
    /// 2.1.25x; 92 metas in the reference corpus). Load-bearing for a `/fork` child: its
    /// transcript is a CLONE of the parent's and therefore carries the spawning tool_use
    /// itself, so the tool_use-graph join names the child as its own parent. This field
    /// breaks that self-cycle.
    pub parent_agent_id: Option<String>,
}

/// Read `{agentType, description, toolUseId, name, taskKind, teamName}` from a subagent's
/// `meta.json`, if readable. Returns [`MetaFields::default`] (all `None`) for a missing path,
/// unreadable file, malformed JSON, or any key absent.
pub(crate) fn read_meta(meta_path: Option<&Path>) -> MetaFields {
    let Some(p) = meta_path else {
        return MetaFields::default();
    };
    let Ok(bytes) = std::fs::read(p) else {
        return MetaFields::default();
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return MetaFields::default();
    };
    let str_field = |k: &str| {
        v.get(k)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    MetaFields {
        agent_type: str_field("agentType"),
        description: str_field("description"),
        tool_use_id: str_field("toolUseId"),
        name: str_field("name"),
        task_kind: str_field("taskKind"),
        team_name: str_field("teamName"),
        parent_agent_id: str_field("parentAgentId"),
    }
}

/// True iff the workflow journal alongside a workflow subagent carries a `result`
/// event for `agent_id` (the completion signal, section C). For a built-in subagent (no
/// journal) this is always `false` - completion is inferred from the transcript.
pub(crate) fn journal_reports_completion(subagent: &Subagent, journals: &JournalCache) -> bool {
    journals
        .events_for(subagent)
        .is_some_and(|data| data.results.contains_key(&subagent.agent_id))
}

/// Per-topology-build cache of every distinct `wf_<id>/journal.jsonl`, read + parsed
/// ONCE and shared across the whole node/lifecycle fan-out. Without it each of a
/// workflow run's N agents re-read and re-parsed the SAME journal (an O(N x journal)
/// blowup - a 104-agent run re-parsed its 236 KB journal 104 times, and a 3.5k-agent
/// session re-parsed ~600 MB of journal JSON in aggregate). The cached view is exactly
/// what the two former per-agent scans extracted - first `result` event per agentId -
/// so behaviour is byte-identical, only WHEN the journal is read changes.
#[derive(Debug, Default)]
pub struct JournalCache {
    /// journal path -> its parsed per-agent result events. An unreadable/absent journal
    /// has no entry (the same "no journal => no completion signal" the direct reads had).
    pub(crate) by_path: HashMap<PathBuf, JournalData>,
}

/// What one workflow RUN's own files say about its agents: the journal's per-agent events
/// and the run manifest.
#[derive(Debug, Default)]
pub(crate) struct JournalData {
    /// agentId -> the FIRST `result` event's payload for that agent: `Some(text)` when the
    /// event carries a `result` field (string kept as-is, non-string JSON-rendered so it
    /// is never lost), `None` when it does not (a completion signal with no payload).
    /// Key PRESENCE == "the journal reports this agent completed".
    pub(crate) results: HashMap<String, Option<String>>,
    /// Agent ids a `result` OR a `failed` line closes. Both are terminal (claim TAIL-008), so
    /// either settles that agent's lane.
    pub(crate) closed: std::collections::HashSet<String>,
    /// `started` lines, and `result` / `failed` lines: their difference is the run's agents in
    /// flight while the run has no manifest.
    pub(crate) started: usize,
    pub(crate) closing: usize,
    /// The run MANIFEST's status, when `<sidecar>/workflows/<run>.json` exists. The harness
    /// writes that file only once the run has ENDED (claim TAIL-009), so it settles every
    /// agent of the run whatever a lane's tail shows: a lane the harness stopped after its
    /// structured answer keeps an unreturned call at its tail for good.
    pub(crate) ended: Option<String>,
}

impl JournalCache {
    /// Read + parse each DISTINCT journal among these subagents once. Malformed journal
    /// lines are skipped exactly as the former per-agent scans skipped them.
    pub fn build(subs: &[Subagent]) -> Self {
        let mut cache = Self::default();
        for sub in subs {
            if let Some(journal) = Self::journal_path(sub) {
                cache.add_run(journal);
            }
        }
        cache
    }

    /// Add every workflow run under `main_jsonl`'s sidecar that no discovered lane led to: a
    /// run whose journal has started an agent before its transcript reached the disk is still
    /// a run with agents in flight.
    pub(crate) fn add_session_runs(&mut self, main_jsonl: &Path) {
        let root = main_jsonl
            .with_extension("")
            .join("subagents")
            .join("workflows");
        let Ok(entries) = std::fs::read_dir(&root) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                self.add_run(entry.path().join("journal.jsonl"));
            }
        }
    }

    /// Read one run's journal and manifest once. A run with neither readable has no entry,
    /// the same "no journal => no signal" a missing file always meant.
    fn add_run(&mut self, journal: PathBuf) {
        if self.by_path.contains_key(&journal) {
            return;
        }
        let mut data = JournalData::default();
        let readable = std::fs::read(&journal).is_ok_and(|bytes| {
            for line in bytes.split(|&b| b == b'\n') {
                read_journal_line(line, &mut data);
            }
            true
        });
        data.ended = run_manifest(&journal).map(|bytes| {
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|v| v.get("status")?.as_str().map(str::to_string))
                .unwrap_or_else(|| "unreadable".to_string())
        });
        if readable || data.ended.is_some() {
            self.by_path.insert(journal, data);
        }
    }

    /// The verdict a workflow lane's own RUN gives before its tail is read: an ended run (its
    /// manifest exists) or a journal line closing this agent settles it; `None` leaves the lane
    /// to its tail. ONE rule, read by `status`'s child census and by `agents`' lifecycle, so the
    /// two surfaces agree on which lanes are live.
    pub(crate) fn settled_by_run(&self, subagent: &Subagent) -> Option<String> {
        let run = self.events_for(subagent)?;
        if let Some(status) = &run.ended {
            return Some(format!("its workflow run ended (manifest status {status})"));
        }
        run.closed
            .contains(&subagent.agent_id)
            .then(|| "the workflow journal closes this agent (result or failed)".to_string())
    }

    /// Workflow agents a run's journal has started and not closed, across every run with no
    /// manifest yet. An ended run contributes nothing: an agent killed with its run never
    /// writes a closing line, so its imbalance is permanent and says nothing live.
    pub(crate) fn journal_in_flight(&self) -> usize {
        self.by_path
            .values()
            .filter(|r| r.ended.is_none())
            .map(|r| r.started.saturating_sub(r.closing))
            .sum()
    }

    /// The journal path a workflow subagent's events live in (`None` for a built-in /
    /// teammate - no `workflow_id` => no journal, the same guard the direct reads had).
    pub(crate) fn journal_path(subagent: &Subagent) -> Option<PathBuf> {
        subagent.workflow_id.as_ref()?;
        Some(subagent.path.parent()?.join("journal.jsonl"))
    }

    /// This subagent's parsed journal, when it has one that was readable.
    pub(crate) fn events_for(&self, subagent: &Subagent) -> Option<&JournalData> {
        self.by_path.get(&Self::journal_path(subagent)?)
    }
}

/// Fold one journal line into its run's facts. A line that is not an object with a `type` is
/// skipped, as every journal reader skips it.
fn read_journal_line(line: &[u8], data: &mut JournalData) {
    if line.is_empty() {
        return;
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
        return;
    };
    let kind = v.get("type").and_then(serde_json::Value::as_str);
    let agent = v.get("agentId").and_then(serde_json::Value::as_str);
    match kind {
        Some("started") => data.started += 1,
        Some("result" | "failed") => {
            data.closing += 1;
            if let Some(agent) = agent {
                data.closed.insert(agent.to_string());
            }
        }
        _ => {}
    }
    if kind != Some("result") {
        return;
    }
    let Some(agent) = agent else {
        return;
    };
    let payload = match v.get("result") {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(other) => Some(other.to_string()),
        None => None,
    };
    // FIRST event per agent wins - the former scans returned on first match.
    data.results.entry(agent.to_string()).or_insert(payload);
}

/// The run manifest beside a run's journal: `<sidecar>/subagents/workflows/<run>/journal.jsonl`
/// -> `<sidecar>/workflows/<run>.json`, read when it exists.
fn run_manifest(journal: &Path) -> Option<Vec<u8>> {
    let run = journal.parent()?;
    let name = run.file_name()?.to_str()?;
    let sidecar = run.parent()?.parent()?.parent()?;
    std::fs::read(sidecar.join("workflows").join(format!("{name}.json"))).ok()
}
