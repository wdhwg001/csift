//! Subagent kinds, lifecycle types, pending classification.

use super::*;

/// Subagent kind, keyed off the on-disk path location (authoritative; see module
/// docs - `agentType` is descriptive only, not the discriminator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentKind {
    /// `subagents/agent-<hex>.jsonl` - a built-in Task/Agent-tool subagent.
    BuiltinTask,
    /// `subagents/workflows/wf_<id>/agent-<hex>.jsonl` - a workflow / OMC agent.
    Workflow,
    /// A "teammate" (`taskKind:"in_process_teammate"`) - Claude Code's persistent, directly
    /// addressable team-member agent. It lands at the built-in on-disk LOCATION
    /// (`subagents/agent-<id>.jsonl`), so location alone can't tell it apart; the discriminator
    /// is the meta.json `taskKind`. Its canonical id embeds the teammate name
    /// (`aVSRepro-68a2a1661c9390c1`, see [`crate::path::is_subagent_id`]), its meta carries
    /// `teamName`/`color`/`model` and NO `toolUseId`, and it is spawned by an `Agent` tool_use
    /// joined by NAME (not tool_use id) - see [`ParentSpawnIndex::spawn_id_for_name`].
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
    /// The enclosing `<session-uuid>` dir name - the parent session id (filesystem
    /// linkage; corroborated by the record `sessionId`).
    pub parent_session_id: String,
    /// The `wf_<id>` workflow id for a workflow subagent; `None` for built-in.
    pub workflow_id: Option<String>,
    /// The parent `Task`/`Agent` tool_use `id` that SPAWNED this subagent, read from the
    /// built-in `meta.json` `toolUseId` (on disk for every built-in subagent; `None` for
    /// workflow agents, whose meta carries only `agentType`). This is the join key into
    /// the parent transcript's spawn index ([`ParentSpawnIndex`]) - it recovers the true
    /// trigger time + the returned message.
    pub spawn_tool_use_id: Option<String>,
    /// The agent's `name` from meta.json (the `Agent` tool's `name` param, e.g. a teammate
    /// handle like `VSRepro` or an OMC lane name like `LaneDONE`). `None` when absent. For a
    /// teammate this is the NAME-join key into the spawning `Agent` tool_use (whose meta carries
    /// no `toolUseId`), recovering the spawn linkage the flat layout otherwise drops.
    pub name: Option<String>,
    /// The `teamName` from meta.json - present only for a teammate (`taskKind:in_process_teammate`);
    /// `None` for built-in/workflow agents. Identifies the team a teammate belongs to.
    pub team_name: Option<String>,
    /// `agentType` from meta.json (descriptive sub-label, e.g. `Explore`,
    /// `oh-my-claudecode:executor`, `workflow-subagent`). Read ONCE here at discovery so
    /// [`lifecycle`] takes it from the `Subagent` instead of re-reading the meta (FIX3 - kills
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
/// [`Subagent`], not duplicated here - `lifecycle` is consumed by [`node_for`], which
/// already holds the `Subagent`. This struct carries ONLY the transcript-derived facts.
#[derive(Debug, Clone)]
pub struct SubagentLifecycle {
    /// `agentType` from meta.json (descriptive sub-label, e.g. `Explore`,
    /// `oh-my-claudecode:executor`, `workflow-subagent`).
    pub agent_type: Option<String>,
    /// Short description from a built-in meta.json (the Task tool's `description`).
    pub description: Option<String>,
    /// First transcript record's timestamp (raw UTC) - the START.
    pub started_utc: Option<String>,
    /// Last transcript record's timestamp (raw UTC) - the COMPLETION (best-effort).
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
    /// The transcript exists but shows no completion signal - likely still running,
    /// or interrupted. We do not over-claim "failed"; this is the honest middle.
    Running,
    /// No timestamps / empty transcript - status cannot be determined.
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
/// tool_use with no following tool_result - see [`SubagentLifecycle::pending`]). The escalation
/// itself never reaches jsonl (it lives only in CC process memory), so these three states share
/// one on-disk signature; only the danger heuristic can POSITIVELY distinguish the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingClassification {
    /// The pending tool_use is a Bash command CC's classifier would HOIST for human approval even
    /// under bypass-permissions (a dangerous `rm`/`rmdir`, see [`crate::bash_danger`]). Almost
    /// certainly waiting for a human to click "Yes", NOT dying. The high-value disambiguation.
    EscalationBlocked,
    /// A pending tool_use that is NOT a known-hoisted danger - a (possibly slow) tool still
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
/// tool_result resolves - it is the last record, so nothing followed it.
#[derive(Debug, Clone)]
pub struct PendingToolUse {
    pub tool_use_id: String,
    pub tool_name: String,
    /// The Bash `input.command`, when `tool_name == "Bash"` (the danger-heuristic input).
    pub command: Option<String>,
    /// The pending tool_use record's timestamp - when the lane froze.
    pub since_utc: Option<String>,
}
