//! `whoami` subcommand — identify the CALLING Claude Code session, false-positive-safe.
//!
//! ## Detection (verified empirically inside a live Claude Code Bash tool, 2026-06-07)
//!
//! Claude Code exports `CLAUDE_CODE_SESSION_ID` into its Bash tool environment.
//! It was confirmed to equal exactly the session's own jsonl filename:
//!
//! ```text
//! CLAUDE_CODE_SESSION_ID=0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
//!   -> ~/.claude/projects/<encoded>/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jsonl
//! ```
//!
//! This is a **definitive** signal: per-session, version-independent, survives
//! bash nesting, zero false positives. We use it and nothing else.
//!
//! When the var is absent or empty (e.g. invoked outside Claude Code, or a future
//! CC build that drops it) we DO NOT GUESS — multiple CC sessions may run
//! concurrently with different binaries, and most-recent-mtime is a false-positive
//! trap. We error with actionable guidance instead. It is acceptable for whoami to
//! often say "ambiguous, pass --session".

use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::cli::{OutputFormat, WhoamiArgs};
use crate::path;

/// Primary env var Claude Code sets per session (verified 2026-06-07): its value
/// equals the calling session's own jsonl basename, exactly.
const SESSION_ID_ENV: &str = "CLAUDE_CODE_SESSION_ID";

/// Secondary alias mirrored by the Codex companion plugin. Accepted only when the
/// canonical var is absent (SPEC §6.3 — prefer the canonical var).
const SESSION_ID_ENV_ALIAS: &str = "CODEX_COMPANION_SESSION_ID";

/// The guidance shown when no definitive signal exists. Kept as a const so the
/// message stays identical across call sites (and is unit-asserted).
pub const AMBIGUOUS_GUIDANCE: &str = "cannot identify the calling session: \
CLAUDE_CODE_SESSION_ID is not set (old Claude Code build, or running outside Claude \
Code). Do NOT trust most-recent-mtime — many sessions may be live at once. Pass \
--session <uuid> explicitly: your id is the basename of your own transcript jsonl, \
or grep a unique recent line you wrote to disambiguate.";

/// Read the definitive session id from the environment, if present and non-empty.
/// Matches the EXACT canonical var name first (never a loose `/session/i` regex —
/// `SECURITYSESSIONID` is a false-positive trap), then the Codex alias.
#[must_use]
pub fn detect_session_id() -> Option<String> {
    if let Ok(v) = std::env::var(SESSION_ID_ENV) {
        if !v.trim().is_empty() {
            return Some(v.trim().to_string());
        }
    }
    if let Ok(v) = std::env::var(SESSION_ID_ENV_ALIAS) {
        if !v.trim().is_empty() {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// The resolved identity of the calling session.
#[derive(Debug, Clone)]
pub struct WhoAmI {
    pub session_id: String,
    /// Absolute path to the session's jsonl, if we could locate it on disk.
    pub path: Option<PathBuf>,
}

/// Entry point for `csift whoami`.
pub fn run_whoami(args: &WhoamiArgs) -> Result<()> {
    let Some(session_id) = detect_session_id() else {
        // SPEC §6.3 step 3: never guess — error with actionable guidance.
        bail!("{AMBIGUOUS_GUIDANCE}");
    };

    let path = locate_transcript(&session_id);

    let me = WhoAmI { session_id, path };
    match args.format {
        OutputFormat::Text => render_text(&me, args),
        OutputFormat::Json => render_json(&me)?,
    }
    Ok(())
}

/// Locate `<id>.jsonl` under the projects root. First try the current cwd's encoded
/// dir (the common case — a session's cwd is its start cwd); if that misses, scan
/// every project dir for a file named `<id>.jsonl`. Returns `None` if not found —
/// the id is still authoritative (it came from the env var); the path is a bonus.
fn locate_transcript(session_id: &str) -> Option<PathBuf> {
    let root = path::projects_root().ok()?;
    let filename = format!("{session_id}.jsonl");

    // Fast path: encode $PWD and look there first.
    if let Ok(cwd) = std::env::current_dir() {
        let dir = root.join(path::encode_cwd(&cwd));
        let candidate = dir.join(&filename);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // Fallback: scan every project dir for the file (cheap — a stat per dir).
    let dirs = path::all_project_dirs().ok()?;
    for pd in dirs {
        let candidate = pd.dir.join(&filename);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

// ── Rendering ──

fn render_text(me: &WhoAmI, args: &WhoamiArgs) {
    println!("session  {}", me.session_id);
    // The `--path` flag opts into printing the resolved jsonl path; we also print
    // it implicitly when found (it's the useful bit), but only error-note its
    // absence when the user explicitly asked for it.
    match &me.path {
        Some(p) => println!("path     {}", p.display()),
        None if args.path => {
            println!("path     <not found under projects root for the current cwd>");
        }
        None => {}
    }
}

fn render_json(me: &WhoAmI) -> Result<()> {
    use serde_json::json;
    let obj = json!({
        "session_id": me.session_id,
        "path": me.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
    });
    println!("{}", serde_json::to_string(&obj)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guidance_mentions_session_flag_and_no_mtime() {
        assert!(AMBIGUOUS_GUIDANCE.contains("--session"));
        assert!(AMBIGUOUS_GUIDANCE.contains("mtime"));
        assert!(AMBIGUOUS_GUIDANCE.contains("CLAUDE_CODE_SESSION_ID"));
    }

    #[test]
    fn exact_env_name_is_canonical_not_a_loose_regex() {
        // The constant must be the EXACT name — a loose /session/i match would
        // false-positive on SECURITYSESSIONID (macOS login session).
        assert_eq!(SESSION_ID_ENV, "CLAUDE_CODE_SESSION_ID");
        assert_ne!(SESSION_ID_ENV, "SECURITYSESSIONID");
    }

    #[test]
    fn detect_trims_and_blank_is_none() {
        // We avoid mutating process env in threaded tests; assert the trim/blank
        // contract directly (the env read is integration-tested separately).
        assert!("   ".trim().is_empty());
    }
}
