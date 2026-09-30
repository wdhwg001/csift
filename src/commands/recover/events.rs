//! Event extraction: tool_use path joins, the input-side fallback, path matching, and
//! the per-record dispatcher (carrier-side extractors live in `carriers`).

use super::*;

/// Record, for each Read/Edit/Write/MultiEdit tool_use block, its `tool_use_id ->
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
/// tool RESULT as a bare `tool_result` string (`"File created successfully at: ..."`) with
/// NO structured `toolUseResult` echo - unlike a top-level session, whose carrier carries
/// `{type:create, filePath, content, ...}`. `extract_from_tool_use_result` reads that echo,
/// so without this fallback a file WRITTEN BY A SUBAGENT is invisible to `recover`
/// (`no recoverable history`) even though `files`/`search` see it (they read the tool_use
/// input directly). The authoritative content IS in the input - `Write.content`,
/// `Edit.{old_string,new_string,replace_all}`, `MultiEdit.edits[]` - present in EVERY
/// transcript. An Edit reconstructs via `apply_string_edit` (old->new), so the missing
/// `structuredPatch` is not needed.
///
/// Gated on `ids_with_result` so it never double-emits in a top-level session.
pub(crate) fn extract_input_fallback(
    line_no: usize,
    turn_index: usize,
    rec: &Record,
    target_file: &Target<'_>,
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
        if !path_matches(target_file, path, rec.cwd.as_deref()) {
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

/// The `--file` target as the extractors match it, plus every file it matched.
///
/// A suffix target (`lib.rs`, `a/lib.rs`) names every record path it ends, so it can name
/// several different files, and replaying their events together stitches one buffer out of
/// several files' edits - content that never existed. So the target records each file it
/// matched and the driver refuses one that matched more than one before replaying. EVERY
/// matched path is a candidate, a relative one included: `other/lib.rs` and `/p/m/lib.rs`
/// are two files as surely as two absolute paths are. A relative path is resolved against
/// the shell cwd its own record carries ([`candidate`]), so the relative and the absolute
/// spelling of ONE file are one candidate; with no cwd to resolve against it stays the raw
/// string, and a single such path alone is not ambiguous. One transcript's extraction runs
/// on one thread, so the cell is never shared.
#[derive(Debug)]
pub(crate) struct Target<'a> {
    file: Option<&'a str>,
    matched: std::cell::RefCell<std::collections::BTreeSet<String>>,
}

impl<'a> Target<'a> {
    pub(crate) fn new(file: Option<&'a str>) -> Self {
        Target {
            file,
            matched: std::cell::RefCell::default(),
        }
    }

    /// True when no `--file` was given: every path then fails to match.
    pub(crate) fn is_none(&self) -> bool {
        self.file.is_none()
    }

    /// The distinct full paths this target matched, in order.
    pub(crate) fn into_matched(self) -> std::collections::BTreeSet<String> {
        self.matched.into_inner()
    }
}

/// True when the `--file` target names `path` (see [`names_file`]) as written or as the
/// file it resolves to against `cwd`, the shell cwd of the record carrying it; a path that
/// matches is recorded on the target as that resolved candidate. No target matches nothing
/// (handled by callers that gate on the mode).
pub(crate) fn path_matches(target: &Target<'_>, path: &str, cwd: Option<&str>) -> bool {
    path_matches_as(target, path, path, cwd)
}

/// [`path_matches`] on `path`, recording `key` as the candidate: the bash extractors match a
/// command's operand verbatim as a belt beside the operand they RESOLVED through the
/// command's own `cd`s, and the resolved form is the file either spelling names - so the
/// belt's match must not mint a second candidate for it.
pub(crate) fn path_matches_as(
    target: &Target<'_>,
    path: &str,
    key: &str,
    cwd: Option<&str>,
) -> bool {
    // An empty path names no file - a carrier missing its path field reads as "" - and its
    // resolved form would be the cwd itself, which a suffix target could end.
    let Some(t) = target.file.filter(|_| !path.is_empty()) else {
        return false;
    };
    // The target names the path as written OR the file it resolves to: the candidate a
    // refusal prints for a relative path is the resolved one, so passing it back must find
    // the same events.
    let file = candidate(key, cwd);
    let hit = names_file(t, path) || names_file(t, &file);
    if hit {
        target.matched.borrow_mut().insert(file);
    }
    hit
}

/// The file a matched record path names, as a candidate: a full path as written; a relative
/// one joined to the record's absolute shell cwd (lexically, in the cwd's separator family);
/// the raw string where there is no such cwd, or where the path leads with `$` or `~`, whose
/// expansion the transcript does not record.
pub(crate) fn candidate(path: &str, cwd: Option<&str>) -> String {
    use crate::bash::mutations::{is_absolute_shell_path, join_shell_path};
    if is_full_path(path) || path.starts_with(['$', '~']) {
        return path.to_string();
    }
    match cwd.filter(|c| is_absolute_shell_path(c)) {
        Some(c) => join_shell_path(c, &[path]),
        None => path.to_string(),
    }
}

/// True when `target` names `path`: an exact raw-string match, or a basename-suffix
/// fallback so a user may pass a short path - component-aligned, so `b.rs` does not match
/// `/x/ab.rs`.
pub(crate) fn names_file(target: &str, path: &str) -> bool {
    if target == path {
        return true;
    }
    path.strip_suffix(target)
        .map(|prefix| prefix.is_empty() || prefix.ends_with(['/', '\\']))
        .unwrap_or(false)
}

/// True for a path that names a file on its own: rooted (`/...`, `\\...`) or
/// drive-qualified (`C:\\...`, `C:/...`).
pub(crate) fn is_full_path(path: &str) -> bool {
    let b = path.as_bytes();
    path.starts_with(['/', '\\'])
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'/' | b'\\'))
}

/// Extract every `--file` event carried by ONE record.
pub(crate) fn extract_from_record(
    line_no: usize,
    turn_index: usize,
    rec: &Record,
    target_file: &Target<'_>,
    id_to_path: &BTreeMap<String, String>,
    events: &mut Vec<FileEvent>,
) {
    let ts = rec.timestamp.clone();

    // -- (8) file-history-snapshot marker (a top-level sibling, no `message`) --
    if let Some(snap) = rec.snapshot.as_ref() {
        if let Some(tfb) = snap.get("trackedFileBackups").and_then(|v| v.as_object()) {
            for (path, entry) in tfb {
                if path_matches(target_file, path, rec.cwd.as_deref()) {
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

    // -- (8b) file-history-delta: ONE path's version bump (`trackingPath` + `backup`) --
    // The same instrument as the snapshot table, at a finer grain: a delta lands whenever
    // that one file's backed-up bytes changed. Reading both shapes is what lets the
    // external-write inference name the exact interval it bounds instead of the whole gap
    // between two per-prompt snapshots.
    if let (Some(path), Some(backup)) = (rec.tracking_path.as_deref(), rec.backup.as_ref()) {
        if path_matches(target_file, path, rec.cwd.as_deref()) {
            events.push(FileEvent {
                line_no,
                turn_index,
                timestamp_utc: ts.clone(),
                kind: EventKind::HistorySnapshotMarker {
                    version: backup.get("version").and_then(serde_json::Value::as_u64),
                    backup_file: backup
                        .get("backupFileName")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                    backup_time: backup
                        .get("backupTime")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                    content: None,
                },
            });
        }
    }

    // -- (7) attachment (edited_text_file external edit / file snapshot) --
    // The raw blob is parsed to a full tree ON DEMAND (the model keeps it unparsed so
    // the scanning subcommands never pay for it; recover is the deep consumer).
    if let Some(att) = rec.attachment_value() {
        extract_from_attachment(
            line_no,
            turn_index,
            &ts,
            att,
            rec.cwd.as_deref(),
            target_file,
            events,
        );
    }

    // -- toolUseResult-bearing carriers: Read / Write / Edit / Bash-hint results --
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

    // -- Per-block extraction over message.content[] --
    let Some(blocks) = rec.blocks() else { return };
    for b in blocks {
        match b {
            // (5) integrity error on a tool_result carrier (no inline path -> id-join).
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
                    if path_matches(
                        target_file,
                        attributed.unwrap_or_default(),
                        rec.cwd.as_deref(),
                    ) {
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
            // `bash::mutations::cwd`) BEFORE matching, so an absolute `--file` joins
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
                    for bm in crate::bash::mutations::parse_bash_mutations(cmd) {
                        if crate::bash::mutations::is_class_marker(&bm.path) {
                            continue;
                        }
                        let (resolved, resolution) = bm.resolve(rec.cwd.as_deref());
                        let cwd = rec.cwd.as_deref();
                        if path_matches(target_file, &resolved, cwd)
                            || path_matches_as(target_file, &bm.path, &resolved, cwd)
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
