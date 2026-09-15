//! `list` subcommand - enumerate sessions with quick identity fields.
//!
//! For each session jsonl, emit: session-id, FIRST genuine-user message, LAST
//! genuine-user message, LAST agent message (each with its timestamp), plus the
//! decoded cwd / version / gitBranch - the fast "which session is this?" view.
//! Uses a forward HEAD read for the first user message and a backward TAIL read
//! for the last user/agent messages (never a full parse). Timestamps render in the
//! system-local timezone alongside raw UTC (see [`crate::timez`]). Files are
//! processed in parallel across the corpus (`rayon`), then sorted for deterministic
//! output.
//!
//! ## Scope resolution + parallelism
//!
//! Target resolution (positional PATH(s) / `@<uuid>` / `*.jsonl`, with subagent spanning)
//! goes through the SHARED [`crate::path::resolve_session_files`] resolver - the SAME one
//! `search`/`agents`/`files`/`recover`/`turns` use - so `list` is no longer a separate
//! scope dialect: a `csift list @<uuid>` identifies that one session, exactly like its
//! siblings. The dominant work - the per-session head+tail parse -
//! then runs `rayon` `par_iter()` across the resolved files on the default pool (= CPU
//! count); results are sorted by path for deterministic output.

use std::path::{Path, PathBuf};

use anyhow::Result;
use rayon::prelude::*;

use crate::cli::{ListArgs, OutputFormat};
use crate::model::Record;
use crate::parse::{head_records_prefiltered, tail_records_prefiltered};
use crate::path::{self, SubagentScope};
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

mod render;
mod rows;
mod run;
mod summarize;

pub(crate) use render::*;
pub(crate) use rows::*;
pub(crate) use run::*;
pub(crate) use summarize::*;

#[cfg(test)]
mod tests;
