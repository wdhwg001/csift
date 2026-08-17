//! `turns` subcommand - turn-fidelity reconstruction.
//!
//! A Claude Code COMPACTION SUMMARY preserves task STATE (the 9-section synthesis:
//! intent, file ledger, errors+fixes, plan, next step) in high fidelity, but provably
//! LOSES turn fidelity - its "All user messages" section clips ~22 real prose turns to
//! ~17 `...`-truncated bullets, and the assistant side collapses to a SINGLE verbatim
//! quote (the last pre-compaction message). `turns` SUPPLEMENTS (never replaces) the
//! summary: it re-emits the clipped user phrasings + discarded assistant end-of-turn
//! replies, in ORIGINAL ORDER, each line carrying the jsonl LINE NUMBER so a consumer
//! can `Read` the raw transcript at the cited line.
//!
//! ## Reuse, never re-parse
//!
//! This module sits squarely on the `recover` extraction layer: the same
//! mmap → forward line-numbered [`crate::parse::scan_lines_bytes`] scan (so the local
//! `line_no` counter is a 1:1 map to the jsonl), the same
//! [`crate::model::group_turn_indices`] turn delimiter, the same `Record` helpers
//! ([`Record::is_genuine_user`] / [`Record::genuine_user_text`] /
//! [`Record::agent_text`] / [`Record::blocks`] / `is_compact_summary`), the same
//! [`crate::path::resolve_session_files`] / [`crate::time_window::TimeWindow`] /
//! [`crate::timez`] rendering. The `Record`/`Block` model needs no change.
//!
//! ## Selection vs render order
//!
//! Selection walks BACKWARD from EOF (recency-first) so the budget is spent on what a
//! resumed agent most needs; the emitted document is sorted ASCENDING so it reads as a
//! forward transcript. The backward walk is TRANSPARENT to `isCompactSummary` records
//! (a summary is a turn member, never a delimiter - `src/model.rs`), so it reaches back
//! across multiple compaction boundaries by default.
//!
//! ## Multi-agent-message model + richness filtering (the model-expansion)
//!
//! A single genuine-user turn can own a LONG RUN of agent messages - a debugging/build
//! chain the model narrates step by step - that a compaction summary clips to its single
//! §9 EOT quote. Each [`TurnSlice`] therefore carries `agents: Vec<AgentMsg>` (EVERY
//! agent-text record of the turn, in file order), and a derived `assistant_eot()`
//! accessor returns the LAST element - the EOT anchor - preserving the whole existing
//! dedup / round-trip / render call-graph with zero churn.
//!
//! Selection ([`select_agent_messages`]) reduces the run to a survivor set, gated by the
//! master `--agent-msgs` mode:
//!   - `eot-only` (DEFAULT, non-breaking) - keep only the last agent message; the output
//!     is byte-identical to the pre-expansion single-EOT document.
//!   - `rich` - on a LONG run (`agents.len() > run_threshold`, default 6): the LAST is
//!     always kept; the FIRST is kept by position privilege under `--keep-first`; each
//!     MIDDLE is kept UNLESS it is a PROVEN pure declaration. Collapsed contiguous runs
//!     fuse into one `△ L…  [X agent messages, Y tool calls, Z failed]` placeholder
//!     carrying the fetchable jsonl line range + the per-message tool/failed attribution.
//!   - `all` - keep every agent message (maximal fidelity, no placeholder).
//!
//! "Rich" ([`agent_msg_is_rich`]) is a cheap single-pass OR of a LENGTH gate (kept on
//! length alone ≥ `rich_min_chars`) and a SIGNAL test (a number-of-substance, a commit
//! hash, a `file.rs:NNN` ref, a backtick code path, or a finding/decision lexeme).
//! KEEP-ON-DOUBT is the spine: [`agent_msg_is_droppable`] collapses ONLY a short,
//! signal-less, intent-verb opener - everything uncertain is kept (a wrongly-kept
//! declaration costs ≤ one capped body; a wrongly-dropped finding is unrecoverable). A
//! FUSED finding+declaration body trips a signal → kept WHOLE; its trailing declaration
//! is shed only by the existing within-message `ASST_CAP` char-ellipsis, never by
//! whole-message drop. `--profile heavy|light` bundles the thresholds; defaults equal
//! today's behavior so the whole feature is dead code until a non-default mode is chosen.
//!
//! ## Never fabricate, never silently drop
//!
//! An over-cap unit is MIDDLE-truncated (head+tail kept) with an explicit
//! `… [+K chars, L lines elided] …` marker that carries the exact elided counts; its
//! `Lnnnnn` points at the full record. Dedup against the live summary is
//! DEMOTE-AND-FLAG, never delete. A collapsed agent-message run is NEVER silently
//! dropped either - its placeholder carries the exact counts + the line range to fetch.

use std::path::Path;

use anyhow::{bail, Result};
use memchr::memmem;
use rayon::prelude::*;

use crate::cli::{OutputFormat, VerbatimArgs};
use crate::model::{group_turn_indices_deduped, normalize_line, Block, Content, PlanIndex, Record};
use crate::parse::mmap_bytes;
use crate::path;
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

mod build;
mod config;
mod json;
mod planning;
mod render;
mod richness;
mod run;
mod scope;
mod select;
mod units;

pub(crate) use build::*;
pub(crate) use config::*;
pub(crate) use json::*;
pub(crate) use planning::*;
pub(crate) use render::*;
pub(crate) use richness::*;
pub(crate) use run::*;
pub(crate) use scope::*;
pub(crate) use select::*;
pub(crate) use units::*;

#[cfg(test)]
mod tests;
