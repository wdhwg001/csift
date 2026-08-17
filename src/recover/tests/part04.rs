use super::*;

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
    let got = apply_line_range(
        lines.clone(),
        Some(crate::text::parse_range_spec("5..10", "--file-lines", true).unwrap()),
    );
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
