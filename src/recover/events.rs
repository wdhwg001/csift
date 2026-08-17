//! Event extraction: tool_use paths, toolUseResult, attachments, structured patches.

use super::*;

/// Record, for each Read/Edit/Write/MultiEdit tool_use block, its `tool_use_id →
/// file_path` so a later integrity-error carrier (which has no inline path) can be
/// attributed by id.
pub(crate) fn collect_tool_use_paths(blocks: Option<&[Block]>, out: &mut BTreeMap<String, String>) {
    let Some(blocks) = blocks else { return };
    for b in blocks {
        if let Block::ToolUse {
            id: Some(id),
            name: Some(name),
            input: Some(input),
        } = b
        {
            let key = match name.as_str() {
                "Read" | "Edit" | "Write" | "MultiEdit" => "file_path",
                "NotebookEdit" => "notebook_path",
                _ => continue,
            };
            if let Some(p) = input.get(key).and_then(serde_json::Value::as_str) {
                if !p.is_empty() {
                    out.insert(id.clone(), p.to_string());
                }
            }
        }
    }
}

/// Reconstruct a Write/Edit/MultiEdit content event from the tool_use INPUT when the op
/// has NO `toolUseResult` carrier (its id is absent from `ids_with_result`).
///
/// WHY: a subagent (built-in Task/Agent-tool) and a workflow-agent transcript record the
/// tool RESULT as a bare `tool_result` string (`"File created successfully at: …"`) with
/// NO structured `toolUseResult` echo - unlike a top-level session, whose carrier carries
/// `{type:create, filePath, content, …}`. `extract_from_tool_use_result` reads that echo,
/// so without this fallback a file WRITTEN BY A SUBAGENT is invisible to `recover`
/// (`no recoverable history`) even though `files`/`search` see it (they read the tool_use
/// input directly). The authoritative content IS in the input - `Write.content`,
/// `Edit.{old_string,new_string,replace_all}`, `MultiEdit.edits[]` - present in EVERY
/// transcript. An Edit reconstructs via `apply_string_edit` (old→new), so the missing
/// `structuredPatch` is not needed.
///
/// Gated on `ids_with_result` so it never double-emits in a top-level session.
pub(crate) fn extract_input_fallback(
    line_no: usize,
    turn_index: usize,
    rec: &Record,
    target_file: Option<&str>,
    ids_with_result: &std::collections::HashSet<String>,
    failed_ids: &std::collections::HashSet<String>,
    events: &mut Vec<FileEvent>,
) {
    let ts = rec.timestamp.clone();
    let Some(blocks) = rec.blocks() else { return };
    for b in blocks {
        let Block::ToolUse {
            id,
            name: Some(name),
            input: Some(input),
        } = b
        else {
            continue;
        };
        // Skip when this op already has a `toolUseResult` carrier to reconstruct from, OR
        // when its result was an ERROR (a failed Edit/Write never mutated the file, so its
        // input is a phantom - `is_error:true` covers both "String to replace not found"
        // and the Edit-before-Read "File has not been read yet" wall, incl. the
        // Bash-created-then-directly-Edited and the must-re-Read-a-plan cases).
        if let Some(id) = id {
            if ids_with_result.contains(id) || failed_ids.contains(id) {
                continue;
            }
        }
        let path = input
            .get("file_path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if !path_matches(target_file, path) {
            continue;
        }
        match name.as_str() {
            "Write" => {
                if let Some(content) = input.get("content").and_then(serde_json::Value::as_str) {
                    events.push(FileEvent {
                        line_no,
                        turn_index,
                        timestamp_utc: ts.clone(),
                        kind: EventKind::FullSnapshot {
                            content: content.to_string(),
                            total_lines: line_count(content),
                            source: SnapSource::Write,
                        },
                    });
                }
            }
            "Edit" => {
                let hunks = vec![EditHunk {
                    old_string: input
                        .get("old_string")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    new_string: input
                        .get("new_string")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    replace_all: input
                        .get("replace_all")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                }];
                events.push(FileEvent {
                    line_no,
                    turn_index,
                    timestamp_utc: ts.clone(),
                    kind: EventKind::Edit {
                        hunks,
                        original_file: None,
                        structured_patch: None,
                    },
                });
            }
            "MultiEdit" => {
                let hunks: Vec<EditHunk> = input
                    .get("edits")
                    .and_then(serde_json::Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .map(|e| EditHunk {
                                old_string: e
                                    .get("old_string")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                                new_string: e
                                    .get("new_string")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                                replace_all: e
                                    .get("replace_all")
                                    .and_then(serde_json::Value::as_bool)
                                    .unwrap_or(false),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if !hunks.is_empty() {
                    events.push(FileEvent {
                        line_no,
                        turn_index,
                        timestamp_utc: ts.clone(),
                        kind: EventKind::Edit {
                            hunks,
                            original_file: None,
                            structured_patch: None,
                        },
                    });
                }
            }
            _ => {}
        }
    }
}

/// True when `path` matches `--file`: exact raw-string match, or a basename-suffix
/// fallback (so a user may pass a short path). `None` target matches nothing (handled
/// by callers that gate on the mode).
pub(crate) fn path_matches(target: Option<&str>, path: &str) -> bool {
    let Some(t) = target else { return false };
    if t == path {
        return true;
    }
    // Basename-suffix fallback: the target is a trailing path segment of the record's
    // path (component-aligned, so `b.rs` does not match `/x/ab.rs`).
    path.strip_suffix(t)
        .map(|prefix| prefix.is_empty() || prefix.ends_with(['/', '\\']))
        .unwrap_or(false)
}

/// Extract every `--file` event carried by ONE record.
pub(crate) fn extract_from_record(
    line_no: usize,
    turn_index: usize,
    rec: &Record,
    target_file: Option<&str>,
    id_to_path: &BTreeMap<String, String>,
    events: &mut Vec<FileEvent>,
) {
    let ts = rec.timestamp.clone();

    // ── (8) file-history-snapshot marker (a top-level sibling, no `message`) ──
    if let Some(snap) = rec.snapshot.as_ref() {
        if let Some(tfb) = snap.get("trackedFileBackups").and_then(|v| v.as_object()) {
            for path in tfb.keys() {
                if path_matches(target_file, path) {
                    events.push(FileEvent {
                        line_no,
                        turn_index,
                        timestamp_utc: snap
                            .get("timestamp")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string)
                            .or_else(|| ts.clone()),
                        kind: EventKind::HistorySnapshotMarker,
                    });
                }
            }
        }
    }

    // ── (7) attachment (edited_text_file external edit / file snapshot) ──
    // The raw blob is parsed to a full tree ON DEMAND (the model keeps it unparsed so
    // the scanning subcommands never pay for it; recover is the deep consumer).
    if let Some(att) = rec.attachment_value() {
        extract_from_attachment(line_no, turn_index, &ts, &att, target_file, events);
    }

    // ── toolUseResult-bearing carriers: Read / Write / Edit results ──
    if let Some(tur) = rec.tool_use_result_value() {
        extract_from_tool_use_result(line_no, turn_index, &ts, &tur, target_file, events);
    }

    // ── Per-block extraction over message.content[] ──
    let Some(blocks) = rec.blocks() else { return };
    for b in blocks {
        match b {
            // (5) integrity error on a tool_result carrier (no inline path → id-join).
            Block::ToolResult {
                tool_use_id,
                content: Some(content),
                is_error: Some(true),
            } => {
                if let Some(kind) = classify_integrity_error(content) {
                    let attributed = tool_use_id
                        .as_ref()
                        .and_then(|id| id_to_path.get(id))
                        .map(String::as_str);
                    if path_matches(target_file, attributed.unwrap_or_default()) {
                        events.push(FileEvent {
                            line_no,
                            turn_index,
                            timestamp_utc: ts.clone(),
                            kind: EventKind::IntegrityError {
                                kind,
                                raw: crate::model::tool_result_content_text(content),
                            },
                        });
                    }
                }
            }
            // (6) Bash heuristic mutation touching `--file`.
            Block::ToolUse {
                name: Some(name),
                input: Some(input),
                ..
            } if name == "Bash" => {
                if let Some(cmd) = input.get("command").and_then(serde_json::Value::as_str) {
                    for bm in crate::bash_mutations::parse_bash_mutations(cmd) {
                        if path_matches(target_file, &bm.path) {
                            events.push(FileEvent {
                                line_no,
                                turn_index,
                                timestamp_utc: ts.clone(),
                                kind: EventKind::BashTouch {
                                    verb: bm.verb.to_string(),
                                },
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Extract a `FullSnapshot` / `PartialRead` (Read) or a `FullSnapshot` (Write) / `Edit`
/// from a `toolUseResult` carrier.
pub(crate) fn extract_from_tool_use_result(
    line_no: usize,
    turn_index: usize,
    ts: &Option<String>,
    tur: &serde_json::Value,
    target_file: Option<&str>,
    events: &mut Vec<FileEvent>,
) {
    // ── (1a) Read result: toolUseResult.file = {filePath, content, startLine, …} ──
    if let Some(file) = tur.get("file").and_then(|v| v.as_object()) {
        let path = file.get("filePath").and_then(serde_json::Value::as_str);
        if path_matches(target_file, path.unwrap_or_default()) {
            let content = file
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let start_line = file
                .get("startLine")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1) as usize;
            let total_lines = file
                .get("totalLines")
                .and_then(serde_json::Value::as_u64)
                .map(|n| n as usize);
            let num_lines = file
                .get("numLines")
                .and_then(serde_json::Value::as_u64)
                .map(|n| n as usize);
            push_read_event(
                line_no,
                turn_index,
                ts,
                &content,
                start_line,
                num_lines,
                total_lines,
                SnapSource::FullRead,
                events,
            );
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

/// Push a Read event as either a `FullSnapshot` (whole file seen) or a `PartialRead`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_read_event(
    line_no: usize,
    turn_index: usize,
    ts: &Option<String>,
    content: &str,
    start_line: usize,
    num_lines: Option<usize>,
    total_lines: Option<usize>,
    source: SnapSource,
    events: &mut Vec<FileEvent>,
) {
    let lines: Vec<String> = split_lines(content);
    let observed = num_lines.unwrap_or(lines.len());
    let total = total_lines.unwrap_or(observed.max(start_line + lines.len().saturating_sub(1)));
    let is_full = start_line == 1 && observed >= total && total > 0;
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
            let content = file
                .get("content")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let start_line = file
                .get("startLine")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1) as usize;
            let total_lines = file
                .get("totalLines")
                .and_then(serde_json::Value::as_u64)
                .map(|n| n as usize);
            let num_lines = file
                .get("numLines")
                .and_then(serde_json::Value::as_u64)
                .map(|n| n as usize);
            push_read_event(
                line_no,
                turn_index,
                ts,
                &content,
                start_line,
                num_lines,
                total_lines,
                SnapSource::FileAttachment,
                events,
            );
        }
    }
}

/// Classify a tool_result error body as an integrity error, or `None` if it is some
/// other tool error (which is not a content boundary).
pub(crate) fn classify_integrity_error(content: &serde_json::Value) -> Option<IntegrityKind> {
    let text = crate::model::tool_result_content_text(content);
    if text.contains("has been modified since read") || text.contains("File has been modified") {
        Some(IntegrityKind::ModifiedSinceRead)
    } else if text.contains("has not been read yet") || text.contains("Read it first") {
        Some(IntegrityKind::NotReadYet)
    } else {
        None
    }
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
