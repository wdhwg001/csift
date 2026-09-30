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

/// Record, for each tool_use block, its `tool_use_id -> cwd` of the record carrying it: the
/// shell cwd its call ran in, which the anchor law (AGENTS 3.11) reads off the TOOL_USE
/// record rather than off the result written after the call returned.
/// Both sides borrow from the record, which outlives the turn the map is built for.
pub(crate) fn collect_tool_use_cwds<'a>(rec: &'a Record, out: &mut BTreeMap<&'a str, &'a str>) {
    let (Some(cwd), Some(blocks)) = (rec.cwd.as_deref(), rec.blocks()) else {
        return;
    };
    for b in blocks {
        if let Block::ToolUse { id: Some(id), .. } = b {
            out.insert(id.as_str(), cwd);
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
/// spelling of ONE file are one candidate. A record that carries NO cwd - a file-history
/// snapshot line or delta, whose `trackedFileBackups` keys and `trackingPath` are relative -
/// resolves against the transcript's ORIGINAL cwd instead ([`original_cwd`]), because that
/// is what such a key is relative to: joined to the first cwd, every relative key a live
/// census paired with an absolute Write/Edit path of its own transcript names a path that
/// transcript's tools wrote, while the nearest record's cwd names fewer and never one the
/// first misses (SPEC (124) carries the counts). With no cwd anywhere a path stays the raw
/// string, and a single such path alone is not ambiguous. One transcript's extraction runs
/// on one thread, so the cell is never shared.
#[derive(Debug)]
pub(crate) struct Target<'a> {
    file: Option<&'a str>,
    /// The target as it is compared: [`normalize_path`] of `file`.
    norm: Option<String>,
    /// The transcript's original shell cwd, the base for a record path whose own record
    /// carries none.
    origin_cwd: Option<String>,
    /// Match only the file whose candidate IS the target: set when a refusal's own candidate
    /// is passed back and other candidates end with it (a raw relative one, see
    /// [`Target::exact`]).
    exact: bool,
    matched: std::cell::RefCell<std::collections::BTreeSet<String>>,
}

impl<'a> Target<'a> {
    pub(crate) fn new(file: Option<&'a str>) -> Self {
        Target {
            file,
            norm: file.map(|f| normalize_path(f).into_owned()),
            origin_cwd: None,
            exact: false,
            matched: std::cell::RefCell::default(),
        }
    }

    /// The same target with the transcript's original cwd for cwd-less record paths.
    pub(crate) fn with_origin_cwd(mut self, cwd: Option<&str>) -> Self {
        self.origin_cwd = cwd.map(str::to_string);
        self
    }

    /// The same target matching only the file whose candidate equals it. A candidate the
    /// several-files refusal printed and the reader passes back is itself one of the files,
    /// but a suffix target also names every OTHER candidate ending with it - a raw relative
    /// `a/lib.rs` from a transcript with no cwd, beside `/w/a/lib.rs` - so without this the
    /// refusal's own advice would be refused again.
    pub(crate) fn exact(mut self) -> Self {
        self.exact = true;
        self
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
    let Some(t) = target.norm.as_deref().filter(|_| !path.is_empty()) else {
        return false;
    };
    // A record with no cwd of its own (a file-history line) takes the transcript's.
    let cwd = cwd
        .filter(|c| crate::bash::mutations::is_absolute_shell_path(c))
        .or(target.origin_cwd.as_deref());
    // The target names the path as written OR the file it resolves to: the candidate a
    // refusal prints for a relative path is the resolved one, so passing it back must find
    // the same events.
    let written = !target.exact && names_file(t, &normalize_path(path));
    // Most record paths name another file, and the candidate is a string built per call (a
    // join to the cwd, a normalised copy), so it is built only where it can match: a path the
    // target names ends with the target's last component, and every candidate ends with the
    // key's own last component whenever that is a plain name - the join appends it, and
    // normalising keeps it. A key ending in `.`, `..` or a separator takes the long way.
    let key_last = last_component(key);
    if !written && !matches!(key_last, "" | "." | "..") && key_last != last_component(t) {
        return false;
    }
    let file = candidate(key, cwd);
    let hit = if target.exact {
        file == t
    } else {
        written || names_file(t, &file)
    };
    if hit {
        target.matched.borrow_mut().insert(file);
    }
    hit
}

/// The transcript's ORIGINAL shell cwd: the absolute `cwd` of its first record carrying one.
/// Claude Code starts the shell there and stamps it on every conversation record, so a
/// transcript's first prompt carries it - `recover`'s prefilter admits every user-role
/// line, which puts that record among the ones parsed.
pub(crate) fn original_cwd(records: &[(usize, Record)]) -> Option<&str> {
    records.iter().find_map(|(_, r)| {
        r.cwd
            .as_deref()
            .filter(|c| crate::bash::mutations::is_absolute_shell_path(c))
    })
}

/// Extract every `--file` event carried by ONE record.
pub(crate) fn extract_from_record(
    line_no: usize,
    turn_index: usize,
    rec: &Record,
    target_file: &Target<'_>,
    id_to_path: &BTreeMap<String, String>,
    id_to_cwd: &BTreeMap<&str, &str>,
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
        // A relative `filePath` in the result is relative to the cwd its CALL ran in (the
        // anchor law, AGENTS 3.11): the tool_use this result answers, paired by
        // `tool_use_id` within the turn, and the result's own cwd only when no call is in
        // reach. The result record is written after the call returned and can carry a later
        // cwd; on the live root none of the relative ones does, so this is the law's reading
        // rather than a correction of a measured miss.
        let call_cwd = rec
            .blocks()
            .and_then(|bs| {
                bs.iter().find_map(|b| match b {
                    Block::ToolResult {
                        tool_use_id: Some(id),
                        ..
                    } => id_to_cwd.get(id.as_str()).copied(),
                    _ => None,
                })
            })
            .or(rec.cwd.as_deref());
        extract_from_tool_use_result(
            line_no,
            turn_index,
            &ts,
            &tur,
            CarrierCwds {
                record: rec.cwd.as_deref(),
                call: call_cwd,
            },
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
