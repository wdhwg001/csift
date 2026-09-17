//! JSON-line record + content-block data model.
//!
//! One JSON object per line. The shape below was verified + extended against real
//! `~/.claude/projects/**/*.jsonl` data (2026-06-07). Parsing discipline:
//!
//! - **Tolerate unknown fields.** Real records carry far more than the brief
//!   listed (`attachment`, `file-history-snapshot`, `queue-operation`, `isMeta`,
//!   `isSidechain`, `userType`, `toolUseResult`, `slug`, `entrypoint`, …). We
//!   deserialize only what we use and ignore the rest - never crash on a new field.
//! - **Tolerate missing `timestamp`.** Metadata-only records (`last-prompt`,
//!   `ai-title`, `permission-mode`, `file-history-snapshot`) have no timestamp;
//!   they are skipped in time logic, never panic.
//! - **`message.content` is string OR array.** Older / genuine-user text is a bare
//!   string; everything else is an array of typed blocks.
//!
//! ## Genuine-user vs tool-result-carrier (load-bearing)
//!
//! A `type:"user"` record is NOT always a human turn. In one real session: 332
//! genuine string-content users + 61 text-block users vs **1619** tool_result
//! carriers. A genuine user turn is: string content (and NOT `isCompactSummary`),
//! or content whose blocks are text (no `tool_result`). See [`Record::is_genuine_user`].
//!
//! ## Compaction
//!
//! A compaction summary is a `type:"user"` record with `isCompactSummary: true`
//! and `isVisibleInTranscriptOnly: true`, carrying string content - it must be
//! excluded from "genuine user". A separate `type:"system"`
//! `subtype:"compact_boundary"` record carries the metrics
//! (`trigger`, `preTokens`, `postTokens`, `durationMs`).

use serde::Deserialize;
use std::collections::{HashMap, HashSet};

mod automation;
mod chain;
mod classify;
mod compaction;
mod delivery;
mod exchange;
mod grouping;
mod inventory;
/// The specimen LINES the test suites read, re-exported so the emission sweep in
/// `commands::search` and the taxonomy drift tests read ONE copy of them.
#[cfg(test)]
pub(crate) use inventory::specimens;
mod markers;
mod mutation;
mod narration;
mod peer;
mod predicates;
mod provenance;
mod record;
mod taxonomy;

pub(crate) use automation::*;
pub(crate) use chain::*;
pub(crate) use classify::attachments::classify as attachment_leaf;
pub(crate) use classify::bookkeeping::classify as bookkeeping_leaf;
pub(crate) use classify::support::*;
pub(crate) use classify::system::classify as system_leaf;
pub(crate) use classify::thinking::classify as thinking_block_leaf;
pub(crate) use classify::thinking::is_cancel_salvage as thinking_block_is_salvage;
pub(crate) use classify::thinking::THINKING_REMOVED_TEXT;
pub(crate) use compaction::*;
pub(crate) use grouping::*;
pub(crate) use inventory::*;
pub(crate) use markers::*;
pub(crate) use mutation::*;
pub(crate) use narration::*;
pub(crate) use peer::*;
// `predicates` is otherwise an `impl Record` block, but it also declares the shape its
// one memoised accessor hands back, which `Record`'s own field type names.
pub(crate) use predicates::ConversionText;
pub(crate) use provenance::*;
pub(crate) use record::*;
pub(crate) use taxonomy::*;

#[cfg(test)]
mod tests;
