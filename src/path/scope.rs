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

/// Resolve the CALLING session id from the environment - the value of `CLAUDE_CODE_SESSION_ID`,
/// which CC sets to the process-global MAIN session id even inside a subagent (verified
/// empirically + against the shipping binary; an in-process subagent's OWN id is NOT exported to the
/// subprocess env). Used by `@main` and as the `@trap:` search root. There is no env-based
/// `@self` because CC withholds the per-subagent id from the Bash env - `@trap:<marker>`
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
