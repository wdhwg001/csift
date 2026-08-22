//! Heredoc delimiter forms and body masking until the exact closer.

use super::*;

// ── Mutation-kill pins (cargo-mutants survivors): heredoc scanning boundaries the
//    ordinary fixtures never reached - wrong consumed-offset arithmetic corrupts the
//    SECOND delimiter on a line, and a broken closer comparison leaks body lines. ──

#[test]
fn heredoc_delims_forms_and_offsets() {
    assert_eq!(heredoc_delims("cat <<EOF"), ["EOF"]);
    assert_eq!(heredoc_delims("cat <<-END x"), ["END"]);
    assert_eq!(heredoc_delims("cat <<'A B'"), ["A B"]);
    assert_eq!(heredoc_delims(r#"cat <<"QD" y"#), ["QD"]);
    // Two on one line, in order - the consumed-offset arithmetic must be exact.
    assert_eq!(heredoc_delims("cat <<ONE <<'TWO'"), ["ONE", "TWO"]);
    // A here-STRING is not a heredoc (no body line follows).
    assert!(heredoc_delims("cat <<<word").is_empty());
    // A bare delimiter stops at a shell metacharacter.
    assert_eq!(heredoc_delims("cat <<EOF;echo x"), ["EOF"]);
}

#[test]
fn heredoc_bodies_dropped_until_exact_closer() {
    let cmd = "cat <<EOF > $OUT\nline one\nline two\nEOF\necho after";
    let (stripped, bodies) = strip_heredoc_bodies_keeping(cmd);
    assert_eq!(
        bodies,
        ["line one\nline two"],
        "the body text is kept aside"
    );
    assert!(
        !stripped.contains("line one") && !stripped.contains("line two"),
        "body lines must drop: {stripped}"
    );
    assert!(
        stripped.contains("echo after"),
        "post-closer line kept: {stripped}"
    );
    // A body line that merely CONTAINS the delimiter is not the closer.
    let (s2, _) = strip_heredoc_bodies_keeping("cat <<EOF\nnot EOF here\nEOF\necho tail");
    assert!(
        !s2.contains("not EOF here") && s2.contains("echo tail"),
        "closer is exact-trimmed-match only: {s2}"
    );
}

#[test]
fn inline_python_literal_write_is_extracted() {
    // An inline `python3 -c "open('/tmp/x','w')…"` script with a LITERAL first
    // argument and a write mode is a provable write: the target is a real row now
    // (the former out-of-scope contract moved to the interp analyzer, which still
    // never fabricates - see the interp tests for the opaque cases).
    assert_eq!(
        paths(r#"python3 -c "open('/tmp/out.json','w').write('x')""#),
        [("/tmp/out.json".to_string(), "interp-write")]
    );
}

// ── R1: heredoc body is SKIPPED, not mis-reported (the never-mis-reported contract) ──

#[test]
fn heredoc_body_redirect_char_does_not_fabricate_a_row() {
    // A `>` inside a heredoc body must NOT become a redirect row, and the opener's own
    // real trailing redirect (if any) IS still caught. The body is not a SHELL stream:
    // its only legitimate surface is the interpreter write-idiom analyzer, whose
    // literal extraction here names exactly the one real write.
    let body_only = "python3 - <<'PY'\nprint('a > b')\nopen('/tmp/real.json','w')\nPY";
    let got = just_paths(body_only);
    assert_eq!(
        got,
        vec!["/tmp/real.json".to_string()],
        "the body's shell-looking bytes must not fabricate rows; the interpreter \
         write is the only row"
    );
    assert_eq!(paths(body_only)[0].1, "interp-write");
    // Opener-line redirect survives heredoc-body stripping.
    let with_redirect = "cat <<EOF > /tmp/out.txt\nbody > not a redirect\nEOF";
    assert_eq!(
        just_paths(with_redirect),
        vec!["/tmp/out.txt".to_string()],
        "opener-line redirect must be caught; body `>` must not"
    );
}

#[test]
fn heredoc_quoted_and_dash_delim_forms() {
    // Quoted delimiter `<<'EOF'` and tab-stripping `<<-EOF` both close correctly.
    let q = "cat <<'EOF'\n> garbage > here\nEOF\ntouch /tmp/after.txt";
    let got = just_paths(q);
    assert!(
        got.contains(&"/tmp/after.txt".to_string()),
        "post-heredoc cmd lost: {got:?}"
    );
    assert!(
        !got.iter().any(|p| p.contains("garbage")),
        "body leaked: {got:?}"
    );
    // here-string `<<<` is NOT a heredoc (no body) - the command still parses normally.
    assert_eq!(
        just_paths("grep x <<< 'data' > /tmp/hs.txt"),
        vec!["/tmp/hs.txt".to_string()]
    );
}

#[test]
fn heredoc_multiple_delims_and_unclosed_quote() {
    // Two heredocs opened on one line: both bodies dropped, in order.
    let multi = "cat <<A <<B\nbodyA\nA\nbodyB\nB\ntouch /tmp/end.txt";
    let got = just_paths(multi);
    assert!(
        got.contains(&"/tmp/end.txt".to_string()),
        "trailing cmd lost: {got:?}"
    );
    assert!(
        !got.iter().any(|p| p.contains("body")),
        "body leaked: {got:?}"
    );
    // A simple heredoc whose body carries a stray `>` → no real path, no fabrication.
    let unterminated = "cat <<EOF\nx > y\nEOF";
    assert!(just_paths(unterminated).is_empty(), "no real path here");
}

#[test]
fn heredoc_body_then_more_commands_after_closer() {
    // A heredoc whose closer is followed by MORE commands exercises the active→None
    // transition inside strip_heredoc_bodies (pop the body, resume scanning).
    let cmd = "cat <<EOF\nline one > fake\nline two\nEOF\nmkdir -p /tmp/after && touch /tmp/also";
    let got = just_paths(cmd);
    assert!(
        got.contains(&"/tmp/after".to_string()),
        "post-closer cmd lost: {got:?}"
    );
    assert!(
        got.contains(&"/tmp/also".to_string()),
        "post-closer cmd lost: {got:?}"
    );
    assert!(
        !got.iter().any(|p| p.contains("fake")),
        "body leaked: {got:?}"
    );
}

#[test]
fn heredoc_here_string_is_not_a_heredoc() {
    // `<<<` is a here-STRING (no body line) - heredoc_delims must skip it, so a
    // following command on the next line is NOT swallowed as a body.
    let hs = "grep x <<< 'data'\ntouch /tmp/post.txt";
    assert!(just_paths(hs).contains(&"/tmp/post.txt".to_string()));
}

#[test]
fn heredoc_delims_direct_coverage() {
    // Direct coverage of the delimiter scanner's shapes.
    assert_eq!(heredoc_delims("cat <<EOF"), vec!["EOF".to_string()]);
    assert_eq!(heredoc_delims("cat <<-EOF"), vec!["EOF".to_string()]);
    assert_eq!(heredoc_delims("cat <<'Q'"), vec!["Q".to_string()]);
    assert_eq!(heredoc_delims("cat <<\"D\""), vec!["D".to_string()]);
    assert!(heredoc_delims("cat <<< here-string").is_empty());
    assert!(heredoc_delims("no heredoc here").is_empty());
    // `read_heredoc_word` bare-word stop at a metachar.
    assert_eq!(read_heredoc_word("EOF;rest").0, "EOF");
    // An unterminated quote has no closing `'`, so it falls through to the bare-word
    // scan, which keeps the leading `'` and runs to the end (no metachar).
    assert_eq!(read_heredoc_word("'unterminated").0, "'unterminated");
    // A command with NO `<<` at all → the strip fast path (no change, no bodies).
    let (unchanged, none) = strip_heredoc_bodies_keeping("echo hi > /tmp/x");
    assert_eq!(unchanged, "echo hi > /tmp/x");
    assert!(none.is_empty());
}
