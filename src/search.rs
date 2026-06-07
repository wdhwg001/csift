//! `search` subcommand — regex over transcripts, returning complete x exchanges.
//!
//! Behavior (SPEC.md § search):
//! - Pattern is ripgrep-like, default smart-case (`-i` forces insensitive,
//!   `--multiline` lets `.` cross newlines). Empty pattern == pure filter.
//! - Filters: `--category/-t` (repeatable), `--turn-range` XOR (`--since`/`--until`),
//!   `--session`, `--path` (repeatable, multi-target).
//! - A **turn** is delimited by GENUINE user messages; a tool_result-carrier does
//!   NOT start a turn.
//! - On a hit, return the COMPLETE round-trip: a matched `tool_use` WITH its
//!   `tool_result`; a matched user turn WITH the agent response. Exchanges are
//!   reconstructed by `uuid`/`parentUuid` linking.
//! - `--max-count` caps results but NEVER silently — the dropped count is reported.
//! - rayon parallelizes across files; lazy parse keeps it fast on 200MB+ inputs.

use anyhow::Result;

use crate::cli::{Category, SearchArgs};

/// A single category-tagged hit inside an exchange.
#[derive(Debug, Clone)]
pub struct Hit {
    pub category: Category,
    /// The matched text excerpt (or full block, per render mode).
    pub excerpt: String,
    pub timestamp_utc: Option<String>,
}

/// A complete reconstructed request/response exchange (x) containing the hit(s).
#[derive(Debug, Clone)]
pub struct Exchange {
    pub session_id: String,
    /// 0-based turn index (turns delimited by genuine-user messages).
    pub turn_index: usize,
    pub hits: Vec<Hit>,
    /// Uuids of every record stitched into this exchange (for traceability).
    pub record_uuids: Vec<String>,
}

/// Outcome of a search run, including the no-silent-truncation accounting.
#[derive(Debug, Clone, Default)]
pub struct SearchOutcome {
    pub exchanges: Vec<Exchange>,
    /// How many matching exchanges were dropped by `--max-count` (0 if none).
    pub dropped_by_cap: usize,
}

/// Entry point for `csift search`.
pub fn run_search(_args: &SearchArgs) -> Result<()> {
    todo!("compile regex (smart-case), resolve targets, scan, stitch x, render; Phase 2")
}

/// Compile the user pattern honoring smart-case / `-i` / `--multiline`.
pub fn build_matcher(_args: &SearchArgs) -> Result<regex::Regex> {
    todo!("smart-case detection + RegexBuilder flags; Phase 2")
}

/// Search a single session file, returning the matching exchanges.
pub fn search_session(_path: &std::path::Path, _args: &SearchArgs) -> Result<Vec<Exchange>> {
    todo!("lazy-parse scan + parentUuid stitching + filters; Phase 2")
}
