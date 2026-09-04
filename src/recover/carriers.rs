//! Carrier-side extraction: toolUseResult (Read/Write/Edit + the Bash freshness
//! hint), attachments (edited_text_file / file), the integrity-error classifier, and
//! the structured-patch parser.

use super::*;

/// Extract a `FullSnapshot` / `PartialRead` (Read) or a `FullSnapshot` (Write) / `Edit`
/// from a `toolUseResult` carrier, plus the two Claude Code freshness signals riding
/// the same carrier: a Bash result's `staleReadFileStateHint` (CC's own modified-file
/// attribution) and an Edit result's `staleRecovered` flag. `record_cwd` is the
/// carrying record's own `cwd`, the base the hint's relative paths resolve against.
#[allow(clippy::too_many_arguments)]
pub(crate) fn extract_from_tool_use_result(
    line_no: usize,
    turn_index: usize,
    ts: &Option<String>,
    tur: &serde_json::Value,
    record_cwd: Option<&str>,
    target_file: Option<&str>,
    events: &mut Vec<FileEvent>,
) {
    // ── (0) Bash result hint: `staleReadFileStateHint` names files THIS command
    //    modified out of the read set. CC computed the list from its own readFileState
    //    (an mtime stat over every read file), so a named match is authoritative. The
    //    paths are rendered relative to the shell's cwd; resolve before matching. A
    //    truncated tail ("… and N more") names no paths, so only the named first-5 can
    //    ever match: a target hidden in the remainder is NOT detected here (documented
    //    limit; the window's bash disclosure still covers the command itself).
    if let Some(hint) = tur
        .get("staleReadFileStateHint")
        .and_then(serde_json::Value::as_str)
    {
        if let Some((paths, _more)) = parse_stale_read_hint(hint) {
            for p in paths {
                let resolved = if crate::bash_mutations::is_absolute_shell_path(&p) {
                    p
                } else if let Some(cwd) =
                    record_cwd.filter(|c| crate::bash_mutations::is_absolute_shell_path(c))
                {
                    crate::bash_mutations::join_shell_path(cwd, &[&p])
                } else {
                    p
                };
                if path_matches(target_file, &resolved) {
                    events.push(FileEvent {
                        line_no,
                        turn_index,
                        timestamp_utc: ts.clone(),
                        kind: EventKind::StaleReadHint { path: resolved },
                    });
                }
            }
        }
    }
    // ── (1a) Read result: toolUseResult.file = {filePath, content, startLine, …} ──
    if let Some(file) = tur.get("file").and_then(|v| v.as_object()) {
        let path = file.get("filePath").and_then(serde_json::Value::as_str);
        if path_matches(target_file, path.unwrap_or_default()) {
            push_read_file_object(line_no, turn_index, ts, file, SnapSource::FullRead, events);
            return;
        }
    }

    // ── (2) Write result: {type:create|update, filePath, content, …} ──
    // ── (3) Edit result: {filePath, oldString, newString, structuredPatch, …} (no type) ──
    let path = tur.get("filePath").and_then(serde_json::Value::as_str);
    if !path_matches(target_file, path.unwrap_or_default()) {
        return;
    }
    let has_edit_strings = tur.get("oldString").is_some() || tur.get("newString").is_some();
    let structured_patch = parse_structured_patch(tur.get("structuredPatch"));

    if has_edit_strings {
        // An Edit (carrier side): keep the strings + structuredPatch + originalFile.
        let hunks = vec![EditHunk {
            old_string: tur
                .get("oldString")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            new_string: tur
                .get("newString")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            replace_all: tur
                .get("replaceAll")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }];
        events.push(FileEvent {
            line_no,
            turn_index,
            timestamp_utc: ts.clone(),
            kind: EventKind::Edit {
                hunks,
                original_file: tur
                    .get("originalFile")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                structured_patch,
            },
        });
        // `staleRecovered:true` on a SUCCESSFUL Edit: CC found the file modified on
        // disk since the last read, but old_string stayed unique so the edit applied.
        // The disk holds changes this stream never saw - an authoritative annotation.
        if tur
            .get("staleRecovered")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            events.push(FileEvent {
                line_no,
                turn_index,
                timestamp_utc: ts.clone(),
                kind: EventKind::StaleRecovered,
            });
        }
        return;
    }

    // A Write result: full-content anchor.
    if let Some(content) = tur.get("content").and_then(serde_json::Value::as_str) {
        let total = line_count(content);
        events.push(FileEvent {
            line_no,
            turn_index,
            timestamp_utc: ts.clone(),
            kind: EventKind::FullSnapshot {
                content: content.to_string(),
                total_lines: total,
                source: SnapSource::Write,
            },
        });
    }
}

/// The Read `file` object (`{filePath, content, numLines, startLine, totalLines,
/// truncatedByTokenCap}`) from a `toolUseResult` or a `file` attachment: a content-
/// bearing echo becomes a snapshot or a partial read via [`push_read_event`]; an echo
/// with NO text becomes a counted [`EventKind::BlankedRead`] (ledger FH-048). Two
/// contentless shapes exist: the `content` key absent (the `file_unchanged`, `pdf`,
/// `parts` and `notebook` result arms), and `content` blanked to the empty string
/// while `numLines`/`totalLines` still count the lines the file had (the harness
/// clears the text of a tool result older than its retention window before
/// persisting it). A genuinely empty file (`totalLines` 0, or no counts at all with an
/// empty text) still goes through the ordinary path.
pub(crate) fn push_read_file_object(
    line_no: usize,
    turn_index: usize,
    ts: &Option<String>,
    file: &serde_json::Map<String, serde_json::Value>,
    source: SnapSource,
    events: &mut Vec<FileEvent>,
) {
    let content = file.get("content").and_then(serde_json::Value::as_str);
    let count = |k: &str| {
        file.get(k)
            .and_then(serde_json::Value::as_u64)
            .map(|n| n as usize)
    };
    let start_line = count("startLine").unwrap_or(1);
    let total_lines = count("totalLines");
    let num_lines = count("numLines");
    let counted = num_lines.unwrap_or(0).max(total_lines.unwrap_or(0));
    let blanked = match content {
        None => true,
        Some("") => counted > 0,
        Some(_) => false,
    };
    if blanked {
        events.push(FileEvent {
            line_no,
            turn_index,
            timestamp_utc: ts.clone(),
            kind: EventKind::BlankedRead {
                total_lines: total_lines.or(num_lines),
            },
        });
        return;
    }
    let truncated = file
        .get("truncatedByTokenCap")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    push_read_event(
        line_no,
        turn_index,
        ts,
        content.unwrap_or_default(),
        start_line,
        num_lines,
        total_lines,
        truncated,
        source,
        events,
    );
}

/// Push a Read event as either a `FullSnapshot` (whole file seen) or a `PartialRead`.
///
/// `truncated` is the carrier's `truncatedByTokenCap` (Claude Code 2.1.145+): the Read
/// tool cut the content at its token budget. On the line-truncation branch the kept
/// lines are whole and `numLines < totalLines`; on the CHARACTER-truncation branch (a
/// file whose lines are too long to paginate) `numLines` is recomputed from the cut
/// slice and can EQUAL `totalLines`, so a capped read looked like a whole-file read
/// (v0.10.3, ledger REC-047). A truncated read is never a full snapshot, and when its
/// line count reaches the total the last line is the cut one and is dropped; nothing
/// is pushed when no whole line remains.
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_read_event(
    line_no: usize,
    turn_index: usize,
    ts: &Option<String>,
    content: &str,
    start_line: usize,
    num_lines: Option<usize>,
    total_lines: Option<usize>,
    truncated: bool,
    source: SnapSource,
    events: &mut Vec<FileEvent>,
) {
    let mut lines: Vec<String> = split_lines(content);
    let observed = num_lines.unwrap_or(lines.len());
    let total = total_lines.unwrap_or(observed.max(start_line + lines.len().saturating_sub(1)));
    if truncated && observed >= total {
        // the character branch: the final kept line is cut mid-line
        lines.pop();
        if lines.is_empty() {
            return;
        }
    }
    let is_full = !truncated && start_line == 1 && observed >= total && total > 0;
    if is_full {
        events.push(FileEvent {
            line_no,
            turn_index,
            timestamp_utc: ts.clone(),
            kind: EventKind::FullSnapshot {
                content: content.to_string(),
                total_lines: total.max(lines.len()),
                source,
            },
        });
    } else {
        events.push(FileEvent {
            line_no,
            turn_index,
            timestamp_utc: ts.clone(),
            kind: EventKind::PartialRead {
                start_line: start_line.max(1),
                lines,
                total_lines: total,
            },
        });
    }
}

/// Extract `edited_text_file` (external edit) or `file` (snapshot) from an attachment.
pub(crate) fn extract_from_attachment(
    line_no: usize,
    turn_index: usize,
    ts: &Option<String>,
    att: &serde_json::Value,
    target_file: Option<&str>,
    events: &mut Vec<FileEvent>,
) {
    let atype = att.get("type").and_then(serde_json::Value::as_str);

    // (7a) edited_text_file → an external edit (hard boundary).
    if atype == Some("edited_text_file") {
        let path = att
            .get("filename")
            .or_else(|| att.get("filePath"))
            .and_then(serde_json::Value::as_str);
        if path_matches(target_file, path.unwrap_or_default()) {
            let snippet_text = att
                .get("snippet")
                .or_else(|| att.get("content"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let snippet = strip_gutter(snippet_text);
            events.push(FileEvent {
                line_no,
                turn_index,
                timestamp_utc: ts.clone(),
                kind: EventKind::ExternalEdit { snippet },
            });
        }
        return;
    }

    // (7b) a `file` attachment → same shape as a structured Read.
    if let Some(file) = att
        .get("content")
        .and_then(|c| c.get("file"))
        .or_else(|| att.get("file"))
    {
        let path = file.get("filePath").and_then(serde_json::Value::as_str);
        if path_matches(target_file, path.unwrap_or_default()) {
            if let Some(obj) = file.as_object() {
                push_read_file_object(
                    line_no,
                    turn_index,
                    ts,
                    obj,
                    SnapSource::FileAttachment,
                    events,
                );
            }
        }
    }
}

/// Classify a tool_result error body as an integrity error, or `None` if it is some
/// other tool error. Only [`IntegrityKind::ModifiedSinceRead`] becomes a boundary;
/// the others are COUNTED annotations (the op never landed).
pub(crate) fn classify_integrity_error(content: &serde_json::Value) -> Option<IntegrityKind> {
    let text = crate::model::tool_result_content_text(content);
    if text.contains("has been modified since read") || text.contains("File has been modified") {
        Some(IntegrityKind::ModifiedSinceRead)
    } else if text.contains("has not been read yet") || text.contains("Read it first") {
        Some(IntegrityKind::NotReadYet)
    } else if text.contains("String to replace not found in file") {
        Some(IntegrityKind::StringNotFound)
    } else if text.contains("File does not exist") {
        Some(IntegrityKind::FileDoesNotExist)
    } else {
        None
    }
}

/// Parse a `toolUseResult.staleReadFileStateHint` string into its named paths plus
/// the truncated-remainder count. Wire shape (Claude Code 2.1.237, corpus-verified):
/// `[This command modified N file(s) you've previously read: p1, p2 and M more. Call
/// Read before editing.]` - the list caps at five names; `M` counts the unnamed rest
/// (0 when the list is complete). Paths are comma-space separated and rendered
/// relative to the recording shell's cwd.
pub(crate) fn parse_stale_read_hint(hint: &str) -> Option<(Vec<String>, usize)> {
    let rest = hint.strip_prefix("[This command modified ")?;
    let colon = rest.find(": ")?;
    let mut tail = &rest[colon + 2..];
    if let Some(end) = tail.rfind(". Call Read before editing.]") {
        tail = &tail[..end];
    } else if let Some(stripped) = tail.strip_suffix(']') {
        tail = stripped;
    }
    let mut more = 0usize;
    if let Some(pos) = tail.rfind(" and ") {
        if let Some(n) = tail[pos + 5..]
            .strip_suffix(" more")
            .and_then(|n| n.trim().parse::<usize>().ok())
        {
            more = n;
            tail = &tail[..pos];
        }
    }
    let paths: Vec<String> = tail
        .split(", ")
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    (!paths.is_empty()).then_some((paths, more))
}

/// Parse `toolUseResult.structuredPatch` (an array of hunks) into [`PatchHunk`]s.
pub(crate) fn parse_structured_patch(v: Option<&serde_json::Value>) -> Option<Vec<PatchHunk>> {
    let arr = v?.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for h in arr {
        let old_start = h.get("oldStart").and_then(serde_json::Value::as_u64)? as usize;
        let old_lines = h.get("oldLines").and_then(serde_json::Value::as_u64)? as usize;
        let new_lines = h.get("newLines").and_then(serde_json::Value::as_u64)? as usize;
        let lines = h
            .get("lines")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|l| l.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        out.push(PatchHunk {
            old_start,
            old_lines,
            new_lines,
            lines,
        });
    }
    Some(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Reconstruction - the sparse line-keyed buffer
// ─────────────────────────────────────────────────────────────────────────────
