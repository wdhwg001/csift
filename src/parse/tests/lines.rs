//! Line-level scanning: mmap, newline scan, syntax validation, malformed shapes.

use super::*;

#[test]
fn shape_malformed_needs_both_braces() {
    // Mutation pin: EACH missing brace alone marks the line malformed (crash-truncation
    // loses the trailing `}`; a torn head loses the leading `{`; free text has neither).
    assert!(line_shape_malformed(b"{\"a\":1")); // trailing brace lost
    assert!(line_shape_malformed(b"\"a\":1}")); // leading brace lost
    assert!(line_shape_malformed(b"free text garbage"));
    assert!(!line_shape_malformed(b"{\"a\":1}"));
    assert!(!line_shape_malformed(b"  ")); // blank is not malformed
    assert!(!line_shape_malformed(b"")); // empty is not malformed
}

#[test]
fn validate_line_syntax_counts_corruption_like_parse_line() {
    // Blank lines are fine for both (never counted).
    assert!(validate_line_syntax(b"").is_ok());
    assert!(validate_line_syntax(b"   \r").is_ok());
    // Valid JSON passes both.
    let ok = br#"{"type":"user","message":{"role":"user","content":"x"}}"#;
    assert!(validate_line_syntax(ok).is_ok());
    assert!(parse_line(ok).unwrap().is_some());
    // Real-world corruption (a torn tail write) fails BOTH the same way — the
    // parity `search`'s whole-file gate relies on for its malformed count.
    for torn in [
        br#"{"type":"user","message":{"role":"user","content":"tor"#.as_slice(),
        b"{ garbage not json".as_slice(),
        br#"{"role":"user""#.as_slice(),
    ] {
        assert!(validate_line_syntax(torn).is_err(), "{torn:?}");
        assert!(parse_line(torn).is_err(), "{torn:?}");
    }
}

#[test]
fn empty_file_is_safe() {
    let f = tempfile_path::TempJsonl::empty();
    let s = tail_records(f.path(), 0, |_| true).expect("tail empty");
    assert_eq!(s, 0);
    let (s, consumed) = head_records(f.path(), |_| true).expect("head empty");
    assert_eq!(s, 0);
    assert_eq!(consumed, 0);
}

// ── Branch-completeness ──

#[test]
fn mmap_open_error_surfaces_context() {
    // A path that does not exist → the `File::open` error context arm of mmap_file.
    let missing = std::env::temp_dir().join(format!(
        "csift-missing-{}-{}.jsonl",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let err = mmap_bytes(&missing).unwrap_err();
    assert!(
        err.to_string().contains("cannot open")
            || err.to_string().contains(&missing.display().to_string()),
        "expected open-context error, got: {err:#}"
    );
    // head/tail over a missing file surface the same open error.
    assert!(head_records(&missing, |_| true).is_err());
    assert!(tail_records(&missing, 0, |_| true).is_err());
}

#[test]
fn mmap_bytes_some_for_nonempty_none_for_empty() {
    let f = tmp_jsonl(&[r#"{"type":"user","message":{"role":"user","content":"x"}}"#]);
    assert!(mmap_bytes(f.path()).unwrap().is_some());
    let e = tempfile_path::TempJsonl::empty();
    assert!(mmap_bytes(e.path()).unwrap().is_none());
}

#[test]
fn scan_lines_bytes_visits_every_line_including_torn_tail() {
    // A trailing fragment with NO newline must still be visited (the
    // `start < bytes.len()` true arm).
    let mut seen: Vec<String> = Vec::new();
    scan_lines_bytes(b"aa\nbb\ncc", |line| {
        seen.push(String::from_utf8_lossy(line).into_owned());
    })
    .unwrap();
    assert_eq!(seen, vec!["aa", "bb", "cc"]);
    // A slice ending exactly on a newline does NOT emit a trailing empty line
    // (the `start < bytes.len()` false arm).
    let mut seen2: Vec<String> = Vec::new();
    scan_lines_bytes(b"aa\nbb\n", |line| {
        seen2.push(String::from_utf8_lossy(line).into_owned());
    })
    .unwrap();
    assert_eq!(seen2, vec!["aa", "bb"]);
}

#[test]
fn scan_lines_parallel_chunked_matches_serial_for_any_chunk_count() {
    // A mix of candidate lines, blank lines (counted in line numbering, never kept) and a
    // malformed candidate (parsed → skipped). The (kept line_no list, skip count) must be
    // IDENTICAL across every chunk split — that is the contract that lets recover/search/
    // files swap in the parallel scan with zero behaviour change.
    let mut raw = String::new();
    for i in 0..60 {
        if i % 7 == 0 {
            raw.push('\n'); // blank line: counts as a line, visitor ignores it
        }
        raw.push_str(&format!(
                r#"{{"type":"user","uuid":"u{i}","timestamp":"2026-06-07T05:00:0{}.000Z","message":{{"role":"user","content":"keep {i}"}}}}"#,
                i % 10
            ));
        raw.push('\n');
        if i % 11 == 5 {
            raw.push_str("{ broken json keep but unparseable\n"); // candidate → skip
        }
    }
    let bytes = raw.as_bytes();
    // Visitor keeps each parseable "keep" line's NUMBER; a malformed "keep" line → Skip.
    let visit = |line: &[u8], line_no: usize| -> LineVerdict<usize> {
        if !line.windows(4).any(|w| w == b"keep") {
            return LineVerdict::Ignore;
        }
        match parse_line(line) {
            Ok(Some(_)) => LineVerdict::Keep(line_no),
            Ok(None) => LineVerdict::Ignore,
            Err(_) => LineVerdict::Skip,
        }
    };

    let (serial, serial_skip) = scan_lines_parallel_chunked(bytes, &visit, 1);
    assert!(
        !serial.is_empty() && serial_skip > 0,
        "fixture exercises both arms"
    );
    // Line numbers strictly ascending (1:1 with the file, no duplicates/reordering).
    assert!(serial.windows(2).all(|w| w[0] < w[1]));

    for chunks in [2usize, 3, 5, 9, 17, 60, 500] {
        let (got, skip) = scan_lines_parallel_chunked(bytes, &visit, chunks);
        assert_eq!(got, serial, "line numbers diverge at chunks={chunks}");
        assert_eq!(skip, serial_skip, "skip count diverges at chunks={chunks}");
    }
}

#[test]
fn scan_lines_bytes_empty_slice_visits_nothing() {
    let mut n = 0;
    scan_lines_bytes(b"", |_| n += 1).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn role_marker_is_serialization_tolerant() {
    // R13: the compact wire form and every JSON-whitespace variant are the SAME
    // record — all must be candidates.
    for ok in [
        br#"{"message":{"role":"user","content":"x"}}"#.as_slice(),
        br#"{"message":{"role": "user","content":"x"}}"#.as_slice(),
        br#"{"message":{"role" : "user","content":"x"}}"#.as_slice(),
        b"{\"message\":{\"role\":\t\"assistant\",\"content\":[]}}".as_slice(),
        br#"{"message":{"role": "assistant"}}"#.as_slice(),
    ] {
        assert!(
            line_has_role_marker(ok),
            "{:?}",
            String::from_utf8_lossy(ok)
        );
    }
    // Non-markers: other role values, keyless mentions, and content-embedded
    // (escaped-quote) forms must NOT be admitted.
    for no in [
        br#"{"input":{"role":"admin"}}"#.as_slice(),
        br#"{"text":"the role of the user"}"#.as_slice(),
        br#"{"text":"quoted {\"role\": \"user\"} in prose"}"#.as_slice(),
        br#"{"role":}"#.as_slice(),
        br#"{"role"}"#.as_slice(),
    ] {
        assert!(
            !line_has_role_marker(no),
            "{:?}",
            String::from_utf8_lossy(no)
        );
    }
    // The user-only variant (files/recover) rejects assistant markers.
    assert!(line_has_user_role_marker(
        br#"{"message":{"role": "user"}}"#
    ));
    assert!(!line_has_user_role_marker(
        br#"{"message":{"role": "assistant"}}"#
    ));
    // A later valid marker after an earlier false hit is still found.
    assert!(line_has_role_marker(
        br#"{"a":{"role":"admin"},"message":{"role": "user"}}"#
    ));
}
