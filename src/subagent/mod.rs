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

mod discover;
mod ids;
mod lifecycle;
mod meta;
mod spawn;
mod topology;
mod types;

pub(crate) use discover::*;
pub(crate) use ids::*;
pub(crate) use lifecycle::*;
pub(crate) use meta::*;
pub(crate) use spawn::*;
pub(crate) use topology::*;
pub(crate) use types::*;

#[cfg(test)]
mod tests;
