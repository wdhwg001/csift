//! `status` + `wait` - the live-truth command pair.
//!
//! A deliberate, documented departure from the forensic contract: these two answer
//! "NOW", are point-in-time, and are explicitly NON-reproducible (every other command
//! stays reproducible). The verdict is a THREE-WAY JOIN, never an inference from one
//! surface:
//!
//! 1. the harness's session registry (`<claude-home>/sessions/<pid>.json`) - `status`
//!    transitions land sub-second, but the file is written ONLY on transitions (never a
//!    heartbeat: an hours-old `statusUpdatedAt` just means the state has not changed;
//!    the real failure mode is a SIGKILLed session's stale entry, guarded by pid
//!    liveness + a process-start-time check against pid reuse);
//! 2. the transcript tail state machine - an unpaired shell/tool call at the tail = a
//!    tool in flight (subagent transcripts flush per content block; main lands ~1-3.4s
//!    after dispatch); the last assistant record's `stop_reason` is trustworthy on the
//!    main lane (0.0-0.3% null) and NORMALLY null mid-message on subagents;
//! 3. owner-process liveness - a `ps`-based probe with a `/proc/<pid>` fallback where
//!    ps lacks the flags (unix; other hosts degrade honestly).
//!
//! Growth classification (the F9 trap): a main transcript GROWS while idle (notification
//! enqueues, attachments), so mtime alone is never "busy" - what grew must be classified
//! before any verdict moves toward RUNNING. Child liveness comes from each child
//! transcript's own tail plus the incremental workflow journal (`started` minus `result`
//! = agents in flight); `subagents/*.meta.json` has NO status field and workflow result
//! files are terminal-only.
//!
//! Honesty limits, stated in the output when they bite: a pending PERMISSION prompt
//! lives only in Claude Code process memory (it masquerades as idle until a sidecar
//! exists); the registry covers top-level interactive sessions only.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::cli::{OutputFormat, StatusArgs, WaitArgs};
use crate::model::{extract_xml_tag, Block, Content, Record, TASK_NOTIFICATION_PREFIX};
use crate::parse::{mmap_bytes, read_range, read_tail};

mod activity;
mod background;
mod background_scan;
mod children;
mod conditions;
mod last;
mod registry;
mod render;
mod status;
mod tail;
mod tasks;
mod verdict;
mod wait;

pub(crate) use activity::*;
pub(crate) use background::*;
pub(crate) use background_scan::*;
pub(crate) use children::*;
pub(crate) use conditions::*;
pub(crate) use last::*;
pub(crate) use registry::*;
pub(crate) use render::*;
pub(crate) use status::*;
pub(crate) use tail::*;
pub(crate) use tasks::*;
pub(crate) use verdict::*;
pub(crate) use wait::*;

#[cfg(test)]
mod tests;
