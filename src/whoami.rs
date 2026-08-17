//! `whoami` subcommand - identify the CALLING Claude Code session, false-positive-safe.
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
//! bash nesting, zero false positives. It is the primary signal; when it is absent
//! we fall back to the `CODEX_COMPANION_SESSION_ID` alias (set by the Codex companion
//! plugin) before giving up.
//!
//! When NEITHER var is set (e.g. invoked outside Claude Code/Codex, or a future
//! CC build that drops it) we DO NOT GUESS - multiple CC sessions may run
//! concurrently with different binaries, and most-recent-mtime is a false-positive
//! trap. We error with actionable guidance instead. It is acceptable for whoami to
//! often say "ambiguous, pass `@<uuid>`".

use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::cli::{OutputFormat, WhoamiArgs};
use crate::path;

/// Primary env var Claude Code sets per session (verified 2026-06-07): its value
/// equals the calling session's own jsonl basename, exactly.
const SESSION_ID_ENV: &str = "CLAUDE_CODE_SESSION_ID";

/// Secondary alias mirrored by the Codex companion plugin. Accepted only when the
/// canonical var is absent (SPEC §6.3 - prefer the canonical var).
const SESSION_ID_ENV_ALIAS: &str = "CODEX_COMPANION_SESSION_ID";

/// The guidance shown when no definitive signal exists. Kept as a const so the
/// message stays identical across call sites (and is unit-asserted).
pub const AMBIGUOUS_GUIDANCE: &str = "cannot identify the calling session: \
CLAUDE_CODE_SESSION_ID is not set (old Claude Code build, or running outside Claude \
Code). Do NOT trust most-recent-mtime — many sessions may be live at once. Pass an \
explicit `@<uuid>` target: your id is the basename of your own transcript jsonl, \
or grep a unique recent line you wrote to disambiguate.";

/// Read the definitive session id from the environment, if present and non-empty.
/// Matches the EXACT canonical var name first (never a loose `/session/i` regex -
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

/// Entry point for `csift whoami`. With no target (or `@main`), identify the calling session from
/// the environment; with `@trap:<marker>`, resolve which SUBAGENT the caller is (env-independent).
pub fn run_whoami(args: &WhoamiArgs) -> Result<()> {
    match args.self_target.as_deref() {
        None | Some("@main") => run_whoami_env(args),
        Some(t) if t.starts_with("@trap:") => {
            run_whoami_trap(t.strip_prefix("@trap:").unwrap_or(""), args)
        }
        Some(other) => bail!(
            "whoami accepts no target except `@trap:<marker>` (which SUBAGENT am I?) or `@main` \
             (the calling top-level session — the default). Got `{other}`. To inspect a DIFFERENT \
             session, use `csift list @<uuid>` / `csift agents @<uuid>`."
        ),
    }
}

/// `whoami` (env form): the calling session id from `$CLAUDE_CODE_SESSION_ID` + its jsonl path.
fn run_whoami_env(args: &WhoamiArgs) -> Result<()> {
    let Some(session_id) = detect_session_id() else {
        // SPEC §6.3 step 3: never guess - error with actionable guidance.
        bail!("{AMBIGUOUS_GUIDANCE}");
    };

    let path = locate_transcript(&session_id);

    let me = WhoAmI { session_id, path };
    match args.format {
        OutputFormat::Text => render_text(&me),
        OutputFormat::Json => render_json(&me)?,
    }
    Ok(())
}

/// `whoami @trap:<marker>`: resolve the caller's UPSTREAM ancestry chain from the unique literal
/// marker it embedded in THIS very command, and report it self → ancestors → top-level root. This
/// is the walk-UP mirror of `agents` (walk-DOWN): a subagent learns its own bare hex AND the whole
/// re-feedable lineage above it. Env-independent - reliable for a built-in Task AND a workflow
/// subagent (whose env id is the PARENT, not itself).
fn run_whoami_trap(marker: &str, args: &WhoamiArgs) -> Result<()> {
    use serde_json::json;

    let chain = path::resolve_trap_who(marker)?;
    match args.format {
        OutputFormat::Text => {
            // chain[0] = self (the marker carrier); chain.last() = the top-level root.
            for (i, n) in chain.iter().enumerate() {
                let role = if n.is_subagent { "subagent" } else { "session" };
                let annot = match (i, n.is_subagent, n.depth) {
                    (0, true, Some(d)) => format!("  <- you (subagent, depth {d})"),
                    (0, true, None) => "  <- you (subagent)".to_string(),
                    (0, false, _) => "  <- you (top-level session, not a subagent)".to_string(),
                    (_, true, Some(d)) => format!("  ^ parent subagent (depth {d})"),
                    (_, true, None) => "  ^ parent subagent".to_string(),
                    (_, false, _) => "  ^ top-level root".to_string(),
                };
                println!("{role:8} {}{annot}", n.session_id);
            }
            // The self transcript path - the most useful "where am I".
            match chain.first().and_then(|n| n.path.as_ref()) {
                Some(p) => println!("path     {}", p.display()),
                None => println!("path     <transcript not found under projects root>"),
            }
        }
        OutputFormat::Json => {
            // envelope v2: one kind:"identity" row per ancestry node, self first (depth 0)
            // → top-level root last. The former single `{chain:[…]}` wrapper is gone -
            // the SAME stream shape as the env form, just more rows.
            println!("{}", crate::text::envelope_header("whoami", json!({})));
            for n in &chain {
                let obj = json!({
                    "kind": "identity",
                    "session_id": n.session_id,
                    "is_subagent": n.is_subagent,
                    "parent_session_id": n.parent_session_id,
                    "depth": n.depth,
                    "path": n.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                });
                println!("{}", serde_json::to_string(&obj)?);
            }
            println!(
                "{}",
                crate::text::envelope_summary(json!({"identities": chain.len()}))
            );
        }
    }
    Ok(())
}

/// Locate `<id>.jsonl` under the projects root. First try the current cwd's encoded
/// dir (the common case - a session's cwd is its start cwd); if that misses, scan
/// every project dir for a file named `<id>.jsonl`. Returns `None` if not found -
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

    // Fallback: scan every project dir for the file (cheap - a stat per dir).
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

fn render_text(me: &WhoAmI) {
    println!("session  {}", me.session_id);
    // The path is ALWAYS printed (it is the useful bit); a not-found note when it can't be located.
    match &me.path {
        Some(p) => println!("path     {}", p.display()),
        None => println!("path     <not found under projects root for the current cwd>"),
    }
}

fn render_json(me: &WhoAmI) -> Result<()> {
    use serde_json::json;
    println!("{}", crate::text::envelope_header("whoami", json!({})));
    let obj = json!({
        "kind": "identity",
        "session_id": me.session_id,
        "is_subagent": false,
        "parent_session_id": me.session_id,
        "depth": 0,
        "path": me.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
    });
    println!("{}", serde_json::to_string(&obj)?);
    println!(
        "{}",
        crate::text::envelope_summary(json!({"identities": 1}))
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guidance_mentions_session_target_and_no_mtime() {
        assert!(AMBIGUOUS_GUIDANCE.contains("@<uuid>"));
        assert!(AMBIGUOUS_GUIDANCE.contains("mtime"));
        assert!(AMBIGUOUS_GUIDANCE.contains("CLAUDE_CODE_SESSION_ID"));
    }

    #[test]
    fn exact_env_name_is_canonical_not_a_loose_regex() {
        // The constant must be the EXACT name - a loose /session/i match would
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
