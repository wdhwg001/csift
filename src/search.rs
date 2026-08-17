//! `search` subcommand — regex over transcripts, returning complete round-trip
//! exchanges.
//!
//! Behavior (SPEC.md §6.2, §6.4):
//! - Pattern is ripgrep-like, default smart-case (`-i` forces insensitive,
//!   `--multiline` lets `.` cross newlines). Empty pattern == pure filter.
//! - Filters: `--category/-t` (repeatable), `--turn` XOR (`--since`/`--until`),
//!   a positional `[PATH]...` target (cwd / encoded dir / `@<uuid>` / `*.jsonl`, repeatable,
//!   multi-target).
//! - A **turn** is delimited by GENUINE user messages; a `tool_result`-carrier, an
//!   `isMeta` pseudo-turn, and a compaction summary never start a turn.
//! - On a hit, the COMPLETE round-trip (Exchange) is returned: a matched `tool_use`
//!   WITH its `tool_result`; a matched user turn WITH the agent response; etc. The
//!   exchange is the whole turn (opening genuine-user + every record chained under
//!   it until the next genuine-user), so every form of completeness in §6.4 holds.
//! - `--max-count` caps results but NEVER silently — the dropped count is reported.
//! - rayon parallelizes across files; lazy parse keeps it fast on 200 MB+ inputs.
//!
//! ## Scan strategy
//!
//! `list` can head/tail-read, but `search` must see the whole session to delimit
//! turns and stitch exchanges, so it mmaps the file once and does a single forward
//! [`crate::parse::scan_lines_bytes`] pass with the two-stage byte prefilter (§7d):
//! the category prefilter gates the `serde_json` parse (dropping the ~54%
//! attachment/noise lines pre-JSON); the keyword prefilter marks `can_hit` so the
//! match phase skips regex work on records that provably lack the literal. Turn
//! reconstruction then runs over the retained transcript records.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use memchr::memmem;
use rayon::prelude::*;
use regex::bytes::Regex as BytesRegex;

use crate::cli::{LabelFilter, OutputFormat, SearchArgs};
use crate::model::{
    group_turn_indices_deduped, normalize_line, tool_result_content_text, Block, Class,
    ClassifyCtx, Content, PlanIndex, Record, SpawnLookup,
};
use crate::parse::mmap_bytes;
use crate::path::{self};
use crate::subagent::{discover_subagents, is_subagent_path};
use crate::time_window::TimeWindow;
use crate::timez::{format_local_compact, local_iso};

mod census;
mod hits;
mod matcher;
mod record_text;
mod render;
mod run;
mod scan;
mod turns_match;
mod types;

pub(crate) use census::*;
pub(crate) use hits::*;
pub(crate) use matcher::*;
pub(crate) use record_text::*;
pub(crate) use render::*;
pub(crate) use run::*;
pub(crate) use scan::*;
pub(crate) use turns_match::*;
pub(crate) use types::*;

#[cfg(test)]
mod tests;
