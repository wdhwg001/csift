//! Subagent / nested-transcript discovery, classification, and lifecycle.
//!
//! ## On-disk layout (empirically mapped against `~/.claude/projects`, 2026-06-07)
//!
//! A top-level session `<ENCODED>/<session-uuid>.jsonl` may own a sibling sidecar
//! directory `<ENCODED>/<session-uuid>/` holding its subagent transcripts in THREE
//! distinct shapes:
//!
//! - **(A) built-in Task/Agent-tool subagent** —
//!   `subagents/agent-<hex>.jsonl` (+ companion `agent-<hex>.meta.json`). The `.jsonl`
//!   uses the identical [`crate::model::Record`] model; its first record has `isSidechain:true`, an
//!   `agentId` field (== the `agent-<hex>` filename stem), and `sessionId` == the
//!   enclosing `<session-uuid>`. meta.json = `{agentType, description, name?, toolUseId}`.
//! - **(B) workflow / OMC workflow-subagent** —
//!   `subagents/workflows/wf_<id>/agent-<hex>.jsonl` (+ `.meta.json`, the dominant
//!   kind). Same record model + `isSidechain:true` + `agentId`. meta.json = `{agentType}`.
//!   Its `cwd` is often a DEEPER in-session path — never re-encode a subagent cwd to
//!   find its project dir.
//! - **(C) workflow journal** — `subagents/workflows/wf_<id>/journal.jsonl`. **NOT a
//!   transcript**: records are workflow events `{agentId, key, type}` (`type` ∈
//!   {`started`, `result`}) with no `message`/role. Excluded from every transcript
//!   list/search; read ONLY to corroborate completion status.
//!
//! ## Kind is determined by PATH LOCATION, not `agentType`
//!
//! `agentType` is NOT a reliable kind discriminator: both (A) and (B) carry the same
//! spread of values (`Explore`, `general-purpose`, `oh-my-claudecode:*`); only the
//! special `workflow-subagent` value is workflow-exclusive. So the authoritative kind
//! is the on-disk location — directly under `subagents/` ⇒ [`SubagentKind::BuiltinTask`];
//! under `subagents/workflows/wf_*/` ⇒ [`SubagentKind::Workflow`] — with `agentType`
//! retained as a descriptive per-row sub-label.
//!
//! ## Linkage back to the parent session
//!
//! Primarily FILESYSTEM (the enclosing `<session-uuid>` dir name), corroborated by the
//! record's `sessionId` field (verified 600/0 mismatches). We use the directory name
//! as the parent-session id (it is the on-disk truth and needs no record parse).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::model::{tool_result_content_text, Block, Record};
use crate::parse::{head_records, scan_lines_bytes, tail_records};
use crate::parse::{mmap_bytes, parse_line};

/// Strip the on-disk `agent-` filename prefix to the bare-hex canonical agent id (the
/// value the transcript record's `agentId` field AND the workflow journal carry). The
/// single source of truth for this rule — used by `make_subagent` and by the
/// `recover` / `session` / `files` subcommands so a subagent row's printed `session_id`
/// is the SAME bare hex `agents` prints, hence joinable across surfaces.
#[must_use]
pub fn bare_agent_id(stem: &str) -> &str {
    stem.strip_prefix("agent-").unwrap_or(stem)
}

/// Subagent kind, keyed off the on-disk path location (authoritative; see module
/// docs — `agentType` is descriptive only, not the discriminator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentKind {
    /// `subagents/agent-<hex>.jsonl` — a built-in Task/Agent-tool subagent.
    BuiltinTask,
    /// `subagents/workflows/wf_<id>/agent-<hex>.jsonl` — a workflow / OMC agent.
    Workflow,
}

impl SubagentKind {
    /// Stable lowercase label used in CLI output + JSON (matches the `--kind` enum).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            SubagentKind::BuiltinTask => "builtin-task",
            SubagentKind::Workflow => "workflow",
        }
    }
}

/// One discovered subagent transcript and its located companion files.
#[derive(Debug, Clone)]
pub struct Subagent {
    /// The `agent-<hex>` filename stem (== the record `agentId`).
    pub agent_id: String,
    pub kind: SubagentKind,
    /// Absolute path to the subagent `.jsonl` transcript.
    pub path: PathBuf,
    /// The enclosing `<session-uuid>` dir name — the parent session id (filesystem
    /// linkage; corroborated by the record `sessionId`).
    pub parent_session_id: String,
    /// The `wf_<id>` workflow id for a workflow subagent; `None` for built-in.
    pub workflow_id: Option<String>,
    /// Companion `agent-<hex>.meta.json` path, if it exists alongside the transcript.
    pub meta_path: Option<PathBuf>,
    /// The parent `Task`/`Agent` tool_use `id` that SPAWNED this subagent, read from the
    /// built-in `meta.json` `toolUseId` (on disk for every built-in subagent; `None` for
    /// workflow agents, whose meta carries only `agentType`). This is the join key into
    /// the parent transcript's spawn index ([`ParentSpawnIndex`]) — it recovers the true
    /// trigger time + the returned message.
    pub spawn_tool_use_id: Option<String>,
}

/// Lifecycle facts derived from a subagent transcript (+ its workflow journal, when
/// present). Timestamps are raw ISO8601 UTC from the transcript's first/last record.
///
/// Identity (agent_id / kind / parent_session_id / workflow_id) lives on the owning
/// [`Subagent`], not duplicated here — `lifecycle` is consumed by [`node_for`], which
/// already holds the `Subagent`. This struct carries ONLY the transcript-derived facts.
#[derive(Debug, Clone)]
pub struct SubagentLifecycle {
    /// `agentType` from meta.json (descriptive sub-label, e.g. `Explore`,
    /// `oh-my-claudecode:executor`, `workflow-subagent`).
    pub agent_type: Option<String>,
    /// Short description from a built-in meta.json (the Task tool's `description`).
    pub description: Option<String>,
    /// First transcript record's timestamp (raw UTC) — the START.
    pub started_utc: Option<String>,
    /// Last transcript record's timestamp (raw UTC) — the COMPLETION (best-effort).
    pub completed_utc: Option<String>,
    /// Resolved status (see [`SubagentStatus`]).
    pub status: SubagentStatus,
    /// Malformed lines skipped while reading the transcript (never hidden).
    pub skipped_lines: usize,
}

/// Determinable subagent run status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentStatus {
    /// A workflow journal carries a `result` event for this agent, OR the transcript
    /// terminates with an assistant end-of-turn message (a clean finish).
    Completed,
    /// The transcript exists but shows no completion signal — likely still running,
    /// or interrupted. We do not over-claim "failed"; this is the honest middle.
    Running,
    /// No timestamps / empty transcript — status cannot be determined.
    Unknown,
}

impl SubagentStatus {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            SubagentStatus::Completed => "completed",
            SubagentStatus::Running => "running",
            SubagentStatus::Unknown => "unknown",
        }
    }
}

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
const MAX_NESTED_DEPTH: usize = 3;

/// Defensive bounded walk: descend into any directory literally named `subagents` nested
/// under `dir` (a hypothetical future `subagents/agent-<hex>/subagents/…` layout),
/// collecting flat `agent-<hex>.jsonl` transcripts there as built-in subagents. Skips
/// symlinks (no follow), excludes the `workflows/` subtree (handled by (B)) and any path
/// already discovered, and stops at `depth == 0`. Kind is classified by path location
/// (these nested ones sit under a `subagents/` dir ⇒ `BuiltinTask`).
fn discover_nested_defensive(
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
fn agent_jsonls_in(dir: &Path) -> Result<Vec<PathBuf>> {
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
fn subdirs_in(dir: &Path) -> Result<Vec<PathBuf>> {
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

/// Build a [`Subagent`], locating its `.meta.json` companion if present and reading the
/// `toolUseId` + `name` from it (the spawn linkage for the topology, §1).
fn make_subagent(
    path: PathBuf,
    kind: SubagentKind,
    parent_session_id: &str,
    workflow_id: Option<String>,
) -> Subagent {
    // The on-disk filename stem is `agent-<hex>`, but the CANONICAL agent id — the
    // value in the transcript record's `agentId` field AND in the workflow journal's
    // `agentId` — is the bare `<hex>` WITHOUT the `agent-` prefix (verified against
    // real data). We store the bare hex so journal-completion lookup matches and the
    // id we print equals the record's own `agentId`.
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let agent_id = bare_agent_id(stem).to_string();
    let meta_path = path.with_extension("meta.json");
    let meta_path = meta_path.is_file().then_some(meta_path);
    let meta = read_meta(meta_path.as_deref());
    Subagent {
        agent_id,
        kind,
        path,
        parent_session_id: parent_session_id.to_string(),
        workflow_id,
        meta_path,
        spawn_tool_use_id: meta.tool_use_id,
    }
}

/// The fields csift reads from a subagent's `meta.json`. A built-in meta carries
/// `{agentType, description, toolUseId}` (+ rarely `name`); a workflow agent meta carries
/// only `{agentType}`. All four are optional — a malformed / missing / key-absent meta
/// yields all-`None` (never an error; the lifecycle still resolves from the transcript).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetaFields {
    pub agent_type: Option<String>,
    pub description: Option<String>,
    /// The spawning parent `Task`/`Agent` tool_use id (built-in only; the topology join
    /// key). Captured here so the previously-dropped `toolUseId` reaches the topology.
    pub tool_use_id: Option<String>,
    pub name: Option<String>,
}

/// Read `{agentType, description, toolUseId, name}` from a subagent's `meta.json`, if
/// readable. Returns [`MetaFields::default`] (all `None`) for a missing path, unreadable
/// file, malformed JSON, or any key absent.
fn read_meta(meta_path: Option<&Path>) -> MetaFields {
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
    }
}

/// True iff the workflow journal alongside a workflow subagent carries a `result`
/// event for `agent_id` (the completion signal, §C). For a built-in subagent (no
/// journal) this is always `false` — completion is inferred from the transcript.
fn journal_reports_completion(subagent: &Subagent) -> bool {
    let Some(wf_id) = &subagent.workflow_id else {
        return false;
    };
    // journal.jsonl sits beside the agent transcript inside `wf_<id>/`.
    let Some(wf_dir) = subagent.path.parent() else {
        return false;
    };
    let journal = wf_dir.join("journal.jsonl");
    let _ = wf_id; // wf_id already implied by wf_dir; kept for clarity/debugging.
    let Ok(bytes) = std::fs::read(&journal) else {
        return false;
    };
    // Each line is a small event object; scan for a `result` event matching agentId.
    for line in bytes.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        let is_result = v.get("type").and_then(serde_json::Value::as_str) == Some("result");
        let same_agent =
            v.get("agentId").and_then(serde_json::Value::as_str) == Some(&subagent.agent_id);
        if is_result && same_agent {
            return true;
        }
    }
    false
}

/// Compute the lifecycle of one subagent: read its transcript HEAD for the start
/// timestamp + TAIL for the completion timestamp & terminal-message signal, consult
/// the workflow journal for an explicit `result`, then resolve a status.
pub fn lifecycle(subagent: &Subagent) -> Result<SubagentLifecycle> {
    let meta = read_meta(subagent.meta_path.as_deref());
    let MetaFields {
        agent_type,
        description,
        ..
    } = meta;

    // HEAD: first record's timestamp == start. We do not need genuine-user logic
    // here — the very first record (isSidechain user seed) IS the start instant.
    let mut started_utc: Option<String> = None;
    let head_skipped = head_records(&subagent.path, |rec| {
        if let Some(ts) = &rec.timestamp {
            started_utc = Some(ts.clone());
            return false; // first timestamped record is enough
        }
        true
    })?;

    // TAIL: last record's timestamp == completion (best-effort), and whether the
    // transcript terminates with a visible assistant message (a clean finish).
    let mut completed_utc: Option<String> = None;
    let mut terminal_agent_msg = false;
    let mut saw_any = false;
    let tail_skipped = tail_records(&subagent.path, |rec| {
        saw_any = true;
        if completed_utc.is_none() {
            if let Some(ts) = &rec.timestamp {
                completed_utc = Some(ts.clone());
            }
        }
        // The newest assistant record carrying visible text == a clean end-of-turn.
        if !terminal_agent_msg && rec.agent_text().is_some() {
            terminal_agent_msg = true;
        }
        // Stop once we have the completion timestamp AND a terminal-message verdict;
        // if the very newest record has no text we still only need a couple of reads.
        completed_utc.is_none() || !terminal_agent_msg
    })?;

    let journal_done = journal_reports_completion(subagent);
    let status = resolve_status(
        saw_any,
        journal_done,
        terminal_agent_msg,
        started_utc.is_some(),
    );

    Ok(SubagentLifecycle {
        agent_type,
        description,
        started_utc,
        completed_utc,
        status,
        skipped_lines: head_skipped + tail_skipped,
    })
}

/// Status resolution rule (honest, never over-claiming "failed"):
/// - journal `result` event present ⇒ `Completed` (the authoritative workflow signal);
/// - else a terminal visible-assistant message ⇒ `Completed` (clean transcript end);
/// - else if we saw records but no completion signal ⇒ `Running`;
/// - else (no records / no start) ⇒ `Unknown`.
fn resolve_status(
    saw_any: bool,
    journal_done: bool,
    terminal_agent_msg: bool,
    has_start: bool,
) -> SubagentStatus {
    if journal_done || terminal_agent_msg {
        SubagentStatus::Completed
    } else if saw_any && has_start {
        SubagentStatus::Running
    } else {
        SubagentStatus::Unknown
    }
}

/// Human-readable duration between start and completion (raw UTC ISO8601), e.g.
/// `14m20s`, `3s`, `2h05m`. `None` when either bound is missing/unparseable.
#[must_use]
pub fn duration_label(started: Option<&str>, completed: Option<&str>) -> Option<String> {
    let s: jiff::Timestamp = started?.parse().ok()?;
    let c: jiff::Timestamp = completed?.parse().ok()?;
    let secs = (c.as_second() - s.as_second()).max(0);
    Some(fmt_secs(secs))
}

/// Format a whole-second duration compactly.
fn fmt_secs(total: i64) -> String {
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}h{m:02}m")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

// ───────────────────────── TOPOLOGY (Part A) ─────────────────────────
//
// csift modelled subagents as a flat list of detached session-like files. The topology
// builder LINKS each subagent back to the parent tool_use that spawned it, via the
// `meta.json` `toolUseId` ⇆ parent transcript `tool_use.id` join. From that join it
// recovers (a) the TRUE trigger time (the parent tool_use ts, which the child-head ts
// lags by seconds), and (b) the RETURNED MESSAGE through a 3-way resolver. Workflow runs
// are surfaced as `WorkflowRun` parent nodes from the unscanned top-level
// `workflows/wf_*.json` manifests. The build is ADDITIVE — it reuses the existing
// discovery + lifecycle primitives, never rewrites them.

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
    /// The parent tool_use record's timestamp — the TRUE trigger instant (§4).
    pub trigger_utc: Option<String>,
    /// `input.description` on the spawning tool_use.
    pub description: Option<String>,
    /// `input.subagent_type` on the spawning tool_use — the richer agent-type label used
    /// as a fallback when the built-in meta.json's `agentType` is absent.
    pub subagent_type: Option<String>,
}

/// A parent-session spawn index (§2): `tool_use_id → SpawnMeta` for every
/// `Task`/`Agent`/`Workflow` tool_use, PLUS `tool_use_id → tool_result_text` for the
/// paired result (the sync returned-message source). Built once per session by a single
/// forward scan of the parent transcript, then joined to each subagent.
#[derive(Debug, Clone, Default)]
pub struct ParentSpawnIndex {
    spawns: std::collections::HashMap<String, SpawnMeta>,
    tool_results: std::collections::HashMap<String, String>,
}

impl ParentSpawnIndex {
    /// The [`SpawnMeta`] for a spawning tool_use id, if indexed.
    #[must_use]
    pub fn spawn(&self, tool_use_id: &str) -> Option<&SpawnMeta> {
        self.spawns.get(tool_use_id)
    }

    /// The paired tool_result text for a tool_use id, if present (the sync returned
    /// message — may be the `Async agent launched …` sentinel).
    #[must_use]
    pub fn tool_result_text(&self, tool_use_id: &str) -> Option<&str> {
        self.tool_results.get(tool_use_id).map(String::as_str)
    }
}

/// Build the [`ParentSpawnIndex`] for a session by a single forward scan of its parent
/// transcript (§2). For each `Task`/`Agent`/`Workflow` tool_use, record its id → spawn
/// facts (name, trigger ts, description, subagent_type). For each tool_result, record its
/// `tool_use_id → rendered text`. A missing / unreadable parent jsonl yields an empty
/// index (degrade, never error).
pub fn index_parent_spawns(parent_jsonl: &Path) -> Result<ParentSpawnIndex> {
    let mut idx = ParentSpawnIndex::default();
    let Some(mmap) = mmap_bytes(parent_jsonl)? else {
        return Ok(idx);
    };
    let bytes: &[u8] = &mmap;
    scan_lines_bytes(bytes, |line| {
        let Ok(Some(rec)) = parse_line(line) else {
            return;
        };
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
    })?;
    Ok(idx)
}

/// True for a tool name that SPAWNS a subagent. The real transcript spelling is `Agent`
/// (151× in session 0a1b2c3d) and `Workflow` (22×); `Task` is matched defensively (the
/// canonical built-in Task-tool name, present in other corpora).
fn is_spawn_tool(name: &str) -> bool {
    matches!(name, "Agent" | "Task" | "Workflow")
}

/// The synthesized prefix Claude Code writes into a tool_result when a subagent is
/// launched ASYNCHRONOUSLY (run_in_background) — the real returned message is then NOT in
/// the parent tool_result but in the child transcript tail. Verified 17× in session
/// 0a1b2c3d.
const ASYNC_LAUNCH_SENTINEL: &str = "Async agent launched";

/// Where a subagent's returned message was resolved FROM (§3) — surfaced so a consumer
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
pub fn journal_result(subagent: &Subagent) -> Option<String> {
    subagent.workflow_id.as_ref()?;
    let wf_dir = subagent.path.parent()?;
    let journal = wf_dir.join("journal.jsonl");
    let bytes = std::fs::read(&journal).ok()?;
    for line in bytes.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        let is_result = v.get("type").and_then(serde_json::Value::as_str) == Some("result");
        let same_agent =
            v.get("agentId").and_then(serde_json::Value::as_str) == Some(&subagent.agent_id);
        if is_result && same_agent {
            return match v.get("result") {
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                Some(other) => Some(other.to_string()),
                None => None,
            };
        }
    }
    None
}

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
    pub description: Option<String>,
    /// TRUE trigger time = the parent tool_use ts (§4); falls back to the child-head ts
    /// (`started_utc`) when the spawn index has no entry.
    pub trigger_utc: Option<String>,
    /// Child transcript HEAD ts (the lagging secondary "when").
    pub started_utc: Option<String>,
    pub completed_utc: Option<String>,
    /// The subagent's returned message (§3), resolved 3-ways. `None` when unresolved.
    pub returned_message: Option<String>,
    pub returned_message_source: Option<ReturnedMsgSource>,
    pub status: SubagentStatus,
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
fn resolve_returned_message(
    subagent: &Subagent,
    index: &ParentSpawnIndex,
) -> (Option<String>, Option<ReturnedMsgSource>) {
    // Workflow agents always resolve through the journal (their parent tool_result is the
    // Workflow-tool launch echo, not the per-agent message).
    if subagent.kind == SubagentKind::Workflow {
        if let Some(msg) = journal_result(subagent) {
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
fn child_tail_text(path: &Path) -> Option<String> {
    let mut found: Option<String> = None;
    let _ = tail_records(path, |rec| {
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
    let index = index_parent_spawns(session_jsonl)?;
    subs.iter()
        .map(|s| node_for(s, &index, with_files))
        .collect()
}

/// Build one [`SubagentNode`] from a discovered [`Subagent`] + the session spawn index.
fn node_for(
    subagent: &Subagent,
    index: &ParentSpawnIndex,
    with_files: bool,
) -> Result<SubagentNode> {
    let lc = lifecycle(subagent)?;
    let spawn = subagent
        .spawn_tool_use_id
        .as_deref()
        .and_then(|id| index.spawn(id));
    // True trigger time = parent tool_use ts; fall back to the child-head ts.
    let trigger_utc = spawn
        .and_then(|s| s.trigger_utc.clone())
        .or_else(|| lc.started_utc.clone());
    // Description: prefer the built-in meta's, fall back to the spawn input's.
    let description = lc
        .description
        .clone()
        .or_else(|| spawn.and_then(|s| s.description.clone()));
    // agentType: prefer the meta's, fall back to the spawn's `subagent_type` (richer than
    // the bare `workflow-subagent` for an unlabeled meta).
    let agent_type = lc
        .agent_type
        .clone()
        .or_else(|| spawn.and_then(|s| s.subagent_type.clone()));
    let spawn_tool = spawn.and_then(|s| s.name.clone());
    let (returned_message, returned_message_source) = resolve_returned_message(subagent, index);
    let files_changed = if with_files {
        node_files_changed(&subagent.path)?
    } else {
        Vec::new()
    };
    Ok(SubagentNode {
        agent_id: subagent.agent_id.clone(),
        kind: subagent.kind,
        parent_session_id: subagent.parent_session_id.clone(),
        parent_agent_id: None,
        spawn_tool_use_id: subagent.spawn_tool_use_id.clone(),
        spawn_tool,
        workflow_id: subagent.workflow_id.clone(),
        agent_type,
        description,
        trigger_utc,
        started_utc: lc.started_utc.clone(),
        completed_utc: lc.completed_utc.clone(),
        returned_message,
        returned_message_source,
        status: lc.status,
        files_changed,
        depth: 0,
        children: Vec::new(),
        skipped_lines: lc.skipped_lines,
    })
}

/// Extract a subagent's files-changed by running the SAME structured + Bash mutation
/// extractors `files` uses, over the node's own transcript (§query-5). Returns
/// `(path, op_label, is_create)` per mutation, de-duplicated to one row per (path, op).
fn node_files_changed(path: &Path) -> Result<Vec<(String, String, bool)>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A scratch projects-root mimicking the real on-disk layout, removed on drop.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            // A process-wide atomic sequence guarantees a unique root per instance even
            // when two `Fixture::new()` calls on parallel test threads land on the same
            // PID + nanosecond — otherwise their `Drop` `remove_dir_all` could wipe a
            // sibling test's tree mid-run (a ~8% flake on the default parallel runner).
            static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let n = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root =
                std::env::temp_dir().join(format!("csift-sub-{}-{n}-{seq}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            Fixture { root }
        }

        fn write(&self, rel: &str, contents: &str) -> PathBuf {
            let p = self.root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            let mut f = std::fs::File::create(&p).unwrap();
            f.write_all(contents.as_bytes()).unwrap();
            p
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    const SESS: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

    /// Build a session jsonl + its sidecar with one built-in, one workflow agent,
    /// and a workflow journal (which must be ignored as a transcript).
    ///
    /// The PARENT transcript carries the spawn linkage the topology joins on: an `Agent`
    /// tool_use (`toolu_x`, == the built-in meta's `toolUseId`) at 04:59:58 whose paired
    /// tool_result is the SYNC returned message, plus a `Workflow` tool_use (`toolu_w`).
    /// A top-level `workflows/wf_abc.json` manifest is also written (NOT under
    /// `subagents/workflows/`) so the WorkflowRun reader has a manifest to find.
    fn layout(fx: &Fixture) -> PathBuf {
        let enc = "-Users-testuser-Projects-foo";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            concat!(
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
                // Agent tool_use that spawned aaa111 — its ts is the TRUE trigger time
                // (~2s BEFORE the child-head ts of 05:00:00).
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T04:59:58.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_x\",\"name\":\"Agent\",\"input\":{\"description\":\"run it\",\"subagent_type\":\"oh-my-claudecode:executor\"}}]}}\n",
                // The SYNC tool_result carrying aaa111's returned message.
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_x\",\"content\":\"SYNC RETURN: the built-in answer\"}]}}\n",
                // Workflow tool_use that launched wf_abc.
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:59:55.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_w\",\"name\":\"Workflow\",\"input\":{\"description\":\"the wf\"}}]}}\n"
            ),
        );

        // (A) built-in agent transcript + meta.
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aaa111.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"aaa111\",\"sessionId\":\"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"do the thing\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:03:20.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"done\"}]}}\n"
            ),
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aaa111.meta.json"),
            "{\"agentType\":\"oh-my-claudecode:executor\",\"description\":\"run it\",\"toolUseId\":\"toolu_x\"}",
        );

        // (B) workflow agent transcript + meta + (C) journal with a result event.
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_abc/agent-bbb222.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"bbb222\",\"timestamp\":\"2026-06-07T06:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"wf task\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T06:01:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_abc/agent-bbb222.meta.json"),
            "{\"agentType\":\"workflow-subagent\"}",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_abc/journal.jsonl"),
            concat!(
                "{\"type\":\"started\",\"agentId\":\"bbb222\",\"key\":\"v2:abc\"}\n",
                "{\"type\":\"result\",\"agentId\":\"bbb222\",\"key\":\"v2:abc\",\"result\":\"WF RETURN: workflow journal payload\"}\n"
            ),
        );

        // Top-level workflow RUN manifest (NOT under subagents/) — the WorkflowRun source.
        fx.write(
            &format!("{enc}/{SESS}/workflows/wf_abc.json"),
            "{\"runId\":\"wf_abc\",\"taskId\":\"t9\",\"workflowName\":\"demo-wf\",\"status\":\"completed\",\"agentCount\":1,\"durationMs\":62000,\"totalTokens\":12345,\"totalToolCalls\":7,\"defaultModel\":\"claude-opus-4-8[1m]\",\"startTime\":\"2026-06-07T05:59:55.000Z\"}",
        );

        session
    }

    #[test]
    fn discovers_both_kinds_excludes_journal() {
        let fx = Fixture::new();
        let session = layout(&fx);
        let subs = discover_subagents(&session).unwrap();
        // Exactly two transcripts; the journal must NOT appear.
        assert_eq!(subs.len(), 2, "got: {subs:?}");
        let builtin = subs
            .iter()
            .find(|s| s.kind == SubagentKind::BuiltinTask)
            .expect("a builtin");
        // The id is the bare hex (record/journal `agentId`), NOT the `agent-` stem.
        assert_eq!(builtin.agent_id, "aaa111");
        let wf = subs
            .iter()
            .find(|s| s.kind == SubagentKind::Workflow)
            .expect("a workflow");
        assert_eq!(wf.agent_id, "bbb222");
        assert_eq!(wf.workflow_id.as_deref(), Some("wf_abc"));
        // None of the discovered paths is a journal.
        assert!(subs.iter().all(|s| !s.path.ends_with("journal.jsonl")));
    }

    #[test]
    fn transcript_files_helper_excludes_journal_and_meta() {
        let fx = Fixture::new();
        let session = layout(&fx);
        let files = subagent_transcript_files(&session).unwrap();
        assert_eq!(files.len(), 2);
        for f in &files {
            let name = f.file_name().unwrap().to_str().unwrap();
            assert!(name.starts_with("agent-"), "unexpected file: {name}");
            assert!(name.ends_with(".jsonl"));
            assert_ne!(name, "journal.jsonl");
        }
    }

    #[test]
    fn lifecycle_builtin_from_transcript_terminal_message() {
        let fx = Fixture::new();
        let session = layout(&fx);
        let subs = discover_subagents(&session).unwrap();
        let builtin = subs
            .iter()
            .find(|s| s.kind == SubagentKind::BuiltinTask)
            .unwrap();
        let lc = lifecycle(builtin).unwrap();
        assert_eq!(lc.started_utc.as_deref(), Some("2026-06-07T05:00:00.000Z"));
        assert_eq!(
            lc.completed_utc.as_deref(),
            Some("2026-06-07T05:03:20.000Z")
        );
        // Terminal assistant text ⇒ completed (no journal for a built-in).
        assert_eq!(lc.status, SubagentStatus::Completed);
        assert_eq!(lc.agent_type.as_deref(), Some("oh-my-claudecode:executor"));
        assert_eq!(lc.description.as_deref(), Some("run it"));
    }

    #[test]
    fn lifecycle_workflow_completion_from_journal_result() {
        let fx = Fixture::new();
        let session = layout(&fx);
        let subs = discover_subagents(&session).unwrap();
        let wf = subs
            .iter()
            .find(|s| s.kind == SubagentKind::Workflow)
            .unwrap();
        let lc = lifecycle(wf).unwrap();
        // The workflow transcript ends on a tool_use (no terminal text), but the
        // journal carries a `result` event ⇒ completed.
        assert_eq!(lc.status, SubagentStatus::Completed);
        assert_eq!(lc.agent_type.as_deref(), Some("workflow-subagent"));
        assert_eq!(lc.started_utc.as_deref(), Some("2026-06-07T06:00:00.000Z"));
        assert_eq!(
            lc.completed_utc.as_deref(),
            Some("2026-06-07T06:01:00.000Z")
        );
    }

    #[test]
    fn running_when_no_completion_signal() {
        let fx = Fixture::new();
        let enc = "-Users-testuser-Projects-bar";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        // A subagent whose transcript ends mid-tool with NO journal result.
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-ccc333.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"ccc333\",\"timestamp\":\"2026-06-07T07:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"start\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T07:00:10.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
        let subs = discover_subagents(&session).unwrap();
        let lc = lifecycle(&subs[0]).unwrap();
        assert_eq!(lc.status, SubagentStatus::Running);
    }

    #[test]
    fn no_sidecar_is_empty_not_error() {
        let fx = Fixture::new();
        let session = fx.write(
            &format!("-Users-x/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        assert!(discover_subagents(&session).unwrap().is_empty());
        assert!(subagent_transcript_files(&session).unwrap().is_empty());
    }

    #[test]
    fn duration_label_formats() {
        assert_eq!(
            duration_label(
                Some("2026-06-07T05:00:00.000Z"),
                Some("2026-06-07T05:03:20.000Z")
            )
            .as_deref(),
            Some("3m20s")
        );
        assert_eq!(
            duration_label(Some("2026-06-07T05:00:00Z"), Some("2026-06-07T07:05:00Z")).as_deref(),
            Some("2h05m")
        );
        assert_eq!(
            duration_label(Some("2026-06-07T05:00:00Z"), Some("2026-06-07T05:00:03Z")).as_deref(),
            Some("3s")
        );
        assert!(duration_label(None, Some("2026-06-07T05:00:00Z")).is_none());
    }

    #[test]
    fn kind_and_status_labels() {
        assert_eq!(SubagentKind::BuiltinTask.label(), "builtin-task");
        assert_eq!(SubagentKind::Workflow.label(), "workflow");
        assert_eq!(SubagentStatus::Completed.label(), "completed");
        assert_eq!(SubagentStatus::Running.label(), "running");
        assert_eq!(SubagentStatus::Unknown.label(), "unknown");
    }

    // ── Branch-completeness ──

    #[test]
    fn sidecar_dir_none_for_pathological_paths() {
        // A path with no file stem (root) → the `file_stem()?` None arm.
        assert!(sidecar_dir_for_session(Path::new("/")).is_none());
        // A bare relative filename has no parent dir component that is a real dir, and
        // the sidecar dir won't exist → None.
        assert!(sidecar_dir_for_session(Path::new("nonexistent.jsonl")).is_none());
    }

    #[test]
    fn discover_empty_when_sidecar_exists_but_no_subagents_dir() {
        // The sidecar `<uuid>/` dir exists but has NO `subagents/` child → empty, not
        // an error (the `!subagents_dir.is_dir()` early return).
        let fx = Fixture::new();
        let enc = "-Users-x";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        // Create the sidecar dir (named after the uuid) but with an unrelated child,
        // NOT a `subagents/` dir.
        fx.write(&format!("{enc}/{SESS}/other/file.txt"), "x");
        assert!(discover_subagents(&session).unwrap().is_empty());
    }

    #[test]
    fn discover_ignores_stray_file_under_workflows() {
        // A non-directory entry sitting directly under `subagents/workflows/` must be
        // skipped by `subdirs_in` (the `if is_dir` FALSE arm). Only the real wf_* dir
        // contributes a workflow agent.
        let fx = Fixture::new();
        let enc = "-Users-strayfile";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        // A real workflow dir with an agent.
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_real/agent-kkk111.jsonl"),
            "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"kkk111\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
        );
        // A STRAY FILE (not a dir) directly under workflows/ → must be ignored.
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/stray-not-a-dir.txt"),
            "ignore me",
        );
        let subs = discover_subagents(&session).unwrap();
        assert_eq!(
            subs.len(),
            1,
            "only the real wf agent; stray file ignored: {subs:?}"
        );
        assert_eq!(subs[0].kind, SubagentKind::Workflow);
        assert_eq!(subs[0].workflow_id.as_deref(), Some("wf_real"));
    }

    #[test]
    fn discover_handles_subagents_dir_without_workflows() {
        // `subagents/` exists with a built-in agent but NO `workflows/` subdir (the
        // `workflows_dir.is_dir()` false arm).
        let fx = Fixture::new();
        let enc = "-Users-y";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-ddd444.jsonl"),
            "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"ddd444\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
        );
        let subs = discover_subagents(&session).unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].kind, SubagentKind::BuiltinTask);
        assert!(subs[0].workflow_id.is_none());
    }

    #[test]
    fn read_meta_none_for_missing_unreadable_and_malformed() {
        // No meta path at all → all-None (the `let Some(p) else` arm).
        assert_eq!(read_meta(None), MetaFields::default());
        let fx = Fixture::new();
        // A path that does not exist → the `std::fs::read` Err arm.
        let missing = fx.root.join("does-not-exist.meta.json");
        assert_eq!(read_meta(Some(&missing)), MetaFields::default());
        // A file with invalid JSON → the `serde_json::from_slice` Err arm.
        let bad = fx.write("bad.meta.json", "{ not valid json");
        assert_eq!(read_meta(Some(&bad)), MetaFields::default());
        // Valid JSON carrying ONLY toolUseId → the topology join key is now CAPTURED
        // (previously dropped to (None,None)); agentType/description/name stay None.
        let only_id = fx.write("only-id.meta.json", "{\"toolUseId\":\"toolu_x\"}");
        assert_eq!(
            read_meta(Some(&only_id)),
            MetaFields {
                agent_type: None,
                description: None,
                tool_use_id: Some("toolu_x".to_string()),
                name: None,
            }
        );
    }

    #[test]
    fn read_meta_captures_all_builtin_fields() {
        // A real built-in meta.json carries agentType + description + toolUseId; csift
        // must capture all three (the toolUseId is the topology spawn-link, §1).
        let fx = Fixture::new();
        let full = fx.write(
            "full.meta.json",
            "{\"agentType\":\"oh-my-claudecode:executor\",\"description\":\"run it\",\"toolUseId\":\"toolu_01R7Zi2gHHGkaTvzuDMH7bK3\"}",
        );
        assert_eq!(
            read_meta(Some(&full)),
            MetaFields {
                agent_type: Some("oh-my-claudecode:executor".to_string()),
                description: Some("run it".to_string()),
                tool_use_id: Some("toolu_01R7Zi2gHHGkaTvzuDMH7bK3".to_string()),
                name: None,
            }
        );
    }

    #[test]
    fn make_subagent_threads_tool_use_id_onto_struct() {
        // The built-in subagent's `spawn_tool_use_id` must equal its meta's toolUseId
        // (the join key into the parent spawn index); a workflow agent (meta has only
        // agentType) has `None`.
        let fx = Fixture::new();
        let session = layout(&fx);
        let subs = discover_subagents(&session).unwrap();
        let builtin = subs
            .iter()
            .find(|s| s.kind == SubagentKind::BuiltinTask)
            .unwrap();
        assert_eq!(builtin.spawn_tool_use_id.as_deref(), Some("toolu_x"));
        let wf = subs
            .iter()
            .find(|s| s.kind == SubagentKind::Workflow)
            .unwrap();
        assert!(wf.spawn_tool_use_id.is_none());
    }

    #[test]
    fn meta_without_agent_type_yields_none_label() {
        // A workflow agent whose meta.json lacks agentType → agent_type None on the
        // lifecycle, exercising read_meta's both-keys-absent path end to end.
        let fx = Fixture::new();
        let enc = "-Users-z";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-eee555.jsonl"),
            "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"eee555\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
        );
        // meta.json present but empty object.
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-eee555.meta.json"),
            "{}",
        );
        let subs = discover_subagents(&session).unwrap();
        let lc = lifecycle(&subs[0]).unwrap();
        assert!(lc.agent_type.is_none());
        assert!(lc.description.is_none());
    }

    #[test]
    fn journal_completion_false_for_builtin_no_workflow() {
        // A built-in subagent has no workflow_id → journal_reports_completion is false
        // via the `let Some(wf_id) else` arm (reached through lifecycle: a built-in
        // ending on a tool_use with no terminal text → Running, proving no journal).
        let fx = Fixture::new();
        let enc = "-Users-nojournal";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-fff666.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"fff666\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:00:10.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
        let subs = discover_subagents(&session).unwrap();
        let lc = lifecycle(&subs[0]).unwrap();
        assert_eq!(lc.status, SubagentStatus::Running);
    }

    #[test]
    fn journal_completion_handles_missing_malformed_and_nonmatching() {
        // A workflow subagent whose journal exists but carries ONLY a `started` event
        // (no `result`) AND a malformed line AND a result for a DIFFERENT agent →
        // completion not reported from the journal; the transcript ends on a tool_use
        // (no terminal text) → Running. Exercises the journal scan's continue arms
        // (blank line, malformed line, non-result type, wrong agentId) + the
        // fall-off-the-end `false`.
        let fx = Fixture::new();
        let enc = "-Users-wfjournal";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_z/agent-ggg777.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"ggg777\",\"timestamp\":\"2026-06-07T06:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T06:00:30.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_z/journal.jsonl"),
            concat!(
                "\n",                                                   // blank line → continue
                "{ this is not valid json }\n",                         // malformed → continue
                "{\"type\":\"started\",\"agentId\":\"ggg777\"}\n",      // non-result → no match
                "{\"type\":\"result\",\"agentId\":\"someone-else\"}\n" // result, wrong agent → no match
            ),
        );
        let subs = discover_subagents(&session).unwrap();
        let lc = lifecycle(&subs[0]).unwrap();
        assert_eq!(
            lc.status,
            SubagentStatus::Running,
            "no matching result → not completed"
        );
    }

    #[test]
    fn journal_completion_false_when_journal_absent() {
        // A workflow subagent with NO journal.jsonl at all → the `std::fs::read` Err
        // arm. The transcript here ends with terminal assistant text, so the AGENT is
        // still Completed (via transcript), but the journal path itself returns false.
        let fx = Fixture::new();
        let enc = "-Users-wfnojournal";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_n/agent-hhh888.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"hhh888\",\"timestamp\":\"2026-06-07T06:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T06:00:30.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"done cleanly\"}]}}\n"
            ),
        );
        // intentionally NO journal.jsonl
        let subs = discover_subagents(&session).unwrap();
        let lc = lifecycle(&subs[0]).unwrap();
        // Completed via the transcript terminal message, not the (absent) journal.
        assert_eq!(lc.status, SubagentStatus::Completed);
    }

    #[test]
    fn lifecycle_unknown_when_no_timestamps() {
        // A transcript whose records carry NO timestamp at all → started_utc stays
        // None, no terminal text, no journal → status Unknown (resolve_status's else
        // arm). The head scan also exercises the `if let Some(ts)` false arm (record
        // with no timestamp) before falling off the end.
        let fx = Fixture::new();
        let enc = "-Users-nots";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-iii999.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"iii999\",\"message\":{\"role\":\"user\",\"content\":\"no ts\"}}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
        let subs = discover_subagents(&session).unwrap();
        let lc = lifecycle(&subs[0]).unwrap();
        assert!(lc.started_utc.is_none());
        assert_eq!(lc.status, SubagentStatus::Unknown);
    }

    #[test]
    fn lifecycle_tail_terminal_flag_short_circuits_on_later_record() {
        // To reach the `!terminal_agent_msg` FALSE arm of the tail guard, the NEWEST
        // record must set terminal_agent_msg=true but leave completed_utc still None
        // (no timestamp) so the scan CONTINUES; the next (older) record is then
        // evaluated with terminal already true → `!terminal_agent_msg && …` short-
        // circuits on its false left operand.
        let fx = Fixture::new();
        let enc = "-Users-shortcirc";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-jjj000.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"jjj000\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"start\"}}\n",
                // older assistant WITH a timestamp (provides completed_utc later in the scan)
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:00:10.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"older text\"}]}}\n",
                // NEWEST assistant text but NO timestamp → terminal=true, completed_utc stays None → scan continues
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"newest no-ts text\"}]}}\n"
            ),
        );
        let subs = discover_subagents(&session).unwrap();
        let lc = lifecycle(&subs[0]).unwrap();
        assert_eq!(lc.status, SubagentStatus::Completed);
        // completed_utc comes from the older timestamped record (newest had none).
        assert_eq!(
            lc.completed_utc.as_deref(),
            Some("2026-06-07T05:00:10.000Z")
        );
    }

    #[test]
    fn resolve_status_all_arms() {
        // Direct unit coverage of every resolve_status branch.
        assert_eq!(
            resolve_status(true, true, false, true),
            SubagentStatus::Completed
        ); // journal
        assert_eq!(
            resolve_status(true, false, true, true),
            SubagentStatus::Completed
        ); // terminal msg
        assert_eq!(
            resolve_status(true, false, false, true),
            SubagentStatus::Running
        ); // saw + start
        assert_eq!(
            resolve_status(false, false, false, false),
            SubagentStatus::Unknown
        ); // nothing
        assert_eq!(
            resolve_status(true, false, false, false),
            SubagentStatus::Unknown
        ); // saw but no start
    }

    #[test]
    fn duration_label_none_for_unparseable_bounds() {
        // Both bounds present but one is unparseable → None (the `.parse().ok()?` arm).
        assert!(duration_label(Some("garbage"), Some("2026-06-07T05:00:00Z")).is_none());
        assert!(duration_label(Some("2026-06-07T05:00:00Z"), Some("not-a-time")).is_none());
        // Completion before start → clamped to 0 ("0s"), never negative.
        assert_eq!(
            duration_label(Some("2026-06-07T05:00:10Z"), Some("2026-06-07T05:00:00Z")).as_deref(),
            Some("0s")
        );
    }

    #[test]
    fn defensive_recursion_catches_a_hypothetical_nested_sub_sub_agent() {
        // The REAL layout is flat (no sub-sub-agents exist on disk — verified across 2348
        // transcripts). This test fabricates the FUTURE nested layout the defensive walk
        // insures against: a child transcript under
        // `subagents/agent-<hex>/subagents/agent-<hex>.jsonl`. (A)/(B) alone would drop it;
        // the bounded recursive walk must discover it as a built-in subagent.
        let fx = Fixture::new();
        let session = layout(&fx);
        let enc = "-Users-testuser-Projects-foo";
        // A nested sub-sub-agent transcript two `subagents/` levels deep.
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aaa111/subagents/agent-ccc333.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"ccc333\",\"timestamp\":\"2026-06-07T07:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"nested task\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T07:01:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"nested done\"}]}}\n"
            ),
        );
        let subs = discover_subagents(&session).unwrap();
        // The two original transcripts PLUS the nested one — none dropped, none duplicated.
        assert_eq!(
            subs.len(),
            3,
            "nested sub-sub-agent must be discovered: {subs:?}"
        );
        let nested = subs
            .iter()
            .find(|s| s.agent_id == "ccc333")
            .expect("the nested sub-sub-agent");
        // Classified by path location: under a `subagents/` dir ⇒ BuiltinTask.
        assert_eq!(nested.kind, SubagentKind::BuiltinTask);
        assert!(nested.path.ends_with("agent-ccc333.jsonl"));
        // No path is double-counted (dedup-by-absolute-path holds).
        let mut paths: Vec<_> = subs.iter().map(|s| s.path.clone()).collect();
        paths.sort();
        let n = paths.len();
        paths.dedup();
        assert_eq!(paths.len(), n, "no duplicate paths");
    }

    #[test]
    fn defensive_recursion_does_not_change_the_flat_real_layout() {
        // The insurance must be a NO-OP on the real flat layout: exactly the (A)+(B) two,
        // no spurious extra rows from the recursive walk over the normal tree.
        let fx = Fixture::new();
        let session = layout(&fx);
        let subs = discover_subagents(&session).unwrap();
        assert_eq!(subs.len(), 2, "flat layout unchanged by the defensive walk");
    }

    // ───────────────────── TOPOLOGY (Part A) tests ─────────────────────

    #[test]
    fn index_parent_spawns_finds_agent_and_workflow_tool_uses() {
        let fx = Fixture::new();
        let session = layout(&fx);
        let idx = index_parent_spawns(&session).unwrap();
        // Both spawns indexed: the Agent (toolu_x) + the Workflow (toolu_w). Task is
        // matched defensively but absent in this fixture (so it stays unindexed).
        let agent = idx.spawn("toolu_x").expect("the Agent spawn");
        assert_eq!(agent.name.as_deref(), Some("Agent"));
        assert_eq!(
            agent.trigger_utc.as_deref(),
            Some("2026-06-07T04:59:58.000Z")
        );
        assert_eq!(
            agent.subagent_type.as_deref(),
            Some("oh-my-claudecode:executor")
        );
        // The paired SYNC tool_result is indexed as the returned-message source.
        assert_eq!(
            idx.tool_result_text("toolu_x"),
            Some("SYNC RETURN: the built-in answer")
        );
        let wf = idx.spawn("toolu_w").expect("the Workflow spawn");
        assert_eq!(wf.name.as_deref(), Some("Workflow"));
    }

    #[test]
    fn index_parent_spawns_empty_for_empty_parent_transcript() {
        // An EMPTY parent jsonl (mmap → Ok(None)) → an empty index (degrade, never error).
        // This is the real graceful path; a TRULY-missing file is a genuine I/O error
        // (and `build_topology` only indexes a session whose file exists).
        let fx = Fixture::new();
        let empty = fx.write("-Users-empty/empty-session.jsonl", "");
        let idx = index_parent_spawns(&empty).unwrap();
        assert!(idx.spawn("anything").is_none());
        assert!(idx.tool_result_text("anything").is_none());
    }

    #[test]
    fn journal_result_captures_the_payload_not_just_a_bool() {
        let fx = Fixture::new();
        let session = layout(&fx);
        let subs = discover_subagents(&session).unwrap();
        let wf = subs
            .iter()
            .find(|s| s.kind == SubagentKind::Workflow)
            .unwrap();
        assert_eq!(
            journal_result(wf).as_deref(),
            Some("WF RETURN: workflow journal payload")
        );
        // A built-in has no journal → None.
        let builtin = subs
            .iter()
            .find(|s| s.kind == SubagentKind::BuiltinTask)
            .unwrap();
        assert!(journal_result(builtin).is_none());
    }

    #[test]
    fn build_topology_links_trigger_time_and_sync_returned_message() {
        let fx = Fixture::new();
        let session = layout(&fx);
        let nodes = build_topology(&session, false).unwrap();
        assert_eq!(nodes.len(), 2);
        let builtin = nodes
            .iter()
            .find(|n| n.kind == SubagentKind::BuiltinTask)
            .unwrap();
        // TRUE trigger = the parent Agent tool_use ts (04:59:58), NOT the child-head ts
        // (05:00:00) — they DIVERGE, proving the trigger axis is real.
        assert_eq!(
            builtin.trigger_utc.as_deref(),
            Some("2026-06-07T04:59:58.000Z")
        );
        assert_eq!(
            builtin.started_utc.as_deref(),
            Some("2026-06-07T05:00:00.000Z")
        );
        assert_ne!(builtin.trigger_utc, builtin.started_utc);
        assert_eq!(builtin.spawn_tool.as_deref(), Some("Agent"));
        // SYNC built-in → the returned message is the parent tool_result text.
        assert_eq!(
            builtin.returned_message.as_deref(),
            Some("SYNC RETURN: the built-in answer")
        );
        assert_eq!(
            builtin.returned_message_source,
            Some(ReturnedMsgSource::SyncToolResult)
        );
    }

    #[test]
    fn build_topology_resolves_workflow_returned_message_from_journal() {
        let fx = Fixture::new();
        let session = layout(&fx);
        let nodes = build_topology(&session, false).unwrap();
        let wf = nodes
            .iter()
            .find(|n| n.kind == SubagentKind::Workflow)
            .unwrap();
        // Workflow → the journal `result` payload, NOT the parent Workflow-tool echo.
        assert_eq!(
            wf.returned_message.as_deref(),
            Some("WF RETURN: workflow journal payload")
        );
        assert_eq!(
            wf.returned_message_source,
            Some(ReturnedMsgSource::WorkflowJournal)
        );
    }

    #[test]
    fn async_launch_falls_back_to_child_transcript_tail() {
        // A built-in whose parent tool_result is the `Async agent launched …` sentinel
        // must resolve its returned message from the CHILD transcript tail.
        let fx = Fixture::new();
        let enc = "-Users-async";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            concat!(
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T04:00:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_a\",\"name\":\"Agent\",\"input\":{\"run_in_background\":true}}]}}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_a\",\"content\":\"Async agent launched successfully.\\nagentId: zzz999\"}]}}\n"
            ),
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-zzz999.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"zzz999\",\"timestamp\":\"2026-06-07T04:00:05.000Z\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T04:05:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"ASYNC TAIL: the real async answer\"}]}}\n"
            ),
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-zzz999.meta.json"),
            "{\"agentType\":\"general-purpose\",\"toolUseId\":\"toolu_a\"}",
        );
        let nodes = build_topology(&session, false).unwrap();
        assert_eq!(nodes.len(), 1);
        let n = &nodes[0];
        assert_eq!(
            n.returned_message.as_deref(),
            Some("ASYNC TAIL: the real async answer")
        );
        assert_eq!(
            n.returned_message_source,
            Some(ReturnedMsgSource::AsyncChildTail)
        );
    }

    #[test]
    fn build_topology_with_files_attaches_node_files_changed() {
        // A built-in whose transcript edits a file → its files_changed lists that path.
        let fx = Fixture::new();
        let enc = "-Users-nodefiles";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T04:00:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_f\",\"name\":\"Agent\",\"input\":{}}]}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-fff111.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"fff111\",\"timestamp\":\"2026-06-07T04:00:05.000Z\",\"message\":{\"role\":\"user\",\"content\":\"edit\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T04:01:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"e1\",\"name\":\"Edit\",\"input\":{\"file_path\":\"/repo/src/lib.rs\"}}]}}\n"
            ),
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-fff111.meta.json"),
            "{\"agentType\":\"general-purpose\",\"toolUseId\":\"toolu_f\"}",
        );
        let nodes = build_topology(&session, true).unwrap();
        assert_eq!(nodes.len(), 1);
        let files = &nodes[0].files_changed;
        assert_eq!(files.len(), 1, "got: {files:?}");
        assert_eq!(files[0].0, "/repo/src/lib.rs");
        assert_eq!(files[0].1, "edit");
        // with_files=false leaves it empty (the cheap default).
        let lean = build_topology(&session, false).unwrap();
        assert!(lean[0].files_changed.is_empty());
    }

    #[test]
    fn discover_workflow_runs_reads_top_level_manifests() {
        let fx = Fixture::new();
        let session = layout(&fx);
        let runs = discover_workflow_runs(&session).unwrap();
        assert_eq!(runs.len(), 1);
        let r = &runs[0];
        assert_eq!(r.run_id, "wf_abc");
        assert_eq!(r.task_id.as_deref(), Some("t9"));
        assert_eq!(r.workflow_name.as_deref(), Some("demo-wf"));
        assert_eq!(r.status.as_deref(), Some("completed"));
        assert_eq!(r.agent_count, Some(1));
        assert_eq!(r.duration_ms, Some(62000));
        assert_eq!(r.total_tokens, Some(12345));
        assert_eq!(r.total_tool_calls, Some(7));
        assert_eq!(r.default_model.as_deref(), Some("claude-opus-4-8[1m]"));
        // The run_id matches the subagents/workflows/wf_abc/ dir → joins to its agent.
        let nodes = build_topology(&session, false).unwrap();
        assert!(nodes
            .iter()
            .any(|n| n.workflow_id.as_deref() == Some("wf_abc")));
    }

    #[test]
    fn discover_workflow_runs_empty_without_workflows_dir() {
        // A session whose sidecar has no top-level workflows/ dir → empty, not an error.
        let fx = Fixture::new();
        let enc = "-Users-nowf";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-q.jsonl"),
            "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"q\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
        );
        assert!(discover_workflow_runs(&session).unwrap().is_empty());
    }

    #[test]
    fn discover_workflow_runs_ignores_non_manifest_entries() {
        // The `workflows/scripts/` subdir + a non-wf_*.json file must be skipped.
        let fx = Fixture::new();
        let session = layout(&fx);
        let enc = "-Users-testuser-Projects-foo";
        fx.write(&format!("{enc}/{SESS}/workflows/scripts/x.js"), "noop");
        fx.write(&format!("{enc}/{SESS}/workflows/not-a-manifest.txt"), "x");
        // Still exactly the one real wf_abc.json manifest.
        let runs = discover_workflow_runs(&session).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run_id, "wf_abc");
    }

    #[test]
    fn bare_agent_id_strips_prefix_only_when_present() {
        // The one rule, shared by recover/session/files: a subagent stem loses `agent-`;
        // a top-level uuid (no prefix) is unchanged.
        assert_eq!(bare_agent_id("agent-aaa111"), "aaa111");
        assert_eq!(
            bare_agent_id("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"),
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
        );
    }
}
