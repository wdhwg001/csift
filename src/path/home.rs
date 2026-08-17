//! CC-exact cwd encoding (NFC + UTF-16 units) + config-home resolution.

use super::*;

/// Encode an absolute cwd to its Claude Code project-dir basename — the EXACT transform
/// CC applies (extracted verbatim from the 2.1.228 binary): the cwd is first
/// NFC-normalized (every CC path rides `.normalize("NFC")` — a macOS filesystem hands out
/// NFD, so an accented path is re-composed before encoding), then the JS regex
/// `replace(/[^a-zA-Z0-9]/g,"-")` runs per UTF-16 CODE UNIT: every unit outside ASCII
/// alphanumerics becomes ONE `-` — so an astral char (two surrogate units) yields TWO
/// dashes, and a Windows `C:\Users\x` yields `C--Users-x` (`:` and `\` are one unit
/// each). No dash collapsing, no case folding. A char-wise or byte-wise replacement
/// DIVERGES from CC on any non-ASCII cwd and resolves the wrong dir.
#[must_use]
pub fn encode_cwd(cwd: &Path) -> String {
    use unicode_normalization::UnicodeNormalization;
    let s = cwd.to_string_lossy();
    let s: String = s.nfc().collect();
    let mut out = String::with_capacity(s.len());
    for unit in s.encode_utf16() {
        match u8::try_from(unit) {
            Ok(b) if b.is_ascii_alphanumeric() => out.push(char::from(b)),
            _ => out.push('-'),
        }
    }
    out
}

/// The user's home directory, resolved the way Claude Code itself resolves it (Node's
/// `os.homedir()`): `$HOME` on Unix, `%USERPROFILE%` on Windows. The per-platform split is
/// load-bearing on Windows — CC never consults `HOME` there, but Git-Bash/MSYS shells
/// export one (often a POSIX-style `/c/Users/...` a native process cannot use), and
/// honoring it would point csift at a `.claude` dir CC never writes. The conventional env
/// var is read first so a test harness can relocate home per-subprocess; `std::env::home_dir`
/// (un-deprecated, Windows-correct since Rust 1.85 — MSRV is above both) is the fallback.
pub(crate) fn home_dir() -> Result<PathBuf> {
    #[cfg(not(windows))]
    if let Some(h) = std::env::var_os("HOME") {
        if !h.is_empty() {
            return Ok(PathBuf::from(h));
        }
    }
    #[cfg(windows)]
    if let Some(h) = std::env::var_os("USERPROFILE") {
        if !h.is_empty() {
            return Ok(PathBuf::from(h));
        }
    }
    // Last resort on platforms / test envs without the conventional variable.
    std::env::home_dir().ok_or_else(|| {
        anyhow!("cannot determine home directory ($HOME on Unix / %USERPROFILE% on Windows unset)")
    })
}

/// Process-wide override for the Claude config dir, installed once from the global
/// `--claude-home` flag (see [`set_claude_home_override`]). `OnceLock` because it is set
/// exactly once in `main` before any path resolution and never mutated afterwards.
pub(crate) static CLAUDE_HOME_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// Claude Code's own config-dir relocation env var. When set, "every `~/.claude` path
/// lives under that directory instead", so csift — which reads Claude Code's data — must
/// honor it to keep pointing at the same files.
pub const CLAUDE_CONFIG_DIR_ENV: &str = "CLAUDE_CONFIG_DIR";

/// Install an explicit Claude config dir (the `~/.claude` equivalent) from the global
/// `--claude-home` flag. Call ONCE, early in `main`, before any path resolution. Only the
/// first call wins (matching the parse-once-at-startup lifecycle); later calls are no-ops.
pub fn set_claude_home_override(dir: PathBuf) {
    let _ = CLAUDE_HOME_OVERRIDE.set(dir);
}

/// Pure precedence resolver for the Claude config dir, factored out so the ordering is
/// unit-testable without touching process-global env / `OnceLock` state. Order:
/// 1. explicit `--claude-home` flag, 2. `$CLAUDE_CONFIG_DIR` (when non-empty),
/// 3. `$HOME/.claude`.
pub(crate) fn resolve_claude_home(
    flag_override: Option<&Path>,
    config_dir_env: Option<&OsStr>,
    home: &Path,
) -> PathBuf {
    if let Some(p) = flag_override {
        return p.to_path_buf();
    }
    if let Some(d) = config_dir_env {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    home.join(".claude")
}

/// The Claude Code config dir — the `~/.claude` directory, or wherever it has been
/// relocated. Honors, in priority order, the `--claude-home` flag, the `$CLAUDE_CONFIG_DIR`
/// env var (Claude Code's own relocation mechanism), then `$HOME/.claude`. EVERY
/// subcommand reaches Claude's data through here (via [`projects_root`]), so this single
/// override point covers the whole CLI.
pub fn claude_home() -> Result<PathBuf> {
    let flag = CLAUDE_HOME_OVERRIDE.get();
    let env = std::env::var_os(CLAUDE_CONFIG_DIR_ENV);
    // `$HOME` feeds only the default branch; resolve it lazily so a relocated config dir
    // (flag or env) still works when `$HOME` is unset.
    let have_higher = flag.is_some() || env.as_deref().is_some_and(|d| !d.is_empty());
    let home = if have_higher {
        PathBuf::new()
    } else {
        home_dir()?
    };
    Ok(resolve_claude_home(
        flag.map(PathBuf::as_path),
        env.as_deref(),
        &home,
    ))
}

/// Absolute path to the `projects/` dir under the (possibly relocated) Claude config dir.
/// Honors `--claude-home` / `$CLAUDE_CONFIG_DIR` / `$HOME/.claude` (see [`claude_home`]).
pub fn projects_root() -> Result<PathBuf> {
    Ok(claude_home()?.join("projects"))
}

/// A resolved project target: the encoded dir under the projects root.
#[derive(Debug, Clone)]
pub struct ProjectDir {
    /// Absolute path to the `<encoded>` directory under the projects root.
    pub dir: PathBuf,
    /// The canonical cwd of a REAL-path target — `Some` when the user passed an actual
    /// filesystem path, `None` for a pre-encoded `<ENCODED>` dir token (where the user
    /// explicitly named the dir) or an all-projects scan. When `Some`, session enumeration
    /// filters this dir's files to those whose recorded `cwd` IS this path, so a lossy-
    /// encoding COLLISION (a different cwd that encodes to the same dir, §2.1) never leaks a
    /// sibling's sessions — or their subagents — into the result.
    pub target_cwd: Option<PathBuf>,
}

/// True iff `token` is a plausible pre-encoded projects-dir basename, per §2.3 step 1:
/// only `[A-Za-z0-9-]` (so no `/`), and one of the two shapes CC's encoder can emit for
/// an absolute path — a Unix cwd leads with `-` (the leading `/` encodes to `-`; a UNC
/// `\\server\…` leads with `--`), a WINDOWS cwd leads with `<drive-letter>--` (the `:`
/// and `\` of `C:\` each encode to `-`, verbatim from the 2.1.228 binary's sanitizer).
pub(crate) fn looks_like_encoded_token(token: &str) -> bool {
    if !token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return false;
    }
    token.starts_with('-') || is_drive_encoded_token(token)
}

/// The Windows drive-letter encoded shape: `<letter>--…` (`C:\Users\x` → `C--Users-x`).
/// Distinct from every other token class: a Unix encoded dir leads with `-`, an id is
/// hex/uuid-shaped, and a real RELATIVE path named like this is disambiguated by the
/// caller (encoded-dir lookup first, real-path fallthrough on a miss).
pub(crate) fn is_drive_encoded_token(token: &str) -> bool {
    let b = token.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b'-' && b[2] == b'-'
}

/// If `target` is (or lives directly under) the projects root and names a single
/// encoded dir, return that basename token; else `None`. Handles both
/// `<encoded>` and `~/.claude/projects/<encoded>` forms.
pub(crate) fn strip_projects_root_prefix(target: &Path, root: &Path) -> Option<String> {
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
pub fn absolutize(p: &Path) -> Result<PathBuf> {
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
pub(crate) fn lexical_normalize(p: &Path) -> PathBuf {
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
