use super::*;

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
    // counting nothing below the floor — anchor semantics are byte-identical to the
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
