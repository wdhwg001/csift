//! Event extraction: tool_use path joins, the input-side fallback, path matching, and
//! the per-record dispatcher (carrier-side extractors live in `carriers`).

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
            for (path, entry) in tfb {
                if path_matches(target_file, path) {
                    events.push(FileEvent {
                        line_no,
                        turn_index,
                        timestamp_utc: snap
                            .get("timestamp")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string)
                            .or_else(|| ts.clone()),
                        kind: EventKind::HistorySnapshotMarker {
                            version: entry.get("version").and_then(serde_json::Value::as_u64),
                            backup_file: entry
                                .get("backupFileName")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                            backup_time: entry
                                .get("backupTime")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string),
                            content: None,
                        },
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

    // ── toolUseResult-bearing carriers: Read / Write / Edit / Bash-hint results ──
    if let Some(tur) = rec.tool_use_result_value() {
        extract_from_tool_use_result(
            line_no,
            turn_index,
            &ts,
            &tur,
            rec.cwd.as_deref(),
            target_file,
            events,
        );
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
            // (6) Bash heuristic mutation touching `--file`. The operand is RESOLVED
            // against the recording shell's cwd (the record's own `cwd` field, see
            // `bash_mutations::cwd`) BEFORE matching, so an absolute `--file` now joins
            // the dominant real shape `cd <proj> && sed -i rel/path`; the verbatim
            // spelling stays as a belt so a relative `--file` keeps matching exactly
            // what the command typed. A class-marker pseudo-path is never a file: it is
            // accounted per scope in `ScanResult::opaque`, not here.
            Block::ToolUse {
                name: Some(name),
                input: Some(input),
                ..
            } if name == "Bash" => {
                if let Some(cmd) = input.get("command").and_then(serde_json::Value::as_str) {
                    for bm in crate::bash_mutations::parse_bash_mutations(cmd) {
                        if crate::bash_mutations::is_class_marker(&bm.path) {
                            continue;
                        }
                        let (resolved, resolution) = bm.resolve(rec.cwd.as_deref());
                        if path_matches(target_file, &resolved)
                            || path_matches(target_file, &bm.path)
                        {
                            events.push(FileEvent {
                                line_no,
                                turn_index,
                                timestamp_utc: ts.clone(),
                                kind: EventKind::BashTouch {
                                    verb: bm.verb.to_string(),
                                    path: resolved,
                                    resolution: resolution.as_str(),
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
