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

/// Whether a session-file resolution spans subagent transcripts, and how.
///
/// `search` / `agents` / `recover` / `turns` / `list` only ever need the two-state
/// include/exclude decision (built from a `--no-subagents` bool via `From<bool>`).
/// `files` additionally offers `--subagents-only`, the COMPLEMENT of `--no-subagents`:
/// dump ONLY the files a session's subagents touched, with the top-level session's own
/// `<uuid>.jsonl` excluded — previously reachable only by a two-run set-difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentScope {
    /// Top-level session jsonl(s) PLUS each one's subagent transcripts (the default).
    WithSubagents,
    /// Only the top-level `<uuid>.jsonl` session(s); no subagent transcripts.
    TopLevelOnly,
    /// Only the subagent transcripts; the top-level `<uuid>.jsonl` itself is excluded.
    SubagentsOnly,
}

impl From<bool> for SubagentScope {
    /// `true` (the historical `include_subagents`) ⇒ `WithSubagents`; `false` ⇒
    /// `TopLevelOnly`. There is no bool that maps to `SubagentsOnly` — that mode is
    /// only reachable by constructing the variant directly (the `files` surface).
    fn from(include_subagents: bool) -> Self {
        if include_subagents {
            SubagentScope::WithSubagents
        } else {
            SubagentScope::TopLevelOnly
        }
    }
}

/// Resolve `--path` targets (+ optional `--session`) into the concrete, sorted,
/// de-duplicated list of session `*.jsonl` files to operate on, with the subagent span
/// governed by `scope`.
///
/// This is the SINGLE shared target resolver for `search` / `agents` / `files` /
/// `recover` / `turns` (each previously carried a near-identical copy): 0 `paths` ⇒
/// every project under the projects root; a `--session` restricts to the parent session
/// whose jsonl basename matches. The subagent transcripts of a selected session (built-in
/// Task/Agent-tool + workflow / OMC agents under `subagents/**`) are gathered from the
/// already-`--session`-filtered top-level set; workflow `journal.jsonl` event logs are
/// never transcripts and are excluded (see [`crate::subagent::subagent_transcript_files`]).
/// Per [`SubagentScope`]:
/// - `WithSubagents` — top-level session(s) + their subagents (the default).
/// - `TopLevelOnly` — only the top-level `<uuid>.jsonl` session(s).
/// - `SubagentsOnly` — only the subagent transcripts (the top-level jsonl is dropped).
///
/// Bails (never returns an empty silent result) when a `--session` was given but no
/// matching file exists under the resolved target(s) — in `SubagentsOnly` this fires
/// when the session exists but spawned no subagents. With no `--session`, an empty
/// result is allowed (the caller renders an honest "nothing found").
pub fn resolve_session_files(
    paths: &[std::path::PathBuf],
    session: Option<&str>,
    scope: SubagentScope,
) -> Result<Vec<PathBuf>> {
    // A POSITIONAL target that is a bare SESSION UUID (not a project dir) is routed to the
    // session filter, so the documented `csift files <uuid>` / `recover <uuid>` / `turns
    // <uuid>` forms work as written (a uuid is NOT a filesystem path — encoding+looking it
    // up as a project dir is what used to error). Project-shaped targets stay project
    // targets; uuid-shaped ones join the --session set. The result selects sessions whose
    // basename matches ANY collected id.
    let mut session_ids: Vec<String> = Vec::new();
    if let Some(s) = session {
        session_ids.push(s.to_string());
    }
    let mut project_paths: Vec<&std::path::PathBuf> = Vec::new();
    for p in paths {
        match p.to_str() {
            // A session-id-shaped positional routes to the session filter. This can never
            // collide with a real project dir: an encoded projects-dir basename ALWAYS
            // starts with `-` (an absolute cwd's leading `/` encodes to `-`) and carries
            // `-` separators, whereas a uuid has no leading `-` and a bare-hex agent id has
            // no `-` at all — so `looks_like_session_id` and "is a project dir" are
            // mutually exclusive by construction.
            Some(s) if looks_like_session_id(s) => {
                session_ids.push(s.to_string());
            }
            _ => project_paths.push(p),
        }
    }

    let dirs: Vec<ProjectDir> = if project_paths.is_empty() {
        // No project target: scan every project (a uuid-only invocation searches all
        // projects for that session, exactly like `--session <uuid>` with no path).
        all_project_dirs()?
    } else {
        let mut d = Vec::with_capacity(project_paths.len());
        for p in project_paths {
            d.push(resolve_target(p)?);
        }
        d
    };

    // The top-level `<uuid>.jsonl` session files (after the optional session-id filter).
    // These ALWAYS drive subagent discovery — even in `SubagentsOnly`, where they are
    // not themselves emitted — so the session restriction still selects the right
    // parent whose subagents to dump.
    let mut top_level: Vec<PathBuf> = Vec::new();
    for pd in &dirs {
        let read = match std::fs::read_dir(&pd.dir) {
            Ok(r) => r,
            Err(_) => continue, // tolerate a vanished dir mid-scan
        };
        for entry in read.flatten() {
            let p = entry.path();
            let is_file = entry.file_type().map(|ft| ft.is_file()).unwrap_or(false);
            if is_file && p.extension().is_some_and(|e| e == "jsonl") {
                // The collected session id(s) restrict to those uuids (the jsonl basename).
                if !session_ids.is_empty() {
                    let stem = p.file_stem().and_then(|s| s.to_str());
                    if !stem.is_some_and(|st| session_ids.iter().any(|sid| sid == st)) {
                        continue;
                    }
                }
                top_level.push(p);
            }
        }
    }

    // Subagent transcripts of each selected top-level session (empty unless the scope
    // asks for them). The --session restriction already applied to the parent above.
    let mut sub_files: Vec<PathBuf> = Vec::new();
    if matches!(
        scope,
        SubagentScope::WithSubagents | SubagentScope::SubagentsOnly
    ) {
        for sf in &top_level {
            sub_files.extend(crate::subagent::subagent_transcript_files(sf)?);
        }
    }

    let mut files: Vec<PathBuf> = match scope {
        SubagentScope::TopLevelOnly => top_level,
        SubagentScope::SubagentsOnly => sub_files,
        SubagentScope::WithSubagents => {
            top_level.extend(sub_files);
            top_level
        }
    };

    files.sort();
    files.dedup();

    if files.is_empty() && !session_ids.is_empty() {
        let ids = session_ids.join(", ");
        // A bare-hex SUBAGENT id (17 hex, no dashes) never names a TOP-LEVEL jsonl — guide
        // the caller to the right surface instead of implying the session is gone.
        if session_ids.iter().any(|s| is_bare_subagent_hex(s)) {
            bail!(
                "no top-level session matched [{ids}]. If this is a SUBAGENT id from \
                 `csift agents`, inspect it with `csift agents --agent <id>`, or pass the \
                 PARENT session uuid with `--subagents-only` to scope its subagents."
            );
        }
        bail!("no session file found for session id [{ids}] under the resolved target(s)");
    }
    Ok(files)
}

/// True when `s` is shaped like a CC SESSION ID a caller might pass as a positional in
/// place of a project path: either a full `8-4-4-4-12` lowercase-hex UUID (a top-level
/// session jsonl basename) or a bare-hex agent id (a subagent transcript basename minus
/// `agent-`). Used to route such a positional to the session filter rather than encoding
/// it as a (non-existent) project directory.
#[must_use]
fn looks_like_session_id(s: &str) -> bool {
    is_uuid(s) || is_bare_subagent_hex(s)
}

/// True for a canonical `8-4-4-4-12` hex UUID (the top-level session jsonl basename).
fn is_uuid(s: &str) -> bool {
    let groups = [8usize, 4, 4, 4, 12];
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != groups.len() {
        return false;
    }
    parts
        .iter()
        .zip(groups)
        .all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// True for a bare-hex SUBAGENT id: a dash-less run of hex digits long enough to be an
/// agent id (≥12), never a short word. (`agents` prints these; `bare_agent_id` produces
/// them.) Used only to GUIDE the error, not to resolve — subagent transcripts are not
/// top-level jsonl basenames.
fn is_bare_subagent_hex(s: &str) -> bool {
    s.len() >= 12 && !s.contains('-') && s.bytes().all(|b| b.is_ascii_hexdigit())
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

    // ── Branch-completeness for the pure helpers (env-touching arms are covered by
    //    the tests/cli_integration.rs suite, which sets a child-scoped $HOME). ──

    #[test]
    fn absolutize_existing_path_canonicalizes() {
        // An existing dir → the `canonicalize` Ok arm.
        let tmp = std::env::temp_dir();
        let abs = absolutize(&tmp).expect("absolutize existing");
        assert!(abs.is_absolute());
    }

    #[test]
    fn absolutize_nonexistent_absolute_path_normalizes_lexically() {
        // A non-existent ABSOLUTE path → canonicalize fails → the `p.is_absolute()`
        // true arm + lexical_normalize (resolving the `..`).
        let abs = absolutize(Path::new("/no/such/csift/a/../b")).expect("absolutize");
        assert_eq!(abs, PathBuf::from("/no/such/csift/b"));
    }

    #[test]
    fn absolutize_nonexistent_relative_path_joins_cwd() {
        // A non-existent RELATIVE path → canonicalize fails → the `else` (join cwd)
        // arm. The result is absolute and ends with our unique segment.
        let rel = Path::new("csift-nonexistent-xyzzy-rel");
        let abs = absolutize(rel).expect("absolutize relative");
        assert!(abs.is_absolute());
        assert!(abs.ends_with("csift-nonexistent-xyzzy-rel"));
    }

    #[test]
    fn lexical_normalize_handles_curdir_and_parentdir_and_plain() {
        assert_eq!(
            lexical_normalize(Path::new("/a/./b/../c")),
            PathBuf::from("/a/c")
        );
        // A leading `.` (CurDir) is dropped; a plain component is pushed.
        assert_eq!(lexical_normalize(Path::new("a/b")), PathBuf::from("a/b"));
    }

    #[test]
    fn strip_prefix_under_root_with_token() {
        // The under-root branch where exactly one component remains AND it is a valid
        // encoded token.
        let root = Path::new("/home/u/.claude/projects");
        assert_eq!(
            strip_projects_root_prefix(Path::new("/home/u/.claude/projects/-Enc-Token"), root)
                .as_deref(),
            Some("-Enc-Token")
        );
        // Under root but the single component is NOT a valid encoded token (no leading
        // dash) → None (the `looks_like_encoded_token` false arm under-root).
        assert!(
            strip_projects_root_prefix(Path::new("/home/u/.claude/projects/plain"), root).is_none()
        );
    }

    #[test]
    fn strip_prefix_under_root_empty_remainder_is_none() {
        // `target == root` exactly → zero components remain → not a single-token match.
        let root = Path::new("/home/u/.claude/projects");
        assert!(strip_projects_root_prefix(root, root).is_none());
    }

    #[test]
    fn looks_like_encoded_token_empty_is_false() {
        // An empty string has no leading `-` → false (the `chars.next()` None arm).
        assert!(!looks_like_encoded_token(""));
    }

    #[test]
    fn resolve_target_real_path_that_does_not_resolve_errors() {
        // A real absolute path whose encoded dir does not exist under the projects
        // root → the final `bail!` arm (step 4). Uses a path guaranteed absent.
        let err = resolve_target(Path::new(
            "/Users/csift-definitely-no-such-project-9999/zzz",
        ))
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("no Claude Code project dir"),
            "expected step-4 bail, got: {msg}"
        );
    }

    #[test]
    fn resolve_target_encoded_token_not_a_dir_errors() {
        // A leading-`-` token that is NOT an existing dir under the root → the
        // token-branch bail (does not fall through to path-encoding).
        let err = resolve_target(Path::new("-csift-no-such-encoded-token-zzzz")).unwrap_err();
        assert!(
            err.to_string().contains("no Claude Code project dir named"),
            "expected token bail, got: {err}"
        );
    }

    // ── Bare-uuid positional routing (so `csift files <uuid>` works as documented) ──

    #[test]
    fn is_uuid_recognizes_canonical_form() {
        assert!(is_uuid("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"));
        assert!(is_uuid("00000000-0000-0000-0000-000000000000"));
        // Wrong group lengths, non-hex, missing dashes, real paths → not a uuid.
        assert!(!is_uuid("0a1b2c3d-4e5f-4a6b-8c7d"));
        assert!(!is_uuid("zzzzzzzz-4e5f-4a6b-8c7d-9e0f1a2b3c4d"));
        assert!(!is_uuid("-Users-testuser-Projects-foo"));
        assert!(!is_uuid("/Users/testuser/Projects/foo"));
        assert!(!is_uuid("."));
    }

    #[test]
    fn is_bare_subagent_hex_recognizes_agent_ids() {
        assert!(is_bare_subagent_hex("ae24045bd6d4bdaff"));
        assert!(is_bare_subagent_hex("a585e25a580c59e7a"));
        // Too short, dashed (uuid), or a word → not a bare subagent hex.
        assert!(!is_bare_subagent_hex("abc123")); // < 12
        assert!(!is_bare_subagent_hex(
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
        ));
        assert!(!is_bare_subagent_hex("plain-token"));
    }

    #[test]
    fn looks_like_session_id_covers_uuid_and_bare_hex() {
        assert!(looks_like_session_id(
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
        ));
        assert!(looks_like_session_id("ae24045bd6d4bdaff"));
        assert!(!looks_like_session_id("-Users-testuser-Projects-foo"));
        assert!(!looks_like_session_id("."));
    }
}
