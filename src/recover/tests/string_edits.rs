//! String-edit application and fallbacks: anchoring refusals, originalFile anchors.

use super::*;

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
    // the file CREATION (empty → a,b) - the no-op added no further change on top of it.
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
