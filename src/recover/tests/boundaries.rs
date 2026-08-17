//! Modified-since-read and originalFile-disagreement boundary detection.

use super::*;

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
fn buffer_disagrees_requires_partial_overlap_and_threshold() {
    // Known lines that fall OUTSIDE the original's length are skipped (the `*k <= len`
    // guard), and a single mismatch below the 25% threshold does NOT flag.
    let mut buf = SparseBuffer::default();
    // Known lines 1,2,3,4 - original is only 4 long; one of four mismatches = 25% → flags.
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
