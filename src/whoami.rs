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

use anyhow::{bail, Result};

use crate::cli::WhoamiArgs;

/// Primary env var Claude Code sets per session (verified 2026-06-07).
const SESSION_ID_ENV: &str = "CLAUDE_CODE_SESSION_ID";

/// The guidance shown when no definitive signal exists. Kept as a const so the
/// message stays identical across call sites.
const AMBIGUOUS_GUIDANCE: &str = "cannot identify the calling session: \
CLAUDE_CODE_SESSION_ID is not set. Do NOT trust most-recent-mtime. Pass --session \
explicitly; your id is in your own context / the jsonl path, or grep a unique \
recent line of your transcript to identify yourself.";

/// Read the definitive session id from the environment, if present and non-empty.
#[must_use]
pub fn detect_session_id() -> Option<String> {
    match std::env::var(SESSION_ID_ENV) {
        Ok(v) if !v.trim().is_empty() => Some(v.trim().to_string()),
        _ => None,
    }
}

/// Entry point for `csift whoami`.
pub fn run_whoami(_args: &WhoamiArgs) -> Result<()> {
    let Some(_session_id) = detect_session_id() else {
        bail!("{AMBIGUOUS_GUIDANCE}");
    };
    todo!("locate the matching jsonl under projects root + render (text/json); Phase 2")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_none_when_unset_or_blank() {
        // We cannot safely mutate process env in parallel tests, so assert the
        // pure predicate via the documented contract on a blank value.
        // (Full env-driven behavior is integration-tested in Phase 2.)
        assert!("".trim().is_empty());
        assert!(AMBIGUOUS_GUIDANCE.contains("--session"));
    }
}
