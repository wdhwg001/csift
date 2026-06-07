//! Unit tests for `recover`: per-arm branch-completeness over lightweight fixtures, in
//! the style of `files.rs` / `parse.rs`. Locale-neutral multi-byte tokens only
//! (accented Latin / emoji — `café🛠`), the house fixture style.

use super::*;

fn rec(line: &str) -> Record {
    serde_json::from_str(line).expect("valid fixture record")
}

// ── (3) jsonl line-number counter (the new capability) ──

#[test]
fn line_number_counter_is_one_to_one_with_jsonl_lines() {
    // A scan over content + blank + malformed lines: line_no must equal the true file
    // line for every retained record, and a malformed line must not desync the count.
    use std::io::Write as _;
    let dir = std::env::temp_dir();
    let p = dir.join(format!("csift-recover-ln-{}.jsonl", std::process::id()));
    let mut f = std::fs::File::create(&p).unwrap();
    // line 1: genuine user; line 2: BLANK; line 3: a Read result for /p/a.rs;
    // line 4: malformed (carries "Read" so it survives the prefilter); line 5: another read.
    writeln!(
        f,
        r#"{{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"go"}}}}"#
    )
    .unwrap();
    writeln!(f).unwrap(); // blank
    writeln!(
        f,
        r#"{{"type":"user","toolUseResult":{{"file":{{"filePath":"/p/a.rs","content":"x\ny","startLine":1,"numLines":2,"totalLines":2}}}},"message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"r1","content":"ok"}}]}}}}"#
    )
    .unwrap();
    writeln!(f, r#"{{"name":"Read" broken json}}"#).unwrap(); // malformed
    writeln!(
        f,
        r#"{{"type":"user","toolUseResult":{{"file":{{"filePath":"/p/a.rs","content":"x\ny\nz","startLine":1,"numLines":3,"totalLines":3}}}},"message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"r2","content":"ok"}}]}}}}"#
    )
    .unwrap();
    f.flush().unwrap();

    let sr = scan_one_file(&p, Some("/p/a.rs")).expect("scan");
    std::fs::remove_file(&p).ok();
    // Two read events; their line numbers are the TRUE jsonl lines (3 and 5), with the
    // blank + malformed lines counted in between (so 5, not 4).
    assert_eq!(sr.events.len(), 2, "two read events");
    assert_eq!(sr.events[0].line_no, 3, "first read is on jsonl line 3");
    assert_eq!(
        sr.events[1].line_no, 5,
        "second read on line 5 (blank+malformed counted)"
    );
    assert_eq!(sr.skipped_lines, 1, "the one malformed line is counted");
}

// ── (4) Extraction per EventKind ──

fn extract_events(records: &[(usize, Record)], file: &str) -> Vec<FileEvent> {
    extract(records, Some(file)).0
}

fn numbered(lines: &[&str]) -> Vec<(usize, Record)> {
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| (i + 1, rec(l)))
        .collect()
}

#[test]
fn extract_full_read_is_full_snapshot() {
    let recs = numbered(&[
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"user","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/a.rs","content":"café🛠\nline2\nline3","startLine":1,"numLines":3,"totalLines":3}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"r1","content":"ok"}]}}"#,
    ]);
    let ev = extract_events(&recs, "/p/a.rs");
    assert_eq!(ev.len(), 1);
    match &ev[0].kind {
        EventKind::FullSnapshot {
            content,
            total_lines,
            source,
        } => {
            assert!(content.starts_with("café🛠"), "UTF-8 verbatim round-trip");
            assert_eq!(*total_lines, 3);
            assert_eq!(*source, SnapSource::FullRead);
        }
        other => panic!("expected FullSnapshot, got {other:?}"),
    }
}

#[test]
fn extract_windowed_read_is_partial() {
    let recs = numbered(&[
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"user","toolUseResult":{"file":{"filePath":"/p/a.rs","content":"l245\nl246","startLine":245,"numLines":2,"totalLines":371}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"r1","content":"ok"}]}}"#,
    ]);
    let ev = extract_events(&recs, "/p/a.rs");
    assert_eq!(ev.len(), 1);
    match &ev[0].kind {
        EventKind::PartialRead {
            start_line,
            lines,
            total_lines,
        } => {
            assert_eq!(*start_line, 245);
            assert_eq!(lines, &vec!["l245".to_string(), "l246".to_string()]);
            assert_eq!(*total_lines, 371);
        }
        other => panic!("expected PartialRead, got {other:?}"),
    }
}

#[test]
fn extract_write_create_is_full_snapshot_write() {
    let recs = numbered(&[
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"user","toolUseResult":{"type":"create","filePath":"/p/a.rs","content":"a\nb\nc","structuredPatch":[]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#,
    ]);
    let ev = extract_events(&recs, "/p/a.rs");
    assert_eq!(ev.len(), 1);
    match &ev[0].kind {
        EventKind::FullSnapshot {
            total_lines,
            source,
            ..
        } => {
            assert_eq!(*total_lines, 3);
            assert_eq!(*source, SnapSource::Write);
        }
        other => panic!("expected Write FullSnapshot, got {other:?}"),
    }
}

#[test]
fn extract_edit_with_null_originalfile_keeps_strings_and_patch() {
    // Edit result: no `type`, has oldString/newString/structuredPatch, originalFile null.
    let recs = numbered(&[
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"user","toolUseResult":{"filePath":"/p/a.rs","oldString":"b","newString":"B","originalFile":null,"replaceAll":false,"structuredPatch":[{"oldStart":2,"oldLines":1,"newStart":2,"newLines":1,"lines":["-b","+B"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#,
    ]);
    let ev = extract_events(&recs, "/p/a.rs");
    assert_eq!(ev.len(), 1);
    match &ev[0].kind {
        EventKind::Edit {
            hunks,
            original_file,
            structured_patch,
        } => {
            assert_eq!(hunks[0].old_string, "b");
            assert_eq!(hunks[0].new_string, "B");
            assert!(original_file.is_none(), "null originalFile → None");
            let p = structured_patch.as_ref().expect("structured patch");
            assert_eq!(p[0].old_start, 2);
            assert_eq!(p[0].old_lines, 1);
        }
        other => panic!("expected Edit, got {other:?}"),
    }
}

#[test]
fn extract_edit_with_originalfile_present() {
    let recs = numbered(&[
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"user","toolUseResult":{"filePath":"/p/a.rs","oldString":"x","newString":"y","originalFile":"a\nb\nc","structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":[" a"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#,
    ]);
    let ev = extract_events(&recs, "/p/a.rs");
    match &ev[0].kind {
        EventKind::Edit { original_file, .. } => {
            assert_eq!(original_file.as_deref(), Some("a\nb\nc"));
        }
        other => panic!("expected Edit, got {other:?}"),
    }
}

#[test]
fn extract_integrity_errors_both_kinds_attributed_by_id() {
    // The error carrier has NO inline path — it is attributed via the tool_use_id join to
    // a same-turn Edit tool_use naming /p/a.rs. Both phrasings classified correctly.
    let modified = numbered(&[
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","is_error":true,"content":"<tool_use_error>File has been modified since read, either by the user or by a linter.</tool_use_error>"}]}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/p/a.rs","old_string":"x","new_string":"y"}}]}}"#,
    ]);
    let ev = extract_events(&modified, "/p/a.rs");
    let kinds: Vec<&EventKind> = ev.iter().map(|e| &e.kind).collect();
    assert!(
        kinds.iter().any(|k| matches!(
            k,
            EventKind::IntegrityError {
                kind: IntegrityKind::ModifiedSinceRead,
                ..
            }
        )),
        "modified-since-read classified + attributed by id even though it precedes the tool_use"
    );

    let notread = numbered(&[
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"e2","name":"Edit","input":{"file_path":"/p/a.rs","old_string":"x","new_string":"y"}}]}}"#,
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e2","is_error":true,"content":"<tool_use_error>File has not been read yet. Read it first before writing to it.</tool_use_error>"}]}}"#,
    ]);
    let ev2 = extract_events(&notread, "/p/a.rs");
    assert!(
        ev2.iter().any(|e| matches!(
            e.kind,
            EventKind::IntegrityError {
                kind: IntegrityKind::NotReadYet,
                ..
            }
        )),
        "not-read-yet classified"
    );
}

#[test]
fn extract_bash_touch_is_heuristic() {
    let recs = numbered(&[
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"sed -i 's/a/b/' /p/a.rs"}}]}}"#,
    ]);
    let ev = extract_events(&recs, "/p/a.rs");
    assert!(
        ev.iter()
            .any(|e| matches!(e.kind, EventKind::BashTouch { .. })),
        "bash sed -i on /p/a.rs is a BashTouch"
    );
}

#[test]
fn extract_history_snapshot_marker_for_target() {
    let recs = numbered(&[
        r#"{"type":"file-history-snapshot","snapshot":{"timestamp":"2026-06-07T05:00:00.000Z","trackedFileBackups":{"/p/a.rs":{"backupFileName":null,"version":1,"backupTime":"2026-06-07T05:00:00.000Z"}}}}"#,
    ]);
    let ev = extract_events(&recs, "/p/a.rs");
    assert_eq!(ev.len(), 1);
    assert!(matches!(ev[0].kind, EventKind::HistorySnapshotMarker));
    // A snapshot for a DIFFERENT file is not extracted for our target.
    let other = extract_events(&recs, "/p/other.rs");
    assert!(other.is_empty());
}

#[test]
fn extract_external_edit_from_attachment() {
    // An edited_text_file attachment whose snippet uses a TAB gutter (`\d+\t<text>`).
    let recs = numbered(&[
        r#"{"type":"attachment","attachment":{"type":"edited_text_file","filename":"/p/a.rs","snippet":"10\tfn café() {}\n11\t// edited"}}"#,
    ]);
    let ev = extract_events(&recs, "/p/a.rs");
    assert_eq!(ev.len(), 1);
    match &ev[0].kind {
        EventKind::ExternalEdit { snippet } => {
            assert_eq!(snippet[0], (10, "fn café() {}".to_string()));
            assert_eq!(snippet[1], (11, "// edited".to_string()));
        }
        other => panic!("expected ExternalEdit, got {other:?}"),
    }
}

#[test]
fn extract_file_attachment_is_snapshot() {
    let recs = numbered(&[
        r#"{"type":"attachment","attachment":{"type":"file","content":{"file":{"filePath":"/p/a.rs","content":"a\nb","startLine":1,"numLines":2,"totalLines":2}}}}"#,
    ]);
    let ev = extract_events(&recs, "/p/a.rs");
    assert_eq!(ev.len(), 1);
    assert!(matches!(
        ev[0].kind,
        EventKind::FullSnapshot {
            source: SnapSource::FileAttachment,
            ..
        }
    ));
}

#[test]
fn extract_multiedit_synthetic_hunks_apply_in_order() {
    // MultiEdit is ABSENT in the real fixture, so a synthetic line-fixture proves the
    // batch structuredPatch replays. Two hunks change line 1 then line 3.
    let recs = numbered(&[
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"user","toolUseResult":{"file":{"filePath":"/p/m.rs","content":"one\ntwo\nthree","startLine":1,"numLines":3,"totalLines":3}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"r1","content":"ok"}]}}"#,
        r#"{"type":"user","toolUseResult":{"filePath":"/p/m.rs","oldString":"one","newString":"ONE","structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-one","+ONE"]},{"oldStart":3,"oldLines":1,"newStart":3,"newLines":1,"lines":["-three","+THREE"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"m1","content":"ok"}]}}"#,
    ]);
    let ev = extract_events(&recs, "/p/m.rs");
    let rep = replay(&ev, None);
    let lines = rep.final_buffer.known_lines();
    assert_eq!(
        lines,
        vec![
            (1, "ONE".to_string()),
            (2, "two".to_string()),
            (3, "THREE".to_string())
        ],
        "both hunks applied at exact positions"
    );
}

// ── path matching ──

#[test]
fn path_matches_exact_and_basename_suffix() {
    assert!(path_matches(Some("/p/a.rs"), "/p/a.rs"), "exact");
    assert!(path_matches(Some("a.rs"), "/p/a.rs"), "basename suffix");
    assert!(
        path_matches(Some("p/a.rs"), "/x/p/a.rs"),
        "multi-segment suffix"
    );
    assert!(
        !path_matches(Some("b.rs"), "/p/ab.rs"),
        "not a component-aligned suffix"
    );
    assert!(!path_matches(None, "/p/a.rs"), "no target matches nothing");
}

// ── (1b) gutter strip — BOTH tab and arrow forms ──

#[test]
fn strip_gutter_handles_tab_and_arrow_and_skips_unguttered() {
    // Tab gutter (current CC), arrow gutter (older), and a no-gutter line (skipped, never
    // fabricated). Locale-neutral content with an emoji.
    let snippet = "1\tcafé🛠\n2\u{2192}second\nno gutter here\n3\tthird";
    let got = strip_gutter(snippet);
    assert_eq!(
        got,
        vec![
            (1, "café🛠".to_string()),
            (2, "second".to_string()),
            (3, "third".to_string())
        ],
        "tab + arrow recovered, un-guttered line skipped (never fabricated)"
    );
}

// ── (4) sparse-buffer replay ──

#[test]
fn replay_full_snapshot_resets_then_partial_splices_without_padding() {
    let mut buf = SparseBuffer::default();
    buf.reset_to_full("a\nb\nc", 3, 10);
    assert_eq!(
        buf.known_lines(),
        vec![(1, "a".into()), (2, "b".into()), (3, "c".into())]
    );
    // A windowed read at line 6 splices WITHOUT padding lines 4-5 (they stay gaps).
    buf.splice(6, &["f".to_string()], 6, 20);
    let ranges = buf.covered_ranges();
    assert_eq!(
        ranges,
        vec![(1, 3), (6, 6)],
        "no fabricated padding for the 4-5 gap"
    );
    assert_eq!(buf.seen_total_lines, Some(6));
}

#[test]
fn replay_edit_applies_to_running_buffer_not_originalfile() {
    // The buffer holds a\nb\nc; an edit's structuredPatch changes line 2 b→B. We apply to
    // the BUFFER (structured patch), not the originalFile field.
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "a\nb\nc".into(),
                total_lines: 3,
                source: SnapSource::Write,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "b".into(),
                    new_string: "B".into(),
                    replace_all: false,
                }],
                original_file: None,
                structured_patch: Some(vec![PatchHunk {
                    old_start: 2,
                    old_lines: 1,
                    new_lines: 1,
                    lines: vec!["-b".into(), "+B".into()],
                }]),
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![(1, "a".into()), (2, "B".into()), (3, "c".into())]
    );
}

#[test]
fn replay_unanchorable_edit_is_a_hole_not_a_fabrication() {
    // An edit whose old region falls in an unknown gap (no anchor) is un-anchorable: it
    // becomes a coverage hole and does NOT invent island lines.
    let events = vec![
        // A windowed read of lines 1-2 only.
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::PartialRead {
                start_line: 1,
                lines: vec!["a".into(), "b".into()],
                total_lines: 100,
            },
        },
        // An edit at line 50 (deep in the unknown gap).
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "zzz".into(),
                    new_string: "Z".into(),
                    replace_all: false,
                }],
                original_file: None,
                structured_patch: Some(vec![PatchHunk {
                    old_start: 50,
                    old_lines: 1,
                    new_lines: 1,
                    lines: vec!["-zzz".into(), "+Z".into()],
                }]),
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(
        rep.counts.edit_unanchorable, 1,
        "the drifted edit is a hole"
    );
    // No island line was created at 50.
    assert!(
        !rep.final_buffer.known.contains_key(&50),
        "no fabricated island at line 50"
    );
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![(1, "a".into()), (2, "b".into())]
    );
}

#[test]
fn replay_string_edit_fallback_when_no_structured_patch() {
    // No structuredPatch → string replacement over the contiguous-from-1 known text.
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "hello\nworld".into(),
                total_lines: 2,
                source: SnapSource::Write,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "world".into(),
                    new_string: "café🛠".into(),
                    replace_all: false,
                }],
                original_file: None,
                structured_patch: None,
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![(1, "hello".into()), (2, "café🛠".into())]
    );
}

// ── (5) boundary detection ──

#[test]
fn boundary_modified_since_read_is_hard() {
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "a\nb".into(),
                total_lines: 2,
                source: SnapSource::FullRead,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::IntegrityError {
                kind: IntegrityKind::ModifiedSinceRead,
                raw: "File has been modified since read".into(),
            },
        },
        FileEvent {
            line_no: 3,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "a\nb\nc".into(),
                total_lines: 3,
                source: SnapSource::FullRead,
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(rep.boundaries.len(), 1);
    assert_eq!(rep.boundaries[0].kind, "modified_since_read");
    assert_eq!(rep.boundaries[0].confidence, Confidence::Authoritative);
    assert_eq!(
        rep.segments.len(),
        2,
        "the hard boundary splits into 2 segments"
    );
}

#[test]
fn boundary_not_read_yet_is_not_a_boundary() {
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "a".into(),
                total_lines: 1,
                source: SnapSource::FullRead,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::IntegrityError {
                kind: IntegrityKind::NotReadYet,
                raw: "not read yet".into(),
            },
        },
    ];
    let rep = replay(&events, None);
    assert!(
        rep.boundaries.is_empty(),
        "not-read-yet never segments (the edit never landed)"
    );
    assert_eq!(rep.segments.len(), 1);
}

#[test]
fn boundary_external_edit_is_hard() {
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "a\nb".into(),
                total_lines: 2,
                source: SnapSource::FullRead,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::ExternalEdit {
                snippet: vec![(2, "B".into())],
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(rep.boundaries.len(), 1);
    assert_eq!(rep.boundaries[0].kind, "external_edit");
    assert_eq!(rep.boundaries[0].confidence, Confidence::Authoritative);
}

#[test]
fn boundary_bash_is_heuristic_soft() {
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "a".into(),
                total_lines: 1,
                source: SnapSource::FullRead,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::BashTouch {
                verb: "sed -i".into(),
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(rep.boundaries.len(), 1);
    assert_eq!(rep.boundaries[0].kind, "bash_mutation");
    assert_eq!(rep.boundaries[0].confidence, Confidence::Heuristic);
}

#[test]
fn boundary_originalfile_disagreement_is_hard() {
    // A full anchor sets a\nb\nc; an edit's originalFile claims X\nY\nZ (a total drift) →
    // a disagreement boundary (the signal claude-file-recovery discards).
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "a\nb\nc".into(),
                total_lines: 3,
                source: SnapSource::FullRead,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "a".into(),
                    new_string: "A".into(),
                    replace_all: false,
                }],
                original_file: Some("X\nY\nZ".into()),
                structured_patch: Some(vec![PatchHunk {
                    old_start: 1,
                    old_lines: 1,
                    new_lines: 1,
                    lines: vec!["-a".into(), "+A".into()],
                }]),
            },
        },
    ];
    let rep = replay(&events, None);
    assert!(
        rep.boundaries
            .iter()
            .any(|b| b.kind == "original_file_disagreement"),
        "originalFile vs replayed buffer disagreement is flagged"
    );
}

// ── (6) unified diff ──

#[test]
fn unified_diff_basic_change() {
    let old = vec![
        "import os".to_string(),
        "raw = open(s).read()".to_string(),
        "use(raw)".to_string(),
    ];
    let new = vec![
        "import os".to_string(),
        "with open(s) as fh:".to_string(),
        "    raw = fh.read()".to_string(),
        "use(raw)".to_string(),
    ];
    let d = unified_diff(&old, &new);
    assert!(d.contains("@@ -"), "carries a hunk header: {d}");
    assert!(d.contains("-raw = open(s).read()"), "removed line: {d}");
    assert!(d.contains("+with open(s) as fh:"), "added line: {d}");
    assert!(d.contains("+    raw = fh.read()"), "added line 2: {d}");
}

#[test]
fn unified_diff_identical_is_empty() {
    let v = vec!["a".to_string(), "b".to_string()];
    assert_eq!(unified_diff(&v, &v), "");
}

#[test]
fn unified_diff_pure_insertion_header_form() {
    let old: Vec<String> = vec![];
    let new = vec!["new1".to_string(), "new2".to_string()];
    let d = unified_diff(&old, &new);
    assert!(d.contains("+new1") && d.contains("+new2"), "{d}");
    // A zero-length old side uses the 0,0 form.
    assert!(d.contains("@@ -0,0 +1,2 @@"), "insertion header: {d}");
}

#[test]
fn lcs_diff_op_script_is_minimal() {
    let old = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let new = vec!["a".to_string(), "x".to_string(), "c".to_string()];
    let ops = lcs_diff(&old, &new);
    // a equal, b delete, x insert, c equal.
    let kinds: Vec<DiffOp> = ops.iter().map(|(o, _, _)| *o).collect();
    assert!(kinds.contains(&DiffOp::Delete) && kinds.contains(&DiffOp::Insert));
    assert_eq!(
        kinds.iter().filter(|o| **o == DiffOp::Equal).count(),
        2,
        "a and c stay equal"
    );
}

// ── (7) no-op / duplicate edit do not inflate ──

#[test]
fn noop_edit_does_not_change_buffer_or_inflate_diff() {
    // An edit whose old==new (a no-op) leaves the buffer unchanged → an empty segment diff.
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "a\nb".into(),
                total_lines: 2,
                source: SnapSource::Write,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "a".into(),
                    new_string: "a".into(),
                    replace_all: false,
                }],
                original_file: None,
                structured_patch: Some(vec![PatchHunk {
                    old_start: 1,
                    old_lines: 1,
                    new_lines: 1,
                    lines: vec![" a".into()],
                }]),
            },
        },
    ];
    let rep = replay(&events, None);
    // The no-op edit changes nothing: the final buffer is exactly the snapshot content,
    // and the edit was applied (not counted as un-anchorable) but left the lines intact.
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![(1, "a".into()), (2, "b".into())]
    );
    assert_eq!(rep.counts.edit, 1, "the edit is counted");
    assert_eq!(
        rep.counts.edit_unanchorable, 0,
        "a matching no-op is anchorable, not a hole"
    );
    // The single segment was opened by the Write anchor (pre-state empty), so its diff is
    // the file CREATION (empty → a,b) — the no-op added no further change on top of it.
    let seg = &rep.segments[0];
    let diff = unified_diff(
        &filter_lines(&seg.start_buffer, None),
        &filter_lines(&seg.end_buffer, None),
    );
    assert_eq!(
        diff, "@@ -0,0 +1,2 @@\n+a\n+b\n",
        "creation diff only; the no-op added nothing"
    );
}

// ── cutoff resolution (--at) ──

#[test]
fn resolve_cutoff_line_turn_and_datetime() {
    let events = vec![
        FileEvent {
            line_no: 10,
            turn_index: 0,
            timestamp_utc: Some("2026-06-07T05:00:00Z".into()),
            kind: EventKind::HistorySnapshotMarker,
        },
        FileEvent {
            line_no: 20,
            turn_index: 1,
            timestamp_utc: Some("2026-06-07T06:00:00Z".into()),
            kind: EventKind::HistorySnapshotMarker,
        },
        FileEvent {
            line_no: 30,
            turn_index: 2,
            timestamp_utc: Some("2026-06-07T07:00:00Z".into()),
            kind: EventKind::HistorySnapshotMarker,
        },
    ];
    assert_eq!(resolve_cutoff("@line:25", &events).unwrap(), Some(25));
    // @turn:1 → the last line whose turn ≤ 1 is line 20.
    assert_eq!(resolve_cutoff("@turn:1", &events).unwrap(), Some(20));
    // a datetime bound: ≤ 06:30 → line 20 is the highest admitted.
    assert_eq!(
        resolve_cutoff("2026-06-07T06:30:00Z", &events).unwrap(),
        Some(20)
    );
    // empty → no cutoff.
    assert_eq!(resolve_cutoff("", &events).unwrap(), None);
}

#[test]
fn at_snapshot_marks_gaps_never_fabricates() {
    // Only a windowed read of lines 3-4 → lines 1-2 and 5-6 are explicit gaps.
    let events = vec![FileEvent {
        line_no: 1,
        turn_index: 0,
        timestamp_utc: None,
        kind: EventKind::PartialRead {
            start_line: 3,
            lines: vec!["three".into(), "four".into()],
            total_lines: 6,
        },
    }];
    let rep = replay(&events, None);
    let known = rep.final_buffer.known_lines();
    let body = render_snapshot_body(&known, 6, false);
    assert!(
        body.contains("??? lines 1..2 unknown"),
        "leading gap: {body}"
    );
    assert!(body.contains("    3  three"), "known line numbered: {body}");
    assert!(
        body.contains("??? lines 5..6 unknown"),
        "trailing gap: {body}"
    );
    // No fabricated content for any gap line.
    assert!(
        !body.contains("    1  "),
        "line 1 is never emitted as content: {body}"
    );
}

// ── range parsers ──

#[test]
fn parse_line_range_is_one_based_and_validated() {
    assert_eq!(parse_line_range("100..200").unwrap(), (100, 200));
    assert!(
        parse_line_range("0..5").is_err(),
        "0 start rejected (1-based)"
    );
    assert!(parse_line_range("5..3").is_err(), "end before start");
    assert!(parse_line_range("notarange").is_err());
}

#[test]
fn parse_turn_range_matches_files_contract() {
    assert_eq!(parse_turn_range("0..1").unwrap(), (0, 1));
    assert!(parse_turn_range("3..1").is_err());
}

// ── plan candidate extraction ──

#[test]
fn extract_plan_candidates_exitplanmode_and_plan_write() {
    let recs = numbered(&[
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
        r##"{"type":"assistant","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"p1","name":"ExitPlanMode","input":{"plan":"# Plan café🛠\nstep 1","planFilePath":"/u/.claude/plans/x.md"}}]}}"##,
        r##"{"type":"assistant","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/u/.claude/plans/x.md","content":"# Plan v2"}}]}}"##,
    ]);
    let (_, plans) = extract(&recs, None);
    assert_eq!(plans.len(), 2, "ExitPlanMode + a plan-ish Write");
    assert!(plans
        .iter()
        .any(|p| p.source == "ExitPlanMode" && p.text.contains("café🛠")));
    assert!(plans
        .iter()
        .any(|p| p.source == "plan-write" && p.text == "# Plan v2"));
}

#[test]
fn is_plan_path_heuristic() {
    assert!(is_plan_path("/u/.claude/plans/x.md"));
    assert!(is_plan_path("/repo/PLAN.md"));
    assert!(is_plan_path("/repo/my-plan.md"));
    assert!(!is_plan_path("/repo/src/main.rs"));
}

// ── coverage / spans / counts formatting ──

#[test]
fn covered_spans_collapse_contiguous_runs() {
    let lines = vec![
        (1usize, "a".to_string()),
        (2, "b".to_string()),
        (4, "d".to_string()),
        (5, "e".to_string()),
    ];
    assert_eq!(covered_spans(&lines), vec![(1, 2), (4, 5)]);
    assert_eq!(fmt_spans(&covered_spans(&lines)), "[1..2] [4..5]");
    assert_eq!(fmt_spans(&[]), "(none)");
}

#[test]
fn gap_ranges_include_interior_and_trailing() {
    let known = vec![
        (2usize, "b".to_string()),
        (3, "c".to_string()),
        (5, "e".to_string()),
    ];
    // gap before 2 (1..1), gap 4..4, trailing 6..10.
    assert_eq!(gap_ranges(&known, 10), vec![(1, 1), (4, 4), (6, 10)]);
    // Empty known + a total → the whole file is a gap.
    assert_eq!(gap_ranges(&[], 5), vec![(1, 5)]);
}

#[test]
fn fmt_counts_omits_zeroes_and_flags_heuristic_bash() {
    let c = EventCounts {
        read_full: 2,
        read_windowed: 1,
        edit: 3,
        edit_unanchorable: 1,
        bash: 2,
        ..EventCounts::default()
    };
    let s = fmt_counts(&c);
    assert!(s.contains("3 read (2 full, 1 windowed)"), "{s}");
    assert!(s.contains("3 edit (1 un-anchorable)"), "{s}");
    assert!(s.contains("2 bash (heuristic)"), "{s}");
    assert!(!s.contains("write"), "zero writes omitted: {s}");
    assert_eq!(fmt_counts(&EventCounts::default()), "0");
}

// ── helpers ──

#[test]
fn split_lines_drops_only_trailing_newline() {
    assert_eq!(
        split_lines("a\nb\n"),
        vec!["a".to_string(), "b".to_string()]
    );
    assert_eq!(split_lines("a\nb"), vec!["a".to_string(), "b".to_string()]);
    assert_eq!(split_lines(""), Vec::<String>::new());
    assert_eq!(line_count("a\nb\nc"), 3);
}

#[test]
fn truncate_excerpt_is_char_counted_and_explicit() {
    let long: String = "é".repeat(EXCERPT_MAX + 5);
    let t = truncate_excerpt(&long);
    assert!(t.contains("… (+5 chars)"), "explicit marker: {t}");
    assert_eq!(truncate_excerpt("short"), "short");
}

#[test]
fn buffer_disagrees_with_original_only_on_real_mismatch() {
    let mut buf = SparseBuffer::default();
    buf.reset_to_full("a\nb\nc", 3, 1);
    // Same content → no disagreement.
    assert!(!buffer_disagrees_with_original(&buf, "a\nb\nc"));
    // Total drift → disagreement.
    assert!(buffer_disagrees_with_original(&buf, "X\nY\nZ"));
    // Nothing comparable (empty original) → no false positive.
    assert!(!buffer_disagrees_with_original(&buf, ""));
}

#[test]
fn first_line_trims_and_takes_first() {
    assert_eq!(first_line("  hello \nworld"), "hello");
    assert_eq!(first_line(""), "");
}

#[test]
fn window_admits_turn_and_time() {
    let tw = TimeWindow::default();
    assert!(window_admits(
        5,
        Some("2026-06-07T05:00:00Z"),
        Some((0, 10)),
        &tw
    ));
    assert!(
        !window_admits(11, Some("2026-06-07T05:00:00Z"), Some((0, 10)), &tw),
        "turn out of range"
    );
    // A bounded time window excludes a timestamp-less event.
    let bounded = TimeWindow::from_args(Some("2026-06-01"), None).unwrap();
    assert!(
        !window_admits(5, None, None, &bounded),
        "ts-less excluded from bounded window"
    );
}

// ── Branch-completeness: error / edge arms ──

#[test]
fn scan_one_file_empty_is_safe() {
    // A zero-byte file → mmap None → empty ScanResult (the early-return arm).
    let p = std::env::temp_dir().join(format!("csift-recover-empty-{}.jsonl", std::process::id()));
    std::fs::File::create(&p).unwrap();
    let sr = scan_one_file(&p, Some("/p/a.rs")).expect("scan empty");
    std::fs::remove_file(&p).ok();
    assert!(sr.events.is_empty());
    assert!(sr.plans.is_empty());
    assert_eq!(sr.skipped_lines, 0);
}

#[test]
fn string_edit_unanchorable_when_buffer_not_contiguous_from_one() {
    // A non-contiguous buffer (a windowed read starting at line 5) → a string-replacement
    // edit (no structuredPatch) cannot safely anchor → un-anchorable, no fabrication.
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::PartialRead {
                start_line: 5,
                lines: vec!["five".into()],
                total_lines: 10,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "five".into(),
                    new_string: "FIVE".into(),
                    replace_all: false,
                }],
                original_file: None,
                structured_patch: None,
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(
        rep.counts.edit_unanchorable, 1,
        "non-contiguous buffer → un-anchorable"
    );
    // The original windowed read line is untouched (never corrupted by a refused edit).
    assert_eq!(
        rep.final_buffer.known.get(&5).map(|c| c.text.as_str()),
        Some("five")
    );
}

#[test]
fn string_edit_unanchorable_when_old_string_absent() {
    // The buffer is contiguous from 1, but old_string is not present → un-anchorable.
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "alpha\nbeta".into(),
                total_lines: 2,
                source: SnapSource::Write,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "not-present".into(),
                    new_string: "X".into(),
                    replace_all: false,
                }],
                original_file: None,
                structured_patch: None,
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(rep.counts.edit_unanchorable, 1);
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![(1, "alpha".into()), (2, "beta".into())]
    );
}

#[test]
fn string_edit_replace_all_replaces_every_occurrence() {
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "x\nx\ny".into(),
                total_lines: 3,
                source: SnapSource::Write,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "x".into(),
                    new_string: "z".into(),
                    replace_all: true,
                }],
                original_file: None,
                structured_patch: None,
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![(1, "z".into()), (2, "z".into()), (3, "y".into())]
    );
}

#[test]
fn structured_patch_insertion_into_isolated_gap_is_unanchorable() {
    // A pure insertion (old_lines == 0) whose position is an isolated gap (no adjacent
    // known line) must NOT fabricate island lines → un-anchorable.
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::PartialRead {
                start_line: 1,
                lines: vec!["a".into(), "b".into()],
                total_lines: 100,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: String::new(),
                    new_string: "ins".into(),
                    replace_all: false,
                }],
                original_file: None,
                // Insert at line 60 (deep in the gap, no adjacent known line).
                structured_patch: Some(vec![PatchHunk {
                    old_start: 60,
                    old_lines: 0,
                    new_lines: 1,
                    lines: vec!["+ins".into()],
                }]),
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(
        rep.counts.edit_unanchorable, 1,
        "isolated-gap insertion is un-anchorable"
    );
    assert!(
        !rep.final_buffer.known.contains_key(&60),
        "no island at line 60"
    );
}

#[test]
fn structured_patch_context_mismatch_refuses_to_corrupt() {
    // The buffer holds a\nb\nc; a patch hunk at line 2 claims its old context is "WRONG"
    // (the buffer disagrees) → the edit is refused (un-anchorable), the known line stays.
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::FullSnapshot {
                content: "a\nb\nc".into(),
                total_lines: 3,
                source: SnapSource::Write,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "WRONG".into(),
                    new_string: "Z".into(),
                    replace_all: false,
                }],
                original_file: None,
                structured_patch: Some(vec![PatchHunk {
                    old_start: 2,
                    old_lines: 1,
                    new_lines: 1,
                    lines: vec!["-WRONG".into(), "+Z".into()],
                }]),
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(rep.counts.edit_unanchorable, 1, "context mismatch → refuse");
    assert_eq!(
        rep.final_buffer.known.get(&2).map(|c| c.text.as_str()),
        Some("b"),
        "the known line is NOT corrupted by the refused edit"
    );
}

#[test]
fn snap_source_and_confidence_labels() {
    assert_eq!(SnapSource::Write.label(), "write");
    assert_eq!(SnapSource::FullRead.label(), "full-read");
    assert_eq!(SnapSource::FileAttachment.label(), "file-attachment");
    assert_eq!(Confidence::Authoritative.label(), "AUTHORITATIVE");
    assert_eq!(Confidence::Heuristic.label(), "HEURISTIC");
    assert_eq!(Confidence::Authoritative.json(), "authoritative");
    assert_eq!(Confidence::Heuristic.json(), "heuristic");
}

#[test]
fn plan_is_later_by_timestamp_then_line() {
    let a = PlanCandidate {
        line_no: 10,
        turn_index: 0,
        timestamp_utc: Some("2026-06-07T06:00:00Z".into()),
        source: "ExitPlanMode",
        path: None,
        text: "a".into(),
    };
    let b = PlanCandidate {
        line_no: 99,
        turn_index: 0,
        timestamp_utc: Some("2026-06-07T05:00:00Z".into()),
        source: "ExitPlanMode",
        path: None,
        text: "b".into(),
    };
    // a has the later TIMESTAMP despite a lower line_no → a is later.
    assert!(plan_is_later(&a, &b));
    // With no timestamps, fall back to line_no.
    let c = PlanCandidate {
        timestamp_utc: None,
        line_no: 5,
        ..a.clone()
    };
    let d = PlanCandidate {
        timestamp_utc: None,
        line_no: 3,
        ..b.clone()
    };
    assert!(plan_is_later(&c, &d));
}

#[test]
fn render_snapshot_body_empty_known_is_explicit() {
    // No known lines + a seen total → the whole file is one explicit gap, never content.
    let body = render_snapshot_body(&[], 5, false);
    assert!(body.contains("??? lines 1..5 unknown"), "{body}");
    // No known lines + no total → an honest "no content" note.
    let none = render_snapshot_body(&[], 0, false);
    assert!(none.contains("no content seen"), "{none}");
}

#[test]
fn apply_line_range_filters_known_lines() {
    let lines = vec![
        (1usize, "a".to_string()),
        (5, "b".to_string()),
        (10, "c".to_string()),
    ];
    let got = apply_line_range(lines.clone(), Some((5, 10)));
    assert_eq!(got, vec![(5, "b".to_string()), (10, "c".to_string())]);
    // None → unchanged.
    assert_eq!(apply_line_range(lines.clone(), None), lines);
}

#[test]
fn classify_integrity_error_distinguishes_kinds_and_ignores_other_errors() {
    let modified = serde_json::json!(
        "<tool_use_error>File has been modified since read, either by the user.</tool_use_error>"
    );
    assert_eq!(
        classify_integrity_error(&modified),
        Some(IntegrityKind::ModifiedSinceRead)
    );
    let notread = serde_json::json!("<tool_use_error>File has not been read yet.</tool_use_error>");
    assert_eq!(
        classify_integrity_error(&notread),
        Some(IntegrityKind::NotReadYet)
    );
    // A different tool error is NOT an integrity boundary.
    let other = serde_json::json!("<tool_use_error>Command timed out.</tool_use_error>");
    assert_eq!(classify_integrity_error(&other), None);
}
