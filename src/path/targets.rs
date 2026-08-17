//! File targets + --sessions-from union.

use super::*;

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
