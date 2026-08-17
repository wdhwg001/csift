//! `csift image` — list and EXTRACT the images a session carries.
//!
//! A user-pasted/attached image (and a tool-result screenshot) rides INLINE on a record as
//! an `{type:"image", source:{type:"base64", media_type:"image/png", data:"<base64>"}}`
//! block — verified against real `~/.claude/projects` data (2026-06-16). The bytes live in
//! the jsonl, so `image` decodes them straight back to files; nothing is externalised.
//!
//! Stable image id = `L<line>i<n>`: the 1-based JSONL line of the carrying record plus the
//! 1-based ordinal of the image among that record's image blocks. It is stable because the
//! transcript is append-only, and it is consistent with the `Lnnnnn` line references used
//! across `recover` / `turns` / `search` (so an id surfaced there feeds straight back here).
//!
//! Default action is to LIST. Pass `--out <PATH>` to EXTRACT — a DIRECTORY keeps each image's
//! source format; a FILE path's extension converts to that format (the `convert in out.jpg` idiom).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use memchr::memmem;
use serde_json::{json, Value};

use crate::cli::{ImageArgs, ImageOutFormat, OutputFormat};
use crate::model::{Block, Record};
use crate::parse::{mmap_bytes, parse_candidates_parallel};
use crate::timez::{format_timestamp, local_iso};

mod convert;
mod refs;
mod render;
mod run;
mod selection;

pub(crate) use convert::*;
pub(crate) use refs::*;
pub(crate) use render::*;
pub(crate) use run::*;
pub(crate) use selection::*;

#[cfg(test)]
mod tests;
