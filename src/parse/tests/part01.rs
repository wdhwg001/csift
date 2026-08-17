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
fn revlines_simple_three_lines() {
    let data = b"alpha\nbeta\ngamma\n";
    // Newest-first.
    assert_eq!(
        rev_nonblank(data, 64 * 1024),
        vec!["gamma", "beta", "alpha"]
    );
}

#[test]
fn revlines_no_trailing_newline() {
    let data = b"alpha\nbeta\ngamma"; // last line has no \n
    assert_eq!(
        rev_nonblank(data, 64 * 1024),
        vec!["gamma", "beta", "alpha"]
    );
}

#[test]
fn revlines_carry_across_tiny_chunks() {
    // Chunk size 3 forces nearly every line to straddle a chunk boundary, so
    // the carry logic is exercised hard. Must STILL yield newest-first, intact.
    let data = b"one\ntwotwo\nthreethreethree\nfour\n";
    assert_eq!(
        rev_nonblank(data, 3),
        vec!["four", "threethreethree", "twotwo", "one"]
    );
}

#[test]
fn revlines_chunk_size_one() {
    // Pathological chunk=1: every byte is its own read; carry must reassemble.
    let data = b"ab\ncd\nef\n";
    assert_eq!(rev_nonblank(data, 1), vec!["ef", "cd", "ab"]);
}

#[test]
fn revlines_single_line_no_newline() {
    let data = b"only-one-line";
    assert_eq!(rev_nonblank(data, 4), vec!["only-one-line"]);
}

#[test]
fn revlines_blank_lines_dropped_order_kept() {
    let data = b"first\n\n\nlast\n";
    assert_eq!(rev_nonblank(data, 2), vec!["last", "first"]);
}

#[test]
fn revlines_chunk_boundary_at_newline() {
    // Chunk boundary lands exactly on the newline positions.
    let data = b"aaa\nbbb\nccc\n"; // each line+nl is 4 bytes
    assert_eq!(rev_nonblank(data, 4), vec!["ccc", "bbb", "aaa"]);
}

#[test]
fn revlines_matches_forward_split_reversed_for_random_chunks() {
    // Property: for ANY chunk size, backward non-blank == forward non-blank
    // reversed. Build content with varied line lengths incl. a long line.
    let content = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        "a",
        "bb",
        "c".repeat(200), // long line spanning multiple small chunks
        "",              // a blank
        "dddd",
        "eeeee" // no trailing newline
    );
    let bytes = content.as_bytes();
    let forward: Vec<String> = content
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect();
    let mut expected = forward.clone();
    expected.reverse();
    for chunk in [1usize, 2, 3, 7, 16, 64, 1000, 1 << 16] {
        assert_eq!(rev_nonblank(bytes, chunk), expected, "chunk={chunk}");
    }
}

#[test]
fn tail_records_finds_last_user_and_agent() {
    let f = tmp_jsonl(&[
        r#"{"type":"user","message":{"role":"user","content":"first q"}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"first a"}]}}"#,
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"r"}]}}"#,
        r#"{"type":"user","message":{"role":"user","content":"last q"}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"last a"}]}}"#,
    ]);
    let mut last_user: Option<String> = None;
    let mut last_agent: Option<String> = None;
    // Use a tiny chunk to force the carry path even on this small file.
    let skipped = tail_records_chunked(f.path(), 8, 0, |rec| {
        if last_agent.is_none() {
            if let Some(t) = rec.agent_text() {
                last_agent = Some(t);
            }
        }
        if last_user.is_none() {
            if let Some(t) = rec.genuine_user_text() {
                last_user = Some(t);
            }
        }
        // Keep going until BOTH are filled.
        last_user.is_none() || last_agent.is_none()
    })
    .expect("tail read");
    assert_eq!(skipped, 0);
    assert_eq!(last_user.as_deref(), Some("last q"));
    assert_eq!(last_agent.as_deref(), Some("last a"));
}

#[test]
fn tail_records_skips_malformed_lines_and_counts() {
    let f = tmp_jsonl(&[
        r#"{"type":"user","message":{"role":"user","content":"ok"}}"#,
        r#"{ this is not valid json"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"a"}]}}"#,
    ]);
    let mut agent: Option<String> = None;
    let skipped = tail_records_chunked(f.path(), 4, 0, |rec| {
        if let Some(t) = rec.agent_text() {
            agent = Some(t);
        }
        agent.is_none()
    })
    .expect("tail read");
    // The malformed middle line is between agent (end) and the first record,
    // so whether it is counted depends on how far we scan; here we stop at the
    // agent line (newest), so the malformed line is not reached → skipped==0.
    assert_eq!(agent.as_deref(), Some("a"));
    assert_eq!(skipped, 0);
}

#[test]
fn tail_records_malformed_at_end_is_counted() {
    let f = tmp_jsonl(&[
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"a"}]}}"#,
        r#"{ broken tail line"#,
    ]);
    let mut seen = 0usize;
    let skipped = tail_records_chunked(f.path(), 4, 0, |_rec| {
        seen += 1;
        true // scan everything
    })
    .expect("tail read");
    assert_eq!(skipped, 1, "the broken newest line must be skipped+counted");
    assert_eq!(seen, 1, "one valid record");
}

#[test]
fn head_records_stops_at_first_genuine_user() {
    let f = tmp_jsonl(&[
        r#"{"type":"last-prompt","leafUuid":"x"}"#,
        r#"{"type":"attachment","timestamp":"2026-06-07T00:00:00.000Z"}"#,
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off."}}"#,
        r#"{"type":"user","message":{"role":"user","content":"the real first question"}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"never reached"}]}}"#,
    ]);
    let mut first: Option<String> = None;
    let mut count = 0usize;
    head_records(f.path(), |rec| {
        count += 1;
        if let Some(t) = rec.genuine_user_text() {
            first = Some(t);
            return false; // stop
        }
        true
    })
    .expect("head read");
    assert_eq!(first.as_deref(), Some("the real first question"));
    // Must have stopped at the genuine user (line 4 of valid records: the
    // metadata + attachment + isMeta were visited but not matched).
    assert_eq!(count, 4, "stopped at the first genuine user, not later");
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
fn revlines_carry_flushed_at_bof_with_leading_partial() {
    // A file whose FIRST line straddles below the lowest chunk read so it lands in
    // the carry, then is flushed when `hi` reaches 0 (the `fill` hi==0 +
    // non-empty-carry flush arm). Force it with chunk=1 over content whose first
    // line is long and has no newline until late.
    let data = b"leadingline\nx\n";
    assert_eq!(rev_nonblank(data, 1), vec!["x", "leadingline"]);
}

#[test]
fn head_records_skips_blank_and_malformed_then_continues() {
    // A blank line (Ok(None) arm), a malformed line (Err skip+count arm), then a
    // valid record reached at the torn-tail position (no trailing newline).
    let f = tmp_jsonl(&[
        "",                 // blank → Ok(None)
        r#"{ broken json"#, // malformed → counted
        r#"{"type":"user","message":{"role":"user","content":"real"}}"#,
    ]);
    let mut first: Option<String> = None;
    let (skipped, _) = head_records(f.path(), |rec| {
        if let Some(t) = rec.genuine_user_text() {
            first = Some(t);
            return false;
        }
        true
    })
    .expect("head read");
    assert_eq!(first.as_deref(), Some("real"));
    assert_eq!(skipped, 1, "the one malformed line is counted");
}

#[test]
fn head_records_visits_torn_tail_fragment() {
    // The final record has NO trailing newline (the `start < bytes.len()` arm of
    // head_records). Scan everything (never early-stop) so the tail is reached.
    let mut content = String::new();
    content.push_str(r#"{"type":"user","message":{"role":"user","content":"a"}}"#);
    content.push('\n');
    content.push_str(r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"tail-no-newline"}]}}"#);
    let p = std::env::temp_dir().join(format!(
        "csift-torn-{}-{}.jsonl",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&p, content.as_bytes()).unwrap();
    let mut seen = 0usize;
    let mut last_agent: Option<String> = None;
    head_records(&p, |rec| {
        seen += 1;
        if let Some(t) = rec.agent_text() {
            last_agent = Some(t);
        }
        true // scan all → reach the torn tail
    })
    .expect("head read");
    std::fs::remove_file(&p).ok();
    assert_eq!(seen, 2);
    assert_eq!(last_agent.as_deref(), Some("tail-no-newline"));
}

#[test]
fn head_records_early_stop_before_tail_fragment() {
    // Early-stop on the FIRST record (return false) so the `if stop { return }`
    // arm fires mid-loop and the torn-tail branch is NOT reached.
    let f = tmp_jsonl(&[
        r#"{"type":"user","message":{"role":"user","content":"first"}}"#,
        r#"{"type":"user","message":{"role":"user","content":"second"}}"#,
    ]);
    let mut count = 0;
    head_records(f.path(), |_rec| {
        count += 1;
        false // stop immediately
    })
    .expect("head read");
    assert_eq!(count, 1, "stopped after the first record");
}
