use super::*;

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
    assert_eq!(
        parse_line_range("100..200").unwrap().resolve(1000, true),
        (100, 200)
    );
    assert!(
        parse_line_range("0..5").is_err(),
        "0 start rejected (1-based)"
    );
    assert!(parse_line_range("5..3").is_err(), "end before start");
    assert!(parse_line_range("notarange").is_err());
}

#[test]
fn parse_turn_range_matches_files_contract() {
    assert_eq!(
        parse_turn_range("0..1").unwrap().resolve(100, false),
        (0, 1)
    );
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
