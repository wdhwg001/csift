//! Head/tail record readers and the reverse-line iterator: floors, disjoint windows.

use super::*;

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

#[test]
fn head_tail_disjoint_windows_never_double_count() {
    // R12: a head scan + tail scan over the SAME file used to double-book every
    // malformed line both passes walked (an all-garbage file reported exactly 2×).
    // The head returns its consumed-end offset; passing it as the tail floor keeps
    // the windows disjoint, so each line is booked exactly once.
    let f = tmp_jsonl(&["{ garbage one", "{ garbage two", "{ garbage three"]);
    let (head_skipped, consumed) = head_records(f.path(), |_| true).expect("head");
    assert_eq!(head_skipped, 3);
    let tail_skipped = tail_records(f.path(), consumed, |_| true).expect("tail");
    assert_eq!(
        tail_skipped, 0,
        "the tail must not re-book head-counted lines"
    );
}

#[test]
fn tail_floor_still_walks_below_for_anchors_without_counting() {
    // The ONLY genuine user sits inside the head window. The tail scan (floor past
    // it) must still find it as an anchor (phase 2 of the backward walk) while
    // counting nothing below the floor - anchor semantics are byte-identical to the
    // old full-file walk; only the double-booked count changed.
    let f = tmp_jsonl(&[
        r#"{ broken head line"#,
        r#"{"type":"user","message":{"role":"user","content":"only q"}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"a"}]}}"#,
    ]);
    let mut first: Option<String> = None;
    let (head_skipped, consumed) = head_records(f.path(), |rec| {
        if let Some(t) = rec.genuine_user_text() {
            first = Some(t);
            return false;
        }
        true
    })
    .expect("head");
    assert_eq!(head_skipped, 1);
    assert_eq!(first.as_deref(), Some("only q"));
    let mut last_user: Option<String> = None;
    let mut last_agent: Option<String> = None;
    let tail_skipped = tail_records(f.path(), consumed, |rec| {
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
        last_user.is_none() || last_agent.is_none()
    })
    .expect("tail");
    assert_eq!(last_agent.as_deref(), Some("a"));
    assert_eq!(
        last_user.as_deref(),
        Some("only q"),
        "anchor below the floor is still found"
    );
    assert_eq!(
        tail_skipped, 0,
        "the broken head line is booked by the head scan only"
    );
}

#[test]
fn revlines_all_blank_slice() {
    // A slice of only newlines → every line is blank, none survive the filter.
    assert!(rev_nonblank(b"\n\n\n", 2).is_empty());
}

#[test]
fn revlines_next_after_exhaustion_returns_none() {
    // Calling `next()` again after the iterator is exhausted hits the `if
    // self.done { return None }` true arm (a normal `for` loop never re-polls).
    // Content has no trailing newline → exactly two raw lines yielded.
    let mut it = RevLines::with_chunk(b"a\nb", 64);
    let mut all = Vec::new();
    for l in it.by_ref() {
        all.push(l);
    }
    assert_eq!(all.len(), 2, "two lines, newest-first: {all:?}");
    assert_eq!(all[0], b"b");
    assert_eq!(all[1], b"a");
    assert!(
        it.next().is_none(),
        "post-exhaustion next is None via done flag"
    );
    assert!(it.next().is_none(), "still None");
}

#[test]
fn revlines_empty_slice_is_immediately_none() {
    // An empty slice: hi==0 at construction, carry empty → fill returns false on
    // the first poll (the `self.carry.is_empty()` TRUE arm at hi==0).
    let mut it = RevLines::with_chunk(b"", 4);
    assert!(it.next().is_none());
}

#[test]
fn revlines_trailing_newline_only_carry_empty_at_bof() {
    // Content that is a single newline: the high tail after the newline is empty,
    // seg0 (low edge) is also empty at BOF → buf-empty / empty-carry arms. No
    // non-blank lines survive.
    assert!(rev_nonblank(b"\n", 1).is_empty());
    assert!(rev_nonblank(b"\n", 64).is_empty());
}

#[test]
fn revlines_carry_nonempty_flush_at_bof() {
    // A leading partial line with NO newline before it, reached only after the
    // carry has accumulated across chunks → the `self.carry.is_empty()` FALSE arm
    // at hi==0 (flush the carry as the first line). chunk=2 forces accumulation.
    let data = b"abcdefghij\nz\n";
    assert_eq!(rev_nonblank(data, 2), vec!["z", "abcdefghij"]);
}
