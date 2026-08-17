//! Structured-patch application: hunk anchoring, offsets, context checks.

use super::*;

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
