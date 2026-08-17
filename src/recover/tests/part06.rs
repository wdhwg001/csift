use super::*;

#[test]
fn strip_gutter_skips_digits_without_a_separator_and_unparseable_widths() {
    // A line with leading digits but NEITHER a tab NOR an arrow separator → the final else
    // `continue` (no gutter recognized). And a digit run too large for usize → the
    // `digits.parse::<usize>()` Err side: skipped, never fabricated.
    let huge = "9".repeat(40); // overflows usize
    let snippet = format!("12 no-separator-here\n{huge}\tlost\n7\tkept");
    let got = strip_gutter(&snippet);
    assert_eq!(
        got,
        vec![(7, "kept".to_string())],
        "only the well-formed tab-guttered line survives: {got:?}"
    );
}

#[test]
fn path_matches_deep_suffix_after_slash_boundary() {
    // Directly drives the `prefix.ends_with('/')` operand: a target that is a trailing
    // multi-segment slice whose stripped prefix is non-empty and slash-terminated.
    assert!(
        path_matches(Some("src/relay/engine.py"), "/root/app/src/relay/engine.py"),
        "deep suffix accepted (prefix '/root/app/' is non-empty and ends with '/')"
    );
    assert!(
        path_matches(Some("b/c.rs"), "/a/b/c.rs"),
        "two-segment suffix at a '/' boundary"
    );
}

#[test]
fn buffer_disagrees_requires_partial_overlap_and_threshold() {
    // Known lines that fall OUTSIDE the original's length are skipped (the `*k <= len`
    // guard), and a single mismatch below the 25% threshold does NOT flag.
    let mut buf = SparseBuffer::default();
    // Known lines 1,2,3,4 — original is only 4 long; one of four mismatches = 25% → flags.
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
