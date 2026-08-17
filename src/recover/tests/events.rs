//! Event extraction from tool records: reads, writes, edits, integrity errors, candidates.

use super::*;

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
    // The error carrier has NO inline path - it is attributed via the tool_use_id join to
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
