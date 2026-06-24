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
    extract(records, Some(file))
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
fn reset_to_full_drops_phantom_trailing_line_from_separator_count_total() {
    // CC's Read / file-attachment `totalLines` is a SEPARATOR count: a 2-line file ending in
    // `\n` reports totalLines=3. We hold the full content, so split_lines (2) is authoritative —
    // the phantom 3rd line must NOT inflate seen_total (else restore/salvage/at/coverage report a
    // spurious unknown trailing line). Regression for the node24-migrate.sh 96/97 (98%) bug.
    let mut buf = SparseBuffer::default();
    buf.reset_to_full("a\nb\n", 3, 10);
    assert_eq!(buf.known_lines(), vec![(1, "a".into()), (2, "b".into())]);
    assert_eq!(
        buf.seen_total_lines,
        Some(2),
        "separator-count total 3 must normalise to the terminator count 2"
    );
    // A non-newline-terminated full snapshot keeps its total verbatim (there is no phantom line).
    let mut buf2 = SparseBuffer::default();
    buf2.reset_to_full("a\nb", 2, 10);
    assert_eq!(buf2.seen_total_lines, Some(2));
}

#[test]
fn replay_full_read_after_write_does_not_invent_trailing_gap() {
    // The real node24-migrate.sh sequence: a Write creates the file (terminator count), then a
    // later full READ / file-attachment re-observes it with a SEPARATOR-count `totalLines` (N+1
    // for a newline-terminated file). The read is the LAST event, so it sets the final
    // seen_total — which must stay N, not N+1, so `recover` (restore) sees a COMPLETE file.
    let events = vec![
        FileEvent {
            line_no: 1,
            turn_index: 0,
            timestamp_utc: Some("2026-06-09T00:00:00.000Z".into()),
            kind: EventKind::FullSnapshot {
                content: "l1\nl2\nl3\n".into(),
                total_lines: 3, // Write: terminator count
                source: SnapSource::Write,
            },
        },
        FileEvent {
            line_no: 2,
            turn_index: 1,
            timestamp_utc: Some("2026-06-09T00:01:00.000Z".into()),
            kind: EventKind::FullSnapshot {
                content: "l1\nl2\nl3\n".into(),
                total_lines: 4, // Read / file-attachment: SEPARATOR count (the phantom +1)
                source: SnapSource::FileAttachment,
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(rep.final_buffer.known_lines().len(), 3);
    assert_eq!(
        rep.final_buffer.seen_total_lines,
        Some(3),
        "the separator-count read must not invent a phantom 4th line"
    );
    // complete == (known.len() == seen_total) → restore succeeds instead of hard-failing.
    assert_eq!(
        rep.final_buffer.known_lines().len(),
        rep.final_buffer.seen_total_lines.unwrap()
    );
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
    let d = unified_diff(&old, &new, 3);
    assert!(d.contains("@@ -"), "carries a hunk header: {d}");
    assert!(d.contains("-raw = open(s).read()"), "removed line: {d}");
    assert!(d.contains("+with open(s) as fh:"), "added line: {d}");
    assert!(d.contains("+    raw = fh.read()"), "added line 2: {d}");
}

#[test]
fn unified_diff_identical_is_empty() {
    let v = vec!["a".to_string(), "b".to_string()];
    assert_eq!(unified_diff(&v, &v, 3), "");
}

#[test]
fn unified_diff_pure_insertion_header_form() {
    let old: Vec<String> = vec![];
    let new = vec!["new1".to_string(), "new2".to_string()];
    let d = unified_diff(&old, &new, 3);
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
        usize::MAX,
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

// ─────────────────────────────────────────────────────────────────────────────
// Branch-completeness round 2: error / second-operand / short-circuit arms that
// the first test pass left uncovered. Each drives a SPECIFIC missed branch with a
// real assertion on the documented behavior (never a coverage-only touch).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn classify_integrity_error_accepts_alternate_phrasings() {
    // The classifier ORs two phrasings per kind. The first test only hit the canonical
    // "has been modified since read" / "has not been read yet" wordings; these drive the
    // SECOND operand of each `||` (the harness has emitted both forms in real transcripts).
    let modified_alt =
        serde_json::json!("<tool_use_error>File has been modified externally.</tool_use_error>");
    assert_eq!(
        classify_integrity_error(&modified_alt),
        Some(IntegrityKind::ModifiedSinceRead),
        "the 'File has been modified' phrasing (no 'since read') still classifies as modified"
    );
    let notread_alt = serde_json::json!(
        "<tool_use_error>You must Read it first before editing.</tool_use_error>"
    );
    assert_eq!(
        classify_integrity_error(&notread_alt),
        Some(IntegrityKind::NotReadYet),
        "the 'Read it first' phrasing (no 'has not been read yet') classifies as not-read-yet"
    );
}

#[test]
fn path_matches_multi_segment_suffix_requires_slash_boundary() {
    // The basename-suffix fallback strips `target` off the tail and accepts ONLY when the
    // remaining prefix is empty OR ends in '/'. This drives the `prefix.ends_with('/')`
    // operand: a deep multi-segment suffix whose prefix is non-empty but slash-aligned.
    assert!(
        path_matches(Some("turn_engine/engine.py"), "/a/b/turn_engine/engine.py"),
        "multi-segment suffix accepted at a '/' boundary (prefix '/a/b/' ends in '/')"
    );
    // A suffix that lands mid-component (prefix does NOT end in '/') is rejected.
    assert!(
        !path_matches(Some("engine.py"), "/a/bxengine.py"),
        "mid-component match rejected (prefix 'bx' is non-empty and not slash-aligned)"
    );
}

#[test]
fn line_is_recover_candidate_matches_each_distinct_marker() {
    // The prefilter is a big OR of byte-substring probes; the corpus tests mostly hit the
    // FIRST matching operand, so later operands' "found" sides stay uncovered. Drive each
    // marker with a line that contains ONLY that marker (no earlier-listed substring).
    // toolUseResult (no "role":"user").
    assert!(line_is_recover_candidate(br#"{"toolUseResult":{"x":1}}"#));
    // tool_use_error in isolation (no earlier marker substring).
    assert!(line_is_recover_candidate(
        br#"{"x":"<tool_use_error>boom</tool_use_error>"}"#
    ));
    // file-history-snapshot in isolation.
    assert!(line_is_recover_candidate(
        br#"{"type":"file-history-snapshot"}"#
    ));
    // edited_text_file in isolation.
    assert!(line_is_recover_candidate(
        br#"{"attachment":{"type":"edited_text_file"}}"#
    ));
    // A line carrying NONE of the markers is rejected (the all-false fall-through).
    assert!(!line_is_recover_candidate(
        br#"{"type":"summary","leafUuid":"x"}"#
    ));
}

#[test]
fn structured_patch_resizes_dense_when_hunk_extends_past_known_end() {
    // A patch hunk whose old region runs PAST the current known buffer length forces the
    // `end > dense.len()` resize path. The buffer knows lines 1-2; a hunk at oldStart 2,
    // oldLines 2 reaches line 3 (one past the end). The trailing unknown line is a gap, so
    // the region is not fully known → the edit is un-anchorable (no fabrication), but the
    // resize branch is exercised on the way to that verdict.
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
                    old_string: "b".into(),
                    new_string: "B".into(),
                    replace_all: false,
                }],
                original_file: None,
                structured_patch: Some(vec![PatchHunk {
                    old_start: 2,
                    old_lines: 2, // reaches line 3, past the known end (resize path)
                    new_lines: 1,
                    lines: vec![" b".into(), "-gone".into(), "+B".into()],
                }]),
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(
        rep.counts.edit_unanchorable, 1,
        "a hunk reaching past known content over a gap is un-anchorable"
    );
    // The known line 2 is untouched (the refused edit never corrupts it).
    assert_eq!(
        rep.final_buffer.known.get(&2).map(|c| c.text.as_str()),
        Some("b")
    );
}

#[test]
fn string_edit_empty_old_string_is_unanchorable() {
    // The string-replacement fallback refuses an empty old_string (the FIRST operand of
    // `old_string.is_empty() || !contains`). An empty old_string would match everywhere →
    // we never guess, so it is un-anchorable and the buffer is unchanged.
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
                    old_string: String::new(),
                    new_string: "X".into(),
                    replace_all: false,
                }],
                original_file: None,
                structured_patch: None, // force the string-edit fallback
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(
        rep.counts.edit_unanchorable, 1,
        "empty old_string is refused, not applied everywhere"
    );
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![(1, "alpha".into()), (2, "beta".into())],
        "buffer is left intact"
    );
}

#[test]
fn string_edit_with_no_hunks_is_unanchorable() {
    // An Edit carrying ZERO hunks never sets `any` → the `if !any` guard returns
    // un-anchorable (a real harness shape when a MultiEdit result has an empty edit list).
    let mut buf = SparseBuffer::default();
    buf.reset_to_full("a\nb", 2, 1);
    let outcome = apply_string_edit(&mut buf, &[], 9);
    assert_eq!(
        outcome,
        EditOutcome::UnAnchorable,
        "no hunks → nothing applied → un-anchorable"
    );
    assert_eq!(
        buf.known_lines(),
        vec![(1, "a".into()), (2, "b".into())],
        "buffer unchanged by a no-hunk edit"
    );
}

#[test]
fn replay_edit_into_already_open_segment_does_not_reopen() {
    // When an Edit arrives with a segment already open (a windowed read opened it, no full
    // anchor yet), the `seg_open.is_none()` guard takes its FALSE side — the edit extends
    // the open segment instead of opening a new one. With no full anchor, the string-edit
    // fallback cannot anchor a non-contiguous-from-1 buffer, so it is a hole — but the
    // single-segment shape proves the open segment was reused, not reopened.
    let events = vec![
        FileEvent {
            line_no: 10,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::PartialRead {
                start_line: 1,
                lines: vec!["a".into(), "b".into()],
                total_lines: 2,
            },
        },
        FileEvent {
            line_no: 11,
            turn_index: 0,
            timestamp_utc: None,
            kind: EventKind::Edit {
                hunks: vec![EditHunk {
                    old_string: "a".into(),
                    new_string: "A".into(),
                    replace_all: false,
                }],
                original_file: None,
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
    assert_eq!(
        rep.segments.len(),
        1,
        "the edit extended the read's open segment (seg_open was already Some)"
    );
    // The structured patch anchored on the known contiguous lines 1-2 → applied.
    assert_eq!(
        rep.final_buffer.known.get(&1).map(|c| c.text.as_str()),
        Some("A"),
        "structured patch anchored on the read's known lines"
    );
}

#[test]
fn lcs_diff_emits_trailing_deletions_when_old_is_longer() {
    // old has lines new lacks at the END → the `while i < n` tail-delete loop runs (the
    // first pass only exercised balanced / leading change runs).
    let old = vec!["keep".to_string(), "drop1".to_string(), "drop2".to_string()];
    let new = vec!["keep".to_string()];
    let d = unified_diff(&old, &new, 3);
    assert!(
        d.contains("-drop1") && d.contains("-drop2"),
        "tail deletes: {d}"
    );
    // A pure deletion's new side is zero-length → the 0,0-style new header.
    assert!(
        d.contains("+0,0") || d.contains(" +1,1 "),
        "new header form: {d}"
    );
    let ops = lcs_diff(&old, &new);
    assert_eq!(
        ops.iter().filter(|(o, _, _)| *o == DiffOp::Delete).count(),
        2,
        "exactly the two trailing lines are deletes"
    );
}

#[test]
fn lcs_diff_emits_trailing_insertions_when_new_is_longer() {
    // new has lines old lacks at the END → the `while j < m` tail-insert loop runs.
    let old = vec!["keep".to_string()];
    let new = vec!["keep".to_string(), "add1".to_string(), "add2".to_string()];
    let d = unified_diff(&old, &new, 3);
    assert!(
        d.contains("+add1") && d.contains("+add2"),
        "tail inserts: {d}"
    );
    let ops = lcs_diff(&old, &new);
    assert_eq!(
        ops.iter().filter(|(o, _, _)| *o == DiffOp::Insert).count(),
        2,
        "exactly the two trailing lines are inserts"
    );
}

#[test]
fn unified_diff_pure_deletion_uses_zero_length_new_header() {
    // A diff that ONLY removes lines (no inserts, no surviving context) → `new_count == 0`
    // and `new_lo == usize::MAX`, driving the `if new_lo == usize::MAX` reset and the
    // `if new_count == 0` header form on the NEW side.
    let old = vec!["x".to_string(), "y".to_string()];
    let new: Vec<String> = vec![];
    let d = unified_diff(&old, &new, 3);
    assert!(d.contains("-x") && d.contains("-y"), "both removed: {d}");
    assert!(
        d.contains("@@ -1,2 +0,0 @@"),
        "pure-deletion header uses the 0,0 new form: {d}"
    );
}

#[test]
fn resolve_cutoff_empty_string_means_no_cutoff() {
    // An empty `--at` spec returns None (replay everything). Drives the `when.is_empty()`
    // true arm in isolation (the combined test asserted it among others).
    let events = vec![FileEvent {
        line_no: 7,
        turn_index: 0,
        timestamp_utc: Some("2026-06-07T05:00:00Z".into()),
        kind: EventKind::HistorySnapshotMarker,
    }];
    assert_eq!(
        resolve_cutoff("   ", &events).unwrap(),
        None,
        "blank → no cutoff"
    );
}

#[test]
fn window_admits_turn_below_low_bound_is_rejected() {
    // The `turn_index < lo` operand of the range check: a turn BELOW the window's low bound
    // is excluded (the prior test only drove the `> hi` operand).
    let tw = TimeWindow::default();
    assert!(
        !window_admits(2, None, Some((5, 10)), &tw),
        "turn 2 is below the [5,10] window low bound"
    );
    assert!(
        window_admits(5, None, Some((5, 10)), &tw),
        "turn 5 is the inclusive low bound"
    );
}

#[test]
fn fmt_counts_write_only_and_external_and_integrity_arms() {
    // The first fmt_counts test left the write / external_edit / history_snapshot /
    // integrity_error display arms uncovered (it only had reads/edits/bash). Drive them.
    let c = EventCounts {
        write: 1,
        external_edit: 2,
        history_snapshot: 1,
        integrity_error: 3,
        ..EventCounts::default()
    };
    let s = fmt_counts(&c);
    assert!(s.contains("1 write"), "write arm: {s}");
    assert!(s.contains("2 external-edit"), "external-edit arm: {s}");
    assert!(
        s.contains("1 history-snapshot"),
        "history-snapshot arm: {s}"
    );
    assert!(s.contains("3 integrity-error"), "integrity-error arm: {s}");
    // An edit with NO un-anchorable companion → the empty-suffix branch of the edit arm.
    let only_edit = EventCounts {
        edit: 2,
        ..EventCounts::default()
    };
    let se = fmt_counts(&only_edit);
    assert_eq!(
        se, "2 edit",
        "no un-anchorable suffix when all edits anchored"
    );
}

#[test]
fn render_snapshot_body_inline_truncates_long_known_lines() {
    // `inline_trunc = true` truncates an over-long known line with the explicit marker; the
    // first body test only exercised the verbatim (`false`) path.
    let long = "z".repeat(EXCERPT_MAX + 10);
    let known = vec![(1usize, long.clone())];
    let trunc = render_snapshot_body(&known, 1, true);
    assert!(
        trunc.contains("… (+10 chars)"),
        "inline-truncated with explicit marker: {}",
        &trunc[..trunc.len().min(80)]
    );
    // The verbatim form keeps the full line.
    let verbatim = render_snapshot_body(&known, 1, false);
    assert!(verbatim.contains(&long), "verbatim form is not truncated");
}

#[test]
fn gap_ranges_whole_file_unknown_when_no_known_lines() {
    // Empty known + total>0 → the `else if known.is_empty() && total > 0` arm yields one
    // whole-file gap. (The interior/trailing arms were already covered.)
    assert_eq!(gap_ranges(&[], 4), vec![(1, 4)]);
    // Empty known + total == 0 → no gaps at all (nothing seen, nothing to mark).
    assert_eq!(gap_ranges(&[], 0), Vec::<(usize, usize)>::new());
}

#[test]
fn structured_patch_multi_hunk_running_offset_anchors_later_hunk() {
    // A two-hunk patch where the FIRST hunk inserts extra lines (positive running offset),
    // shifting the SECOND hunk's anchored position. This exercises the cross-hunk offset
    // accounting (each later hunk's oldStart maps onto the already-shifted dense vector).
    // Buffer is a\nb\nc; hunk 1 expands line 1 into three lines, hunk 2 then changes the
    // (offset-shifted) last line.
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
                    old_string: "a".into(),
                    new_string: "a1\na2\na3".into(),
                    replace_all: false,
                }],
                original_file: None,
                structured_patch: Some(vec![
                    // hunk 1: line 1 (a) → three lines → running offset +2.
                    PatchHunk {
                        old_start: 1,
                        old_lines: 1,
                        new_lines: 3,
                        lines: vec!["-a".into(), "+a1".into(), "+a2".into(), "+a3".into()],
                    },
                    // hunk 2: original line 3 (c) → C. With offset +2 its start is 5, end 6,
                    // past the span+1 (=5) dense length → forces the resize.
                    PatchHunk {
                        old_start: 3,
                        old_lines: 1,
                        new_lines: 1,
                        lines: vec!["-c".into(), "+C".into()],
                    },
                ]),
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![
            (1, "a1".into()),
            (2, "a2".into()),
            (3, "a3".into()),
            (4, "b".into()),
            (5, "C".into()),
        ],
        "both hunks applied with the running offset; the trailing line C was reached via the resize"
    );
}

#[test]
fn replay_edit_as_first_event_opens_a_segment() {
    // An Edit arriving with NO prior read/anchor (seg_open == None) takes the TRUE side of
    // `if seg_open.is_none()` and OPENS a fresh segment. With no known content it cannot
    // anchor (un-anchorable hole), but a segment is opened so the op is timeline-visible.
    let events = vec![FileEvent {
        line_no: 1,
        turn_index: 0,
        timestamp_utc: None,
        kind: EventKind::Edit {
            hunks: vec![EditHunk {
                old_string: "x".into(),
                new_string: "y".into(),
                replace_all: false,
            }],
            original_file: None,
            structured_patch: None,
        },
    }];
    let rep = replay(&events, None);
    assert_eq!(
        rep.segments.len(),
        1,
        "the lone edit opened a segment (seg_open was None → the if-body ran)"
    );
    assert_eq!(
        rep.counts.edit_unanchorable, 1,
        "with no known content the edit is an honest hole"
    );
}

#[test]
fn collect_tool_use_paths_skips_missing_and_empty_file_path() {
    // A Read tool_use with NO file_path input, and an Edit tool_use with an EMPTY one →
    // neither is recorded (the `if let Some(p)` None side + the `!p.is_empty()` false side).
    let recs = numbered(&[
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"r1","name":"Read","input":{"limit":5}},{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"","old_string":"x","new_string":"y"}}]}}"#,
    ]);
    let mut out = std::collections::BTreeMap::new();
    for (_, r) in &recs {
        collect_tool_use_paths(r.blocks(), &mut out);
    }
    assert!(
        out.is_empty(),
        "a Read with no file_path and an Edit with an empty one record no id→path entry: {out:?}"
    );
}

#[test]
fn bash_tool_use_without_command_is_ignored() {
    // A Bash tool_use with no `command` input → the `if let Some(cmd)` None side; no
    // BashTouch is produced (and nothing panics on the missing field).
    let recs = numbered(&[
        r#"{"type":"user","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"timeout":1000}}]}}"#,
    ]);
    let ev = extract_events(&recs, "/p/a.rs");
    assert!(
        ev.is_empty(),
        "a Bash tool_use with no command touches nothing: {}",
        ev.len()
    );
}

#[test]
fn history_snapshot_without_tracked_backups_is_ignored() {
    // A file-history-snapshot whose snapshot object lacks `trackedFileBackups` → the
    // `if let Some(tfb)` None side; no marker event is emitted.
    let recs = numbered(&[
        r#"{"type":"file-history-snapshot","snapshot":{"timestamp":"2026-06-07T05:00:00.000Z","messageId":"m1"}}"#,
    ]);
    let ev = extract_events(&recs, "/p/a.rs");
    assert!(
        ev.is_empty(),
        "a snapshot with no trackedFileBackups yields no marker: {}",
        ev.len()
    );
}

#[test]
fn apply_edit_falls_back_to_string_edit_when_structured_patch_is_empty() {
    // A structured_patch of `Some(vec![])` (empty) takes the `if !patches.is_empty()` FALSE
    // side and falls through to the string-replacement path, which still applies cleanly.
    let mut buf = SparseBuffer::default();
    buf.reset_to_full("hello\nworld", 2, 1);
    let hunks = vec![EditHunk {
        old_string: "world".into(),
        new_string: "there".into(),
        replace_all: false,
    }];
    let outcome = apply_edit(&mut buf, &hunks, &Some(vec![]), 9);
    assert_eq!(
        outcome,
        EditOutcome::Applied,
        "empty patch → string fallback applied"
    );
    assert_eq!(
        buf.known_lines(),
        vec![(1, "hello".into()), (2, "there".into())]
    );
}

#[test]
fn structured_patch_pure_insertion_adjacent_to_known_lines_applies() {
    // A pure insertion (old_lines == 0) adjacent to known content drives the
    // `h.old_lines > 0` FALSE side of both the region-known and context-verify guards, and
    // the insertion is applied (the gap-isolated variant is covered separately as a hole).
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
                    new_string: "a\nINS".into(),
                    replace_all: false,
                }],
                original_file: None,
                // Insert after line 1 (old_lines 0, adjacent to the known line 1).
                structured_patch: Some(vec![PatchHunk {
                    old_start: 2,
                    old_lines: 0,
                    new_lines: 1,
                    lines: vec!["+INS".into()],
                }]),
            },
        },
    ];
    let rep = replay(&events, None);
    assert_eq!(
        rep.final_buffer.known_lines(),
        vec![(1, "a".into()), (2, "INS".into()), (3, "b".into())],
        "the adjacent pure insertion is applied without fabricating islands"
    );
    assert_eq!(
        rep.counts.edit_unanchorable, 0,
        "an anchored insertion is not a hole"
    );
}

#[test]
fn edit_originalfile_present_but_no_full_anchor_does_not_flag() {
    // An Edit carrying an originalFile arrives BEFORE any full anchor (had_full_anchor is
    // false) → the `had_full_anchor && …` short-circuit FALSE side: no disagreement
    // boundary is raised (we cannot prove drift without an anchor to compare against).
    let events = vec![FileEvent {
        line_no: 1,
        turn_index: 0,
        timestamp_utc: None,
        kind: EventKind::Edit {
            hunks: vec![EditHunk {
                old_string: "a".into(),
                new_string: "A".into(),
                replace_all: false,
            }],
            original_file: Some("totally\ndifferent".into()),
            structured_patch: None,
        },
    }];
    let rep = replay(&events, None);
    assert!(
        rep.boundaries.is_empty(),
        "no anchor yet → originalFile cannot be cross-checked → no false boundary"
    );
}

#[test]
fn structured_patch_skips_context_check_when_old_region_len_mismatches() {
    // A malformed hunk that claims old_lines=2 but lists only ONE old-region line drives the
    // `old_region.len() == h.old_lines` FALSE side: the context-equality check is skipped
    // (we cannot compare a mismatched region), but the region-known guard still applies. The
    // region IS known here (lines 1-2), so the edit is applied by position without the
    // (impossible) context comparison.
    let mut buf = SparseBuffer::default();
    buf.reset_to_full("a\nb\nc", 3, 1);
    // old_lines=2 but only one " " context line listed → old_region.len()==1 != 2.
    let patches = vec![PatchHunk {
        old_start: 1,
        old_lines: 2,
        new_lines: 1,
        lines: vec![" a".into(), "+merged".into()],
    }];
    let outcome = apply_structured_patch(&mut buf, &patches, 9);
    assert_eq!(
        outcome,
        EditOutcome::Applied,
        "a mismatched-length old-region is applied by position, skipping the context check"
    );
    // The new region is the context (` a`) + added (`+merged`) lines, spliced over the
    // declared 2-line old span (lines 1-2), so line 3 (c) is preserved after them.
    assert_eq!(
        buf.known_lines(),
        vec![(1, "a".into()), (2, "merged".into()), (3, "c".into())]
    );
}

#[test]
fn edit_originalfile_agreeing_with_buffer_raises_no_boundary() {
    // An Edit whose originalFile MATCHES the anchored buffer drives the
    // `buffer_disagrees_with_original(...)` FALSE side (anchor present, no drift) → no
    // disagreement boundary, and the edit applies normally.
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
                    old_string: "b".into(),
                    new_string: "B".into(),
                    replace_all: false,
                }],
                // originalFile AGREES with the replayed buffer → no disagreement.
                original_file: Some("a\nb\nc".into()),
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
    assert!(
        !rep.boundaries
            .iter()
            .any(|b| b.kind == "original_file_disagreement"),
        "an agreeing originalFile raises no boundary"
    );
    assert_eq!(
        rep.final_buffer.known.get(&2).map(|c| c.text.as_str()),
        Some("B"),
        "the edit applied cleanly"
    );
}

#[test]
fn unified_diff_caps_leading_context_at_three_lines() {
    // A change far into the file (preceded by many identical lines) must show AT MOST 3
    // lines of leading context — driving the `ctx_back < CONTEXT` cap of the back-context
    // walk. Lines 1-6 are identical; line 7 changes. The hunk's first context line is line
    // 4 (7 minus 3), never line 1.
    let old: Vec<String> = (1..=8).map(|n| format!("line{n}")).collect();
    let mut new = old.clone();
    new[6] = "line7-CHANGED".to_string(); // change the 7th line (index 6)
    let d = unified_diff(&old, &new, 3);
    assert!(
        d.contains("-line7") && d.contains("+line7-CHANGED"),
        "the change: {d}"
    );
    // Exactly three leading context lines (line4, line5, line6); line3 and earlier excluded.
    assert!(
        d.contains(" line4\n line5\n line6\n"),
        "3 lines of leading context: {d}"
    );
    assert!(
        !d.contains(" line1\n") && !d.contains(" line3\n"),
        "context is capped at 3 lines (line1..line3 excluded): {d}"
    );
}

#[test]
fn unified_diff_full_context_shows_every_line() {
    // usize::MAX context (what --patches passes) reproduces the WHOLE file as context — a
    // far-away change still drags every read line into one spanning hunk. This is what makes
    // `--patches` of a fully-read, one-line-edited file contain all lines (CC's Read-before-Edit
    // guarantees those context lines were genuinely observed, so they are valid to include).
    let old: Vec<String> = (1..=8).map(|n| format!("line{n}")).collect();
    let mut new = old.clone();
    new[6] = "line7-CHANGED".to_string();
    let d = unified_diff(&old, &new, usize::MAX);
    // Every distant line appears as context — line1 and line3 are NOT excluded here.
    for n in [1, 2, 3, 4, 5, 6, 8] {
        assert!(
            d.contains(&format!(" line{n}\n")),
            "full context keeps line{n}: {d}"
        );
    }
    assert!(
        d.contains("-line7") && d.contains("+line7-CHANGED"),
        "the change is still marked: {d}"
    );
    // One spanning hunk over all 8 lines.
    assert_eq!(d.matches("@@ -").count(), 1, "single full-span hunk: {d}");
}

#[test]
fn buffer_disagrees_below_threshold_does_not_flag() {
    // A single mismatch in many comparable lines stays UNDER the 25% threshold → the
    // `mismatches * 4 >= compared` FALSE side: not flagged (one fluke is not a boundary).
    let mut buf = SparseBuffer::default();
    buf.reset_to_full("a\nb\nc\nd\ne\nf\ng\nh", 8, 1);
    // Original differs in exactly ONE of eight lines (12.5% < 25%).
    assert!(
        !buffer_disagrees_with_original(&buf, "a\nb\nc\nX\ne\nf\ng\nh"),
        "1/8 mismatch is below the disagreement threshold"
    );
}

#[test]
fn strip_gutter_skips_digits_without_a_separator_and_unparseable_widths() {
    // A line with leading digits but NEITHER a tab NOR an arrow separator → the final else
    // `continue` (no gutter recognized). And a digit run too large for usize → the
    // `digits.parse::<usize>()` Err side: skipped, never fabricated.
    let huge = "9".repeat(40); // overflows usize
    let snippet = format!("12 no-separator-here\n{huge}\tlost\n7\tkept");
    let got = strip_gutter(&snippet);
    assert_eq!(
        got,
        vec![(7, "kept".to_string())],
        "only the well-formed tab-guttered line survives: {got:?}"
    );
}

#[test]
fn path_matches_deep_suffix_after_slash_boundary() {
    // Directly drives the `prefix.ends_with('/')` operand: a target that is a trailing
    // multi-segment slice whose stripped prefix is non-empty and slash-terminated.
    assert!(
        path_matches(Some("src/relay/engine.py"), "/root/app/src/relay/engine.py"),
        "deep suffix accepted (prefix '/root/app/' is non-empty and ends with '/')"
    );
    assert!(
        path_matches(Some("b/c.rs"), "/a/b/c.rs"),
        "two-segment suffix at a '/' boundary"
    );
}

#[test]
fn buffer_disagrees_requires_partial_overlap_and_threshold() {
    // Known lines that fall OUTSIDE the original's length are skipped (the `*k <= len`
    // guard), and a single mismatch below the 25% threshold does NOT flag.
    let mut buf = SparseBuffer::default();
    // Known lines 1,2,3,4 — original is only 4 long; one of four mismatches = 25% → flags.
    buf.reset_to_full("a\nb\nc\nQ", 4, 1);
    assert!(
        buffer_disagrees_with_original(&buf, "a\nb\nc\nd"),
        "one mismatch in four (25%) meets the threshold"
    );
    // A known line beyond the original's length is ignored (not compared, not a mismatch).
    let mut buf2 = SparseBuffer::default();
    buf2.reset_to_full("a\nb\nEXTRA", 3, 1);
    assert!(
        !buffer_disagrees_with_original(&buf2, "a\nb"),
        "the line-3 EXTRA is beyond the 2-line original → not compared → no false flag"
    );
}
