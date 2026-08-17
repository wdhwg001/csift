//! Redirection parsing: fd-qualified forms, append/truncate, /dev/null, noclobber.

use super::*;

#[test]
fn redirect_combined_stream_amp_after_gt() {
    // `>&file` / `>& file` is the combined stdout+stderr file redirect (equivalent to
    // `&>file`) - a real write. Both spacings, asymmetric-no-longer with `&>`.
    assert_eq!(
        paths("make >& /real/a.log"),
        vec![("/real/a.log".to_string(), ">")]
    );
    assert_eq!(
        paths("make >&/real/b.log"),
        vec![("/real/b.log".to_string(), ">")]
    );
    // `>&N` (N a bare fd number) is an fd-dup, NOT a file → no row. Attached + spaced.
    assert!(paths("cmd >&1").is_empty());
    assert!(paths("cmd >&2").is_empty());
    assert!(paths("cmd >& 2").is_empty());
    // `>&-` closes an fd - also not a file.
    assert!(paths("cmd >&-").is_empty());
    assert!(paths("cmd >& -").is_empty());
    // A bare `>&` with NO following token emits nothing (degenerate, no panic).
    assert!(paths("cmd >&").is_empty());
    // `&>file` (the already-working sibling) is unchanged.
    assert_eq!(
        paths("make &> /real/c.log"),
        vec![("/real/c.log".to_string(), ">")]
    );
}

#[test]
fn is_fd_number_classifies_dup_vs_file() {
    // Direct helper coverage: digits / `-` are fd ops; a word is a file target.
    assert!(is_fd_number("1"));
    assert!(is_fd_number("22"));
    assert!(is_fd_number("-"));
    assert!(is_fd_number("'2'")); // quote-stripped digit
    assert!(!is_fd_number("build.log"));
    assert!(!is_fd_number("/tmp/x"));
    assert!(!is_fd_number(""));
}

#[test]
fn redirection_both_spacings() {
    // Spaced form.
    assert_eq!(
        paths("echo hi > out.txt"),
        vec![("out.txt".to_string(), ">")]
    );
    // Attached form.
    assert_eq!(
        paths("echo hi >out.txt"),
        vec![("out.txt".to_string(), ">")]
    );
    // Append, both spacings.
    assert_eq!(
        paths("echo hi >> log.txt"),
        vec![("log.txt".to_string(), ">>")]
    );
    assert_eq!(
        paths("echo hi >>log.txt"),
        vec![("log.txt".to_string(), ">>")]
    );
}

#[test]
fn redirection_operator_with_no_following_token() {
    // A trailing bare `>` with no filename after it → the `tokens.get(i+1)` None
    // arm (no path emitted), and the bare `>` is not itself a path.
    assert!(paths("echo hi >").is_empty());
    assert!(paths("echo hi >>").is_empty());
}

#[test]
fn input_redirect_file_is_not_a_target() {
    // `tee out.txt < in.txt`: out.txt is written (tee), in.txt is READ (`<`), so
    // only out.txt is reported.
    assert_eq!(
        paths("tee out.txt < in.txt"),
        vec![("out.txt".to_string(), "tee")]
    );
    // Attached form `<in.txt` is also dropped.
    assert_eq!(paths("cp src dst <in.txt"), vec![("dst".to_string(), "cp")]);
}

// ────────────────────────────────────────────────────────────────────────────
// Regression oracle: the synthetic IDIOM MATRIX from the files-attribution
// verdict (csift-files-attribution-verdict.md). Every idiom the verdict marked
// CAUGHT must stay caught; every idiom it marked MISSED (Fixes A–C) must now be
// caught; the precision cases (Fix D) must stay DROPPED.
// ────────────────────────────────────────────────────────────────────────────

// ── Fix A - fd-qualified redirects (the dominant previously-missed class) ──

#[test]
fn fd_stderr_redirect_attached_and_spaced() {
    // `2>/tmp/x.err` (attached) and `2> /tmp/x.err` (spaced) both caught, verb ">".
    assert_eq!(
        paths("pytest 2>/tmp/x.err"),
        vec![("/tmp/x.err".to_string(), ">")]
    );
    assert_eq!(
        paths("pytest 2> /tmp/x.err"),
        vec![("/tmp/x.err".to_string(), ">")]
    );
}

#[test]
fn fd_stdout_redirect_one_caught() {
    // `1>/tmp/x.log` - the stdout fd-redirect form.
    assert_eq!(
        paths("pytest 1>/tmp/x.log"),
        vec![("/tmp/x.log".to_string(), ">")]
    );
    assert_eq!(
        paths("pytest 1> /tmp/x.log"),
        vec![("/tmp/x.log".to_string(), ">")]
    );
}

#[test]
fn fd_ampersand_redirect_caught() {
    // `&>/tmp/x.log` - both-streams redirect (attached + spaced).
    assert_eq!(
        paths("make &>/tmp/x.log"),
        vec![("/tmp/x.log".to_string(), ">")]
    );
    assert_eq!(
        paths("make &> /tmp/x.log"),
        vec![("/tmp/x.log".to_string(), ">")]
    );
}

#[test]
fn fd_both_streams_to_two_paths() {
    // `x 1>/tmp/o.log 2>/tmp/e.err` → BOTH paths caught.
    let got = just_paths("x 1>/tmp/o.log 2>/tmp/e.err");
    assert!(got.contains(&"/tmp/o.log".to_string()), "got: {got:?}");
    assert!(got.contains(&"/tmp/e.err".to_string()), "got: {got:?}");
    assert_eq!(got.len(), 2, "exactly the two redirect targets: {got:?}");
}

#[test]
fn fd_append_redirects_caught() {
    // `2>>/tmp/e.err` and `&>>/tmp/x.log` - the fd-qualified APPEND forms, verb ">>".
    assert_eq!(
        paths("svc 2>>/tmp/e.err"),
        vec![("/tmp/e.err".to_string(), ">>")]
    );
    assert_eq!(
        paths("svc 1>> /tmp/o.log"),
        vec![("/tmp/o.log".to_string(), ">>")]
    );
}

#[test]
fn fd_dup_2_to_1_emits_nothing() {
    // `cmd 2>&1` is a fd-DUP (stderr→stdout), NOT a file write → nothing.
    assert!(paths("pytest 2>&1").is_empty());
    // And combined with a real redirect, only the real path surfaces.
    assert_eq!(
        just_paths("pytest >/tmp/out.log 2>&1"),
        vec!["/tmp/out.log".to_string()]
    );
}

#[test]
fn redirect_to_dev_null_class_is_dropped() {
    // `/dev/null`, `/dev/stderr`, `/dev/stdout` redirect sinks are not real files.
    assert!(paths("noisy 2>/dev/null").is_empty());
    assert!(paths("noisy >/dev/null").is_empty());
    assert!(paths("noisy 1>/dev/stdout").is_empty());
    assert!(paths("noisy 2> /dev/stderr").is_empty());
}

#[test]
fn plain_redirect_still_caught_after_fd_generalization() {
    // The original plain `>`/`>>` paths must NOT regress.
    assert_eq!(paths("echo hi > /tmp/x"), vec![("/tmp/x".to_string(), ">")]);
    assert_eq!(
        paths("echo hi >>/tmp/x"),
        vec![("/tmp/x".to_string(), ">>")]
    );
}

// ── Fix D - PRECISION: noisy pseudo-paths are DROPPED, never fabricated ──

#[test]
fn unresolved_var_redirect_is_dropped() {
    // `> $OUT` / `>${DIR}/x` - an unexpandable variable pseudo-path is dropped.
    assert!(paths("echo hi > $OUT").is_empty());
    assert!(paths("echo hi >${DIR}/x.log").is_empty());
    assert!(paths("svc 2>/tmp/$run.err").is_empty());
}

// ── R1: the DOMINANT garbage class - fd-redirect close-paren / process-sub leaks ──

#[test]
fn devnull_with_glued_close_paren_is_dropped() {
    // `$(… 2>/dev/null)` glues a `)` onto the sink → `/dev/null)`. The single most
    // common idiom in real sessions; it must NOT fabricate a `/dev/null)` row.
    assert!(paths(r#"RESOLVED="$(readlink -f x 2>/dev/null || true)""#).is_empty());
    assert!(paths("x=$(cmd 2>/dev/null)").is_empty());
    assert!(paths("diff <(a) <(b 2>/dev/null)").is_empty());
    // Doubled close parens (nested substitution) also drop.
    assert!(paths("y=$(f $(g 2>/dev/null))").is_empty());
}

#[test]
fn real_redirect_path_with_trailing_struct_punct_kept_clean() {
    // A genuine redirect path with a glued statement terminator keeps the CLEAN path.
    assert_eq!(
        paths("echo x > /tmp/real.log;"),
        vec![("/tmp/real.log".to_string(), ">")]
    );
    assert_eq!(
        paths("(echo x > /tmp/sub.log)"),
        vec![("/tmp/sub.log".to_string(), ">")]
    );
}

#[test]
fn redirect_metachar_and_fd_dup_tokens_are_not_paths() {
    // `2>&1` and a bare `>` never become file rows even when they reach path_operand.
    assert!(paths("pytest 2>&1").is_empty());
    assert_eq!(path_operand("2>&1"), None);
    assert_eq!(path_operand("/tmp/a>b"), None); // embedded redirect → noise.
}

#[test]
fn noclobber_override_redirect_caught() {
    // `>|` is a force-truncate redirect; the `|` must not split off the path.
    assert_eq!(
        paths("echo x >| /tmp/forced.txt"),
        vec![("/tmp/forced.txt".to_string(), ">")]
    );
    // Attached form.
    assert_eq!(
        paths("echo x >|/tmp/forced2.txt"),
        vec![("/tmp/forced2.txt".to_string(), ">")]
    );
    // fd-qualified `2>|`.
    assert_eq!(
        paths("svc 2>| /tmp/e.log"),
        vec![("/tmp/e.log".to_string(), ">")]
    );
}
