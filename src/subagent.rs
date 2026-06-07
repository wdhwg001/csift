//! Subagent / nested-transcript discovery, classification, and lifecycle.
//!
//! ## On-disk layout (empirically mapped against `~/.claude/projects`, 2026-06-07)
//!
//! A top-level session `<ENCODED>/<session-uuid>.jsonl` may own a sibling sidecar
//! directory `<ENCODED>/<session-uuid>/` holding its subagent transcripts in THREE
//! distinct shapes:
//!
//! - **(A) built-in Task/Agent-tool subagent** —
//!   `subagents/agent-<hex>.jsonl` (+ companion `agent-<hex>.meta.json`). The `.jsonl`
//!   uses the identical [`crate::model::Record`] model; its first record has `isSidechain:true`, an
//!   `agentId` field (== the `agent-<hex>` filename stem), and `sessionId` == the
//!   enclosing `<session-uuid>`. meta.json = `{agentType, description, name?, toolUseId}`.
//! - **(B) workflow / OMC workflow-subagent** —
//!   `subagents/workflows/wf_<id>/agent-<hex>.jsonl` (+ `.meta.json`, the dominant
//!   kind). Same record model + `isSidechain:true` + `agentId`. meta.json = `{agentType}`.
//!   Its `cwd` is often a DEEPER in-session path — never re-encode a subagent cwd to
//!   find its project dir.
//! - **(C) workflow journal** — `subagents/workflows/wf_<id>/journal.jsonl`. **NOT a
//!   transcript**: records are workflow events `{agentId, key, type}` (`type` ∈
//!   {`started`, `result`}) with no `message`/role. Excluded from every transcript
//!   list/search; read ONLY to corroborate completion status.
//!
//! ## Kind is determined by PATH LOCATION, not `agentType`
//!
//! `agentType` is NOT a reliable kind discriminator: both (A) and (B) carry the same
//! spread of values (`Explore`, `general-purpose`, `oh-my-claudecode:*`); only the
//! special `workflow-subagent` value is workflow-exclusive. So the authoritative kind
//! is the on-disk location — directly under `subagents/` ⇒ [`SubagentKind::BuiltinTask`];
//! under `subagents/workflows/wf_*/` ⇒ [`SubagentKind::Workflow`] — with `agentType`
//! retained as a descriptive per-row sub-label.
//!
//! ## Linkage back to the parent session
//!
//! Primarily FILESYSTEM (the enclosing `<session-uuid>` dir name), corroborated by the
//! record's `sessionId` field (verified 600/0 mismatches). We use the directory name
//! as the parent-session id (it is the on-disk truth and needs no record parse).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::parse::{head_records, tail_records};

/// Subagent kind, keyed off the on-disk path location (authoritative; see module
/// docs — `agentType` is descriptive only, not the discriminator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentKind {
    /// `subagents/agent-<hex>.jsonl` — a built-in Task/Agent-tool subagent.
    BuiltinTask,
    /// `subagents/workflows/wf_<id>/agent-<hex>.jsonl` — a workflow / OMC agent.
    Workflow,
}

impl SubagentKind {
    /// Stable lowercase label used in CLI output + JSON (matches the `--kind` enum).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            SubagentKind::BuiltinTask => "builtin-task",
            SubagentKind::Workflow => "workflow",
        }
    }
}

/// One discovered subagent transcript and its located companion files.
#[derive(Debug, Clone)]
pub struct Subagent {
    /// The `agent-<hex>` filename stem (== the record `agentId`).
    pub agent_id: String,
    pub kind: SubagentKind,
    /// Absolute path to the subagent `.jsonl` transcript.
    pub path: PathBuf,
    /// The enclosing `<session-uuid>` dir name — the parent session id (filesystem
    /// linkage; corroborated by the record `sessionId`).
    pub parent_session_id: String,
    /// The `wf_<id>` workflow id for a workflow subagent; `None` for built-in.
    pub workflow_id: Option<String>,
    /// Companion `agent-<hex>.meta.json` path, if it exists alongside the transcript.
    pub meta_path: Option<PathBuf>,
}

/// Lifecycle facts derived from a subagent transcript (+ its workflow journal, when
/// present). Timestamps are raw ISO8601 UTC from the transcript's first/last record.
#[derive(Debug, Clone)]
pub struct SubagentLifecycle {
    pub agent_id: String,
    pub kind: SubagentKind,
    pub parent_session_id: String,
    pub workflow_id: Option<String>,
    /// `agentType` from meta.json (descriptive sub-label, e.g. `Explore`,
    /// `oh-my-claudecode:executor`, `workflow-subagent`).
    pub agent_type: Option<String>,
    /// Short description from a built-in meta.json (the Task tool's `description`).
    pub description: Option<String>,
    /// First transcript record's timestamp (raw UTC) — the START.
    pub started_utc: Option<String>,
    /// Last transcript record's timestamp (raw UTC) — the COMPLETION (best-effort).
    pub completed_utc: Option<String>,
    /// Resolved status (see [`SubagentStatus`]).
    pub status: SubagentStatus,
    /// Malformed lines skipped while reading the transcript (never hidden).
    pub skipped_lines: usize,
}

/// Determinable subagent run status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentStatus {
    /// A workflow journal carries a `result` event for this agent, OR the transcript
    /// terminates with an assistant end-of-turn message (a clean finish).
    Completed,
    /// The transcript exists but shows no completion signal — likely still running,
    /// or interrupted. We do not over-claim "failed"; this is the honest middle.
    Running,
    /// No timestamps / empty transcript — status cannot be determined.
    Unknown,
}

impl SubagentStatus {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            SubagentStatus::Completed => "completed",
            SubagentStatus::Running => "running",
            SubagentStatus::Unknown => "unknown",
        }
    }
}

/// The sidecar directory `<ENCODED>/<session-uuid>/` for a top-level session jsonl,
/// or `None` if the session has no sidecar. The sidecar is named after the session
/// uuid (the jsonl basename without `.jsonl`).
#[must_use]
pub fn sidecar_dir_for_session(session_jsonl: &Path) -> Option<PathBuf> {
    let stem = session_jsonl.file_stem()?.to_str()?;
    let parent = session_jsonl.parent()?;
    let dir = parent.join(stem);
    dir.is_dir().then_some(dir)
}

/// Discover every subagent transcript under a top-level session's sidecar dir.
///
/// Walks `<session-uuid>/subagents/` for built-in `agent-<hex>.jsonl` and
/// `<session-uuid>/subagents/workflows/wf_*/agent-<hex>.jsonl` for workflow agents.
/// **`journal.jsonl` is excluded** (it is an event log, not a transcript), as is any
/// non-`agent-*.jsonl` or `.meta.json` file. Returns an empty vec when the session
/// has no sidecar / no subagents (never an error for the common no-subagent case).
pub fn discover_subagents(session_jsonl: &Path) -> Result<Vec<Subagent>> {
    let Some(sidecar) = sidecar_dir_for_session(session_jsonl) else {
        return Ok(Vec::new());
    };
    let parent_session_id = session_jsonl
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();

    let subagents_dir = sidecar.join("subagents");
    if !subagents_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();

    // (A) built-in: direct children `agent-<hex>.jsonl` of `subagents/`.
    for p in agent_jsonls_in(&subagents_dir)? {
        out.push(make_subagent(
            p,
            SubagentKind::BuiltinTask,
            &parent_session_id,
            None,
        ));
    }

    // (B) workflow: `subagents/workflows/wf_*/agent-<hex>.jsonl`.
    let workflows_dir = subagents_dir.join("workflows");
    if workflows_dir.is_dir() {
        for wf_dir in subdirs_in(&workflows_dir)? {
            let workflow_id = wf_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string);
            for p in agent_jsonls_in(&wf_dir)? {
                out.push(make_subagent(
                    p,
                    SubagentKind::Workflow,
                    &parent_session_id,
                    workflow_id.clone(),
                ));
            }
        }
    }

    // Deterministic order: by (kind, agent_id) so output is stable across runs.
    out.sort_by(|a, b| (a.kind.label(), &a.agent_id).cmp(&(b.kind.label(), &b.agent_id)));
    Ok(out)
}

/// Just the subagent transcript file paths for a session — the surface `list` /
/// `search` need to span subagent work (no lifecycle parse). Excludes journals.
pub fn subagent_transcript_files(session_jsonl: &Path) -> Result<Vec<PathBuf>> {
    Ok(discover_subagents(session_jsonl)?
        .into_iter()
        .map(|s| s.path)
        .collect())
}

/// `agent-<hex>.jsonl` files directly inside `dir` (NOT recursing into `workflows/`,
/// and explicitly NOT `journal.jsonl` — only the `agent-` prefix qualifies).
fn agent_jsonls_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let read = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read subagents dir {}", dir.display()))?;
    for entry in read {
        let entry =
            entry.with_context(|| format!("error reading an entry in {}", dir.display()))?;
        let p = entry.path();
        let is_file = entry.file_type().map(|ft| ft.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }
        // Must be `agent-<...>.jsonl`. This naming rule is what keeps `journal.jsonl`
        // (no `agent-` prefix) out, and keeps `.meta.json` companions out (wrong ext).
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if name.starts_with("agent-") && p.extension().is_some_and(|e| e == "jsonl") {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// Immediate subdirectories of `dir` (the `wf_*` workflow dirs). Sorted.
fn subdirs_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let read = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read workflows dir {}", dir.display()))?;
    for entry in read {
        let entry =
            entry.with_context(|| format!("error reading an entry in {}", dir.display()))?;
        let p = entry.path();
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        if is_dir {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// Build a [`Subagent`], locating its `.meta.json` companion if present.
fn make_subagent(
    path: PathBuf,
    kind: SubagentKind,
    parent_session_id: &str,
    workflow_id: Option<String>,
) -> Subagent {
    // The on-disk filename stem is `agent-<hex>`, but the CANONICAL agent id — the
    // value in the transcript record's `agentId` field AND in the workflow journal's
    // `agentId` — is the bare `<hex>` WITHOUT the `agent-` prefix (verified against
    // real data). We store the bare hex so journal-completion lookup matches and the
    // id we print equals the record's own `agentId`.
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let agent_id = stem.strip_prefix("agent-").unwrap_or(stem).to_string();
    let meta_path = path.with_extension("meta.json");
    let meta_path = meta_path.is_file().then_some(meta_path);
    Subagent {
        agent_id,
        kind,
        path,
        parent_session_id: parent_session_id.to_string(),
        workflow_id,
        meta_path,
    }
}

/// `agentType` + `description` from a subagent's `meta.json`, if readable.
fn read_meta(meta_path: Option<&Path>) -> (Option<String>, Option<String>) {
    let Some(p) = meta_path else {
        return (None, None);
    };
    let Ok(bytes) = std::fs::read(p) else {
        return (None, None);
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return (None, None);
    };
    let agent_type = v
        .get("agentType")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let description = v
        .get("description")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    (agent_type, description)
}

/// True iff the workflow journal alongside a workflow subagent carries a `result`
/// event for `agent_id` (the completion signal, §C). For a built-in subagent (no
/// journal) this is always `false` — completion is inferred from the transcript.
fn journal_reports_completion(subagent: &Subagent) -> bool {
    let Some(wf_id) = &subagent.workflow_id else {
        return false;
    };
    // journal.jsonl sits beside the agent transcript inside `wf_<id>/`.
    let Some(wf_dir) = subagent.path.parent() else {
        return false;
    };
    let journal = wf_dir.join("journal.jsonl");
    let _ = wf_id; // wf_id already implied by wf_dir; kept for clarity/debugging.
    let Ok(bytes) = std::fs::read(&journal) else {
        return false;
    };
    // Each line is a small event object; scan for a `result` event matching agentId.
    for line in bytes.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        let is_result = v.get("type").and_then(serde_json::Value::as_str) == Some("result");
        let same_agent =
            v.get("agentId").and_then(serde_json::Value::as_str) == Some(&subagent.agent_id);
        if is_result && same_agent {
            return true;
        }
    }
    false
}

/// Compute the lifecycle of one subagent: read its transcript HEAD for the start
/// timestamp + TAIL for the completion timestamp & terminal-message signal, consult
/// the workflow journal for an explicit `result`, then resolve a status.
pub fn lifecycle(subagent: &Subagent) -> Result<SubagentLifecycle> {
    let (agent_type, description) = read_meta(subagent.meta_path.as_deref());

    // HEAD: first record's timestamp == start. We do not need genuine-user logic
    // here — the very first record (isSidechain user seed) IS the start instant.
    let mut started_utc: Option<String> = None;
    let head_skipped = head_records(&subagent.path, |rec| {
        if let Some(ts) = &rec.timestamp {
            started_utc = Some(ts.clone());
            return false; // first timestamped record is enough
        }
        true
    })?;

    // TAIL: last record's timestamp == completion (best-effort), and whether the
    // transcript terminates with a visible assistant message (a clean finish).
    let mut completed_utc: Option<String> = None;
    let mut terminal_agent_msg = false;
    let mut saw_any = false;
    let tail_skipped = tail_records(&subagent.path, |rec| {
        saw_any = true;
        if completed_utc.is_none() {
            if let Some(ts) = &rec.timestamp {
                completed_utc = Some(ts.clone());
            }
        }
        // The newest assistant record carrying visible text == a clean end-of-turn.
        if !terminal_agent_msg && rec.agent_text().is_some() {
            terminal_agent_msg = true;
        }
        // Stop once we have the completion timestamp AND a terminal-message verdict;
        // if the very newest record has no text we still only need a couple of reads.
        completed_utc.is_none() || !terminal_agent_msg
    })?;

    let journal_done = journal_reports_completion(subagent);
    let status = resolve_status(
        saw_any,
        journal_done,
        terminal_agent_msg,
        started_utc.is_some(),
    );

    Ok(SubagentLifecycle {
        agent_id: subagent.agent_id.clone(),
        kind: subagent.kind,
        parent_session_id: subagent.parent_session_id.clone(),
        workflow_id: subagent.workflow_id.clone(),
        agent_type,
        description,
        started_utc,
        completed_utc,
        status,
        skipped_lines: head_skipped + tail_skipped,
    })
}

/// Status resolution rule (honest, never over-claiming "failed"):
/// - journal `result` event present ⇒ `Completed` (the authoritative workflow signal);
/// - else a terminal visible-assistant message ⇒ `Completed` (clean transcript end);
/// - else if we saw records but no completion signal ⇒ `Running`;
/// - else (no records / no start) ⇒ `Unknown`.
fn resolve_status(
    saw_any: bool,
    journal_done: bool,
    terminal_agent_msg: bool,
    has_start: bool,
) -> SubagentStatus {
    if journal_done || terminal_agent_msg {
        SubagentStatus::Completed
    } else if saw_any && has_start {
        SubagentStatus::Running
    } else {
        SubagentStatus::Unknown
    }
}

/// Human-readable duration between start and completion (raw UTC ISO8601), e.g.
/// `14m20s`, `3s`, `2h05m`. `None` when either bound is missing/unparseable.
#[must_use]
pub fn duration_label(started: Option<&str>, completed: Option<&str>) -> Option<String> {
    let s: jiff::Timestamp = started?.parse().ok()?;
    let c: jiff::Timestamp = completed?.parse().ok()?;
    let secs = (c.as_second() - s.as_second()).max(0);
    Some(fmt_secs(secs))
}

/// Format a whole-second duration compactly.
fn fmt_secs(total: i64) -> String {
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}h{m:02}m")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A scratch projects-root mimicking the real on-disk layout, removed on drop.
    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let n = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("csift-sub-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            Fixture { root }
        }

        fn write(&self, rel: &str, contents: &str) -> PathBuf {
            let p = self.root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            let mut f = std::fs::File::create(&p).unwrap();
            f.write_all(contents.as_bytes()).unwrap();
            p
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    const SESS: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

    /// Build a session jsonl + its sidecar with one built-in, one workflow agent,
    /// and a workflow journal (which must be ignored as a transcript).
    fn layout(fx: &Fixture) -> PathBuf {
        let enc = "-Users-testuser-Projects-foo";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );

        // (A) built-in agent transcript + meta.
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aaa111.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"aaa111\",\"sessionId\":\"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"do the thing\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:03:20.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"done\"}]}}\n"
            ),
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-aaa111.meta.json"),
            "{\"agentType\":\"oh-my-claudecode:executor\",\"description\":\"run it\",\"toolUseId\":\"toolu_x\"}",
        );

        // (B) workflow agent transcript + meta + (C) journal with a result event.
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_abc/agent-bbb222.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"bbb222\",\"timestamp\":\"2026-06-07T06:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"wf task\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T06:01:00.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_abc/agent-bbb222.meta.json"),
            "{\"agentType\":\"workflow-subagent\"}",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_abc/journal.jsonl"),
            concat!(
                "{\"type\":\"started\",\"agentId\":\"bbb222\",\"key\":\"v2:abc\"}\n",
                "{\"type\":\"result\",\"agentId\":\"bbb222\",\"key\":\"v2:abc\",\"result\":\"ok\"}\n"
            ),
        );

        session
    }

    #[test]
    fn discovers_both_kinds_excludes_journal() {
        let fx = Fixture::new();
        let session = layout(&fx);
        let subs = discover_subagents(&session).unwrap();
        // Exactly two transcripts; the journal must NOT appear.
        assert_eq!(subs.len(), 2, "got: {subs:?}");
        let builtin = subs
            .iter()
            .find(|s| s.kind == SubagentKind::BuiltinTask)
            .expect("a builtin");
        // The id is the bare hex (record/journal `agentId`), NOT the `agent-` stem.
        assert_eq!(builtin.agent_id, "aaa111");
        let wf = subs
            .iter()
            .find(|s| s.kind == SubagentKind::Workflow)
            .expect("a workflow");
        assert_eq!(wf.agent_id, "bbb222");
        assert_eq!(wf.workflow_id.as_deref(), Some("wf_abc"));
        // None of the discovered paths is a journal.
        assert!(subs.iter().all(|s| !s.path.ends_with("journal.jsonl")));
    }

    #[test]
    fn transcript_files_helper_excludes_journal_and_meta() {
        let fx = Fixture::new();
        let session = layout(&fx);
        let files = subagent_transcript_files(&session).unwrap();
        assert_eq!(files.len(), 2);
        for f in &files {
            let name = f.file_name().unwrap().to_str().unwrap();
            assert!(name.starts_with("agent-"), "unexpected file: {name}");
            assert!(name.ends_with(".jsonl"));
            assert_ne!(name, "journal.jsonl");
        }
    }

    #[test]
    fn lifecycle_builtin_from_transcript_terminal_message() {
        let fx = Fixture::new();
        let session = layout(&fx);
        let subs = discover_subagents(&session).unwrap();
        let builtin = subs
            .iter()
            .find(|s| s.kind == SubagentKind::BuiltinTask)
            .unwrap();
        let lc = lifecycle(builtin).unwrap();
        assert_eq!(lc.started_utc.as_deref(), Some("2026-06-07T05:00:00.000Z"));
        assert_eq!(
            lc.completed_utc.as_deref(),
            Some("2026-06-07T05:03:20.000Z")
        );
        // Terminal assistant text ⇒ completed (no journal for a built-in).
        assert_eq!(lc.status, SubagentStatus::Completed);
        assert_eq!(lc.agent_type.as_deref(), Some("oh-my-claudecode:executor"));
        assert_eq!(lc.description.as_deref(), Some("run it"));
    }

    #[test]
    fn lifecycle_workflow_completion_from_journal_result() {
        let fx = Fixture::new();
        let session = layout(&fx);
        let subs = discover_subagents(&session).unwrap();
        let wf = subs
            .iter()
            .find(|s| s.kind == SubagentKind::Workflow)
            .unwrap();
        let lc = lifecycle(wf).unwrap();
        // The workflow transcript ends on a tool_use (no terminal text), but the
        // journal carries a `result` event ⇒ completed.
        assert_eq!(lc.status, SubagentStatus::Completed);
        assert_eq!(lc.agent_type.as_deref(), Some("workflow-subagent"));
        assert_eq!(lc.started_utc.as_deref(), Some("2026-06-07T06:00:00.000Z"));
        assert_eq!(
            lc.completed_utc.as_deref(),
            Some("2026-06-07T06:01:00.000Z")
        );
    }

    #[test]
    fn running_when_no_completion_signal() {
        let fx = Fixture::new();
        let enc = "-Users-testuser-Projects-bar";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        // A subagent whose transcript ends mid-tool with NO journal result.
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-ccc333.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"ccc333\",\"timestamp\":\"2026-06-07T07:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"start\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T07:00:10.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
        let subs = discover_subagents(&session).unwrap();
        let lc = lifecycle(&subs[0]).unwrap();
        assert_eq!(lc.status, SubagentStatus::Running);
    }

    #[test]
    fn no_sidecar_is_empty_not_error() {
        let fx = Fixture::new();
        let session = fx.write(
            &format!("-Users-x/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        assert!(discover_subagents(&session).unwrap().is_empty());
        assert!(subagent_transcript_files(&session).unwrap().is_empty());
    }

    #[test]
    fn duration_label_formats() {
        assert_eq!(
            duration_label(
                Some("2026-06-07T05:00:00.000Z"),
                Some("2026-06-07T05:03:20.000Z")
            )
            .as_deref(),
            Some("3m20s")
        );
        assert_eq!(
            duration_label(Some("2026-06-07T05:00:00Z"), Some("2026-06-07T07:05:00Z")).as_deref(),
            Some("2h05m")
        );
        assert_eq!(
            duration_label(Some("2026-06-07T05:00:00Z"), Some("2026-06-07T05:00:03Z")).as_deref(),
            Some("3s")
        );
        assert!(duration_label(None, Some("2026-06-07T05:00:00Z")).is_none());
    }

    #[test]
    fn kind_and_status_labels() {
        assert_eq!(SubagentKind::BuiltinTask.label(), "builtin-task");
        assert_eq!(SubagentKind::Workflow.label(), "workflow");
        assert_eq!(SubagentStatus::Completed.label(), "completed");
        assert_eq!(SubagentStatus::Running.label(), "running");
        assert_eq!(SubagentStatus::Unknown.label(), "unknown");
    }

    // ── Branch-completeness ──

    #[test]
    fn sidecar_dir_none_for_pathological_paths() {
        // A path with no file stem (root) → the `file_stem()?` None arm.
        assert!(sidecar_dir_for_session(Path::new("/")).is_none());
        // A bare relative filename has no parent dir component that is a real dir, and
        // the sidecar dir won't exist → None.
        assert!(sidecar_dir_for_session(Path::new("nonexistent.jsonl")).is_none());
    }

    #[test]
    fn discover_empty_when_sidecar_exists_but_no_subagents_dir() {
        // The sidecar `<uuid>/` dir exists but has NO `subagents/` child → empty, not
        // an error (the `!subagents_dir.is_dir()` early return).
        let fx = Fixture::new();
        let enc = "-Users-x";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        // Create the sidecar dir (named after the uuid) but with an unrelated child,
        // NOT a `subagents/` dir.
        fx.write(&format!("{enc}/{SESS}/other/file.txt"), "x");
        assert!(discover_subagents(&session).unwrap().is_empty());
    }

    #[test]
    fn discover_ignores_stray_file_under_workflows() {
        // A non-directory entry sitting directly under `subagents/workflows/` must be
        // skipped by `subdirs_in` (the `if is_dir` FALSE arm). Only the real wf_* dir
        // contributes a workflow agent.
        let fx = Fixture::new();
        let enc = "-Users-strayfile";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        // A real workflow dir with an agent.
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_real/agent-kkk111.jsonl"),
            "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"kkk111\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
        );
        // A STRAY FILE (not a dir) directly under workflows/ → must be ignored.
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/stray-not-a-dir.txt"),
            "ignore me",
        );
        let subs = discover_subagents(&session).unwrap();
        assert_eq!(
            subs.len(),
            1,
            "only the real wf agent; stray file ignored: {subs:?}"
        );
        assert_eq!(subs[0].kind, SubagentKind::Workflow);
        assert_eq!(subs[0].workflow_id.as_deref(), Some("wf_real"));
    }

    #[test]
    fn discover_handles_subagents_dir_without_workflows() {
        // `subagents/` exists with a built-in agent but NO `workflows/` subdir (the
        // `workflows_dir.is_dir()` false arm).
        let fx = Fixture::new();
        let enc = "-Users-y";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-ddd444.jsonl"),
            "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"ddd444\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
        );
        let subs = discover_subagents(&session).unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].kind, SubagentKind::BuiltinTask);
        assert!(subs[0].workflow_id.is_none());
    }

    #[test]
    fn read_meta_none_for_missing_unreadable_and_malformed() {
        // No meta path at all → (None, None) (the `let Some(p) else` arm).
        assert_eq!(read_meta(None), (None, None));
        let fx = Fixture::new();
        // A path that does not exist → the `std::fs::read` Err arm.
        let missing = fx.root.join("does-not-exist.meta.json");
        assert_eq!(read_meta(Some(&missing)), (None, None));
        // A file with invalid JSON → the `serde_json::from_slice` Err arm.
        let bad = fx.write("bad.meta.json", "{ not valid json");
        assert_eq!(read_meta(Some(&bad)), (None, None));
        // Valid JSON but WITHOUT agentType/description keys → both None.
        let empty = fx.write("empty.meta.json", "{\"toolUseId\":\"x\"}");
        assert_eq!(read_meta(Some(&empty)), (None, None));
    }

    #[test]
    fn meta_without_agent_type_yields_none_label() {
        // A workflow agent whose meta.json lacks agentType → agent_type None on the
        // lifecycle, exercising read_meta's both-keys-absent path end to end.
        let fx = Fixture::new();
        let enc = "-Users-z";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-eee555.jsonl"),
            "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"eee555\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
        );
        // meta.json present but empty object.
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-eee555.meta.json"),
            "{}",
        );
        let subs = discover_subagents(&session).unwrap();
        let lc = lifecycle(&subs[0]).unwrap();
        assert!(lc.agent_type.is_none());
        assert!(lc.description.is_none());
    }

    #[test]
    fn journal_completion_false_for_builtin_no_workflow() {
        // A built-in subagent has no workflow_id → journal_reports_completion is false
        // via the `let Some(wf_id) else` arm (reached through lifecycle: a built-in
        // ending on a tool_use with no terminal text → Running, proving no journal).
        let fx = Fixture::new();
        let enc = "-Users-nojournal";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-fff666.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"fff666\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:00:10.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
        let subs = discover_subagents(&session).unwrap();
        let lc = lifecycle(&subs[0]).unwrap();
        assert_eq!(lc.status, SubagentStatus::Running);
    }

    #[test]
    fn journal_completion_handles_missing_malformed_and_nonmatching() {
        // A workflow subagent whose journal exists but carries ONLY a `started` event
        // (no `result`) AND a malformed line AND a result for a DIFFERENT agent →
        // completion not reported from the journal; the transcript ends on a tool_use
        // (no terminal text) → Running. Exercises the journal scan's continue arms
        // (blank line, malformed line, non-result type, wrong agentId) + the
        // fall-off-the-end `false`.
        let fx = Fixture::new();
        let enc = "-Users-wfjournal";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_z/agent-ggg777.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"ggg777\",\"timestamp\":\"2026-06-07T06:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T06:00:30.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_z/journal.jsonl"),
            concat!(
                "\n",                                                   // blank line → continue
                "{ this is not valid json }\n",                         // malformed → continue
                "{\"type\":\"started\",\"agentId\":\"ggg777\"}\n",      // non-result → no match
                "{\"type\":\"result\",\"agentId\":\"someone-else\"}\n" // result, wrong agent → no match
            ),
        );
        let subs = discover_subagents(&session).unwrap();
        let lc = lifecycle(&subs[0]).unwrap();
        assert_eq!(
            lc.status,
            SubagentStatus::Running,
            "no matching result → not completed"
        );
    }

    #[test]
    fn journal_completion_false_when_journal_absent() {
        // A workflow subagent with NO journal.jsonl at all → the `std::fs::read` Err
        // arm. The transcript here ends with terminal assistant text, so the AGENT is
        // still Completed (via transcript), but the journal path itself returns false.
        let fx = Fixture::new();
        let enc = "-Users-wfnojournal";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_n/agent-hhh888.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"hhh888\",\"timestamp\":\"2026-06-07T06:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T06:00:30.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"done cleanly\"}]}}\n"
            ),
        );
        // intentionally NO journal.jsonl
        let subs = discover_subagents(&session).unwrap();
        let lc = lifecycle(&subs[0]).unwrap();
        // Completed via the transcript terminal message, not the (absent) journal.
        assert_eq!(lc.status, SubagentStatus::Completed);
    }

    #[test]
    fn lifecycle_unknown_when_no_timestamps() {
        // A transcript whose records carry NO timestamp at all → started_utc stays
        // None, no terminal text, no journal → status Unknown (resolve_status's else
        // arm). The head scan also exercises the `if let Some(ts)` false arm (record
        // with no timestamp) before falling off the end.
        let fx = Fixture::new();
        let enc = "-Users-nots";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-iii999.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"iii999\",\"message\":{\"role\":\"user\",\"content\":\"no ts\"}}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Bash\",\"input\":{}}]}}\n"
            ),
        );
        let subs = discover_subagents(&session).unwrap();
        let lc = lifecycle(&subs[0]).unwrap();
        assert!(lc.started_utc.is_none());
        assert_eq!(lc.status, SubagentStatus::Unknown);
    }

    #[test]
    fn lifecycle_tail_terminal_flag_short_circuits_on_later_record() {
        // To reach the `!terminal_agent_msg` FALSE arm of the tail guard, the NEWEST
        // record must set terminal_agent_msg=true but leave completed_utc still None
        // (no timestamp) so the scan CONTINUES; the next (older) record is then
        // evaluated with terminal already true → `!terminal_agent_msg && …` short-
        // circuits on its false left operand.
        let fx = Fixture::new();
        let enc = "-Users-shortcirc";
        let session = fx.write(
            &format!("{enc}/{SESS}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        fx.write(
            &format!("{enc}/{SESS}/subagents/agent-jjj000.jsonl"),
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"jjj000\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"start\"}}\n",
                // older assistant WITH a timestamp (provides completed_utc later in the scan)
                "{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:00:10.000Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"older text\"}]}}\n",
                // NEWEST assistant text but NO timestamp → terminal=true, completed_utc stays None → scan continues
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"newest no-ts text\"}]}}\n"
            ),
        );
        let subs = discover_subagents(&session).unwrap();
        let lc = lifecycle(&subs[0]).unwrap();
        assert_eq!(lc.status, SubagentStatus::Completed);
        // completed_utc comes from the older timestamped record (newest had none).
        assert_eq!(
            lc.completed_utc.as_deref(),
            Some("2026-06-07T05:00:10.000Z")
        );
    }

    #[test]
    fn resolve_status_all_arms() {
        // Direct unit coverage of every resolve_status branch.
        assert_eq!(
            resolve_status(true, true, false, true),
            SubagentStatus::Completed
        ); // journal
        assert_eq!(
            resolve_status(true, false, true, true),
            SubagentStatus::Completed
        ); // terminal msg
        assert_eq!(
            resolve_status(true, false, false, true),
            SubagentStatus::Running
        ); // saw + start
        assert_eq!(
            resolve_status(false, false, false, false),
            SubagentStatus::Unknown
        ); // nothing
        assert_eq!(
            resolve_status(true, false, false, false),
            SubagentStatus::Unknown
        ); // saw but no start
    }

    #[test]
    fn duration_label_none_for_unparseable_bounds() {
        // Both bounds present but one is unparseable → None (the `.parse().ok()?` arm).
        assert!(duration_label(Some("garbage"), Some("2026-06-07T05:00:00Z")).is_none());
        assert!(duration_label(Some("2026-06-07T05:00:00Z"), Some("not-a-time")).is_none());
        // Completion before start → clamped to 0 ("0s"), never negative.
        assert_eq!(
            duration_label(Some("2026-06-07T05:00:10Z"), Some("2026-06-07T05:00:00Z")).as_deref(),
            Some("0s")
        );
    }
}
