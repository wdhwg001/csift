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

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

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

/// The user's home directory, honoring `$HOME` first (the SPEC ties everything to
/// `$HOME`), then falling back to the OS notion of home.
fn home_dir() -> Result<PathBuf> {
    if let Some(h) = std::env::var_os("HOME") {
        if !h.is_empty() {
            return Ok(PathBuf::from(h));
        }
    }
    // Last resort on platforms / test envs without $HOME.
    #[allow(deprecated)]
    std::env::home_dir().ok_or_else(|| anyhow!("cannot determine home directory ($HOME unset)"))
}

/// Absolute path to `~/.claude/projects` (honors `$HOME`).
pub fn projects_root() -> Result<PathBuf> {
    Ok(home_dir()?.join(".claude").join("projects"))
}

/// A resolved project target: the encoded dir under the projects root.
#[derive(Debug, Clone)]
pub struct ProjectDir {
    /// Absolute path to the `<encoded>` directory under the projects root.
    pub dir: PathBuf,
}

/// True iff `token` is a plausible pre-encoded projects-dir basename: starts with
/// `-`, contains only `[A-Za-z0-9-]` (so no `/`), per §2.3 step 1.
fn looks_like_encoded_token(token: &str) -> bool {
    let mut chars = token.chars();
    match chars.next() {
        Some('-') => {}
        _ => return false,
    }
    token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// If `target` is (or lives directly under) the projects root and names a single
/// encoded dir, return that basename token; else `None`. Handles both
/// `<encoded>` and `~/.claude/projects/<encoded>` forms.
fn strip_projects_root_prefix(target: &Path, root: &Path) -> Option<String> {
    // Form: a bare token with no separators (e.g. `-Users-testuser-Projects-foo`).
    if let Some(s) = target.to_str() {
        if !s.contains('/') && looks_like_encoded_token(s) {
            return Some(s.to_string());
        }
    }
    // Form: `<root>/<encoded>` (possibly with `~` already expanded by the shell,
    // or passed literally). Compare component-wise against the known root.
    if let Ok(rest) = target.strip_prefix(root) {
        // Exactly one component left, and it must be an encoded token.
        let mut comps = rest.components();
        if let (Some(first), None) = (comps.next(), comps.next()) {
            if let Some(name) = first.as_os_str().to_str() {
                if looks_like_encoded_token(name) {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

/// Absolutize a real filesystem path WITHOUT requiring it to exist (the project
/// the cwd points at may have been deleted while its transcripts remain). We
/// canonicalize when possible to resolve symlinks/`..`, else fall back to
/// joining with the current dir + lexical normalization.
fn absolutize(p: &Path) -> Result<PathBuf> {
    if let Ok(c) = std::fs::canonicalize(p) {
        return Ok(c);
    }
    let base = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .context("cannot read current dir to absolutize a relative target")?
            .join(p)
    };
    Ok(lexical_normalize(&base))
}

/// Lexically resolve `.`/`..` without touching the filesystem (used when the path
/// does not exist so `canonicalize` can't run). Symlinks are not resolved here —
/// acceptable, since the encoding only needs the textual absolute path.
fn lexical_normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve a user-supplied target (actual cwd OR a pre-encoded dir, §2.3) to a
/// concrete `<encoded>` directory under `~/.claude/projects`. Errors (never an
/// empty silent result) when neither interpretation resolves to a directory.
pub fn resolve_target(target: &Path) -> Result<ProjectDir> {
    let root = projects_root()?;

    // §2.3 step 1: pre-encoded projects-dir token (bare or under the root).
    if let Some(token) = strip_projects_root_prefix(target, &root) {
        let dir = root.join(&token);
        if dir.is_dir() {
            return Ok(ProjectDir { dir });
        }
        // A leading-`-` token that doesn't exist as a dir: don't silently fall
        // through to path-encoding (it can't be a real absolute path anyway).
        bail!(
            "no Claude Code project dir named {:?} under {}",
            token,
            root.display()
        );
    }

    // §2.3 step 2: treat as a real filesystem path — absolutize + encode + look up.
    let abs = absolutize(target)?;
    let encoded = encode_cwd(&abs);
    let dir = root.join(&encoded);
    if dir.is_dir() {
        return Ok(ProjectDir { dir });
    }

    // §2.3 step 4: neither resolved — surface the attempted path, no empty result.
    bail!(
        "no Claude Code project dir for {} (looked for {})",
        abs.display(),
        dir.display()
    )
}

/// Enumerate every project directory directly under `~/.claude/projects`.
/// Returns only entries that are directories (ignores stray files). Order is
/// sorted by basename for deterministic output.
pub fn all_project_dirs() -> Result<Vec<ProjectDir>> {
    let root = projects_root()?;
    let read = std::fs::read_dir(&root)
        .with_context(|| format!("cannot read projects root {}", root.display()))?;
    let mut dirs = Vec::new();
    for entry in read {
        let entry =
            entry.with_context(|| format!("error reading an entry in {}", root.display()))?;
        let path = entry.path();
        // file_type() avoids an extra stat where the OS already knows; fall back
        // to is_dir() (follows symlinks) when the type is unavailable.
        let is_dir = match entry.file_type() {
            Ok(ft) if ft.is_symlink() => path.is_dir(),
            Ok(ft) => ft.is_dir(),
            Err(_) => path.is_dir(),
        };
        if is_dir {
            dirs.push(ProjectDir { dir: path });
        }
    }
    dirs.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Forward encoding: a table of real (cwd, encoded) ground-truth pairs ──
    // Every pair below is taken from an actual `~/.claude/projects` dir name.

    #[test]
    fn encode_real_ground_truth_table() {
        let table: &[(&str, &str)] = &[
            (
                "/Users/testuser/Projects/widget_app_prototype",
                "-Users-testuser-Projects-widget-app-prototype",
            ),
            (
                "/Users/testuser/Projects/Acme/widget_factory-worktrees/main",
                "-Users-testuser-Projects-Acme-widget-factory-worktrees-main",
            ),
            // The `/.claude` segment emits a literal `--` (proves no collapse).
            (
                "/Users/testuser/Projects/Acme/widget_factory/.cache-worktrees/sunny-meadow",
                "-Users-testuser-Projects-Acme-widget-factory--cache-worktrees-sunny-meadow",
            ),
            ("/a/.claude/b", "-a--claude-b"),
            // Case is preserved; digits pass through.
            ("/Users/testuser/Projects/coc", "-Users-testuser-Projects-coc"),
        ];
        for (cwd, encoded) in table {
            assert_eq!(
                encode_cwd(Path::new(cwd)),
                *encoded,
                "encoding mismatch for {cwd}"
            );
        }
    }

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

    #[test]
    fn encode_preserves_case_and_digits() {
        assert_eq!(encode_cwd(Path::new("/Foo/Bar9/Baz")), "-Foo-Bar9-Baz");
    }

    #[test]
    fn encode_space_and_dot_become_dash() {
        assert_eq!(encode_cwd(Path::new("/a b/c.d")), "-a-b-c-d");
    }

    // ── Encoded-token detection (§2.3 step 1) ──

    #[test]
    fn encoded_token_detection() {
        assert!(looks_like_encoded_token("-Users-testuser-Projects-foo"));
        assert!(looks_like_encoded_token("-a--claude-b"));
        assert!(looks_like_encoded_token("-")); // degenerate but well-formed
                                                // A real absolute path has slashes → not a bare token.
        assert!(!looks_like_encoded_token("/Users/testuser/Projects/foo"));
        // Must start with `-` (a real absolute cwd encodes to a leading `-`).
        assert!(!looks_like_encoded_token("Users-foo"));
        // No other punctuation survives in a real encoded name.
        assert!(!looks_like_encoded_token("-a_b"));
        assert!(!looks_like_encoded_token("-a/b"));
    }

    #[test]
    fn strip_prefix_recognizes_bare_token() {
        let root = Path::new("/home/u/.claude/projects");
        assert_eq!(
            strip_projects_root_prefix(Path::new("-Users-foo-bar"), root).as_deref(),
            Some("-Users-foo-bar")
        );
    }

    #[test]
    fn strip_prefix_recognizes_under_root() {
        let root = Path::new("/home/u/.claude/projects");
        assert_eq!(
            strip_projects_root_prefix(Path::new("/home/u/.claude/projects/-Users-foo-bar"), root)
                .as_deref(),
            Some("-Users-foo-bar")
        );
    }

    #[test]
    fn strip_prefix_rejects_real_path() {
        let root = Path::new("/home/u/.claude/projects");
        // A real cwd with slashes is NOT a bare token and is not under the root.
        assert!(strip_projects_root_prefix(Path::new("/Users/testuser/Projects/foo"), root).is_none());
        // Under-root but with an extra nested component (a session dir, not an
        // encoded project token) → not a single-component encoded token.
        assert!(strip_projects_root_prefix(
            Path::new("/home/u/.claude/projects/-Users-foo/sub"),
            root
        )
        .is_none());
    }

    #[test]
    fn lexical_normalize_resolves_dotdot() {
        assert_eq!(
            lexical_normalize(Path::new("/a/b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(
            lexical_normalize(Path::new("/a/./b")),
            PathBuf::from("/a/b")
        );
    }

    #[test]
    fn projects_root_ends_with_claude_projects() {
        // We do NOT mutate $HOME here: env is process-global and cargo runs tests
        // as threads, so a set/restore would race sibling tests. Assert the shape
        // off the ambient $HOME instead.
        let root = projects_root().expect("projects_root");
        assert!(root.ends_with("projects"));
        assert!(root.to_string_lossy().contains(".claude"));
    }
}
