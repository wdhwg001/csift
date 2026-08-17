//! Canonical id derivation from paths (the r5 trio helpers).

use super::*;

/// Strip the on-disk `agent-` filename prefix to the bare-hex canonical agent id (the
/// value the transcript record's `agentId` field AND the workflow journal carry). The
/// single source of truth for this rule — used by `make_subagent` and by the
/// `recover` / `session` / `files` subcommands so a subagent row's printed `session_id`
/// is the SAME bare hex `agents` prints, hence joinable across surfaces.
#[must_use]
pub fn bare_agent_id(stem: &str) -> &str {
    stem.strip_prefix("agent-").unwrap_or(stem)
}

/// The CANONICAL session id for a transcript file: its jsonl basename, with a
/// subagent's `agent-` filename prefix stripped to the bare-hex id ([`bare_agent_id`]).
///
/// This is the SINGLE derivation used by every per-file `session_id` emission
/// (`list` / `search` / `files` / `recover` / `turns`) so the SAME subagent transcript
/// always reports the SAME id, whichever subcommand prints it — id-form unification.
/// A top-level session uuid has no `agent-` prefix and passes through unchanged. An
/// empty / stem-less path yields an empty string (never panics).
#[must_use]
pub fn session_id_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(bare_agent_id)
        .map(str::to_string)
        .unwrap_or_default()
}

/// The re-feedable PARENT session uuid for a transcript path, or `None` when the path is a
/// top-level `<uuid>.jsonl` (which IS its own session). A subagent transcript lives at
/// `…/<PARENT-UUID>/subagents/[workflows/wf_*/]agent-<hex>.jsonl`, so the parent uuid is the
/// directory component immediately BEFORE the `subagents` segment. This is what makes a
/// search/files subagent match re-feedable: its bare-hex `session_id` is NOT a re-feedable
/// `@<uuid>` target, but the `parent_session_id` this returns is (`csift verbatim @<parent>` works).
#[must_use]
pub fn parent_session_id_from_path(path: &Path) -> Option<String> {
    let mut prev: Option<&str> = None;
    for comp in path.components() {
        let c = comp.as_os_str().to_str()?;
        if c == "subagents" {
            // The component just before `subagents` is the parent-session dir name.
            return prev.map(str::to_string);
        }
        prev = Some(c);
    }
    None
}

/// True when `path` is a SUBAGENT transcript (lives under a `subagents/` segment) rather
/// than a top-level `<uuid>.jsonl` session file.
#[must_use]
pub fn is_subagent_path(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str().to_str() == Some("subagents"))
}
