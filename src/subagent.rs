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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::model::{tool_result_content_text, Block, Record};
use crate::parse::{head_records, scan_lines_bytes, tail_records};
use crate::parse::{mmap_bytes, parse_candidates_parallel, parse_line};

/// Strip the on-disk `agent-` filename prefix to the bare-hex canonical agent id (the
/// value the transcript record's `agentId` field AND the workflow journal carry). The
/// single source of truth for this rule — used by `make_subagent` and by the
/// `recover` / `session` / `files` subcommands so a subagent row's printed `session_id`
/// is the SAME bare hex `agents` prints, hence joinable across surfaces.
#[must_use]
pub fn bare_agent_id(stem: &str) -> &str {
    stem.strip_prefix("agent-").unwrap_or(stem)
}

/// The CANONICAL session id for a transcript file: its jsonl basename, with a
/// subagent's `agent-` filename prefix stripped to the bare-hex id ([`bare_agent_id`]).
///
/// This is the SINGLE derivation used by every per-file `session_id` emission
/// (`list` / `search` / `files` / `recover` / `turns`) so the SAME subagent transcript
/// always reports the SAME id, whichever subcommand prints it — id-form unification.
/// A top-level session uuid has no `agent-` prefix and passes through unchanged. An
/// empty / stem-less path yields an empty string (never panics).
#[must_use]
pub fn session_id_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(bare_agent_id)
        .map(str::to_string)
        .unwrap_or_default()
}

/// The re-feedable PARENT session uuid for a transcript path, or `None` when the path is a
/// top-level `<uuid>.jsonl` (which IS its own session). A subagent transcript lives at
/// `…/<PARENT-UUID>/subagents/[workflows/wf_*/]agent-<hex>.jsonl`, so the parent uuid is the
/// directory component immediately BEFORE the `subagents` segment. This is what makes a
/// search/files subagent match re-feedable: its bare-hex `session_id` is NOT a re-feedable
/// `@<uuid>` target, but the `parent_session_id` this returns is (`csift verbatim @<parent>` works).
#[must_use]
pub fn parent_session_id_from_path(path: &Path) -> Option<String> {
    let mut prev: Option<&str> = None;
    for comp in path.components() {
        let c = comp.as_os_str().to_str()?;
        if c == "subagents" {
            // The component just before `subagents` is the parent-session dir name.
            return prev.map(str::to_string);
        }
        prev = Some(c);
    }
    None
}

/// True when `path` is a SUBAGENT transcript (lives under a `subagents/` segment) rather
/// than a top-level `<uuid>.jsonl` session file.
#[must_use]
pub fn is_subagent_path(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str().to_str() == Some("subagents"))
}

/// Subagent kind, keyed off the on-disk path location (authoritative; see module
/// docs — `agentType` is descriptive only, not the discriminator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentKind {
    /// `subagents/agent-<hex>.jsonl` — a built-in Task/Agent-tool subagent.
    BuiltinTask,
    /// `subagents/workflows/wf_<id>/agent-<hex>.jsonl` — a workflow / OMC agent.
    Workflow,
    /// A "teammate" (`taskKind:"in_process_teammate"`) — Claude Code's persistent, directly
    /// addressable team-member agent. It lands at the built-in on-disk LOCATION
    /// (`subagents/agent-<id>.jsonl`), so location alone can't tell it apart; the discriminator
    /// is the meta.json `taskKind`. Its canonical id embeds the teammate name
    /// (`aVSRepro-68a2a1661c9390c1`, see [`crate::path::is_subagent_id`]), its meta carries
    /// `teamName`/`color`/`model` and NO `toolUseId`, and it is spawned by an `Agent` tool_use
    /// joined by NAME (not tool_use id) — see [`ParentSpawnIndex::spawn_id_for_name`].
    Teammate,
}

impl SubagentKind {
    /// Stable lowercase label used in CLI output + JSON (matches the `--kind` enum).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            SubagentKind::BuiltinTask => "builtin-task",
            SubagentKind::Workflow => "workflow",
            SubagentKind::Teammate => "teammate",
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
    /// The parent `Task`/`Agent` tool_use `id` that SPAWNED this subagent, read from the
    /// built-in `meta.json` `toolUseId` (on disk for every built-in subagent; `None` for
    /// workflow agents, whose meta carries only `agentType`). This is the join key into
    /// the parent transcript's spawn index ([`ParentSpawnIndex`]) — it recovers the true
    /// trigger time + the returned message.
    pub spawn_tool_use_id: Option<String>,
    /// The agent's `name` from meta.json (the `Agent` tool's `name` param, e.g. a teammate
    /// handle like `VSRepro` or an OMC lane name like `LaneDONE`). `None` when absent. For a
    /// teammate this is the NAME-join key into the spawning `Agent` tool_use (whose meta carries
    /// no `toolUseId`), recovering the spawn linkage the flat layout otherwise drops.
    pub name: Option<String>,
    /// The `teamName` from meta.json — present only for a teammate (`taskKind:in_process_teammate`);
    /// `None` for built-in/workflow agents. Identifies the team a teammate belongs to.
    pub team_name: Option<String>,
    /// `agentType` from meta.json (descriptive sub-label, e.g. `Explore`,
    /// `oh-my-claudecode:executor`, `workflow-subagent`). Read ONCE here at discovery so
    /// [`lifecycle`] takes it from the `Subagent` instead of re-reading the meta (FIX3 — kills
    /// one redundant meta.json read + parse per subagent). Identical value either way.
    pub agent_type: Option<String>,
    /// `description` from a built-in / teammate meta.json (the Task tool's `description`). Read
    /// ONCE at discovery alongside `agent_type`; `None` for a workflow agent (its meta carries
    /// only `agentType`).
    pub description: Option<String>,
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
    /// The FROZEN-lane signal: `Some` when the newest meaningful record is an unreturned tool_use
    /// (the lane is blocked AT it, never "completed"). Drives the status override + the agents
    /// `pending_*` fields. `None` for a normally-running/completed lane.
    pub pending: Option<PendingToolUse>,
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

/// Why a lane is FROZEN at an unreturned tool_use (its newest meaningful record is an assistant
/// tool_use with no following tool_result — see [`SubagentLifecycle::pending`]). The escalation
/// itself never reaches jsonl (it lives only in CC process memory), so these three states share
/// one on-disk signature; only the danger heuristic can POSITIVELY distinguish the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingClassification {
    /// The pending tool_use is a Bash command CC's classifier would HOIST for human approval even
    /// under bypass-permissions (a dangerous `rm`/`rmdir`, see [`crate::bash_danger`]). Almost
    /// certainly waiting for a human to click "Yes", NOT dying. The high-value disambiguation.
    EscalationBlocked,
    /// A pending tool_use that is NOT a known-hoisted danger — a (possibly slow) tool still
    /// executing, OR an interrupted/wedged lane. jsonl alone CANNOT tell those apart (same
    /// signature); the caller should weigh elapsed-since-`pending_since_utc` for staleness.
    AwaitingExecution,
}

impl PendingClassification {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            PendingClassification::EscalationBlocked => "escalation-blocked",
            PendingClassification::AwaitingExecution => "awaiting-execution",
        }
    }
}

/// A FROZEN lane's unreturned tool_use (the raw facts; [`node_for`] adds the classification). A
/// lane is frozen when its newest meaningful record is an assistant tool_use that no later
/// tool_result resolves — it is the last record, so nothing followed it.
#[derive(Debug, Clone)]
pub struct PendingToolUse {
    pub tool_use_id: String,
    pub tool_name: String,
    /// The Bash `input.command`, when `tool_name == "Bash"` (the danger-heuristic input).
    pub command: Option<String>,
    /// The pending tool_use record's timestamp — when the lane froze.
    pub since_utc: Option<String>,
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
    // Read the companion `agent-<hex>.meta.json` ONCE here (its located path is used only for this
    // read — `lifecycle` now takes agent_type/description off the struct, so the path isn't stored).
    let meta_path = path.with_extension("meta.json");
    let meta_path = meta_path.is_file().then_some(meta_path);
    let meta = read_meta(meta_path.as_deref());
    // A built-in-LOCATION agent whose meta declares `taskKind:"in_process_teammate"` is a
    // teammate, not a plain Task subagent — the only way to tell them apart (both sit at
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
    }
}

/// The fields csift reads from a subagent's `meta.json`. A built-in meta carries
/// `{agentType, description, toolUseId}` (+ often `name`); a workflow agent meta carries only
/// `{agentType}`; a TEAMMATE meta carries `{agentType, description, name, taskKind, teamName,
/// color, model, …}` and NO `toolUseId`. All are optional — a malformed / missing / key-absent
/// meta yields all-`None` (never an error; the lifecycle still resolves from the transcript).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetaFields {
    pub agent_type: Option<String>,
    pub description: Option<String>,
    /// The spawning parent `Task`/`Agent` tool_use id (built-in only; the topology join
    /// key). Captured here so the previously-dropped `toolUseId` reaches the topology.
    pub tool_use_id: Option<String>,
    pub name: Option<String>,
    /// `taskKind` — `"in_process_teammate"` marks a teammate (the only way to distinguish it
    /// from a built-in Task subagent, since both share the on-disk location). `None`/other for
    /// a plain built-in or workflow agent.
    pub task_kind: Option<String>,
    /// `teamName` — the team a teammate belongs to (teammate metas only).
    pub team_name: Option<String>,
}

/// Read `{agentType, description, toolUseId, name, taskKind, teamName}` from a subagent's
/// `meta.json`, if readable. Returns [`MetaFields::default`] (all `None`) for a missing path,
/// unreadable file, malformed JSON, or any key absent.
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
        task_kind: str_field("taskKind"),
        team_name: str_field("teamName"),
    }
}

/// True iff the workflow journal alongside a workflow subagent carries a `result`
/// event for `agent_id` (the completion signal, §C). For a built-in subagent (no
/// journal) this is always `false` — completion is inferred from the transcript.
fn journal_reports_completion(subagent: &Subagent, journals: &JournalCache) -> bool {
    journals
        .events_for(subagent)
        .is_some_and(|data| data.results.contains_key(&subagent.agent_id))
}

/// Per-topology-build cache of every distinct `wf_<id>/journal.jsonl`, read + parsed
/// ONCE and shared across the whole node/lifecycle fan-out. Without it each of a
/// workflow run's N agents re-read and re-parsed the SAME journal (an O(N × journal)
/// blowup — a 104-agent run re-parsed its 236 KB journal 104 times, and a 3.5k-agent
/// session re-parsed ~600 MB of journal JSON in aggregate). The cached view is exactly
/// what the two former per-agent scans extracted — first `result` event per agentId —
/// so behaviour is byte-identical, only WHEN the journal is read changes.
#[derive(Debug, Default)]
pub struct JournalCache {
    /// journal path → its parsed per-agent result events. An unreadable/absent journal
    /// has no entry (the same "no journal ⇒ no completion signal" the direct reads had).
    by_path: HashMap<PathBuf, JournalData>,
}

/// The per-agent `result`-event facts one journal carries.
#[derive(Debug, Default)]
struct JournalData {
    /// agentId → the FIRST `result` event's payload for that agent: `Some(text)` when the
    /// event carries a `result` field (string kept as-is, non-string JSON-rendered so it
    /// is never lost), `None` when it does not (a completion signal with no payload).
    /// Key PRESENCE == "the journal reports this agent completed".
    results: HashMap<String, Option<String>>,
}

impl JournalCache {
    /// Read + parse each DISTINCT journal among these subagents once. Malformed journal
    /// lines are skipped exactly as the former per-agent scans skipped them.
    pub fn build(subs: &[Subagent]) -> Self {
        let mut by_path: HashMap<PathBuf, JournalData> = HashMap::new();
        for sub in subs {
            let Some(journal) = Self::journal_path(sub) else {
                continue;
            };
            if by_path.contains_key(&journal) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&journal) else {
                continue; // unreadable/absent → no entry (matches the old per-read failure arm)
            };
            let mut data = JournalData::default();
            for line in bytes.split(|&b| b == b'\n') {
                if line.is_empty() {
                    continue;
                }
                let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
                    continue;
                };
                if v.get("type").and_then(serde_json::Value::as_str) != Some("result") {
                    continue;
                }
                let Some(agent) = v.get("agentId").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                let payload = match v.get("result") {
                    Some(serde_json::Value::String(s)) => Some(s.clone()),
                    Some(other) => Some(other.to_string()),
                    None => None,
                };
                // FIRST event per agent wins — the former scans returned on first match.
                data.results.entry(agent.to_string()).or_insert(payload);
            }
            by_path.insert(journal, data);
        }
        Self { by_path }
    }

    /// The journal path a workflow subagent's events live in (`None` for a built-in /
    /// teammate — no `workflow_id` ⇒ no journal, the same guard the direct reads had).
    fn journal_path(subagent: &Subagent) -> Option<PathBuf> {
        subagent.workflow_id.as_ref()?;
        Some(subagent.path.parent()?.join("journal.jsonl"))
    }

    /// This subagent's parsed journal, when it has one that was readable.
    fn events_for(&self, subagent: &Subagent) -> Option<&JournalData> {
        self.by_path.get(&Self::journal_path(subagent)?)
    }
}

/// Compute the lifecycle of one subagent: read its transcript HEAD for the start
/// timestamp + TAIL for the completion timestamp & terminal-message signal, consult
/// the workflow journal for an explicit `result`, then resolve a status.
pub fn lifecycle(subagent: &Subagent, journals: &JournalCache) -> Result<SubagentLifecycle> {
    // agent_type + description were read from meta.json ONCE at discovery ([`make_subagent`])
    // and stored on the `Subagent` — no second meta read here (FIX3: kills one redundant
    // meta.json read + parse per subagent). The values are identical to `read_meta`'s.
    let agent_type = subagent.agent_type.clone();
    let description = subagent.description.clone();

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

    // TAIL: last record's timestamp == completion (best-effort), whether the transcript
    // terminates with a visible assistant message (a clean finish), AND whether the lane is
    // FROZEN at an unreturned tool_use. The frozen verdict comes from the NEWEST meaningful
    // record only (the first non-metadata record from EOF): if it is an assistant tool_use, no
    // tool_result followed it (it IS the last record) ⇒ the lane is blocked there, NOT done. The
    // terminal_agent_msg walk-back is UNCHANGED for every non-frozen lane.
    let mut completed_utc: Option<String> = None;
    let mut terminal_agent_msg = false;
    let mut saw_any = false;
    let mut newest_decided = false;
    let mut pending: Option<PendingToolUse> = None;
    let tail_skipped = tail_records(&subagent.path, |rec| {
        saw_any = true;
        if completed_utc.is_none() {
            if let Some(ts) = &rec.timestamp {
                completed_utc = Some(ts.clone());
            }
        }
        if !newest_decided {
            if let Some(tu) = newest_pending_tool_use(rec) {
                pending = Some(tu); // newest meaningful record is an unreturned tool_use → frozen
                newest_decided = true;
            } else if record_is_meaningful(rec) {
                newest_decided = true; // newest meaningful record is resolved/active → not frozen
            }
            // else: isMeta / system / metadata-only → keep looking for the newest meaningful one
        }
        // The newest assistant record carrying visible text == a clean end-of-turn.
        if !terminal_agent_msg && rec.agent_text().is_some() {
            terminal_agent_msg = true;
        }
        // Stop once we have the completion timestamp AND a terminal-message verdict;
        // if the very newest record has no text we still only need a couple of reads.
        completed_utc.is_none() || !terminal_agent_msg
    })?;

    let journal_done = journal_reports_completion(subagent, journals);
    // Clear the frozen signal when it would be meaningless: a journal-completed (workflow) agent is
    // trusted done regardless of a tail tool_use; and a transcript with NO timestamps has
    // undetermined timing (status Unknown), so we cannot claim "frozen" vs merely unreadable.
    if journal_done || started_utc.is_none() {
        pending = None;
    }
    // A genuinely frozen lane is NEVER "completed": override the terminal-text walk-back, which
    // would otherwise find an EARLIER end-of-turn (the assistant's text before the frozen
    // tool_use) and mis-report the stuck lane as done.
    let status = if pending.is_some() {
        SubagentStatus::Running
    } else {
        resolve_status(
            saw_any,
            journal_done,
            terminal_agent_msg,
            started_utc.is_some(),
        )
    };

    Ok(SubagentLifecycle {
        agent_type,
        description,
        started_utc,
        completed_utc,
        status,
        pending,
        skipped_lines: head_skipped + tail_skipped,
    })
}

/// The newest-meaningful-record frozen check: if `rec` is an assistant carrying ≥1 tool_use block,
/// return the pending tool_use (the DANGEROUS Bash one if present, else the first) — because it is
/// the last record, no tool_result resolved it. `None` for any non-(assistant-with-tool_use) record.
fn newest_pending_tool_use(rec: &Record) -> Option<PendingToolUse> {
    if !rec.is_type("assistant") {
        return None;
    }
    let blocks = rec.blocks()?;
    let mut chosen: Option<(String, String, Option<String>)> = None;
    for b in blocks {
        let Block::ToolUse {
            id: Some(id),
            name: Some(name),
            input,
        } = b
        else {
            continue;
        };
        let command = if name == "Bash" {
            input
                .as_ref()
                .and_then(|v| v.get("command"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        } else {
            None
        };
        // Prefer a tool_use CC would hoist (dangerous rm) so classification is escalation-blocked.
        if command
            .as_deref()
            .is_some_and(crate::bash_danger::is_dangerous_rm)
        {
            return Some(PendingToolUse {
                tool_use_id: id.clone(),
                tool_name: name.clone(),
                command,
                since_utc: rec.timestamp.clone(),
            });
        }
        if chosen.is_none() {
            chosen = Some((id.clone(), name.clone(), command));
        }
    }
    chosen.map(|(tool_use_id, tool_name, command)| PendingToolUse {
        tool_use_id,
        tool_name,
        command,
        since_utc: rec.timestamp.clone(),
    })
}

/// True for a record that resolves/advances the lane — a tool_result carrier, a clean assistant
/// end-of-turn text, or a genuine user message. (NOT an unreturned tool_use, NOT isMeta/system
/// metadata.) Used to find the newest MEANINGFUL record when deciding the frozen verdict.
fn record_is_meaningful(rec: &Record) -> bool {
    rec.agent_text().is_some()
        || rec.is_genuine_user()
        || rec
            .blocks()
            .is_some_and(|bs| bs.iter().any(|b| matches!(b, Block::ToolResult { .. })))
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
    /// `spawn tool_use_id → issuing agent` — `Some(agent_id)` when the spawn was recorded
    /// in a SUBAGENT's transcript (so the spawned child is a sub-subagent of that agent),
    /// `None` when issued by the main session itself. Populated only when the index is built
    /// GLOBALLY ([`build_global_spawn_index`]); a single-transcript build leaves it empty.
    issuer: std::collections::HashMap<String, Option<String>>,
    /// `spawn input.name → [(trigger_utc, tool_use_id)]` for every spawn tool_use that named
    /// its agent. The NAME-join fallback for a TEAMMATE, whose meta carries no `toolUseId` (so
    /// the usual id-join can't reach its spawning `Agent` tool_use). Keyed by the `Agent` tool's
    /// `name` param (== the teammate's meta `name`). A name may recur across a session, so the
    /// values are a list disambiguated by trigger time in [`Self::spawn_id_for_name`].
    by_name: std::collections::HashMap<String, Vec<(Option<String>, String)>>,
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

    /// The PARENT agent id for a spawn tool_use id: `Some(agent_id)` when that spawn was
    /// issued from a subagent transcript (⇒ the spawned child nests under that agent),
    /// `None` when main-issued or unknown. The agent→agent topology link.
    #[must_use]
    pub fn parent_agent_for(&self, spawn_tool_use_id: &str) -> Option<String> {
        self.issuer.get(spawn_tool_use_id).cloned().flatten()
    }

    /// The spawning tool_use id for a NAMED spawn (the teammate name-join, §FIX3). Among the
    /// spawns that share `name`, prefer the LATEST whose trigger ≤ `at_or_before` (the child's
    /// head ts — the spawn always precedes the child), so a recurring name binds to the right
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

    /// Fold `other` INTO `self` — used by [`build_global_spawn_index`] to merge the per-subagent
    /// LOCAL indexes built in parallel back into the global one. The unique-keyed maps
    /// (`spawns`/`tool_results`/`issuer`, all keyed by a globally-unique tool_use id) take
    /// `other`'s value on any collision — LATER-wins, matching the old serial accumulation where a
    /// later transcript's insert overwrote an earlier one. The `by_name` lists are APPENDED (self's
    /// entries first), so — since callers merge locals in the deterministic `subs` order — the final
    /// per-name order is byte-identical to the old serial scan (main, then each sub in order).
    fn merge(&mut self, other: ParentSpawnIndex) {
        self.spawns.extend(other.spawns);
        self.tool_results.extend(other.tool_results);
        self.issuer.extend(other.issuer);
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
/// transcript (issuer = that agent's id). This is what makes agent→agent nesting resolvable —
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
fn scan_spawns_into(jsonl: &Path, issuer: Option<&str>, idx: &mut ParentSpawnIndex) -> Result<()> {
    let Some(mmap) = mmap_bytes(jsonl)? else {
        return Ok(());
    };
    let bytes: &[u8] = &mmap;
    // Byte-prefilter + parallel parse (mirrors search's stage-1 / `files`' candidate gate): only a
    // line carrying a `tool_use` (a spawn) or a paired `tool_result` (the sync returned-message
    // source) can contribute to the index; every other line (thinking/text/genuine-user/summary/
    // system) is skipped BEFORE the full serde parse. This also within-file-parallelizes the big
    // 398 MB main scan. Malformed candidate lines are silently dropped (as the old scan did — the
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
fn spawn_line_candidate(line: &[u8]) -> bool {
    memchr::memmem::find(line, b"tool_use").is_some()
        || memchr::memmem::find(line, b"tool_result").is_some()
}

/// Fold one candidate record's spawn tool_uses + paired tool_results into `idx`. `issuer` tags
/// every spawn id with the agent that issued it (`None` = the main session). Split out of
/// [`scan_spawns_into`] so the (prefiltered, parallel) parse and this serial accumulation are
/// separate concerns; the body is identical to the old inline closure.
fn accumulate_spawns(rec: &Record, issuer: Option<&str>, idx: &mut ParentSpawnIndex) {
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
                // teammate — whose meta has no `toolUseId` — can name-join to its launch.
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
pub fn journal_result(subagent: &Subagent, journals: &JournalCache) -> Option<String> {
    journals
        .events_for(subagent)?
        .results
        .get(&subagent.agent_id)?
        .clone()
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
fn resolve_returned_message(
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
fn assign_depths(nodes: &mut [SubagentNode]) {
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
fn node_for(
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

    /// A session that spawned ONE teammate (`taskKind:in_process_teammate`) via an `Agent`
    /// tool_use carrying `input.name` (the name-join key) + the real `subagent_type`. The
    /// teammate meta deliberately overloads `agentType` with the handle (as CC does) and omits
    /// `toolUseId`, so only the NAME-join can recover its spawn linkage + real type.
    fn teammate_layout(fx: &Fixture) -> PathBuf {
        let enc = "-Users-testuser-Projects-foo";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            concat!(
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
                // The Agent tool_use that spawned the teammate: NO paired meta toolUseId on the
                // child, so the topology must join by input.name. Its ts is the TRUE trigger,
                // ~0.5s before the child head (05:00:00.500).
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_team\",\"name\":\"Agent\",\"input\":{\"description\":\"repro the bug\",\"subagent_type\":\"oh-my-claudecode:qa-tester\",\"name\":\"VSRepro\"}}]}}\n"
            ),
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aVSRepro-68a2a1661c9390c1.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"aVSRepro-68a2a1661c9390c1\",\"sessionId\":\"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d\",\"timestamp\":\"2026-06-07T05:00:00.500Z\",\"message\":{\"role\":\"user\",\"content\":\"<teammate-message teammate_id=\\\"team-lead\\\">repro it</teammate-message>\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:10:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"the matrix result\"}]}}\n"
            ),
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aVSRepro-68a2a1661c9390c1.meta.json"),
            "{\"agentType\":\"VSRepro\",\"description\":\"repro the bug\",\"name\":\"VSRepro\",\"taskKind\":\"in_process_teammate\",\"teamName\":\"session-25f56dee\",\"color\":\"purple\"}",
        );
        session
    }

    #[test]
    fn classifies_teammate_and_recovers_spawn_via_name_join() {
        let fx = Fixture::new();
        let session = teammate_layout(&fx);

        // Discovery + classification: the in_process_teammate taskKind upgrades the kind.
        let subs = discover_subagents(&session).unwrap();
        assert_eq!(subs.len(), 1, "got: {subs:?}");
        let s = &subs[0];
        assert_eq!(s.kind, SubagentKind::Teammate);
        assert_eq!(s.agent_id, "aVSRepro-68a2a1661c9390c1");
        assert_eq!(s.name.as_deref(), Some("VSRepro"));
        assert_eq!(s.team_name.as_deref(), Some("session-25f56dee"));
        // The teammate meta carries NO toolUseId — the id-join would find nothing.
        assert_eq!(s.spawn_tool_use_id, None);

        // The full node: the NAME-join recovers the spawn linkage the id-join can't.
        let nodes = build_topology(&session, false).unwrap();
        let n = nodes
            .iter()
            .find(|n| n.kind == SubagentKind::Teammate)
            .expect("the teammate node");
        // agent_type prefers the spawn's REAL subagent_type over the meta's overloaded handle.
        assert_eq!(n.agent_type.as_deref(), Some("oh-my-claudecode:qa-tester"));
        assert_eq!(n.spawn_tool.as_deref(), Some("Agent"));
        assert_eq!(n.spawn_tool_use_id.as_deref(), Some("toolu_team"));
        // trigger = the Agent tool_use ts (the TRUE spawn instant), earlier than the child head.
        assert_eq!(n.trigger_utc.as_deref(), Some("2026-06-07T05:00:00.000Z"));
        assert_eq!(n.started_utc.as_deref(), Some("2026-06-07T05:00:00.500Z"));
        assert_eq!(n.name.as_deref(), Some("VSRepro"));
        assert_eq!(n.team_name.as_deref(), Some("session-25f56dee"));
        // The returned message still resolves (child tail), unaffected by the name-join.
        assert_eq!(n.returned_message.as_deref(), Some("the matrix result"));
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
        let lc = lifecycle(builtin, &JournalCache::build(std::slice::from_ref(builtin))).unwrap();
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
        let lc = lifecycle(wf, &JournalCache::build(std::slice::from_ref(wf))).unwrap();
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
    fn journal_cache_first_result_event_wins_and_renders_nonstring() {
        let fx = Fixture::new();
        let enc = "-Users-testuser-Projects-jc";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        let wf_dir = format!("{enc}/{SESS}/subagents/workflows/wf_jc1");
        for agent in ["aaa111", "bbb222"] {
            fx.write(
                &format!("{wf_dir}/agent-{agent}.jsonl"),
                &format!("{{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"{agent}\",\"timestamp\":\"2026-06-07T07:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"s\"}}}}\n"),
            );
        }
        fx.write(
            &format!("{wf_dir}/journal.jsonl"),
            concat!(
                // FIRST result event for aaa111 carries NO payload — and first wins
                // (the former per-agent scan returned on its first match), so the
                // later "late" payload must never surface.
                "{\"type\":\"result\",\"agentId\":\"aaa111\"}\n",
                "{\"type\":\"result\",\"agentId\":\"aaa111\",\"result\":\"late\"}\n",
                "not json - skipped exactly as the direct scans skipped it\n",
                // A non-string payload is JSON-rendered so it is never lost.
                "{\"type\":\"result\",\"agentId\":\"bbb222\",\"result\":{\"k\":1}}\n",
            ),
        );
        let subs = discover_subagents(&session).unwrap();
        let cache = JournalCache::build(&subs);
        let a = subs.iter().find(|s| s.agent_id == "aaa111").unwrap();
        let b = subs.iter().find(|s| s.agent_id == "bbb222").unwrap();
        // Both report completion (a result event exists, payload or not)...
        assert!(journal_reports_completion(a, &cache));
        assert!(journal_reports_completion(b, &cache));
        // ...first-event-wins keeps aaa111's payload None; bbb222's renders compactly.
        assert_eq!(journal_result(a, &cache), None);
        assert_eq!(journal_result(b, &cache).as_deref(), Some("{\"k\":1}"));
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
        let lc = lifecycle(
            &subs[0],
            &JournalCache::build(std::slice::from_ref(&subs[0])),
        )
        .unwrap();
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
                task_kind: None,
                team_name: None,
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
                task_kind: None,
                team_name: None,
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
        let lc = lifecycle(
            &subs[0],
            &JournalCache::build(std::slice::from_ref(&subs[0])),
        )
        .unwrap();
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
        let lc = lifecycle(
            &subs[0],
            &JournalCache::build(std::slice::from_ref(&subs[0])),
        )
        .unwrap();
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
        let lc = lifecycle(
            &subs[0],
            &JournalCache::build(std::slice::from_ref(&subs[0])),
        )
        .unwrap();
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
        let lc = lifecycle(
            &subs[0],
            &JournalCache::build(std::slice::from_ref(&subs[0])),
        )
        .unwrap();
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
        let lc = lifecycle(
            &subs[0],
            &JournalCache::build(std::slice::from_ref(&subs[0])),
        )
        .unwrap();
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
        let lc = lifecycle(
            &subs[0],
            &JournalCache::build(std::slice::from_ref(&subs[0])),
        )
        .unwrap();
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
            journal_result(wf, &JournalCache::build(std::slice::from_ref(wf))).as_deref(),
            Some("WF RETURN: workflow journal payload")
        );
        // A built-in has no journal → None.
        let builtin = subs
            .iter()
            .find(|s| s.kind == SubagentKind::BuiltinTask)
            .unwrap();
        assert!(
            journal_result(builtin, &JournalCache::build(std::slice::from_ref(builtin))).is_none()
        );
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
    fn frozen_lane_classifies_escalation_blocked_vs_awaiting_execution() {
        let fx = Fixture::new();
        let enc = "-Users-frozen";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n",
        );
        // (1) FROZEN at a dangerous-rm Bash (unreturned), PRECEDED by assistant TEXT — the exact
        // L629→L630 shape that made the old walk-back mis-report `completed`. → escalation-blocked.
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aesc111.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"aesc111\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"teardown\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:01:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Now removing the scratch files.\"}]}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:02:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_rm\",\"name\":\"Bash\",\"input\":{\"command\":\"for f in a b; do rm -rf \\\"$SCRATCH/$f\\\"; done\"}}]}}\n"
            ),
        );
        // (2) FROZEN at a non-danger tool_use (Read, unreturned) → awaiting-execution.
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-await22.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"await22\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"read\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:02:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_read\",\"name\":\"Read\",\"input\":{\"file_path\":\"/x/big.log\"}}]}}\n"
            ),
        );
        // (3) RESOLVED: a dangerous Bash whose tool_result + closing text arrived → NOT pending.
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-done333.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"done333\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"teardown\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:02:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_rm2\",\"name\":\"Bash\",\"input\":{\"command\":\"rm -rf $SCRATCH/*\"}}]}}\n",
                "{\"type\":\"user\",\"timestamp\":\"2026-06-07T05:40:00.000Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_rm2\",\"content\":\"done\"}]}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:41:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Teardown complete.\"}]}}\n"
            ),
        );

        let nodes = build_topology(&session, false).unwrap();
        let esc = nodes.iter().find(|n| n.agent_id == "aesc111").unwrap();
        // The frozen escalation lane is RUNNING (not completed — the bug) + escalation-blocked.
        assert_eq!(esc.status, SubagentStatus::Running);
        assert_eq!(
            esc.pending_classification,
            Some(PendingClassification::EscalationBlocked)
        );
        assert_eq!(esc.pending_tool_name.as_deref(), Some("Bash"));
        assert_eq!(esc.pending_tool_use_id.as_deref(), Some("toolu_rm"));
        assert_eq!(
            esc.pending_since_utc.as_deref(),
            Some("2026-06-07T05:02:00.000Z")
        );

        let awa = nodes.iter().find(|n| n.agent_id == "await22").unwrap();
        assert_eq!(awa.status, SubagentStatus::Running);
        assert_eq!(
            awa.pending_classification,
            Some(PendingClassification::AwaitingExecution)
        );
        assert_eq!(awa.pending_tool_name.as_deref(), Some("Read"));

        let done = nodes.iter().find(|n| n.agent_id == "done333").unwrap();
        assert_eq!(done.status, SubagentStatus::Completed);
        assert!(done.pending_classification.is_none());
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

    #[test]
    fn session_id_from_path_is_canonical_bare_hex() {
        // The SINGLE per-file id derivation every surface (list/search/files/recover/
        // turns) now routes through, so the same transcript reports an IDENTICAL id
        // whichever subcommand prints it. A subagent stem loses its `agent-` prefix; a
        // top-level uuid passes through; a stem-less path yields an empty string.
        assert_eq!(
            session_id_from_path(Path::new(
                "/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d/subagents/agent-a585e25a580c59e7a.jsonl"
            )),
            "a585e25a580c59e7a"
        );
        assert_eq!(
            session_id_from_path(Path::new(
                "/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jsonl"
            )),
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
        );
        // A root path with no file stem → empty (never panics).
        assert_eq!(session_id_from_path(Path::new("/")), "");
    }

    #[test]
    fn parent_session_id_and_is_subagent_from_path() {
        let sub = Path::new(
            "/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d/subagents/agent-a585e25a580c59e7a.jsonl",
        );
        let wf = Path::new(
            "/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d/subagents/workflows/wf_abc/agent-aaa.jsonl",
        );
        let top = Path::new("/x/-Enc/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jsonl");
        // A subagent path → parent is the dir before `subagents`, and is_subagent is true.
        assert_eq!(
            parent_session_id_from_path(sub).as_deref(),
            Some("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d")
        );
        assert!(is_subagent_path(sub));
        // A workflow subagent path → same parent (the segment before `subagents`).
        assert_eq!(
            parent_session_id_from_path(wf).as_deref(),
            Some("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d")
        );
        assert!(is_subagent_path(wf));
        // A top-level path → no parent (it IS its own session), is_subagent false.
        assert_eq!(parent_session_id_from_path(top), None);
        assert!(!is_subagent_path(top));
    }
}
