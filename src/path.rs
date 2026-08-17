//! Claude Code project-directory path encoding + target resolution.
//!
//! ## Encoding rule (verified empirically against `~/.claude/projects`, 2026-06-07)
//!
//! Claude Code encodes a project's absolute cwd into a directory name by
//! replacing **every** non-`[A-Za-z0-9]` byte with a single `-`. There is **no**
//! collapsing of consecutive dashes. Confirmed cases:
//!
//! - `/Users/testuser/Projects/widget_app_prototype`
//!   -> `-Users-testuser-Projects-widget-app-prototype`  (both `/` and `_` -> `-`)
//! - `/Users/testuser/Projects/Acme/widget_factory-worktrees/main`
//!   -> `-Users-testuser-Projects-Acme-widget-factory-worktrees-main`
//! - a source segment `/.claude/` -> `--claude-` (the `.` and the two `/`
//!   each become their own `-`, so a literal `--` double-dash appears — proves
//!   NO consecutive-dash collapse, and `.` -> `-`).
//!
//! Forward (cwd -> encoded) is therefore deterministic. Reverse (encoded -> cwd)
//! is **lossy** (a `-` could have been `/`, `_`, `.`, space, …) so we never try to
//! reverse it; instead a caller-supplied real path is re-encoded and matched.
//!
//! ## Target resolution (§2.3)
//!
//! A user-supplied target is EITHER (a) an actual filesystem cwd — encode it and
//! locate the matching dir under `~/.claude/projects` — OR (b) a pre-encoded
//! `<ENCODED>` dir (optionally under `~/.claude/projects/`). We treat the arg as a
//! pre-encoded token only when, after stripping a leading `~/.claude/projects/`,
//! the remainder has no `/`, matches `^-[A-Za-z0-9-]*$`, AND resolves to a dir.
//! Otherwise it is a real path: absolutize, encode (§2.1), look up the dir.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{anyhow, bail, Context, Result};

mod home;
mod ids;
mod project_dirs;
mod resolver;
mod scope;
mod targets;
mod trap;

pub(crate) use home::*;
pub(crate) use ids::*;
pub(crate) use project_dirs::*;
pub(crate) use resolver::*;
pub(crate) use scope::*;
pub(crate) use targets::*;
pub(crate) use trap::*;

#[cfg(test)]
mod tests;
