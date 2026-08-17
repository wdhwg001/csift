//! Cutoff resolution, line/turn windows, covered spans and gap ranges.

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
fn gap_ranges_whole_file_unknown_when_no_known_lines() {
    // Empty known + total>0 → the `else if known.is_empty() && total > 0` arm yields one
    // whole-file gap. (The interior/trailing arms were already covered.)
    assert_eq!(gap_ranges(&[], 4), vec![(1, 4)]);
    // Empty known + total == 0 → no gaps at all (nothing seen, nothing to mark).
    assert_eq!(gap_ranges(&[], 0), Vec::<(usize, usize)>::new());
}

#[test]
fn basename_of_splits_both_separators_and_raw_safety_gates() {
    assert_eq!(basename_of("/a/b/x.md"), "x.md");
    assert_eq!(basename_of(r"C:\a\b\x.md"), "x.md");
    assert_eq!(basename_of("plain.md"), "plain.md");
    assert!(raw_needle_safe("x.md"));
    assert!(!raw_needle_safe(""));
    assert!(!raw_needle_safe("we\"ird.md"));
}
