//! Elicitation SIDECAR merge — read the hook-written `elicitations.jsonl` and surface the
//! UNRESOLVED-pending records that are MISSING from the native transcript.
//!
//! Three Claude Code elicitations stall a session on a human yet are invisible / ambiguous
//! in the native jsonl while pending: **AskUserQuestion** and **ExitPlanMode** (CC buffers
//! the whole assistant turn until answered — nothing on disk during the wait, see §3.4) and
//! an **MCP Elicitation** (the inner request lives in memory). A Claude Code hook records
//! each one to an append-only SIDECAR jsonl beside the session
//! (`<claude-home>/projects/<ENC>/<uuid>/elicitations.jsonl`, via
//! [`crate::subagent::sidecar_dir_for_session`]) — a `csiftPhase:"pending"` line, shaped like
//! the NATIVE record CC will eventually write, when it OPENS, plus a lightweight
//! `csiftPhase:"resolved"` close marker when it CLOSES.
//!
//! csift reads the sidecar TRANSPARENTLY wherever it reads a session: the unresolved-pending
//! records are merged into the record set as if they were native (they classify naturally —
//! an AskUserQuestion/ExitPlanMode `tool_use`, an MCP system record). Once resolved, CC has
//! written the real record, so the pending is paired off and DROPPED — no duplicates. That
//! auto-dedup is the whole point.
//!
//! ## Pairing semantics
//!
//! Group the sidecar's lines by `csiftKey`. A key with a `csiftPhase:"pending"` record and NO
//! `csiftPhase:"resolved"` record is UNRESOLVED — its pending record is exactly the one
//! missing from the native transcript, so it is emitted. A malformed line is skipped +
//! COUNTED (the never-silent invariant, AGENTS.md §4); a non-marker line (no
//! `csift:"elicitation-marker-v1"`) is skipped silently. A missing sidecar dir / file ⇒ no
//! merge (never an error).
//!
//! ## Keyed by the TOP-LEVEL session
//!
//! The sidecar always lives beside the TOP-LEVEL session jsonl (the hook's `session_id` is
//! the top-level/leader uuid, never a subagent's). Callers therefore merge the sidecar only
//! when reading a top-level session file — a subagent transcript has none.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::model::Record;

/// The fixed sidecar file name inside a session's sidecar dir.
const SIDECAR_FILE: &str = "elicitations.jsonl";

/// The `elicitations.jsonl` path for a session jsonl, or `None` when the session has no
/// sidecar dir / the path has no stem. The sidecar dir is `<ENC>/<uuid>/` (the same dir that
/// holds `subagents/`); the marker file sits inside it.
#[must_use]
pub fn sidecar_path(session_jsonl: &Path) -> Option<PathBuf> {
    Some(crate::subagent::sidecar_dir_for_session(session_jsonl)?.join(SIDECAR_FILE))
}

/// Read a session's elicitation sidecar and return its UNRESOLVED-pending records (parsed as
/// native-shaped [`Record`]s, ordered by `timestamp` ascending) plus the malformed-line skip
/// count.
///
/// A missing sidecar dir / file ⇒ `(vec![], 0)` (no merge, never an error). A plain
/// `read_to_string` suffices — the sidecar is tiny (one short line per elicitation
/// open/close), so the mmap+memchr machinery the big transcripts need is unnecessary — but
/// every malformed line is STILL counted.
pub fn unresolved_pending(session_jsonl: &Path) -> Result<(Vec<Record>, usize)> {
    let Some(path) = sidecar_path(session_jsonl) else {
        return Ok((Vec::new(), 0));
    };
    // A missing file (no elicitations ever) is the common case → empty, not an error.
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return Ok((Vec::new(), 0));
    };
    Ok(pair_unresolved(&contents))
}

/// True when `path` is a csift elicitation sidecar — either by basename (`elicitations.jsonl`)
/// or by content SNIFF (a renamed / moved sidecar): every parseable non-empty line carries
/// the `csift:"elicitation-marker-v1"` marker and there is ≥1 such line and NO genuine CC
/// record. Used by the targeting rejection so the sidecar cannot be searched directly.
#[must_use]
pub fn is_sidecar_path(path: &Path) -> bool {
    if path.file_name().and_then(|s| s.to_str()) == Some(SIDECAR_FILE) {
        return true;
    }
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    content_is_sidecar(&contents)
}

/// Content sniff for [`is_sidecar_path`]: every parseable non-empty line is a
/// `csift`-marked elicitation record, with ≥1 such line and NO genuine CC record. An
/// unparseable line disqualifies (a real transcript has many heavy lines that would not all
/// parse as a small marker, but more importantly a sidecar is hook-written clean JSONL).
fn content_is_sidecar(contents: &str) -> bool {
    let mut marked = 0usize;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match crate::parse::parse_line(trimmed.as_bytes()) {
            Ok(Some(rec)) if rec.is_elicitation_marker() => marked += 1,
            // A parseable NON-marker record OR an unparseable line ⇒ this is not a pure
            // sidecar (a native transcript or a foreign file).
            _ => return false,
        }
    }
    marked > 0
}

/// One pending record awaiting pairing, kept with its raw timestamp for the final sort.
struct PendingRec {
    rec: Record,
    ts: Option<String>,
}

/// Pair a sidecar's lines by `csiftKey` and return (unresolved-pending records sorted by
/// timestamp ascending, malformed-line count). A key with a `pending` record and NO
/// `resolved` record is unresolved → its pending record is emitted (the one CC has not yet
/// written natively). A non-marker line is skipped silently; a malformed (unparseable,
/// non-blank) line is skipped + counted.
fn pair_unresolved(contents: &str) -> (Vec<Record>, usize) {
    // First pass: collect every pending record (keyed) and the set of resolved keys.
    let mut pending: HashMap<String, PendingRec> = HashMap::new();
    // Preserve first-seen key order so two un-timestamped pendings stay deterministic.
    let mut order: Vec<String> = Vec::new();
    let mut resolved: HashSet<String> = HashSet::new();
    let mut skipped = 0usize;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue; // blank — not a record, not malformed.
        }
        let rec = match crate::parse::parse_line(trimmed.as_bytes()) {
            Ok(Some(rec)) => rec,
            Ok(None) => continue, // blank-ish — skip.
            Err(_) => {
                skipped += 1; // never silent: a broken line is COUNTED.
                continue;
            }
        };
        if !rec.is_elicitation_marker() {
            continue; // a foreign / native line in the sidecar — skip silently.
        }
        let key = rec
            .csift_key
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        match rec.csift_phase.as_deref() {
            Some("resolved") => {
                resolved.insert(key);
            }
            Some("pending") => {
                let ts = rec.timestamp.clone();
                if !pending.contains_key(&key) {
                    order.push(key.clone());
                }
                // Keep the LATEST pending for a key (a re-opened key supersedes the prior).
                pending.insert(key, PendingRec { rec, ts });
            }
            _ => {
                // An unknown / absent phase on an otherwise-valid marker — ignore (neither an
                // open nor a close). Not malformed JSON, so not counted.
            }
        }
    }

    // Emit only keys that are pending and NOT resolved, ordered by timestamp ascending
    // (first-seen order as the stable tie-break for un-timestamped records).
    let mut out: Vec<(usize, PendingRec)> = Vec::new();
    for (idx, key) in order.into_iter().enumerate() {
        if resolved.contains(&key) {
            continue;
        }
        if let Some(pr) = pending.remove(&key) {
            out.push((idx, pr));
        }
    }
    out.sort_by(|a, b| {
        // Timestamp-less records sort LAST, then by first-seen index (deterministic).
        let ka = (a.1.ts.is_none(), a.1.ts.as_deref().unwrap_or(""), a.0);
        let kb = (b.1.ts.is_none(), b.1.ts.as_deref().unwrap_or(""), b.0);
        ka.cmp(&kb)
    });
    (out.into_iter().map(|(_, pr)| pr.rec).collect(), skipped)
}

/// A one-line human render of an unresolved-pending elicitation record, for the `turns`
/// reconstruction (where a pending elicitation is its own turn unit) and the `list`
/// annotation. `None` when the record is not a recognisable pending marker.
///
/// - AskUserQuestion: `AskUserQuestion: <first question>[ (+N more)]`
/// - ExitPlanMode: `ExitPlanMode: <plan first line>`
/// - mcp-elicitation: `MCP elicitation [<server>]: <message>` (falls back to the system
///   record's `content` string when the structured fields are absent)
/// - any other kind: the bare kind label.
#[must_use]
pub fn pending_text(rec: &Record) -> Option<String> {
    if !rec.is_elicitation_marker() {
        return None;
    }
    let kind = rec.csift_kind.as_deref().unwrap_or("elicitation");
    let body = match kind {
        "AskUserQuestion" => auq_text(rec),
        "ExitPlanMode" => plan_text(rec),
        "mcp-elicitation" => Some(mcp_text(rec)),
        _ => None,
    };
    Some(match body {
        Some(b) if !b.is_empty() => format!("{kind}: {b}"),
        _ => kind.to_string(),
    })
}

/// `<first question>[ (+N more)]` from an AskUserQuestion pending record's tool_use input.
fn auq_text(rec: &Record) -> Option<String> {
    let questions = first_tool_use_input(rec)?
        .get("questions")
        .and_then(serde_json::Value::as_array)?;
    let first = questions.first()?;
    let text = first
        .get("question")
        .or_else(|| first.get("header"))
        .or_else(|| first.get("prompt"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| first.as_str())
        .unwrap_or("");
    let body = crate::model::normalize_line(text);
    if questions.len() > 1 {
        Some(format!("{body} (+{} more)", questions.len() - 1))
    } else {
        Some(body)
    }
}

/// The plan's first line from an ExitPlanMode pending record's tool_use input.
fn plan_text(rec: &Record) -> Option<String> {
    let plan = first_tool_use_input(rec)?
        .get("plan")
        .and_then(serde_json::Value::as_str)?;
    Some(crate::model::normalize_line(plan))
}

/// `[<server>]: <message>` for an MCP pending record; falls back to the system record's
/// `content` string when the structured fields are missing.
fn mcp_text(rec: &Record) -> String {
    let server = rec.csift_mcp_server.as_deref().unwrap_or("mcp");
    let content = rec
        .content
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .map(crate::model::normalize_line)
        .unwrap_or_default();
    if content.is_empty() {
        format!("[{server}]")
    } else {
        format!("[{server}] {content}")
    }
}

/// The `input` object of the FIRST `tool_use` block on a pending AUQ/ExitPlanMode record.
fn first_tool_use_input(rec: &Record) -> Option<&serde_json::Value> {
    rec.blocks()?.iter().find_map(|b| match b {
        crate::model::Block::ToolUse { input, .. } => input.as_ref(),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auq_pending(key: &str, ts: &str, question: &str) -> String {
        format!(
            r#"{{"type":"assistant","uuid":"u-{key}","timestamp":"{ts}","sessionId":"s","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"{key}","csiftHookEvent":"PreToolUse","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"{key}","name":"AskUserQuestion","input":{{"questions":[{{"question":"{question}"}}]}}}}]}}}}"#
        )
    }

    fn resolved(key: &str, ts: &str) -> String {
        format!(
            r#"{{"type":"csift-elicitation-resolved","uuid":"r-{key}","timestamp":"{ts}","sessionId":"s","csift":"elicitation-marker-v1","csiftPhase":"resolved","csiftKind":"AskUserQuestion","csiftKey":"{key}"}}"#
        )
    }

    fn mcp_pending(key: &str, ts: &str, server: &str, msg: &str) -> String {
        format!(
            r#"{{"type":"system","subtype":"mcp_elicitation","uuid":"m-{key}","timestamp":"{ts}","sessionId":"s","content":"MCP elicitation [{server}] (confirm): {msg}","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"mcp-elicitation","csiftKey":"{key}","csiftMcpServer":"{server}"}}"#
        )
    }

    #[test]
    fn unresolved_pending_is_emitted() {
        let (recs, skipped) = pair_unresolved(&auq_pending(
            "k1",
            "2026-06-27T01:00:00.000Z",
            "Pick a branch?",
        ));
        assert_eq!(skipped, 0);
        assert_eq!(recs.len(), 1);
        assert!(recs[0].is_elicitation_marker());
        assert_eq!(recs[0].csift_kind.as_deref(), Some("AskUserQuestion"));
        assert_eq!(
            pending_text(&recs[0]).as_deref(),
            Some("AskUserQuestion: Pick a branch?")
        );
    }

    #[test]
    fn resolved_pair_is_dropped() {
        let lines = format!(
            "{}\n{}",
            auq_pending("k1", "2026-06-27T01:00:00.000Z", "q"),
            resolved("k1", "2026-06-27T01:05:00.000Z"),
        );
        let (recs, skipped) = pair_unresolved(&lines);
        assert_eq!(skipped, 0);
        assert!(recs.is_empty(), "a paired pending+resolved must be dropped");
    }

    #[test]
    fn malformed_line_is_skipped_and_counted() {
        let lines = format!(
            "{}\n{}\n{}",
            "this is { not valid json",
            auq_pending("k1", "2026-06-27T01:00:00.000Z", "q"),
            "{ also broken",
        );
        let (recs, skipped) = pair_unresolved(&lines);
        assert_eq!(skipped, 2);
        assert_eq!(recs.len(), 1);
    }

    #[test]
    fn non_marker_line_is_skipped_silently() {
        let lines = format!(
            "{}\n{}",
            r#"{"type":"user","message":{"role":"user","content":"hi"}}"#,
            auq_pending("k1", "2026-06-27T01:00:00.000Z", "q"),
        );
        let (recs, skipped) = pair_unresolved(&lines);
        assert_eq!(skipped, 0, "a non-marker is skipped silently, not counted");
        assert_eq!(recs.len(), 1);
    }

    #[test]
    fn mcp_pending_is_emitted_and_rendered() {
        let line = mcp_pending(
            "el-9",
            "2026-06-27T02:00:00.000Z",
            "gdrive",
            "Authorize Google Drive access",
        );
        let (recs, _) = pair_unresolved(&line);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].csift_kind.as_deref(), Some("mcp-elicitation"));
        assert_eq!(
            pending_text(&recs[0]).as_deref(),
            Some("mcp-elicitation: [gdrive] MCP elicitation [gdrive] (confirm): Authorize Google Drive access")
        );
    }

    #[test]
    fn ordered_by_timestamp_ascending() {
        let lines = format!(
            "{}\n{}",
            auq_pending("late", "2026-06-27T03:00:00.000Z", "second"),
            auq_pending("early", "2026-06-27T01:00:00.000Z", "first"),
        );
        let (recs, _) = pair_unresolved(&lines);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].csift_key.as_deref(), Some("early"));
        assert_eq!(recs[1].csift_key.as_deref(), Some("late"));
    }

    #[test]
    fn content_sniff_recognises_a_pure_sidecar() {
        let lines = format!(
            "{}\n{}",
            auq_pending("k1", "2026-06-27T01:00:00.000Z", "q"),
            resolved("k1", "2026-06-27T01:05:00.000Z"),
        );
        assert!(content_is_sidecar(&lines));
    }

    #[test]
    fn content_sniff_rejects_a_native_transcript() {
        let native = r#"{"type":"user","message":{"role":"user","content":"hi"}}"#;
        assert!(!content_is_sidecar(native));
    }

    #[test]
    fn content_sniff_rejects_empty() {
        assert!(!content_is_sidecar(""));
    }

    #[test]
    fn auq_multi_question_marks_count() {
        let line = r#"{"type":"assistant","timestamp":"2026-06-27T01:00:00.000Z","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"k","message":{"role":"assistant","content":[{"type":"tool_use","id":"k","name":"AskUserQuestion","input":{"questions":[{"question":"First?"},{"question":"Second?"}]}}]}}"#;
        let (recs, _) = pair_unresolved(line);
        assert_eq!(
            pending_text(&recs[0]).as_deref(),
            Some("AskUserQuestion: First? (+1 more)")
        );
    }
}
