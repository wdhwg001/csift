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

mod automation;
mod classify;
mod classify_promoted;
mod classify_support;
mod exchange;
mod grouping;
mod markers;
mod mutation;
mod narration;
mod peer;
mod predicates;
mod record;
mod taxonomy;

pub(crate) use automation::*;
pub(crate) use classify_support::*;
pub(crate) use grouping::*;
pub(crate) use markers::*;
pub(crate) use mutation::*;
pub(crate) use narration::*;
pub(crate) use peer::*;
pub(crate) use record::*;
pub(crate) use taxonomy::*;

#[cfg(test)]
mod tests;
