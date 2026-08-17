//! `agents` subcommand - a session's subagent TOPOLOGY, with time-window filters.
//!
//! For each in-scope top-level session, build the toolUseId-linked topology (see
//! [`crate::subagent::build_topology`]): discover its subagent transcripts (built-in
//! Task/Agent-tool + workflow / OMC agents), join each back to the parent tool_use that
//! spawned it, and emit one [`SubagentNode`] per subagent carrying its id, kind,
//! `agentType`, TRUE trigger time (the parent tool_use ts), start + completion
//! timestamps, status, the 3-way-resolved returned message (on demand), and the
//! files-changed list (on demand). The output is ALWAYS the parent->child tree: workflow
//! RUN nodes (from the top-level `workflows/wf_*.json` manifests) parent their workflow
//! agents, and a nested sub-subagent renders under its spawning agent.
//!
//! `--since`/`--until` (ISO8601 or relative, system-local) filter by TRIGGER time by
//! default; `--order-by start|completion` switch the ordering/window axis. Files are
//! processed in parallel across sessions, then sorted for deterministic output.

use std::path::Path;

use anyhow::{bail, Result};
use rayon::prelude::*;

use crate::cli::{AgentKindFilter, AgentTimeAxis, AgentsArgs, OutputFormat};
use crate::path;
use crate::subagent::{
    build_topology, discover_workflow_runs, duration_label, PendingClassification, SubagentKind,
    SubagentNode, WorkflowRun,
};
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

mod json;
mod render;
mod run;

pub(crate) use json::*;
pub(crate) use render::*;
pub(crate) use run::*;

#[cfg(test)]
mod tests;
