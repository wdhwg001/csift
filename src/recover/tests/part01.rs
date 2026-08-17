use super::*;

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
