//! Event replay onto the sparse buffer: snapshots, splices, segments.

use super::*;

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
