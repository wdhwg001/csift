//! `files` subcommand — which files/dirs a session modified, and when.
//!
//! Extracts file mutations from a session's transcript (spanning subagents by
//! default), attributes each to its genuine-user turn (the same §6.4 delimiter
//! `search` uses, via [`crate::model::group_turn_indices`]), then aggregates at the
//! requested detail level (summary / by-dir / by-file / timeline) into text or JSON.
//!
//! ## Extraction split (AUTHORITATIVE vs HEURISTIC)
//!
//! - **Authoritative** — `Write`/`Edit`/`MultiEdit` (`input.file_path`) +
//!   `NotebookEdit` (`input.notebook_path`). create-vs-edit is resolved by JOINING the
//!   structured tool_use to its paired tool_result carrier
//!   (`toolUseResult.type == "create"`) by `tool_use_id` within the turn (see
//!   [`crate::model::Record::carrier_create_paths`]).
//! - **Heuristic** — Bash file mutations, parsed lexically from `input.command` by
//!   [`crate::bash_mutations`] (Bash carries no path field in its result). These are
//!   ALWAYS labelled `(heuristic)` and their `is_create` is itself a heuristic guess.
//!
//! ## Performance shape (the 200 MB+ contract)
//!
//! Like `search`, `files` does a SINGLE forward pass per file (mmap, SIMD newline
//! scan, a pre-JSON mutation byte-prefilter), with full `serde_json` parse only on
//! candidate lines. It must NOT retain large blobs — it extracts a few small owned
//! strings per mutation ([`crate::model::FileMutation`]) and drops the record, never
//! holding `originalFile`/`content`/`structuredPatch` bodies from `toolUseResult`.
//!
//! Per-file fan-out uses the default `rayon` pool, which sizes to
//! `std::thread::available_parallelism()` (= CPU count) — the same pool `search` and
//! `agents` use. No explicit `available_parallelism()` call is added: rayon already
//! consults it implicitly, so an explicit call would be dead code (stated here so a
//! future reader does not "fix" it by adding one).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use globset::{Glob, GlobMatcher};
use memchr::memmem;
use rayon::prelude::*;
use regex::Regex;

use crate::bash_mutations::parse_bash_mutations;
use crate::cli::{FilesArgs, FilesDetail, OutputFormat};
use crate::model::{group_turn_indices_deduped, FileMutation, FileOp, Record};
use crate::parse::mmap_bytes;
use crate::path;
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

mod mutations;
mod render;
mod rollup;
mod run;
mod types;

pub(crate) use mutations::*;
pub(crate) use render::*;
pub(crate) use rollup::*;
pub(crate) use run::*;
pub(crate) use types::*;

#[cfg(test)]
mod tests;
