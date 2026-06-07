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
//! ## Target resolution
//!
//! A user-supplied target is EITHER (a) an actual filesystem path — encode it and
//! locate the matching dir under `~/.claude/projects` — OR (b) a path that already
//! resolves under `~/.claude/projects` (use as-is). We detect (b) by canonical
//! prefix; otherwise we treat it as (a).

use std::path::{Path, PathBuf};

use anyhow::Result;

/// Encode an absolute cwd to its Claude Code project-dir basename.
///
/// Every byte not in `[A-Za-z0-9]` becomes a single `-`; no dash collapsing.
#[must_use]
pub fn encode_cwd(cwd: &Path) -> String {
    let s = cwd.to_string_lossy();
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    out
}

/// Absolute path to `~/.claude/projects`.
pub fn projects_root() -> Result<PathBuf> {
    todo!("resolve $HOME/.claude/projects (honor HOME / dirs); Phase 2")
}

/// A resolved project target: the encoded dir plus the projects-root it lives in.
#[derive(Debug, Clone)]
pub struct ProjectDir {
    /// Absolute path to the `<encoded>` directory under the projects root.
    pub dir: PathBuf,
}

/// Resolve a user-supplied target (actual cwd OR a direct projects path) to a
/// concrete `<encoded>` directory under `~/.claude/projects`.
pub fn resolve_target(_target: &Path) -> Result<ProjectDir> {
    todo!("detect projects-root prefix vs real cwd, encode + match; Phase 2")
}

/// Enumerate every project directory under `~/.claude/projects`.
pub fn all_project_dirs() -> Result<Vec<ProjectDir>> {
    todo!("read_dir projects_root, filter to dirs; Phase 2")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_replaces_slash_and_underscore_with_dash() {
        assert_eq!(
            encode_cwd(Path::new("/Users/testuser/Projects/widget_app_prototype")),
            "-Users-testuser-Projects-widget-app-prototype"
        );
    }

    #[test]
    fn encode_does_not_collapse_consecutive_dashes() {
        // A `/.claude/` segment yields a literal `--` (the two `/` and the `.`).
        assert_eq!(encode_cwd(Path::new("/a/.claude/b")), "-a--claude-b");
    }

    #[test]
    fn encode_handles_worktree_path() {
        assert_eq!(
            encode_cwd(Path::new(
                "/Users/testuser/Projects/Acme/widget_factory-worktrees/main"
            )),
            "-Users-testuser-Projects-Acme-widget-factory-worktrees-main"
        );
    }
}
