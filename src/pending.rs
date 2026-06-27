//! `pending` subcommand — surface sessions currently BLOCKED on a human elicitation.
//!
//! Three Claude Code elicitations stall a session on a human yet are invisible or
//! ambiguous in the native transcript: **AskUserQuestion** (a pending pick is NEVER
//! flushed to jsonl — see AGENTS.md §3.4), **ExitPlanMode** (the whole turn is buffered
//! until answered), and an **MCP Elicitation** (the inner request lives in memory). A
//! Claude Code hook records each one to an append-only SIDECAR jsonl
//! (`<claude-home>/projects/<ENC>/<uuid>/elicitations.jsonl` — the same dir that holds
//! `subagents/`), writing a `phase:"pending"` line when it OPENS and a `phase:"resolved"`
//! line when it CLOSES. `pending` reads those markers and reports the still-UNRESOLVED
//! ones.
//!
//! ## Scope + sidecar resolution
//!
//! Targets resolve through the SHARED [`crate::path::resolve_session_files`] (the same one
//! `list`/`search`/… use), so a `csift pending @<uuid>` scopes to that session, a real
//! cwd / encoded dir scopes to a project, and 0 targets ⇒ every project. The resolver
//! returns transcript files (incl. subagent transcripts when spanning); for v1 the sidecar
//! lives at the SESSION sidecar dir, so each resolved file is mapped to its owning
//! TOP-LEVEL session (a subagent transcript via [`crate::subagent::parent_session_id_from_path`])
//! and each session's `elicitations.jsonl` is read exactly ONCE.
//!
//! ## Pairing semantics
//!
//! A `key` is CURRENTLY PENDING iff it has ≥1 `phase:"pending"` record and NO later
//! matching `phase:"resolved"`. We walk the file IN ORDER maintaining a per-key stack of
//! open timestamps; a `resolved` pops the most recent open for that key. This is robust to a
//! weak/duplicate key (e.g. `unknown`, or a reused MCP server name): greedy LIFO pairing. A
//! malformed line is skipped + COUNTED (the never-silent-truncation invariant, AGENTS.md §4);
//! a non-marker line (missing `csift` field / wrong `type`) is skipped silently.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use crate::cli::{OutputFormat, PendingArgs};
use crate::path::{self, SubagentScope};
use crate::timez::{format_timestamp, local_iso};

/// Max characters of a `detail` excerpt (the scannable preview cap shared with
/// `list`/`agents`; see [`crate::text::truncate_excerpt`]).
const DETAIL_MAX: usize = 200;

/// One currently-pending elicitation, ready to render.
#[derive(Debug, Clone)]
struct PendingElicitation {
    /// The owning TOP-LEVEL session uuid (always a re-feedable `@<uuid>`).
    session_id: String,
    /// `AskUserQuestion` | `ExitPlanMode` | `mcp-elicitation` (whatever the marker carried).
    kind: String,
    /// The pairing key (tool_use_id for AUQ/ExitPlanMode; elicitation_id / MCP server for MCP).
    key: String,
    /// Raw ISO8601 UTC timestamp of the OPEN (when it started blocking), if present.
    since_utc: Option<String>,
    /// A one-line human detail extracted defensively from `hookInput` (capped at [`DETAIL_MAX`]).
    detail: String,
    /// The opening hook event (e.g. `PreToolUse`), if present.
    hook_event: Option<String>,
}

/// One session's pending result: its uuid, the pending elicitations, and the malformed-line
/// skip count for that session's sidecar (never hidden).
#[derive(Debug, Clone)]
struct SessionPending {
    session_id: String,
    pending: Vec<PendingElicitation>,
    skipped_lines: usize,
}

/// Entry point for `csift pending`.
pub fn run_pending(args: &PendingArgs) -> Result<()> {
    let scope = SubagentScope::from(args.want_subagents());
    let session_files = path::resolve_session_files(&args.paths, scope, path::Caller::Other)?;

    // Map each resolved transcript to its owning TOP-LEVEL session jsonl (a subagent
    // transcript → its parent <uuid>.jsonl), deduping so each sidecar is read once. The
    // sidecar always lives beside the TOP-LEVEL session file.
    let mut top_level: Vec<PathBuf> = session_files
        .iter()
        .map(|p| top_level_session_jsonl(p))
        .collect();
    top_level.sort();
    top_level.dedup();

    // Read + pair each session's sidecar.
    let mut results: Vec<SessionPending> = top_level
        .iter()
        .map(|p| read_session_pending(p))
        .collect::<Result<Vec<_>>>()?;
    // Deterministic order regardless of FS / dedup order: by session id.
    results.sort_by(|a, b| a.session_id.cmp(&b.session_id));

    match args.format {
        OutputFormat::Text => render_text(&results),
        OutputFormat::Json => render_json(&results)?,
    }
    Ok(())
}

/// The owning top-level session `<uuid>.jsonl` for a resolved transcript path. For a
/// subagent transcript (`…/<PARENT-UUID>/subagents/…/agent-<hex>.jsonl`) this is the parent
/// `<PARENT-UUID>.jsonl` (the sidecar lives beside the parent). For a top-level file it is
/// the file itself.
fn top_level_session_jsonl(path: &Path) -> PathBuf {
    if !crate::subagent::is_subagent_path(path) {
        return path.to_path_buf();
    }
    // Find the ancestor named `subagents`; the `<PARENT-UUID>` sidecar dir is its PARENT, and
    // the top-level session file is that dir's sibling `<PARENT-UUID>.jsonl`. (Walking from the
    // file alone is wrong: between the file and `subagents` sit `workflows/wf_*` for a workflow
    // subagent, so the sidecar dir is NOT simply `path`'s grandparent.)
    let mut cur = path;
    while let Some(parent) = cur.parent() {
        if cur.file_name().and_then(|s| s.to_str()) == Some("subagents") {
            // `parent` is the `<PARENT-UUID>` sidecar dir; its sibling `<PARENT-UUID>.jsonl`
            // is the top-level session file.
            if let Some(stem) = parent.file_name() {
                let mut jsonl = parent.to_path_buf();
                jsonl.set_file_name(format!("{}.jsonl", stem.to_string_lossy()));
                return jsonl;
            }
        }
        cur = parent;
    }
    path.to_path_buf()
}

/// Read + pair one session's `elicitations.jsonl` sidecar. A missing dir / file ⇒ an empty
/// result (no elicitations ever — never an error). A plain read suffices: these sidecars are
/// tiny (one short line per elicitation open/close), so the mmap+memchr machinery the big
/// transcripts need is unnecessary here — but we STILL count every malformed line.
fn read_session_pending(session_jsonl: &Path) -> Result<SessionPending> {
    let session_id = crate::subagent::session_id_from_path(session_jsonl);
    let Some(sidecar_path) = sidecar_file_for_session(session_jsonl) else {
        return Ok(SessionPending {
            session_id,
            pending: Vec::new(),
            skipped_lines: 0,
        });
    };
    // The dir may not exist (no elicitations ever) → treat as empty, not an error.
    let contents = match std::fs::read_to_string(&sidecar_path) {
        Ok(c) => c,
        Err(_) => {
            return Ok(SessionPending {
                session_id,
                pending: Vec::new(),
                skipped_lines: 0,
            });
        }
    };
    let (pending, skipped_lines) = pair_elicitations(&contents, &session_id);
    Ok(SessionPending {
        session_id,
        pending,
        skipped_lines,
    })
}

/// The `elicitations.jsonl` path for a top-level session, regardless of whether the dir
/// currently exists (we read it tolerantly). The sidecar dir is `<ENC>/<uuid>/` (the same
/// dir that would hold `subagents/`); the marker file is `elicitations.jsonl` inside it.
fn sidecar_file_for_session(session_jsonl: &Path) -> Option<PathBuf> {
    let stem = session_jsonl.file_stem()?.to_str()?;
    let parent = session_jsonl.parent()?;
    Some(parent.join(stem).join("elicitations.jsonl"))
}

/// One parsed open record carried on the per-key open stack (LIFO).
#[derive(Debug, Clone)]
struct OpenRecord {
    kind: String,
    since_utc: Option<String>,
    detail: String,
    hook_event: Option<String>,
}

/// Walk a sidecar's lines IN ORDER and return (still-pending elicitations, malformed-line
/// count). Per-key LIFO pairing: a `pending` pushes an open; a `resolved` pops the most
/// recent open for that key (robust to a weak/reused key). Whatever opens remain after the
/// walk are currently pending. Non-marker lines (wrong `type` / missing `csift`) are skipped
/// silently; a line that is neither valid JSON nor an empty line is malformed → counted.
fn pair_elicitations(contents: &str, session_id: &str) -> (Vec<PendingElicitation>, usize) {
    // Preserve first-open order across keys so the output is stable + meaningful: a Vec of
    // (key, stack-of-opens) rather than an unordered map, with a side index for O(1) lookup.
    let mut order: Vec<String> = Vec::new();
    let mut stacks: HashMap<String, Vec<OpenRecord>> = HashMap::new();
    let mut skipped = 0usize;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue; // blank line — not malformed, not a record.
        }
        let value: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => {
                skipped += 1; // never silent: a broken line is COUNTED.
                continue;
            }
        };
        if !is_marker(&value) {
            continue; // a non-csift-elicitation line — skip silently.
        }
        let phase = value.get("phase").and_then(Value::as_str).unwrap_or("");
        let key = value
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        match phase {
            "pending" => {
                let open = OpenRecord {
                    kind: value
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string(),
                    since_utc: value
                        .get("timestamp")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    detail: extract_detail(&value),
                    hook_event: value
                        .get("hookEvent")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                };
                let stack = stacks.entry(key.clone()).or_insert_with(|| {
                    order.push(key.clone());
                    Vec::new()
                });
                stack.push(open);
            }
            "resolved" => {
                // Pop the most recent open for this key (LIFO). A `resolved` with no matching
                // open is a no-op (a duplicate close / a close for an open in a prior run we
                // never saw) — tolerated, never a crash.
                if let Some(stack) = stacks.get_mut(&key) {
                    stack.pop();
                }
            }
            _ => {
                // An unknown phase value on an otherwise-valid marker — ignore the line (it is
                // neither an open nor a close). Not malformed JSON, so not counted.
            }
        }
    }

    // Whatever opens remain are currently pending. Emit in first-open key order, and within a
    // key in open order (the stack is already oldest→newest).
    let mut pending = Vec::new();
    for key in &order {
        if let Some(stack) = stacks.get(key) {
            for open in stack {
                pending.push(PendingElicitation {
                    session_id: session_id.to_string(),
                    kind: open.kind.clone(),
                    key: key.clone(),
                    since_utc: open.since_utc.clone(),
                    detail: open.detail.clone(),
                    hook_event: open.hook_event.clone(),
                });
            }
        }
    }
    (pending, skipped)
}

/// True when `value` is a csift-elicitation marker line: it carries the `csift` provenance
/// field AND `type:"csift-elicitation"`. Anything else (a foreign line that happened to land
/// in the file) is skipped silently.
fn is_marker(value: &Value) -> bool {
    value.get("csift").and_then(Value::as_str).is_some()
        && value.get("type").and_then(Value::as_str) == Some("csift-elicitation")
}

/// Extract a one-line human `detail` from a marker's `hookInput`, DEFENSIVELY (every field
/// optional; `hookInput` internals vary by elicitation kind). Capped at [`DETAIL_MAX`] via
/// the shared truncation helpers (never hand-rolled). Shapes (per the hook payload):
/// - AskUserQuestion: `hookInput.tool_input.questions[]` → first question text (+ `(N questions)` if >1).
/// - ExitPlanMode: `hookInput.tool_input.plan` → `[plan: <first chars>]`.
/// - mcp-elicitation: `hookInput.mcp_server_name` + `.message` + `.mode` → `<server>: <message> (mode)`.
fn extract_detail(value: &Value) -> String {
    let kind = value.get("kind").and_then(Value::as_str).unwrap_or("");
    let hook_input = value.get("hookInput");
    match kind {
        "AskUserQuestion" => detail_askuserquestion(hook_input),
        "ExitPlanMode" => detail_exitplanmode(hook_input),
        "mcp-elicitation" => detail_mcp(hook_input),
        _ => {
            // Unknown kind — fall back to whatever a `message` field offers, else empty.
            let msg = hook_input
                .and_then(|h| h.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("");
            crate::text::collapse_and_truncate(msg, DETAIL_MAX)
        }
    }
}

fn detail_askuserquestion(hook_input: Option<&Value>) -> String {
    let questions = hook_input
        .and_then(|h| h.get("tool_input"))
        .and_then(|t| t.get("questions"))
        .and_then(Value::as_array);
    let Some(questions) = questions else {
        return String::new();
    };
    // The first question's text — questions[].question is the documented field; fall back to
    // a bare string element or a `header`/`prompt` field defensively.
    let first = questions.first();
    let text = first
        .and_then(|q| {
            q.get("question")
                .or_else(|| q.get("header"))
                .or_else(|| q.get("prompt"))
                .and_then(Value::as_str)
                .or_else(|| q.as_str())
        })
        .unwrap_or("");
    let body = crate::text::collapse_and_truncate(text, DETAIL_MAX);
    if questions.len() > 1 {
        format!("{body} ({} questions)", questions.len())
    } else {
        body
    }
}

fn detail_exitplanmode(hook_input: Option<&Value>) -> String {
    let plan = hook_input
        .and_then(|h| h.get("tool_input"))
        .and_then(|t| t.get("plan"))
        .and_then(Value::as_str)
        .unwrap_or("");
    // `[plan: <first ~80 chars>]` — collapse + truncate the plan body to a short pointer.
    let body = crate::text::collapse_and_truncate(plan, 80);
    format!("[plan: {body}]")
}

fn detail_mcp(hook_input: Option<&Value>) -> String {
    let server = hook_input
        .and_then(|h| h.get("mcp_server_name"))
        .and_then(Value::as_str)
        .unwrap_or("mcp");
    let message = hook_input
        .and_then(|h| h.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let msg = crate::text::collapse_and_truncate(message, 80);
    match hook_input
        .and_then(|h| h.get("mode"))
        .and_then(Value::as_str)
    {
        Some(mode) if !mode.is_empty() => format!("{server}: {msg} ({mode})"),
        _ => format!("{server}: {msg}"),
    }
}

// ── Text rendering ──

fn render_text(results: &[SessionPending]) {
    let any_pending = results.iter().any(|s| !s.pending.is_empty());
    let any_skipped = results.iter().any(|s| s.skipped_lines > 0);
    if !any_pending {
        println!("no pending elicitations");
        // Still surface any malformed-line skips even when nothing is pending (never silent).
        for s in results {
            if s.skipped_lines > 0 {
                println!(
                    "note  SESSION {}  {}",
                    s.session_id,
                    crate::text::malformed_note(s.skipped_lines)
                );
            }
        }
        return;
    }
    let mut first = true;
    for s in results {
        if s.pending.is_empty() {
            // A session with only malformed lines (and no pending) still reports its skip
            // note below in the trailing pass; nothing to print in the per-session block.
            continue;
        }
        if !first {
            println!();
        }
        first = false;
        println!("SESSION {}", s.session_id);
        for p in &s.pending {
            println!(
                "  ⏳ {}  since {}  (key {})",
                p.kind,
                format_timestamp(p.since_utc.as_deref()),
                p.key
            );
            println!(
                "     {}",
                crate::text::truncate_excerpt(&p.detail, DETAIL_MAX)
            );
        }
        if s.skipped_lines > 0 {
            println!("  note  {}", crate::text::malformed_note(s.skipped_lines));
        }
    }
    // A session that had ONLY malformed lines (no pending) is skipped in the loop above; its
    // skip note must still surface (never silent).
    if any_skipped {
        for s in results {
            if s.pending.is_empty() && s.skipped_lines > 0 {
                println!(
                    "note  SESSION {}  {}",
                    s.session_id,
                    crate::text::malformed_note(s.skipped_lines)
                );
            }
        }
    }
}

// ── JSON rendering (NDJSON: one object per currently-pending elicitation) ──

fn render_json(results: &[SessionPending]) -> Result<()> {
    use serde_json::json;
    let mut sessions_with_pending = 0usize;
    let mut total_pending = 0usize;
    let mut total_skipped = 0usize;
    for s in results {
        total_skipped += s.skipped_lines;
        if !s.pending.is_empty() {
            sessions_with_pending += 1;
        }
        for p in &s.pending {
            total_pending += 1;
            let since_local = p.since_utc.as_deref().and_then(local_iso);
            let obj = json!({
                "session_id": p.session_id,
                "kind": p.kind,
                "key": p.key,
                "since_utc": p.since_utc,
                "since_local": since_local,
                "detail": crate::text::truncate_excerpt(&p.detail, DETAIL_MAX),
                "hook_event": p.hook_event,
            });
            println!("{}", serde_json::to_string(&obj)?);
        }
    }
    // Trailing untagged summary (LAST line) — a small accounting record so a JSON consumer
    // learns the totals + any malformed-line skips WITHOUT recounting (mirrors search's
    // trailer; the never-silent-truncation invariant surfaces `skipped_lines` here).
    let summary = json!({
        "sessions": sessions_with_pending,
        "pending": total_pending,
        "skipped_lines": total_skipped,
    });
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker_pending(kind: &str, key: &str, ts: &str, detail_field: &str) -> String {
        format!(
            r#"{{"type":"csift-elicitation","csift":"elicitation-marker-v1","phase":"pending","kind":"{kind}","key":"{key}","hookEvent":"PreToolUse","timestamp":"{ts}","hookInput":{detail_field}}}"#
        )
    }

    fn marker_resolved(kind: &str, key: &str, ts: &str) -> String {
        format!(
            r#"{{"type":"csift-elicitation","csift":"elicitation-marker-v1","phase":"resolved","kind":"{kind}","key":"{key}","hookEvent":"PostToolUse","timestamp":"{ts}"}}"#
        )
    }

    #[test]
    fn unresolved_pending_is_surfaced() {
        let aq = marker_pending(
            "AskUserQuestion",
            "toolu_AQ1",
            "2026-06-27T01:02:03.000Z",
            r#"{"tool_input":{"questions":[{"question":"Pick a branch?"}]}}"#,
        );
        let (pending, skipped) = pair_elicitations(&aq, "sess-1");
        assert_eq!(skipped, 0);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, "AskUserQuestion");
        assert_eq!(pending[0].key, "toolu_AQ1");
        assert_eq!(pending[0].detail, "Pick a branch?");
        assert_eq!(
            pending[0].since_utc.as_deref(),
            Some("2026-06-27T01:02:03.000Z")
        );
    }

    #[test]
    fn resolved_pair_is_not_surfaced() {
        let lines = format!(
            "{}\n{}",
            marker_pending(
                "AskUserQuestion",
                "toolu_AQ1",
                "2026-06-27T01:02:03.000Z",
                r#"{"tool_input":{"questions":[{"question":"q"}]}}"#,
            ),
            marker_resolved("AskUserQuestion", "toolu_AQ1", "2026-06-27T01:05:00.000Z"),
        );
        let (pending, skipped) = pair_elicitations(&lines, "sess-1");
        assert_eq!(skipped, 0);
        assert!(pending.is_empty(), "an open+close pair must NOT be pending");
    }

    #[test]
    fn malformed_line_is_skipped_and_counted() {
        let lines = format!(
            "{}\n{}\n{}",
            "this is { not valid json",
            marker_pending(
                "AskUserQuestion",
                "toolu_AQ1",
                "2026-06-27T01:02:03.000Z",
                r#"{"tool_input":{"questions":[{"question":"q"}]}}"#,
            ),
            "{ also broken",
        );
        let (pending, skipped) = pair_elicitations(&lines, "sess-1");
        assert_eq!(skipped, 2, "two broken lines counted, never silent");
        assert_eq!(pending.len(), 1, "the valid pending still surfaces");
    }

    #[test]
    fn non_marker_lines_are_skipped_silently() {
        // A valid JSON line that is NOT a csift-elicitation marker (a foreign record) →
        // skipped, NOT counted as malformed.
        let lines = format!(
            "{}\n{}",
            r#"{"type":"user","message":{"role":"user","content":"hi"}}"#,
            marker_pending(
                "ExitPlanMode",
                "toolu_PLAN",
                "2026-06-27T02:00:00.000Z",
                r#"{"tool_input":{"plan":"Step 1. Do the thing. Step 2. Verify."}}"#,
            ),
        );
        let (pending, skipped) = pair_elicitations(&lines, "sess-1");
        assert_eq!(skipped, 0, "a non-marker is skipped silently, not counted");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, "ExitPlanMode");
        assert!(
            pending[0].detail.starts_with("[plan: Step 1."),
            "got: {}",
            pending[0].detail
        );
    }

    #[test]
    fn mcp_elicitation_detail() {
        let line = marker_pending(
            "mcp-elicitation",
            "github",
            "2026-06-27T03:00:00.000Z",
            r#"{"mcp_server_name":"github","message":"Approve the push?","mode":"confirm"}"#,
        );
        let (pending, _skipped) = pair_elicitations(&line, "sess-1");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, "mcp-elicitation");
        assert_eq!(pending[0].detail, "github: Approve the push? (confirm)");
    }

    #[test]
    fn auq_multi_question_detail_marks_count() {
        let line = marker_pending(
            "AskUserQuestion",
            "toolu_AQ2",
            "2026-06-27T01:02:03.000Z",
            r#"{"tool_input":{"questions":[{"question":"First?"},{"question":"Second?"}]}}"#,
        );
        let (pending, _) = pair_elicitations(&line, "sess-1");
        assert_eq!(pending[0].detail, "First? (2 questions)");
    }

    #[test]
    fn duplicate_key_lifo_pairing_leaves_one_open() {
        // Two opens, one close on the SAME (weak) key → one still pending (LIFO pop of the
        // most-recent open). Robust to a reused MCP server name / `unknown` key.
        let lines = format!(
            "{}\n{}\n{}",
            marker_pending(
                "mcp-elicitation",
                "shared",
                "2026-06-27T01:00:00.000Z",
                r#"{"mcp_server_name":"shared","message":"first"}"#,
            ),
            marker_pending(
                "mcp-elicitation",
                "shared",
                "2026-06-27T02:00:00.000Z",
                r#"{"mcp_server_name":"shared","message":"second"}"#,
            ),
            marker_resolved("mcp-elicitation", "shared", "2026-06-27T02:30:00.000Z"),
        );
        let (pending, _) = pair_elicitations(&lines, "sess-1");
        assert_eq!(pending.len(), 1, "two opens minus one close = one pending");
        // The remaining open is the OLDEST (the LIFO close popped the newest).
        assert_eq!(
            pending[0].since_utc.as_deref(),
            Some("2026-06-27T01:00:00.000Z")
        );
    }

    #[test]
    fn resolved_without_open_is_a_noop() {
        let line = marker_resolved("AskUserQuestion", "ghost", "2026-06-27T01:00:00.000Z");
        let (pending, skipped) = pair_elicitations(&line, "sess-1");
        assert_eq!(skipped, 0);
        assert!(
            pending.is_empty(),
            "a bare close pairs to nothing, no crash"
        );
    }

    #[test]
    fn blank_lines_are_ignored() {
        let lines = format!(
            "\n{}\n\n",
            marker_pending(
                "AskUserQuestion",
                "k",
                "2026-06-27T01:00:00.000Z",
                r#"{"tool_input":{"questions":[{"question":"q"}]}}"#,
            ),
        );
        let (pending, skipped) = pair_elicitations(&lines, "sess-1");
        assert_eq!(skipped, 0, "blank lines are not malformed");
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn missing_csift_field_is_not_a_marker() {
        // type is right but the `csift` provenance field is absent → not a marker.
        let line =
            r#"{"type":"csift-elicitation","phase":"pending","kind":"AskUserQuestion","key":"k"}"#;
        assert!(!is_marker(&serde_json::from_str::<Value>(line).unwrap()));
    }

    #[test]
    fn top_level_jsonl_for_subagent_path_points_at_parent() {
        let sub = Path::new(
            "/h/projects/-Enc/11111111-2222-3333-4444-555555555555/subagents/workflows/wf_z/agent-deadbeef.jsonl",
        );
        let top = top_level_session_jsonl(sub);
        assert_eq!(
            top,
            Path::new("/h/projects/-Enc/11111111-2222-3333-4444-555555555555.jsonl")
        );
    }

    #[test]
    fn top_level_jsonl_for_top_level_path_is_itself() {
        let top = Path::new("/h/projects/-Enc/11111111-2222-3333-4444-555555555555.jsonl");
        assert_eq!(top_level_session_jsonl(top), top);
    }
}
