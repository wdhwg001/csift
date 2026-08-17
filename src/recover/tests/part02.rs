use super::*;

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
