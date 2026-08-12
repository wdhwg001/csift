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

/// The user's home directory, resolved the way Claude Code itself resolves it (Node's
/// `os.homedir()`): `$HOME` on Unix, `%USERPROFILE%` on Windows. The per-platform split is
/// load-bearing on Windows — CC never consults `HOME` there, but Git-Bash/MSYS shells
/// export one (often a POSIX-style `/c/Users/...` a native process cannot use), and
/// honoring it would point csift at a `.claude` dir CC never writes. The conventional env
/// var is read first so a test harness can relocate home per-subprocess; `std::env::home_dir`
/// (un-deprecated, Windows-correct since Rust 1.85 — MSRV is above both) is the fallback.
fn home_dir() -> Result<PathBuf> {
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
static CLAUDE_HOME_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

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
fn resolve_claude_home(
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
            // An EXPLICIT encoded-dir token: the user named the dir, so don't cwd-filter
            // (a collision is the user's chosen scope here).
            return Ok(ProjectDir {
                dir,
                target_cwd: None,
            });
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
        return Ok(ProjectDir {
            dir,
            target_cwd: Some(abs),
        });
    }

    // §2.3 step 3: LONG-PATH fallback. Claude Code caps the encoded dir name at
    // MAX_SANITIZED_LENGTH (200) chars and appends a hash suffix for anything longer —
    // `<first-200>-<hash>` — so the full encoding above does not exist on disk for a
    // deeply-nested project. The suffix is NOT reconstructible (the CLI uses Bun.hash,
    // the SDK djb2 — different digests for the same path), which is why CC's own
    // `findProjectDir` PREFIX-SCANS rather than recomputing it. We mirror that exactly.
    if encoded.len() > MAX_SANITIZED_LENGTH {
        let prefix = format!("{}-", &encoded[..MAX_SANITIZED_LENGTH]);
        if let Some(found) = find_dir_by_prefix(&root, &prefix, &abs)? {
            return Ok(ProjectDir {
                dir: found,
                target_cwd: Some(abs.clone()),
            });
        }
    }

    // §2.3 step 4: neither resolved — surface the attempted path, no empty result.
    bail!(
        "no Claude Code project dir for {} (looked for {})",
        abs.display(),
        dir.display()
    )
}

/// Claude Code's `MAX_SANITIZED_LENGTH`: a project's encoded dir-name is capped at 200
/// chars; a longer cwd is stored as `<first-200>-<hash>` (§2.1). Matches the cleanroom
/// `sanitizePath` + the shipping binary (`Siq`, verified 2026-06-16).
const MAX_SANITIZED_LENGTH: usize = 200;

/// Resolve a >200-char encoded path to its on-disk dir by prefix-scanning the projects
/// root for `<first-200>-<hash>` (the hash is not reconstructible — see [`resolve_target`]).
/// Among multiple matches (two paths identical for the first 200 encoded chars — vanishingly
/// rare), prefer the dir whose first session's recorded `cwd` equals the target; otherwise
/// fall back to the sole / first match. Returns `None` when nothing matches.
fn find_dir_by_prefix(root: &Path, prefix: &str, abs: &Path) -> Result<Option<PathBuf>> {
    let read = match std::fs::read_dir(root) {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };
    let mut matches: Vec<PathBuf> = Vec::new();
    for entry in read.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(prefix) && entry.path().is_dir() {
            matches.push(entry.path());
        }
    }
    if matches.len() > 1 {
        let want = abs.to_string_lossy();
        if let Some(exact) = matches
            .iter()
            .find(|d| dir_first_cwd(d).as_deref() == Some(want.as_ref()))
        {
            return Ok(Some(exact.clone()));
        }
    }
    matches.sort();
    Ok(matches.into_iter().next())
}

/// Cheaply read the `cwd` of ONE session file from its first record — a BOUNDED head read
/// (≤64 KiB; the `cwd` field sits in the first record's first ~200 bytes), so a first line
/// that is huge (an image record on line 1) never forces a full-line load. No full JSON
/// parse — mirrors CC's `extractJsonStringField`.
fn read_first_cwd(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(64 * 1024)
        .read_to_end(&mut buf)
        .ok()?;
    let head = String::from_utf8_lossy(&buf);
    // Only the FIRST line's cwd (don't pick up a later record's if the head spans a newline).
    let first_line = head.split('\n').next().unwrap_or(&head);
    extract_json_string_field(first_line, "cwd")
}

/// The `cwd` string of a project dir's first session file (any one). Used to disambiguate a
/// 200-char-prefix collision among long-path candidates.
fn dir_first_cwd(dir: &Path) -> Option<String> {
    let read = std::fs::read_dir(dir).ok()?;
    for entry in read.flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "jsonl") {
            if let Some(cwd) = read_first_cwd(&p) {
                return Some(cwd);
            }
        }
    }
    None
}

/// Whether a stored `cwd` string denotes the same directory as `want` (the canonical
/// target). Trailing-slash tolerant. Exact for the ASCII paths that dominate; a non-ASCII
/// NFC-vs-NFD mismatch is the one accepted edge (CC stores realpath+NFC, csift realpath only).
fn cwd_equivalent(stored: &str, want: &Path) -> bool {
    let norm = |s: &str| s.trim_end_matches('/').to_string();
    norm(stored) == norm(&want.to_string_lossy())
}

/// Extract a simple `"key":"value"` JSON string field from raw text without a full parse
/// (escape-aware; stops at the first unescaped `"`). Mirrors CC's `extractJsonStringField`.
fn extract_json_string_field(text: &str, key: &str) -> Option<String> {
    for pat in [format!("\"{key}\":\""), format!("\"{key}\": \"")] {
        let Some(idx) = text.find(&pat) else { continue };
        let bytes = text.as_bytes();
        let start = idx + pat.len();
        let mut i = start;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'"' => return Some(text[start..i].replace("\\\\", "\\").replace("\\\"", "\"")),
                _ => i += 1,
            }
        }
    }
    None
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
            dirs.push(ProjectDir {
                dir: path,
                target_cwd: None,
            });
        }
    }
    dirs.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(dirs)
}

/// Whether a session-file resolution spans subagent transcripts.
///
/// Every span-aware subcommand (`search` / `agents` / `recover` / `turns` / `list` / `files` /
/// `image` / `plan`) needs only the two-state include/exclude decision, built from a
/// `--no-subagents` bool via `From<bool>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentScope {
    /// Top-level session jsonl(s) PLUS each one's subagent transcripts (the default).
    WithSubagents,
    /// Only the top-level `<uuid>.jsonl` session(s); no subagent transcripts.
    TopLevelOnly,
}

/// Which subcommand is resolving session files — threaded into [`resolve_session_files`] so a
/// future subcommand-aware remediation message can branch on the caller. Inert today (the body
/// does not read it), kept for that extension point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    /// `csift files`.
    Files,
    /// Any other span-aware subcommand (`search` / `agents` / `recover` / `turns` / `list` /
    /// `image` / `plan`).
    Other,
}

impl From<bool> for SubagentScope {
    /// `true` (subagents spanned) ⇒ `WithSubagents`; `false` (`--no-subagents`) ⇒
    /// `TopLevelOnly`.
    fn from(include_subagents: bool) -> Self {
        if include_subagents {
            SubagentScope::WithSubagents
        } else {
            SubagentScope::TopLevelOnly
        }
    }
}

/// Resolve the CALLING session id from the environment — the value of `CLAUDE_CODE_SESSION_ID`,
/// which CC sets to the process-global MAIN session id even inside a subagent (verified
/// empirically + against the cleanroom; an in-process subagent's OWN id is NOT exported to the
/// subprocess env). Used by `@main` and as the `@trap:` search root. There is no env-based
/// `@self` because CC withholds the per-subagent id from the Bash env — `@trap:<marker>`
/// recovers it from the transcript instead.
pub fn resolve_env_session() -> Result<String> {
    let read = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    read("CLAUDE_CODE_SESSION_ID")
        .or_else(|| read("CODEX_COMPANION_SESSION_ID"))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "needs the calling session id, but CLAUDE_CODE_SESSION_ID is not set (running \
                 outside Claude Code, or an old build). Pass an explicit `@<uuid>` target instead."
            )
        })
}

/// The identity `@trap:<marker>` resolved to: a specific subagent (its bare hex), or the
/// main-thread session itself (when the marker was issued from the top-level conversation).
enum TrapSelf {
    Agent(String),
    Session(String),
}

/// Resolve `@trap:<marker>` to the CALLING agent/session by finding the transcript whose Bash
/// `tool_use` command carries the (unique, literal) marker AND the literal `csift` — i.e. the
/// very command that launched this run. Mechanism + TIMING (both verified live 2026-07-12): a
/// SUBAGENT's transcript records the launching tool_use eagerly — its own Bash can grep the
/// marker mid-execution, so a first try resolves; the MAIN conversation's record is flushed only
/// AFTER the current Bash call completes (a 3s in-command sleep still saw 0 on disk), so a
/// top-level FIRST use ALWAYS misses and only a re-run of the SAME marker (now in the previous,
/// flushed record) resolves. @trap therefore earns its keep for subagents (which cannot name
/// themselves); a main thread should prefer `@main` (env-based, no race) — the no-match error
/// routes both. It searches the calling session (`CLAUDE_CODE_SESSION_ID`) + its subagent
/// transcripts; a subagent match → that agent (then its subtree, per scope); only the main
/// transcript → the session. The marker grammar is enforced strictly (see
/// [`validate_trap_marker`]) precisely so the discipline cannot be shortcut: it must be a fresh,
/// one-shot, imaginative token the model invents on the spot — never script-generated (a
/// generator would itself be a `csift`-ish Bash call carrying the marker → ambiguity). Errors on
/// a malformed marker, no match (marker not literal / mistyped / not yet flushed, or the command
/// did not actually run `csift`), or >1 subagent (use a fresher random marker).
fn resolve_trap(marker: &str) -> Result<TrapSelf> {
    let marker = marker.trim();
    validate_trap_marker(marker)?;
    let session_id = resolve_env_session()?;
    let main_jsonl = locate_session_jsonl(&session_id).ok_or_else(|| {
        anyhow!(
            "@trap: cannot locate the calling session {session_id}.jsonl under the projects root"
        )
    })?;

    let main_hit = bash_command_carries_trap(&main_jsonl, marker);
    let mut subagent_hits: Vec<String> = Vec::new();
    for sub in crate::subagent::subagent_transcript_files(&main_jsonl)? {
        if bash_command_carries_trap(&sub, marker) {
            subagent_hits.push(crate::subagent::session_id_from_path(&sub));
        }
    }

    match subagent_hits.len() {
        1 => Ok(TrapSelf::Agent(subagent_hits.remove(0))),
        0 if main_hit => Ok(TrapSelf::Session(session_id)),
        0 => bail!(
            "@trap: marker `{marker}` not found in a `csift` Bash command of the calling session. \
             It must appear LITERALLY in THIS csift invocation (no shell variable / concatenation, \
             and the command must actually run `csift`). TIMING: a SUBAGENT's transcript already \
             carries its command mid-run (a first try resolves), but the MAIN conversation's own \
             record is only flushed AFTER the current command finishes — a top-level FIRST use \
             always misses. If you are the top-level thread: use `@main` (env-based, no race), or \
             re-run this EXACT command with the SAME marker as a NEW, SEPARATE Bash invocation — \
             a second attempt inside the SAME shell script does NOT count (the whole script is ONE \
             still-in-flight command; nothing flushes until it exits), and a fresh marker restarts \
             the race and misses again."
        ),
        n => bail!(
            "@trap: marker `{marker}` is AMBIGUOUS — it matched {n} subagents. Use a fresher, \
             more random marker (it must be unique within the conversation)."
        ),
    }
}

/// One node in the `whoami @trap` UPSTREAM ancestry chain — a subagent (or the top-level session)
/// the caller belongs to. The chain runs SELF → ancestors → top-level root, so a subagent learns
/// its own bare hex AND the whole re-feedable session lineage above it: `agents` walks the topology
/// DOWN, `whoami` walks it UP.
pub struct WhoNode {
    /// Bare-hex agent id for a subagent node, or the uuid for the top-level session.
    pub session_id: String,
    pub is_subagent: bool,
    /// The always-re-feedable owning top-level uuid (== `session_id` on the root node).
    pub parent_session_id: String,
    /// tool_use-graph nesting depth for a subagent (0 = a direct child of the session); `None` for
    /// the top-level root.
    pub depth: Option<usize>,
    pub path: Option<PathBuf>,
}

/// Resolve `@trap:<marker>` for `whoami` into the caller's UPSTREAM ancestry chain: the marker
/// carrier FIRST (a subagent, or the top-level session itself), then each parent walked via the
/// topology's `parent_agent_id`, ending at the top-level session. Reuses [`resolve_trap`]'s strict
/// grammar + marker scan and [`crate::subagent::build_topology`] for the walk. Env-independent —
/// reliable for a built-in Task AND a workflow subagent (whose env id is the PARENT, not itself).
/// The topology is flat today (every subagent is depth 0), so the chain is `subagent → top-level`;
/// the walk is future-proof for real nesting (depth > 0).
pub fn resolve_trap_who(marker: &str) -> Result<Vec<WhoNode>> {
    let root = resolve_env_session()?;
    let main_jsonl = locate_session_jsonl(&root);

    match resolve_trap(marker)? {
        // The marker was in the MAIN transcript → the caller IS the top-level session (no ancestry).
        TrapSelf::Session(session_id) => {
            let path = locate_session_jsonl(&session_id);
            Ok(vec![WhoNode {
                parent_session_id: session_id.clone(),
                session_id,
                is_subagent: false,
                depth: None,
                path,
            }])
        }
        // A subagent carried it → walk UP from that hex to the top-level session.
        TrapSelf::Agent(agent_id) => {
            let nodes = main_jsonl
                .as_deref()
                .and_then(|m| crate::subagent::build_topology(m, false).ok())
                .unwrap_or_default();
            let sub_files = main_jsonl
                .as_deref()
                .and_then(|m| crate::subagent::subagent_transcript_files(m).ok())
                .unwrap_or_default();
            let locate_sub = |hex: &str| {
                sub_files
                    .iter()
                    .find(|p| crate::subagent::session_id_from_path(p) == hex)
                    .cloned()
            };

            let mut chain: Vec<WhoNode> = Vec::new();
            let mut cur = Some(agent_id);
            // Walk `parent_agent_id` up; the dedup guard makes any malformed cycle terminate.
            while let Some(hex) = cur {
                if chain.iter().any(|n| n.session_id == hex) {
                    break;
                }
                let node = nodes.iter().find(|n| n.agent_id == hex);
                let depth = node.map(|n| n.depth);
                let next = node.and_then(|n| n.parent_agent_id.clone());
                let path = locate_sub(&hex);
                chain.push(WhoNode {
                    session_id: hex,
                    is_subagent: true,
                    parent_session_id: root.clone(),
                    depth,
                    path,
                });
                cur = next;
            }
            // Append the top-level session as the chain root.
            chain.push(WhoNode {
                parent_session_id: root.clone(),
                session_id: root,
                is_subagent: false,
                depth: None,
                path: main_jsonl,
            });
            Ok(chain)
        }
    }
}

/// Markers that appear as the DOCUMENTED EXAMPLE in the SKILL / `--help` / SPEC. Because the doc
/// text prints them right next to the literal `csift`, any command that quotes or greps that doc
/// satisfies the marker-AND-`csift` scan; worse, every agent that lazily copies the example uses
/// the SAME token, so it can never resolve to ONE transcript (it self-collides into ambiguity).
/// They are RESERVED — [`validate_trap_marker`] always refuses them, forcing a fresh hand-invented
/// marker. Keep this in lockstep with the example literal shown in every doc.
const RESERVED_EXAMPLE_MARKERS: &[&str] = &["JollyShinyBrook4283"];

/// Enforce the STRICT `@trap` marker grammar, rejecting every lazy shortcut at the source so the
/// only way to satisfy it is to invent a fresh, imaginative token by hand. The marker must be
/// EXACTLY 3 CamelCase words (each an uppercase letter + at least two lowercase letters — no single
/// letters, no ALLCAPS acronyms like `HTML` / `USB`) followed by EXACTLY four digits, and those four
/// digits must NOT form a trivial run (all-equal / consecutive / simple odd / simple even — e.g.
/// `0000` / `1234` / `9876` / `1357` / `2468`). The grammar-valid SHAPE looks like
/// `JollyShinyBrook4283`, but that exact literal is the RESERVED doc example (see
/// `RESERVED_EXAMPLE_MARKERS`) and is always refused, so nobody ships a copy-pasted token. The
/// strictness IS
/// the point: it makes a hand-invented, imaginative, CONTEXT-INDEPENDENT literary token the path of
/// least resistance and a scripted or boilerplate token fail loudly.
fn validate_trap_marker(marker: &str) -> Result<()> {
    let guidance = "@trap needs a marker you INVENT one-shot, right now, by hand: EXACTLY 3 \
                    imaginative, CONTEXT-INDEPENDENT CamelCase words (each 1 uppercase + >=2 \
                    lowercase) + 4 non-trivial digits. The shape looks like `JollyShinyBrook4283`, \
                    but that literal is the RESERVED doc example and is itself refused — pick your \
                    OWN. Put it VERBATIM in this csift command (no shell variable / concatenation), \
                    and never generate it with a script. Rejected: not exactly 3 words, \
                    single-letter or ALLCAPS \"words\", missing or !=4 trailing digits, trivial \
                    digits (1111 / 1234 / 9876 / 1357 / 2468 ...), or the reserved doc example.";
    if marker.is_empty() {
        bail!("{guidance}");
    }
    if RESERVED_EXAMPLE_MARKERS.contains(&marker) {
        bail!(
            "@trap: marker `{marker}` is the RESERVED documentation example — the SKILL / --help / \
             SPEC print it next to `csift`, so quoting the doc self-matches it and every agent that \
             copies it clashes into ambiguity. csift always refuses it. {guidance}"
        );
    }
    if !marker.is_ascii() || marker.len() < 13 {
        bail!("@trap: marker `{marker}` is malformed. {guidance}");
    }
    let (words, digits) = marker.split_at(marker.len() - 4);
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        bail!("@trap: marker `{marker}` must END with exactly 4 digits. {guidance}");
    }
    match camel_words(words) {
        Some(w) if w.len() == 3 => {}
        _ => bail!(
            "@trap: marker `{marker}` must be EXACTLY 3 CamelCase words (each: 1 uppercase + >=2 \
             lowercase) before the 4 digits. {guidance}"
        ),
    }
    if is_trivial_4_digits(digits) {
        bail!(
            "@trap: the 4 digits `{digits}` are a trivial run — pick non-sequential random \
             digits. {guidance}"
        );
    }
    Ok(())
}

/// Split a CamelCase run into its words, requiring each to be one uppercase letter followed by
/// `>=2` lowercase letters; returns `None` the moment that shape is violated (a digit, a
/// lone/2-char "word", an ALLCAPS acronym, or any non-ASCII-letter). Used only by
/// [`validate_trap_marker`].
fn camel_words(s: &str) -> Option<Vec<&str>> {
    let b = s.as_bytes();
    let mut words: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_uppercase() {
            return None;
        }
        let start = i;
        i += 1;
        let mut lower = 0;
        while i < b.len() && b[i].is_ascii_lowercase() {
            i += 1;
            lower += 1;
        }
        if lower < 2 {
            return None;
        }
        words.push(&s[start..i]);
    }
    (!words.is_empty()).then_some(words)
}

/// True when four ASCII digits form a trivial arithmetic run — a constant step in `-2..=2`
/// (all-equal `0` / consecutive `±1` / simple odd-or-even `±2`), e.g. `0000` `1234` `9876` `1357`
/// `2468`. Such markers are too guessable / boilerplate to make a unique trap. Caller guarantees
/// exactly four ASCII digits.
fn is_trivial_4_digits(d: &str) -> bool {
    let v: Vec<i32> = d.bytes().map(|b| i32::from(b - b'0')).collect();
    let s1 = v[1] - v[0];
    let s2 = v[2] - v[1];
    let s3 = v[3] - v[2];
    s1 == s2 && s2 == s3 && (-2..=2).contains(&s1)
}

/// Locate a session's top-level `<id>.jsonl` under the projects root: try the cwd-encoded dir
/// first, then scan every project dir. `None` if absent. (Mirrors `whoami`'s locate logic.)
fn locate_session_jsonl(id: &str) -> Option<PathBuf> {
    let root = projects_root().ok()?;
    let fname = format!("{id}.jsonl");
    if let Ok(cwd) = std::env::current_dir() {
        let c = root.join(encode_cwd(&cwd)).join(&fname);
        if c.is_file() {
            return Some(c);
        }
    }
    for pd in all_project_dirs().ok()? {
        let c = pd.dir.join(&fname);
        if c.is_file() {
            return Some(c);
        }
    }
    None
}

/// True when `path`'s transcript contains a Bash `tool_use` whose `input.command` includes BOTH
/// the `marker` AND the literal `csift` — i.e. the actual csift invocation that embedded the
/// trap, not some unrelated command that merely echoed the token. A byte prefilter (`memmem` on
/// the rare marker) skips a transcript that never mentions it without parsing — so a giant main
/// transcript is mmap-scanned, not deserialized, unless the (unique) marker is present. Matching
/// the Bash tool_use INPUT (not anywhere) avoids a false hit on a tool_result that echoed it.
fn bash_command_carries_trap(path: &Path, marker: &str) -> bool {
    let Ok(Some(mmap)) = crate::parse::mmap_bytes(path) else {
        return false;
    };
    let bytes: &[u8] = &mmap;
    if memchr::memmem::find(bytes, marker.as_bytes()).is_none() {
        return false;
    }
    let mut found = false;
    let _ = crate::parse::scan_lines_bytes(bytes, |line| {
        if found {
            return;
        }
        if let Ok(Some(rec)) = crate::parse::parse_line(line) {
            if let Some(blocks) = rec.blocks() {
                for b in blocks {
                    if let crate::model::Block::ToolUse {
                        name: Some(n),
                        input: Some(inp),
                        ..
                    } = b
                    {
                        if n == "Bash"
                            && inp
                                .get("command")
                                .and_then(serde_json::Value::as_str)
                                .is_some_and(|c| c.contains(marker) && c.contains("csift"))
                        {
                            found = true;
                        }
                    }
                }
            }
        }
    });
    found
}

/// Resolve a `*.jsonl` session-file TARGET to `(project_dir, session_id)`. A top-level
/// `<enc>/<uuid>.jsonl` → its parent dir + the uuid stem. A SUBAGENT transcript
/// `<enc>/<uuid>/subagents/[…/]agent-<hex>.jsonl` → the PARENT session (`<enc>/<uuid>`) so the
/// whole conversation is in scope (the agent is reached via subagent expansion). Errors if the
/// file does not exist (never fabricates a target).
pub fn session_file_target(file: &Path) -> Result<(ProjectDir, String)> {
    if !file.is_file() {
        bail!(
            "no session transcript at {} (a `*.jsonl` target must be an existing session file)",
            file.display()
        );
    }
    // A subagent transcript: the parent session dir is the component before `subagents`.
    if file
        .components()
        .any(|c| c.as_os_str().to_str() == Some("subagents"))
    {
        if let Some(parent_uuid) = crate::subagent::parent_session_id_from_path(file) {
            // Walk up to the `<uuid>` session dir (the one whose name is parent_uuid), whose
            // PARENT is the encoded project dir.
            let mut cur = file;
            while let Some(p) = cur.parent() {
                if p.file_name().and_then(|n| n.to_str()) == Some(parent_uuid.as_str()) {
                    if let Some(proj) = p.parent() {
                        return Ok((
                            ProjectDir {
                                dir: proj.to_path_buf(),
                                target_cwd: None,
                            },
                            parent_uuid,
                        ));
                    }
                }
                cur = p;
            }
        }
    }
    // A top-level session jsonl: parent dir is the encoded project dir; stem is the session id.
    let dir = file
        .parent()
        .ok_or_else(|| anyhow::anyhow!("session file {} has no parent dir", file.display()))?
        .to_path_buf();
    let sid = file
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("session file {} has no stem", file.display()))?
        .to_string();
    Ok((
        ProjectDir {
            dir,
            target_cwd: None,
        },
        sid,
    ))
}

/// Read a `--sessions-from` id list (a file path, or `-` for stdin) and append each id onto
/// `paths` as an `@<id>` target token, so the shared resolver treats the list EXACTLY like
/// positional `@` targets (same pin logic, same fail-loud misses). Tokens are whitespace /
/// newline separated; each must be a session uuid, a 4-11-hex uuid prefix, or an agent id —
/// the ids csift itself emits (`search -l`, the JSON summary's `transcript_ids`, any row's
/// `parent_session_id`). A leading `@` is tolerated: ids are DATA (csift's own outputs are
/// bare, a hand-built list may quote them `@`-style), so both spellings of the same id work.
/// Any other token is a hard error naming it. Empty input appends nothing.
pub fn extend_with_session_list(paths: &mut Vec<PathBuf>, src: &Path) -> Result<()> {
    let data = if src.as_os_str() == "-" {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)
            .context("--sessions-from: reading stdin")?;
        s
    } else {
        std::fs::read_to_string(src)
            .with_context(|| format!("--sessions-from: reading {}", src.display()))?
    };
    for tok in data.split_whitespace() {
        let id = tok.strip_prefix('@').unwrap_or(tok);
        if is_uuid(id) || is_uuid_prefix(id) || is_subagent_id(id) {
            paths.push(PathBuf::from(format!("@{id}")));
        } else {
            bail!(
                "--sessions-from: {tok:?} is not a session id (want a uuid, a 4-11-hex uuid \
                 prefix, or an agent id — the ids csift emits)"
            );
        }
    }
    Ok(())
}

/// Shared target assembly for the multi-target commands: positional `paths` ∪ the
/// `--sessions-from` id list, resolved through [`resolve_session_files`]. An EXPLICITLY
/// given but EMPTY id list with no positional targets resolves to an EMPTY scope — the
/// honest-empty a pipeline stage that found nothing should propagate — never a silent
/// widening to every project (0 targets ⇒ ALL is the rule for a BARE invocation only).
pub fn resolve_targets_with_session_list(
    positionals: &[PathBuf],
    sessions_from: Option<&Path>,
    scope: SubagentScope,
    caller: Caller,
) -> Result<Vec<PathBuf>> {
    let mut paths = positionals.to_vec();
    if let Some(src) = sessions_from {
        extend_with_session_list(&mut paths, src)?;
        if paths.is_empty() {
            return Ok(Vec::new());
        }
    }
    resolve_session_files(&paths, scope, caller)
}

/// Resolve positional targets into the concrete, sorted, de-duplicated list of session
/// `*.jsonl` files to operate on, with the subagent span governed by `scope`.
///
/// This is the SINGLE shared target resolver for `list` / `search` / `agents` / `files` /
/// `recover` / `turns` / `image`. There is NO `--session` flag — a session is targeted by a
/// positional `@<uuid>` / `@<agent-hex>` / `@main` / `@trap:<marker>` token or a `*.jsonl` file; a real
/// path / encoded-dir token / `~/.claude/projects/<enc>` scopes to project dir(s). 0 `paths` ⇒
/// every project under the projects root. The subagent transcripts of a selected session
/// (built-in Task/Agent-tool + workflow / OMC agents under `subagents/**`) are gathered from
/// the already-id-filtered top-level set; workflow `journal.jsonl` event logs are never
/// transcripts and are excluded (see [`crate::subagent::subagent_transcript_files`]). Per
/// [`SubagentScope`]:
/// - `WithSubagents` — top-level session(s) + their subagents (the default).
/// - `TopLevelOnly` — only the top-level `<uuid>.jsonl` session(s).
///
/// Bails (never returns an empty silent result) when a session id was pinned but no matching
/// file exists under the resolved target(s). With no id pin, an empty result is allowed (the
/// caller renders an honest "nothing found").
pub fn resolve_session_files(
    paths: &[std::path::PathBuf],
    scope: SubagentScope,
    caller: Caller,
) -> Result<Vec<PathBuf>> {
    // Target grammar (no `--session` flag — a session is an `@<uuid>` / `@main` / `@trap:<marker>`
    // positional, or a `*.jsonl` file). A token is one of: an `@`-prefixed IDENTIFIER (env /
    // session-id / encoded-dir), a `*.jsonl` session file, or a PATH (real cwd, encoded-dir
    // token, or `~/.claude/projects/<enc>`). A BARE uuid is NOT special — it falls to the path
    // branch and fails as "no project dir named <uuid>" (forced-unique: a folder literally named
    // like a uuid would otherwise be ambiguous). Result selects sessions whose basename matches
    // ANY collected id.
    let mut session_ids: Vec<String> = Vec::new();
    // Session-UUID PREFIXES (`@13d9645a` — the leading hex of a uuid, e.g. its first segment):
    // resolved by prefix-match against the enumerated sessions, UNIQUE or an ambiguity error.
    let mut session_prefixes: Vec<String> = Vec::new();
    // AGENT targets (`@<agent-hex>` / `@trap:<marker>`→agent / a subagent `*.jsonl`): each resolves
    // to that subagent + (unless `--no-subagents`) its TOPOLOGICAL descendants.
    let mut agent_hexes: Vec<String> = Vec::new();
    let mut project_paths: Vec<&std::path::PathBuf> = Vec::new();
    // Dirs resolved DIRECTLY from a token (an `@<encoded>` id or a `*.jsonl` file) — kept
    // apart from `project_paths` so they don't trigger the all-projects scan.
    let mut explicit_dirs: Vec<ProjectDir> = Vec::new();
    // True once any SESSION/PROJECT target is seen, so the all-session enumeration runs. An
    // AGENT-ONLY invocation (e.g. just `@<agent-hex>`) leaves it false → only the agent path runs.
    let mut session_target = false;
    for p in paths {
        let t = p.to_str().unwrap_or_default();
        // `@`-prefixed IDENTIFIER tokens (never a path): `@main` (env), `@trap:<marker>` (find
        // the CALLER's transcript by a unique literal marker it embedded in this command),
        // `@<uuid>`, `@<agent-hex>` (the agent subtree), `@<uuid-prefix>` (leading hex → unique
        // session), `@<encoded-dir>` (a project dir by its encoded name).
        if let Some(id) = t.strip_prefix('@') {
            match id {
                "main" => {
                    session_ids.push(resolve_env_session()?);
                    session_target = true;
                }
                // `@trap:<marker>` — the SELF identifier. The caller (an in-process subagent
                // whose own id CC withholds from the env) puts a unique, LITERAL marker in this
                // very command; csift finds the transcript whose Bash tool_use carries it. A
                // subagent match → that agent's subtree; a main-thread match → the session.
                _ if id.starts_with("trap:") => {
                    match resolve_trap(id.strip_prefix("trap:").unwrap_or(""))? {
                        TrapSelf::Agent(hex) => agent_hexes.push(hex),
                        TrapSelf::Session(sid) => {
                            session_ids.push(sid);
                            session_target = true;
                        }
                    }
                }
                _ if is_uuid(id) => {
                    session_ids.push(id.to_string());
                    session_target = true;
                }
                _ if is_subagent_id(id) => agent_hexes.push(id.to_string()),
                // A short dashless hex run (4..=11) is a uuid PREFIX (the first segment is 8),
                // never a full uuid (32+dashes) or an agent hex (≥12) — resolve it uniquely.
                _ if is_uuid_prefix(id) => {
                    session_prefixes.push(id.to_string());
                    session_target = true;
                }
                // `@-Users-…` → an encoded project-dir name (encoded cwds ALWAYS lead with
                // `-`, since the absolute path's `/` encodes to `-`).
                _ if id.starts_with('-') => {
                    explicit_dirs.push(resolve_target(Path::new(id))?);
                    session_target = true;
                }
                // Any other `@`-token is an UNRECOGNIZED id shape: fail loud naming the
                // @-grammar. It must NEVER fall through to path resolution — a stripped
                // `@a` used to become the cwd-relative path `a` and report a misleading
                // "no Claude Code project dir", sending the caller down a filesystem
                // debugging trail for what is an ID typo (the one spot the fail-loud
                // targeting law was silently violated).
                _ => {
                    let hexish = !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit());
                    if hexish && id.len() < 4 {
                        bail!(
                            "`@{id}` is too short for a session-uuid prefix — a prefix \
                             needs 4-11 leading (dashless) hex chars; add more characters \
                             (`csift list` shows the full uuids)"
                        );
                    }
                    bail!(
                        "`@{id}` is not a recognized @-target — expected `@<uuid>` | \
                         `@<uuid-prefix>` (4-11 dashless leading hex) | `@<agent-id>` \
                         (what `csift agents` prints) | `@main` | `@trap:<marker>` | \
                         `@-Users-…` (an encoded project dir). A project PATH is targeted \
                         without `@`."
                    );
                }
            }
            continue;
        }
        // A `*.jsonl` transcript target. A SUBAGENT transcript → that agent (+ its subtree); a
        // top-level `<uuid>.jsonl` → that session. Either way its project dir scopes the search.
        if t.ends_with(".jsonl") {
            let file = Path::new(t);
            // An elicitation SIDECAR (hook-written backfill, csift-elicitation marker records
            // only) is not a Claude Code transcript — it is read AUTOMATICALLY when you target
            // its session and cannot be searched directly. Reject it loudly so a stray target is
            // never silently scanned as a session (the merge is the only supported access).
            if crate::elicitation::is_sidecar_path(file) {
                bail!(
                    "{} is a csift elicitation sidecar (hook-written backfill, \
                     csift-elicitation marker records only), not a Claude Code session \
                     transcript. It is read automatically when you target its session; it \
                     cannot be searched directly.",
                    file.display()
                );
            }
            let is_sub = file
                .components()
                .any(|c| c.as_os_str().to_str() == Some("subagents"));
            let (dir, sid) = session_file_target(file)?;
            explicit_dirs.push(dir);
            if is_sub {
                agent_hexes.push(crate::subagent::session_id_from_path(file));
            } else {
                session_ids.push(sid);
                session_target = true;
            }
            continue;
        }
        // A BARE session/agent id (no `@`) can never be a project path in practice, and
        // the old fall-through error ("no project dir named …") described the WRONG
        // mental model back to the caller. Catch the id SHAPE (unless a real path of
        // that name exists) and say the exact fix.
        if let Some(tok) = p.to_str() {
            if (is_uuid(tok) || is_uuid_prefix(tok) || is_subagent_id(tok)) && !p.exists() {
                bail!(
                    "'{tok}' looks like a session/agent id, not a project path — did you \
                     mean '@{tok}'? (ids are targeted with an `@` prefix: `@<uuid>` | \
                     `@<uuid-prefix>` | `@<agent-id>`)"
                );
            }
        }
        // Everything else is a PATH (real cwd / encoded-dir token / `~/.claude/projects/<enc>`).
        project_paths.push(p);
        session_target = true;
    }

    let mut dirs: Vec<ProjectDir> = explicit_dirs;
    if !project_paths.is_empty() {
        for p in project_paths {
            dirs.push(resolve_target(p)?);
        }
    } else if dirs.is_empty() {
        // No project / encoded / jsonl target: scan every project (a uuid-only or `@main`
        // invocation searches all projects for that session).
        dirs = all_project_dirs()?;
    }

    let _ = caller; // reserved for future subcommand-aware guidance
    let mut files: Vec<PathBuf> = Vec::new();

    // ── SESSION path: the top-level `<uuid>.jsonl` session files (+ subagents per scope).
    // Skipped for an AGENT-ONLY invocation (no session target), so `@<agent-hex>` alone does
    // not list every session — only the agent subtree below runs.
    let session_path_active = session_target || agent_hexes.is_empty();
    if session_path_active {
        let mut top_level: Vec<PathBuf> = Vec::new();
        let mut prefix_hits: std::collections::BTreeMap<
            String,
            std::collections::BTreeSet<String>,
        > = std::collections::BTreeMap::new();
        // The prefix-match domain is the UNION of top-level session uuids and SUBAGENT agent
        // ids — `search` emits an id-prefix header token for subagent exchanges too, and every
        // emitted token must round-trip as an `@` target. Agent hits are collected apart so a
        // unique agent match dispatches down the agent path (its subtree, per scope).
        let mut prefix_agent_hits: std::collections::BTreeMap<
            String,
            std::collections::BTreeSet<String>,
        > = std::collections::BTreeMap::new();
        let have_filter = !session_ids.is_empty() || !session_prefixes.is_empty();
        for pd in &dirs {
            let read = match std::fs::read_dir(&pd.dir) {
                Ok(r) => r,
                Err(_) => continue, // tolerate a vanished dir mid-scan
            };
            for entry in read.flatten() {
                let p = entry.path();
                let is_file = entry.file_type().map(|ft| ft.is_file()).unwrap_or(false);
                if is_file && p.extension().is_some_and(|e| e == "jsonl") {
                    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
                    // COLLISION GUARD (§2.1): when this dir was resolved from a REAL path, the
                    // lossy encoding means a DIFFERENT cwd can share it. Keep only files whose
                    // recorded `cwd` IS this target — so a sibling's sessions (and their
                    // subagents) never leak in. A file whose `cwd` is ABSENT is kept. (Runs
                    // BEFORE the prefix domain below, so an out-of-target session contributes
                    // neither itself nor its subagent ids.)
                    if let Some(want) = &pd.target_cwd {
                        if let Some(stored) = read_first_cwd(&p) {
                            if !cwd_equivalent(&stored, want) {
                                continue;
                            }
                        }
                    }
                    // UNION DOMAIN: a prefix may name a subagent of a session that itself does
                    // NOT match, so agent-id enumeration runs before the keep/skip filter. Cost
                    // is only paid on a prefix-targeted invocation.
                    if !session_prefixes.is_empty() {
                        if let Ok(subs) = crate::subagent::subagent_transcript_files(&p) {
                            for sp in subs {
                                let sid = crate::subagent::session_id_from_path(&sp);
                                for pfx in &session_prefixes {
                                    if sid.starts_with(pfx.as_str()) {
                                        prefix_agent_hits
                                            .entry(pfx.clone())
                                            .or_default()
                                            .insert(sid.clone());
                                    }
                                }
                            }
                        }
                    }
                    // An exact id OR a uuid-PREFIX (`@13d9645a…`) keeps the file.
                    let matched_prefix = session_prefixes
                        .iter()
                        .find(|pfx| stem.starts_with(pfx.as_str()))
                        .cloned();
                    if have_filter {
                        let by_id = session_ids.iter().any(|sid| sid == stem);
                        if !by_id && matched_prefix.is_none() {
                            continue;
                        }
                    }
                    if let Some(pfx) = matched_prefix {
                        prefix_hits.entry(pfx).or_default().insert(stem.to_string());
                    }
                    top_level.push(p);
                }
            }
        }

        // A PREFIX must resolve to EXACTLY ONE id across the union domain — else error (never
        // silently pick). A unique SUBAGENT match dispatches exactly like a full `@<agent-id>`
        // target; the top-level scan kept no file for it, so only the agent path emits.
        for pfx in &session_prefixes {
            let top = prefix_hits.get(pfx);
            let agents = prefix_agent_hits.get(pfx);
            let n_top = top.map_or(0, std::collections::BTreeSet::len);
            let n_agents = agents.map_or(0, std::collections::BTreeSet::len);
            match n_top + n_agents {
                0 => bail!(
                    "no session or agent id starts with `{pfx}` under the resolved target(s) — \
                     check the prefix, or widen the scope."
                ),
                1 => {
                    if let Some(a) = agents.and_then(|s| s.iter().next()) {
                        agent_hexes.push(a.clone());
                    }
                }
                n => {
                    let ids: Vec<&str> = top
                        .into_iter()
                        .flatten()
                        .chain(agents.into_iter().flatten())
                        .map(String::as_str)
                        .collect();
                    bail!(
                        "`@{pfx}` is AMBIGUOUS: {n} ids start with it ({}). Use more of the id.",
                        ids.join(", ")
                    );
                }
            }
        }

        // Subagent transcripts of each selected top-level session (empty unless scoped in).
        let mut sub_files: Vec<PathBuf> = Vec::new();
        if matches!(scope, SubagentScope::WithSubagents) {
            for sf in &top_level {
                sub_files.extend(crate::subagent::subagent_transcript_files(sf)?);
            }
        }
        let session_files: Vec<PathBuf> = match scope {
            SubagentScope::TopLevelOnly => top_level,
            SubagentScope::WithSubagents => {
                top_level.extend(sub_files);
                top_level
            }
        };
        // A given SESSION id that matched nothing is an honest error (a prefix already bailed).
        if session_files.is_empty() && !session_ids.is_empty() {
            bail!(
                "no session file found for session id [{}] under the resolved target(s)",
                session_ids.join(", ")
            );
        }
        files.extend(session_files);
    }

    // ── AGENT path: each `@<agent-hex>` resolves to the subagent + (unless `--no-subagents`)
    // its TOPOLOGICAL descendants. Errors when no such agent exists in scope.
    for hex in &agent_hexes {
        files.extend(resolve_agent_subtree(&dirs, hex, scope)?);
    }

    files.sort();
    files.dedup();
    Ok(files)
}

/// Resolve an `@<agent-hex>` target: the subagent's OWN transcript plus (unless
/// `--no-subagents`) its TOPOLOGICAL descendants. Scans `dirs` for the session that owns the
/// agent (its hex is globally unique), builds that session's topology, and emits transcripts
/// per `scope`: `TopLevelOnly` = the agent alone; `WithSubagents` = the agent + descendants.
/// Errors when no such agent exists in scope.
fn resolve_agent_subtree(
    dirs: &[ProjectDir],
    hex: &str,
    scope: SubagentScope,
) -> Result<Vec<PathBuf>> {
    for pd in dirs {
        for top in top_level_jsonls(&pd.dir) {
            let subs = crate::subagent::subagent_transcript_files(&top)?;
            // agent_id (bare hex) → its transcript path, for this session.
            let by_id: std::collections::HashMap<String, PathBuf> = subs
                .iter()
                .map(|p| (crate::subagent::session_id_from_path(p), p.clone()))
                .collect();
            if !by_id.contains_key(hex) {
                continue; // not this session's agent
            }
            // Found the owning session. The descendants come from the agent→agent topology
            // (`parent_agent_id`); on flat real data an agent has none, so the result is just
            // the agent itself — correct, and it nests automatically once CC nests subagents.
            let nodes = crate::subagent::build_topology(&top, false)?;
            let descendants = subtree_agent_ids(&nodes, hex);
            let mut out: Vec<PathBuf> = Vec::new();
            // The agent's OWN transcript is always included (both scopes keep the target itself).
            if let Some(p) = by_id.get(hex) {
                out.push(p.clone());
            }
            if !matches!(scope, SubagentScope::TopLevelOnly) {
                for d in &descendants {
                    if let Some(p) = by_id.get(d) {
                        out.push(p.clone());
                    }
                }
            }
            return Ok(out);
        }
    }
    // Exact id matched nothing. A collision-lengthened `search` header token is a PREFIX of a
    // bare-hex agent id (12 hex chars routes here as an exact-id shape), so before giving up,
    // try a unique literal-prefix match over the in-scope agent ids — fail-loud on ambiguity,
    // a unique hit resolves exactly like the full id.
    if hex.len() >= 12 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        let mut hits: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for pd in dirs {
            for top in top_level_jsonls(&pd.dir) {
                for p in crate::subagent::subagent_transcript_files(&top)? {
                    let sid = crate::subagent::session_id_from_path(&p);
                    if sid.starts_with(hex) {
                        hits.insert(sid);
                    }
                }
            }
        }
        match hits.len() {
            1 => {
                let full = hits.iter().next().map(String::as_str).unwrap_or(hex);
                return resolve_agent_subtree(dirs, full, scope);
            }
            n if n > 1 => {
                let ids: Vec<&str> = hits.iter().map(String::as_str).collect();
                bail!(
                    "`@{hex}` is AMBIGUOUS: {n} agent ids start with it ({}). Use more of the id.",
                    ids.join(", ")
                );
            }
            _ => {}
        }
    }
    bail!(
        "no subagent `{hex}` found under the resolved target(s). List ids with \
         `csift agents <session>`, then pass one as `@<agent-hex>`."
    )
}

/// The bare-hex agent ids in `nodes` that DESCEND from `root` (children, grandchildren, …) via
/// the `parent_agent_id` chain. Excludes `root` itself. Cycle-safe (a `visited` set).
fn subtree_agent_ids(nodes: &[crate::subagent::SubagentNode], root: &str) -> Vec<String> {
    let mut children: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for n in nodes {
        if let Some(p) = n.parent_agent_id.as_deref() {
            children.entry(p).or_default().push(n.agent_id.as_str());
        }
    }
    let mut out = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut stack = vec![root];
    while let Some(cur) = stack.pop() {
        if let Some(kids) = children.get(cur) {
            for &k in kids {
                if visited.insert(k) {
                    out.push(k.to_string());
                    stack.push(k);
                }
            }
        }
    }
    out
}

/// The top-level `<uuid>.jsonl` session files directly in `dir` (non-recursive). Tolerates an
/// unreadable/vanished dir (empty result).
fn top_level_jsonls(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.flatten() {
            let p = entry.path();
            if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false)
                && p.extension().is_some_and(|e| e == "jsonl")
            {
                out.push(p);
            }
        }
    }
    out
}

/// True when a TARGET token pins a SINGLE transcript/session (so `search`'s empty-pattern
/// warning knows a session filter is present, and `show` can resolve one file): `@main`, a
/// `@trap:<marker>` self-token, an `@<uuid>`/`@<agent-hex>`/`@<uuid-prefix>` id, or a `*.jsonl`
/// file. A plain path or encoded-dir token can span many sessions, so it does NOT pin.
#[must_use]
pub fn pins_single_session(token: &str) -> bool {
    if let Some(id) = token.strip_prefix('@') {
        return id == "main"
            || id.starts_with("trap:")
            || is_uuid(id)
            || is_subagent_id(id)
            || is_uuid_prefix(id);
    }
    token.ends_with(".jsonl")
}

/// True for a canonical `8-4-4-4-12` hex UUID (a top-level session jsonl basename). Re-exports
/// the ONE canonical uuid-shape validator so the `turns` TESTS discriminate a top-level uuid
/// from a bare-hex subagent id without rolling their own (the only remaining caller now that
/// `--session`/bare-uuid routing is gone, hence `#[cfg(test)]` — production code reaches this
/// shape only through `pins_single_session`).
#[cfg(test)]
#[must_use]
pub fn is_session_uuid(s: &str) -> bool {
    is_uuid(s)
}

/// True for a canonical `8-4-4-4-12` hex UUID (the top-level session jsonl basename).
pub(crate) fn is_uuid(s: &str) -> bool {
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
/// them.) Used to GUIDE the error (subagent transcripts are not top-level jsonl basenames)
/// and, via [`is_subagent_id`], to route `@<agent-id>` targets.
pub fn is_bare_subagent_hex(s: &str) -> bool {
    s.len() >= 12 && !s.contains('-') && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// True for the NEW "teammate" subagent id shape (`taskKind:"in_process_teammate"`): the
/// canonical agent id EMBEDS the teammate name, e.g. `aVSRepro-68a2a1661c9390c1` — a leading
/// `a`, the name, then `-<hex≥12>`. Unlike a built-in/workflow agent (a bare hex run) it
/// carries a dash + uppercase, so [`is_bare_subagent_hex`] rejects it. The NAME itself may
/// carry dashes (a real `aP1-engine-9cf2f06d6235ca64` was minted from the teammate name
/// `P1-engine`), so the head accepts `[A-Za-z0-9-]`; the explicit `!is_uuid` guard keeps an
/// `a`-led session uuid out — its final segment is exactly 12 hex, which the dash-tolerant
/// head would otherwise admit. An encoded project dir can never collide (those start with
/// `-`, the leading-slash sanitisation). Recognised here so the id `csift agents` prints
/// round-trips back as an `@<agent-id>` target.
fn is_teammate_agent_id(s: &str) -> bool {
    if is_uuid(s) {
        return false;
    }
    let Some((head, tail)) = s.rsplit_once('-') else {
        return false;
    };
    head.len() >= 2
        && head.starts_with('a')
        && head.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && tail.len() >= 12
        && tail.bytes().all(|b| b.is_ascii_hexdigit())
}

/// True for ANY canonical subagent id `csift agents` emits — a bare hex run
/// ([`is_bare_subagent_hex`], built-in/workflow) OR a name-embedded teammate id
/// ([`is_teammate_agent_id`]). This is the single gate the `@<agent-id>` grammar branch
/// (`resolve_session_files`), the pinned-target existence guard, and `pins_single_session`
/// key on, so EVERY emitted agent_id round-trips regardless of shape (downstream resolution
/// already matches a subagent by exact id).
pub fn is_subagent_id(s: &str) -> bool {
    is_bare_subagent_hex(s) || is_teammate_agent_id(s)
}

/// True for a session-uuid PREFIX in either emitted form:
/// - the short dash-less run (`@13d9645a`, 4..=11 hex) — long enough to be near-collision-free
///   (a uuid's first segment is 8 hex = 4 billion), short enough that it is unambiguously
///   NEITHER a full uuid (32 hex + dashes) NOR an agent hex (≥12);
/// - a longer LITERAL prefix of the canonical `8-4-4-4-12` layout (`@13d9645a-3a5b`, 12..=35
///   chars, a dash exactly at each template position) — the collision-lengthened header token
///   `search` emits when two in-scope transcript ids share their first 8 chars.
///
/// The caller prefix-matches either form against the enumerated ids and errors if it is not
/// unique. (A dash-less run of ≥12 hex is claimed as an agent id BEFORE this predicate runs.)
fn is_uuid_prefix(s: &str) -> bool {
    if (4..=11).contains(&s.len()) {
        return s.bytes().all(|b| b.is_ascii_hexdigit());
    }
    if (12..36).contains(&s.len()) {
        return s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    }
    false
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
            // The `/.cache` segment emits a literal `--` (proves no collapse).
            (
                "/Users/testuser/Projects/Acme/widget_factory/.cache-worktrees/sunny-meadow",
                "-Users-testuser-Projects-Acme-widget-factory--cache-worktrees-sunny-meadow",
            ),
            ("/a/.claude/b", "-a--claude-b"),
            // Case is preserved; digits pass through.
            (
                "/Users/testuser/Projects/Demo3",
                "-Users-testuser-Projects-Demo3",
            ),
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
        assert!(
            strip_projects_root_prefix(Path::new("/Users/testuser/Projects/foo"), root).is_none()
        );
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

    #[test]
    fn resolve_claude_home_precedence_flag_then_env_then_home() {
        let home = Path::new("/home/u");
        // 1. The `--claude-home` flag wins over everything else.
        assert_eq!(
            resolve_claude_home(
                Some(Path::new("/flag/dir")),
                Some(OsStr::new("/env/dir")),
                home
            ),
            PathBuf::from("/flag/dir")
        );
        // 2. With no flag, $CLAUDE_CONFIG_DIR wins over the $HOME default.
        assert_eq!(
            resolve_claude_home(None, Some(OsStr::new("/env/dir")), home),
            PathBuf::from("/env/dir")
        );
        // 3. An EMPTY $CLAUDE_CONFIG_DIR is ignored → falls through to $HOME/.claude.
        assert_eq!(
            resolve_claude_home(None, Some(OsStr::new("")), home),
            PathBuf::from("/home/u/.claude")
        );
        // 4. Nothing set → $HOME/.claude (the historical default, unchanged).
        assert_eq!(
            resolve_claude_home(None, None, home),
            PathBuf::from("/home/u/.claude")
        );
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
    fn is_teammate_agent_id_recognizes_name_embedded_ids() {
        // The new `in_process_teammate` shape: a<Name>-<hex>. These are the canonical ids
        // `csift agents` prints for a teammate, and must round-trip as `@<id>` targets.
        assert!(is_teammate_agent_id("aVSRepro-68a2a1661c9390c1"));
        assert!(is_teammate_agent_id("aVSSpeedField-d5dab904cc98a239"));
        assert!(is_teammate_agent_id("aVSMultiRegion-06fb13dd400b53a5"));
        // A teammate NAME may itself carry dashes (real data: teammate "P1-engine") — the
        // head is dash-tolerant so the id `csift agents` prints still round-trips.
        assert!(is_teammate_agent_id("aP1-engine-9cf2f06d6235ca64"));
        // A bare hex (built-in/workflow) has no dash → NOT teammate-shaped (it routes via
        // is_bare_subagent_hex instead).
        assert!(!is_teammate_agent_id("ae24045bd6d4bdaff"));
        // A uuid is rejected: the explicit is_uuid guard (an `a`-led uuid would otherwise
        // pass the dash-tolerant head with its exactly-12-hex final segment).
        assert!(!is_teammate_agent_id(
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
        ));
        assert!(!is_teammate_agent_id(
            "a93b39f8-1681-4535-88eb-5b8ecce0abcd"
        ));
        // An encoded project dir starts with `-` (leading-slash sanitisation) → head not `a`.
        assert!(!is_teammate_agent_id("-Users-testuser-Projects-foo"));
        // Hex tail too short, or a non-hex tail → rejected.
        assert!(!is_teammate_agent_id("aVSRepro-68a2a1")); // tail < 12
        assert!(!is_teammate_agent_id("aVSRepro-zzzzzzzzzzzz")); // non-hex tail
    }

    #[test]
    fn is_subagent_id_accepts_both_bare_hex_and_teammate() {
        // The unified gate the @-grammar routes through.
        assert!(is_subagent_id("ae24045bd6d4bdaff")); // built-in/workflow bare hex
        assert!(is_subagent_id("aVSRepro-68a2a1661c9390c1")); // teammate
        assert!(is_subagent_id("aP1-engine-9cf2f06d6235ca64")); // teammate with dashed name
        assert!(!is_subagent_id("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d")); // uuid
        assert!(!is_subagent_id("a93b39f8-1681-4535-88eb-5b8ecce0abcd")); // a-led uuid
        assert!(!is_subagent_id("-Users-testuser-Projects-foo")); // encoded dir
        assert!(!is_subagent_id("abc123")); // too short
    }

    #[test]
    fn pins_single_session_covers_at_tokens_and_jsonl() {
        assert!(pins_single_session("@main"));
        assert!(pins_single_session("@trap:CrimsonWillowFen5180"));
        assert!(pins_single_session("@0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"));
        assert!(pins_single_session("@ae24045bd6d4bdaff"));
        assert!(pins_single_session("@aVSRepro-68a2a1661c9390c1")); // teammate id pins one
        assert!(pins_single_session("@aP1-engine-9cf2f06d6235ca64")); // dashed-name teammate
        assert!(pins_single_session("@13d9645a")); // uuid-prefix
        assert!(pins_single_session("/a/b/0a1b2c3d.jsonl"));
        // A bare uuid (no `@`), an encoded token, a plain path, `.` → NOT a session pin.
        assert!(!pins_single_session("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"));
        assert!(!pins_single_session("-Users-testuser-Projects-foo"));
        assert!(!pins_single_session("."));
    }

    #[test]
    fn is_uuid_prefix_covers_first_segment_not_full_or_agent() {
        assert!(is_uuid_prefix("13d9")); // 4 hex (minimum)
        assert!(is_uuid_prefix("13d9645a")); // 8 hex (the uuid first segment)
        assert!(is_uuid_prefix("13d9645a3a5")); // 11 hex (max dash-less)
                                                // Too short (<4), dash-less ≥12 (agent-hex territory), non-hex, off-template dash → NOT a prefix.
        assert!(!is_uuid_prefix("13d")); // 3
        assert!(!is_uuid_prefix("13d9645a3a5b")); // 12 dash-less → agent hex
        assert!(!is_uuid_prefix("13d9645g")); // non-hex g
        assert!(!is_uuid_prefix("13d9-645a")); // dash off the 8-4-4-4-12 template
                                               // LITERAL layout prefixes (collision-lengthened header tokens) ARE prefixes.
        assert!(is_uuid_prefix("13d9645a-3a5")); // 12 chars, dash at template position 8
        assert!(is_uuid_prefix("13d9645a-3a5b-4a92")); // deeper into the layout
        assert!(is_uuid_prefix("13d9645a-3a5b-4a92-b83d-e0f94c5a9b9")); // 35 (max — one short of full)
        assert!(!is_uuid_prefix("13d9645a-3a5b-4a92-b83d-e0f94c5a9b90")); // 36 = a FULL uuid, not a prefix
        assert!(!is_uuid_prefix("13d9645a-3a5g")); // non-hex inside the layout
    }

    #[test]
    fn validate_trap_marker_enforces_the_strict_grammar() {
        // Accepted: EXACTLY 3 imaginative CamelCase words + 4 non-trivial digits.
        for ok in [
            "CrimsonWillowFen5180",
            "MossyLanternCove6024",
            "GildedHeronVale7391",
        ] {
            assert!(validate_trap_marker(ok).is_ok(), "should accept {ok}");
        }
        // Rejected — every lazy shortcut fails loudly.
        for bad in [
            "",                         // empty
            "foo",                      // too short / not the shape
            "CrimsonOwlPond",           // no trailing 4 digits
            "DeepRiverStone12",         // only 2 digits
            "OneTwo4283",               // 2 words (must be EXACTLY 3)
            "WistfulAmberGlenMoor8135", // 4 words (must be EXACTLY 3, not >=3)
            "GoFooBars4283",            // "Go" is a 2-letter word (need >=3 chars)
            "HTTPSPROXYGATE4827",       // ALLCAPS "word" — no lowercase tail
            "HTML0000",                 // the acronym + zeros loophole
            "DeepRiverStone1234",       // trivial: consecutive
            "DeepRiverStone0000",       // trivial: all-equal
            "DeepRiverStone9876",       // trivial: descending
            "DeepRiverStone1357",       // trivial: odd run (+2)
            "DeepRiverStone2468",       // trivial: even run (+2)
        ] {
            assert!(validate_trap_marker(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn validate_trap_marker_refuses_the_reserved_doc_example() {
        // The literal printed in the SKILL / --help / SPEC is reserved: the doc shows it next to
        // `csift` (so quoting the doc self-matches) and every copy-paste collides, so csift refuses
        // it even though it passes the grammar — forcing a fresh hand-invented marker.
        for reserved in RESERVED_EXAMPLE_MARKERS {
            let err = validate_trap_marker(reserved)
                .expect_err("the documented example must be refused")
                .to_string();
            assert!(
                err.contains("RESERVED") && err.to_lowercase().contains("example"),
                "reserved-marker error must explain it is the reserved doc example: {err}"
            );
        }
    }

    #[test]
    fn is_trivial_4_digits_flags_arithmetic_runs_only() {
        for t in ["0000", "1234", "9876", "1357", "2468", "8642", "3210"] {
            assert!(is_trivial_4_digits(t), "{t} is trivial");
        }
        for ok in ["4283", "6024", "7391", "8135", "1212", "1122"] {
            assert!(!is_trivial_4_digits(ok), "{ok} is NOT trivial");
        }
    }
}
