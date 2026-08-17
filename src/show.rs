//! `show` subcommand — fetch specific record(s) of ONE transcript.
//!
//! The reader companion to `search`: search FINDS (match-centered excerpts across a
//! scope), `show` FETCHES the records you name — by 1-based jsonl line number (the
//! `Lnnnn` every csift surface prints) or by record uuid — rendered FULL through the
//! same per-record pipeline search hits use (classify → labels, plan pointers, tool
//! pairing, elicitation-sidecar merge). `--raw` instead emits the VERBATIM raw jsonl
//! line(s): the escape hatch for fields csift does not render (usage tokens,
//! stop_reason, model, …) and for inspecting corruption — raw reads the transcript
//! file only (no sidecar merge, no record parsing).
//!
//! Addressing discipline (SPEC §6.12): an EXPLICITLY named line/uuid that resolves to
//! no record is a HARD error (address-miss = error; filter-empty = ok is the
//! tool-wide exit law); a range clamps to the file but errors when it yields nothing.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::cli::{OutputFormat, ShowArgs};
use crate::parse::{mmap_bytes, scan_lines_bytes};
use crate::path::{self, SubagentScope};
use crate::search::{fetch_records, merged_any_sidecar, print_record_line, role_glyph, Exchange};
use crate::timez::{format_local_compact, local_iso};

mod addr;
mod render;
mod run;

pub(crate) use addr::*;
pub(crate) use render::*;
pub(crate) use run::*;
