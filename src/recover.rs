//! `recover` subcommand — reconstruct a single file's history from a transcript.
//!
//! Where `files` only rolls up THAT a file was touched, `recover` rebuilds the file's
//! CONTENT, line by line, by replaying the transcript's Reads / Writes / Edits in
//! transcript order, and emits one of four views:
//!
//! - `--patches` (default) segmented unified-diff history of `--file`, split at INTEGRITY
//!   BOUNDARIES (a point where reconstruction across it is invalid: a `File has been
//!   modified since read` harness error, an `originalFile` that disagrees with the
//!   replayed buffer, an external `edited_text_file`, or a heuristic Bash mutation).
//! - `--at` the PARTIAL, line-numbered "in the LLM's eyes" snapshot as of a cutoff;
//!   unknown lines are EXPLICIT gaps, never fabricated.
//! - `--coverage` scope a recovery (recoverable ranges + boundaries + counts), no dump.
//!
//! ## The one new capability: jsonl line numbers
//!
//! No line-number tracking exists elsewhere in `src/`: [`crate::parse::scan_lines_bytes`]
//! hands the visitor only `&[u8]`. We add a LOCAL counter here (never touching the shared
//! signature, so `files`/`search` are unperturbed): `scan_lines_bytes` visits every
//! `\n`-delimited segment with no skipping, so incrementing on each visit yields an exact
//! 1:1 jsonl line map (blank + malformed lines are counted too). Every emitted reference
//! carries its `Lnnnnn` so a consumer can `Read` the raw jsonl directly.
//!
//! ## Never fabricate
//!
//! Reconstruction is necessarily PARTIAL. A line never Read/Edited is an explicit gap; an
//! edit whose `old_string` spans an unknown gap is an un-anchorable coverage hole; a Bash
//! touch is a HEURISTIC (soft) boundary, not authoritative. No silent truncation anywhere.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use memchr::memmem;
use rayon::prelude::*;

use crate::cli::{OutputFormat, RecoverArgs, RecoverMode};
use crate::model::{group_turn_indices_deduped, Block, Record};
use crate::parse::mmap_bytes;
use crate::path;
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

/// Max characters of inline content shown before an explicit `… (+N chars)` marker in
/// HUMAN text output (JSON + `--out` are verbatim). Mirrors `search::EXCERPT_MAX`.
const EXCERPT_MAX: usize = 400;

// ─────────────────────────────────────────────────────────────────────────────
// Data model
// ─────────────────────────────────────────────────────────────────────────────

mod buffer;
mod diff;
mod events;
mod render;
mod replay;
mod report;
mod run;
mod scan;
mod timeline;
mod types;

pub(crate) use buffer::*;
pub(crate) use diff::*;
pub(crate) use events::*;
pub(crate) use render::*;
pub(crate) use replay::*;
pub(crate) use report::*;
pub(crate) use run::*;
pub(crate) use scan::*;
pub(crate) use timeline::*;
pub(crate) use types::*;

#[cfg(test)]
mod tests;
