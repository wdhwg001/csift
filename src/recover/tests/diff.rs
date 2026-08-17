//! Unified-diff and LCS op-script generation.

use super::*;

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

#[test]
fn lcs_diff_emits_trailing_deletions_when_old_is_longer() {
    // old has lines new lacks at the END → the `while i < n` tail-delete loop runs (the
    // first pass only exercised balanced / leading change runs).
    let old = vec!["keep".to_string(), "drop1".to_string(), "drop2".to_string()];
    let new = vec!["keep".to_string()];
    let d = unified_diff(&old, &new, 3);
    assert!(
        d.contains("-drop1") && d.contains("-drop2"),
        "tail deletes: {d}"
    );
    // A pure deletion's new side is zero-length → the 0,0-style new header.
    assert!(
        d.contains("+0,0") || d.contains(" +1,1 "),
        "new header form: {d}"
    );
    let ops = lcs_diff(&old, &new);
    assert_eq!(
        ops.iter().filter(|(o, _, _)| *o == DiffOp::Delete).count(),
        2,
        "exactly the two trailing lines are deletes"
    );
}

#[test]
fn lcs_diff_emits_trailing_insertions_when_new_is_longer() {
    // new has lines old lacks at the END → the `while j < m` tail-insert loop runs.
    let old = vec!["keep".to_string()];
    let new = vec!["keep".to_string(), "add1".to_string(), "add2".to_string()];
    let d = unified_diff(&old, &new, 3);
    assert!(
        d.contains("+add1") && d.contains("+add2"),
        "tail inserts: {d}"
    );
    let ops = lcs_diff(&old, &new);
    assert_eq!(
        ops.iter().filter(|(o, _, _)| *o == DiffOp::Insert).count(),
        2,
        "exactly the two trailing lines are inserts"
    );
}

#[test]
fn unified_diff_pure_deletion_uses_zero_length_new_header() {
    // A diff that ONLY removes lines (no inserts, no surviving context) → `new_count == 0`
    // and `new_lo == usize::MAX`, driving the `if new_lo == usize::MAX` reset and the
    // `if new_count == 0` header form on the NEW side.
    let old = vec!["x".to_string(), "y".to_string()];
    let new: Vec<String> = vec![];
    let d = unified_diff(&old, &new, 3);
    assert!(d.contains("-x") && d.contains("-y"), "both removed: {d}");
    assert!(
        d.contains("@@ -1,2 +0,0 @@"),
        "pure-deletion header uses the 0,0 new form: {d}"
    );
}

#[test]
fn unified_diff_caps_leading_context_at_three_lines() {
    // A change far into the file (preceded by many identical lines) must show AT MOST 3
    // lines of leading context — driving the `ctx_back < CONTEXT` cap of the back-context
    // walk. Lines 1-6 are identical; line 7 changes. The hunk's first context line is line
    // 4 (7 minus 3), never line 1.
    let old: Vec<String> = (1..=8).map(|n| format!("line{n}")).collect();
    let mut new = old.clone();
    new[6] = "line7-CHANGED".to_string(); // change the 7th line (index 6)
    let d = unified_diff(&old, &new, 3);
    assert!(
        d.contains("-line7") && d.contains("+line7-CHANGED"),
        "the change: {d}"
    );
    // Exactly three leading context lines (line4, line5, line6); line3 and earlier excluded.
    assert!(
        d.contains(" line4\n line5\n line6\n"),
        "3 lines of leading context: {d}"
    );
    assert!(
        !d.contains(" line1\n") && !d.contains(" line3\n"),
        "context is capped at 3 lines (line1..line3 excluded): {d}"
    );
}

#[test]
fn unified_diff_full_context_shows_every_line() {
    // usize::MAX context (what --patches passes) reproduces the WHOLE file as context — a
    // far-away change still drags every read line into one spanning hunk. This is what makes
    // `--patches` of a fully-read, one-line-edited file contain all lines (CC's Read-before-Edit
    // guarantees those context lines were genuinely observed, so they are valid to include).
    let old: Vec<String> = (1..=8).map(|n| format!("line{n}")).collect();
    let mut new = old.clone();
    new[6] = "line7-CHANGED".to_string();
    let d = unified_diff(&old, &new, usize::MAX);
    // Every distant line appears as context — line1 and line3 are NOT excluded here.
    for n in [1, 2, 3, 4, 5, 6, 8] {
        assert!(
            d.contains(&format!(" line{n}\n")),
            "full context keeps line{n}: {d}"
        );
    }
    assert!(
        d.contains("-line7") && d.contains("+line7-CHANGED"),
        "the change is still marked: {d}"
    );
    // One spanning hunk over all 8 lines.
    assert_eq!(d.matches("@@ -").count(), 1, "single full-span hunk: {d}");
}
