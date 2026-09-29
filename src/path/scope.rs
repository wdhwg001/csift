//! SubagentScope / Caller + the env-session resolver.

use super::*;

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

/// Which subcommand is resolving session files - threaded into [`resolve_session_files`] so a
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
    /// `true` (subagents spanned) => `WithSubagents`; `false` (`--no-subagents`) =>
    /// `TopLevelOnly`.
    fn from(include_subagents: bool) -> Self {
        if include_subagents {
            SubagentScope::WithSubagents
        } else {
            SubagentScope::TopLevelOnly
        }
    }
}

/// Resolve the CALLING session id from the environment - the value of `CLAUDE_CODE_SESSION_ID`,
/// which CC sets to the process-global MAIN session id even inside a subagent (verified
/// empirically + against the shipping binary; an in-process subagent's OWN id is NOT exported to the
/// subprocess env). Used by `@main` and as the `@trap:` search root. There is no env-based
/// `@self` because CC withholds the per-subagent id from the Bash env - `@trap:<marker>`
/// recovers it from the transcript instead.
pub fn resolve_env_session() -> Result<String> {
    env_session()?.ok_or_else(|| {
        anyhow::anyhow!(
            "needs the calling session id, but CLAUDE_CODE_SESSION_ID is not set (running \
             outside Claude Code, or an old build). Pass an explicit `@<uuid>` target instead."
        )
    })
}

/// The session-id variables, in the order they are read: Claude Code's own, then the alias the
/// Codex companion mirrors it under, which counts only when the canonical one is unset or blank.
/// EXACT names, never a loose `/session/i` match: `SECURITYSESSIONID` is a macOS login-session
/// variable that such a match would read.
pub const SESSION_ENV_VARS: [&str; 2] = ["CLAUDE_CODE_SESSION_ID", "CODEX_COMPANION_SESSION_ID"];

/// The calling session id from the environment: `Ok(None)` when no variable is set (or each is
/// blank), the trimmed value when it is a session uuid, and an ERROR naming the value when it is
/// anything else. Claude Code writes the calling session's own jsonl basename there, a uuid on
/// every build, so a value of another shape was set by something else. Reading it as a session
/// answers for a session that does not exist, and treating it as unset would hide that the
/// environment disagrees with itself; both are the guess section 3.8 forbids.
pub fn env_session() -> Result<Option<String>> {
    for var in SESSION_ENV_VARS {
        let Ok(raw) = std::env::var(var) else {
            continue;
        };
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        if crate::path::is_uuid(value) {
            return Ok(Some(value.to_string()));
        }
        let shown: String = value.chars().take(64).collect();
        let more = if value.chars().count() > 64 {
            "..."
        } else {
            ""
        };
        anyhow::bail!(
            "{var} is set to `{shown}{more}`, which is not a session uuid (8-4-4-4-12 hex \
             digits). Claude Code writes the calling session's own id there, so this value was \
             set by something else and csift will not guess which session it meant. Pass an \
             explicit `@<uuid>` target, or unset {var}."
        );
    }
    Ok(None)
}
