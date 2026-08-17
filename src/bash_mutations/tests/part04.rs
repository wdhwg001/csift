use super::*;

#[test]
fn residual_noise_classes_rejected_real_relpaths_kept() {
    // `=`-led value fragments, pure numbers, and code shards (comma / trailing colon
    // with no `/`) are rejected — the residual `by-file` garbage classes.
    assert!(has_syntax_noise("=1.94"));
    assert!(has_syntax_noise("=2980"));
    assert!(has_syntax_noise("="));
    assert!(has_syntax_noise("7000"));
    assert!(has_syntax_noise("0"));
    assert!(has_syntax_noise("o.get('x',0):")); // comma + trailing colon, no slash → code shard
    assert!(has_syntax_noise("turn,t.get")); // comma, no slash → code shard
    assert!(has_syntax_noise("label:")); // trailing colon, no slash
                                         // Genuine RELATIVE paths must survive (no `/`-free code-shard false positive).
    assert!(!has_syntax_noise("src/main.rs"));
    assert!(!has_syntax_noise("Cargo.toml"));
    assert!(!has_syntax_noise("paper.pdf"));
    assert!(!has_syntax_noise("err.log"));
    assert!(!has_syntax_noise("build_turns_fixture.py"));
    // A path containing a colon/comma WITH a slash is left alone (rare but legal).
    assert!(!has_syntax_noise("/tmp/a:b"));
}

#[test]
fn version_compare_and_numeric_args_do_not_become_files() {
    // End-to-end: a `=`-led or numeric token never surfaces as a redirect/operand row.
    assert!(just_paths("python -c 'x' > =1.94").is_empty());
    // A real path on the same redirect still surfaces.
    assert_eq!(
        just_paths("echo x > /tmp/ok.txt"),
        vec!["/tmp/ok.txt".to_string()]
    );
}

#[test]
fn path_operand_post_trim_rejections() {
    // After the trailing-tail trim, the re-checked sink + fd-dup-remnant + syntax-noise
    // rejections each fire (the `/dev/null)` family, a `&1` remnant, a noisy token).
    assert_eq!(path_operand("2>/dev/null)"), None); // trims `)` → sink → None
    assert_eq!(path_operand("/dev/stderr)"), None);
    assert_eq!(path_operand("&1"), None); // fd-dup remnant
    assert_eq!(path_operand("'/tmp/x"), None); // unbalanced quote noise
                                               // A clean path passes (with a trailing `;` peeled).
    assert_eq!(path_operand("/tmp/clean;").as_deref(), Some("/tmp/clean"));
    // A bare `-` (stdin/stdout) and an empty token are not paths.
    assert_eq!(path_operand("-"), None);
    assert_eq!(path_operand(""), None);
}

#[test]
fn trim_structural_tail_brace_and_balanced_cases() {
    // The `}` unbalanced-brace arm (a `${VAR}`-substitution close glued on).
    assert_eq!(trim_structural_tail("/tmp/x}"), "/tmp/x");
    // A balanced `{…}` is left intact (no over-trim).
    assert_eq!(trim_structural_tail("/tmp/{a}"), "/tmp/{a}");
    // Mixed trailing punctuation peeled in sequence.
    assert_eq!(trim_structural_tail("/tmp/x;}"), "/tmp/x");
    // A clean path with no trailing structure is returned unchanged.
    assert_eq!(trim_structural_tail("/tmp/clean"), "/tmp/clean");
}

#[test]
fn target_directory_value_all_forms() {
    // `-t DIR`, `--target-directory DIR`, `--target-directory=DIR`, and absent.
    assert_eq!(target_directory_value(&["-t", "/d", "a"]), Some("/d"));
    assert_eq!(
        target_directory_value(&["--target-directory", "/d", "a"]),
        Some("/d")
    );
    assert_eq!(
        target_directory_value(&["--target-directory=/d", "a"]),
        Some("/d")
    );
    assert_eq!(target_directory_value(&["a", "b"]), None);
    // `-t` at the very end with no value → None (the `get(i+1)` None arm).
    assert_eq!(target_directory_value(&["a", "-t"]), None);
}

#[test]
fn cp_install_without_t_flag_uses_last_operand() {
    // The non-`-t` path of emit_copy_like (last positional is the dest).
    assert_eq!(paths("cp a b /dest"), vec![("/dest".to_string(), "cp")]);
    assert_eq!(
        paths("install -m644 src /etc/conf"),
        vec![("/etc/conf".to_string(), "install")]
    );
    // `-T`/`--no-target-directory` (forces 2-operand) → default last-operand path.
    assert_eq!(
        paths("cp -T src /dest/file"),
        vec![("/dest/file".to_string(), "cp")]
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
fn mv_target_dir_when_dir_token_repeats_in_sources() {
    // The `src == dir` skip arm of emit_mv's `-t` path: the `-t` value is excluded
    // from the mv-from sources even though it is also a non-flag positional.
    let got = paths("mv -t /tmp/d /tmp/a /tmp/b");
    // /tmp/d is the destination (mv); a + b are sources; /tmp/d is NOT a source.
    assert!(got.contains(&("/tmp/d".to_string(), "mv")));
    assert!(got.contains(&("/tmp/a".to_string(), "mv-from")));
    assert!(got.contains(&("/tmp/b".to_string(), "mv-from")));
    assert_eq!(
        got.iter().filter(|(p, _)| p == "/tmp/d").count(),
        1,
        "the -t DIR appears exactly once (as mv), never as a source: {got:?}"
    );
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

// ── R2: quote-aware redirect detection (the dominant remaining fabrication class) ──

#[test]
fn quoted_inline_redirect_does_not_fabricate_a_file() {
    // The MUST-FIX regression: a `>` inside a quoted echo/printf prose or a quoted regex
    // is NOT a real redirect, and the next word inside the quote is NOT a file. The
    // exact oracle commands that fabricated `*dt` / `8min` / `cover` / `base`.
    // A real redirect AFTER the quoted arg is still caught.
    assert_eq!(
        paths(r#"echo "  wf transcript idle >8min OR gone" > /tmp/realfile.txt"#),
        vec![("/tmp/realfile.txt".to_string(), ">")],
        "in-quote `>8min` must not fabricate `8min`; the real redirect is kept"
    );
    // A quoted grep -E regex carrying `> base` / `cur > base` fabricated `base` before.
    assert!(
        just_paths(r#"grep -nE 'cur > base' "$JSONL""#).is_empty(),
        "in-quote regex `>` must not fabricate a file"
    );
    assert!(
        just_paths(r#"grep -rnE "...|> *dt|interval" file.txt"#).is_empty(),
        "in-quote regex `> *dt` must not fabricate `*dt`"
    );
    // printf prose with a `>` (`café > cover`) — the non-ASCII-bearing class.
    assert!(
        just_paths(r#"printf 'layout café > cover scaled déjà'"#).is_empty(),
        "in-quote prose `>` must not fabricate a file"
    );
    // A sample oracle's `echo ">>> NO compact"` / `echo "  >> ABSENT"` family.
    assert!(just_paths(r#"echo ">>> NO compact""#).is_empty());
    assert!(just_paths(r#"echo "  >> ABSENT""#).is_empty());
    assert!(just_paths(r#"echo "  >> NOT assigned""#).is_empty());
}

#[test]
fn process_sub_body_operand_does_not_leak() {
    // `tee >(grep foo) /tmp/real.log`: the real sink is kept; the procsub BODY arg `foo`
    // must NOT be fabricated as a `tee` file (its surviving `)` previously let
    // `trim_structural_tail` peel it back to `foo`).
    let got = paths("tee >(grep foo) /tmp/real.log");
    assert!(
        got.contains(&("/tmp/real.log".to_string(), "tee")),
        "real sink lost: {got:?}"
    );
    assert!(
        !got.iter().any(|(p, _)| p == "foo" || p.contains("grep")),
        "process-sub body leaked: {got:?}"
    );
    // Nested / fd-qualified procsub bodies likewise contribute no file.
    assert!(
        !just_paths("make 2> >(tee /tmp/e.log >&2) 1> >(cat)")
            .iter()
            .any(|p| p == "cat" || p.contains("tee>") || p == "foo"),
        "nested procsub body leaked"
    );
}

#[test]
fn tilde_home_path_is_dropped_per_contract() {
    // The module-doc (line 39) precision contract: a `~`-bearing token yields nothing
    // (the shell would have expanded `~`; the literal `~/x` is not the real on-disk
    // path). Previously `has_unresolved_var` checked only `$`, so these leaked.
    assert!(paths("echo x > ~/notes.txt").is_empty());
    assert!(paths("cp a.txt ~/dest.txt").is_empty());
    assert!(paths("touch ~/scratch").is_empty());
    assert!(paths("dd if=/dev/zero of=~/img.bin").is_empty());
    // A LEADING `~` only: a mid-path literal `~` (a backup-file char) is still a path.
    assert_eq!(
        paths("rm /tmp/file~"),
        vec![("/tmp/file~".to_string(), "rm")]
    );
}

#[test]
fn shell_mask_input_procsub_and_quote_arms() {
    // An INPUT process-sub `<(…)` (the `<` open arm of the mask) is masked just like
    // `>(…)`, so a `diff <(a) <(b)` leaks no inner word.
    let got = just_paths("diff <(sort x) <(sort y) > /tmp/diff.out");
    assert!(
        got.contains(&"/tmp/diff.out".to_string()),
        "real redirect lost: {got:?}"
    );
    assert!(
        !got.iter()
            .any(|p| p == "x" || p == "y" || p.contains("sort")),
        "input-procsub body leaked: {got:?}"
    );
    // A single-quoted span inside a double-quoted command and vice-versa: the OUTER
    // quote governs (the inner quote byte is just masked content), so neither inner
    // `>` fabricates a file.
    assert!(just_paths(r#"echo "it's a > test""#).is_empty());
    assert!(just_paths(r#"echo 'say "a > b"'"#).is_empty());
    // The mask preserves a real redirect that follows a fully-quoted argument.
    assert_eq!(
        paths(r#"printf '%s' "a > b" > /tmp/p.txt"#),
        vec![("/tmp/p.txt".to_string(), ">")]
    );
}

#[test]
fn shell_mask_nested_procsub_and_fd_qualified_forms() {
    // A NESTED process-sub `>( … >(…) …)` exercises the `procsub_depth += 1` arm; the
    // whole nested body is masked, so no inner word leaks.
    let got = just_paths("tee >(grep a >(sort) b) /tmp/real.log");
    assert!(
        got.contains(&"/tmp/real.log".to_string()),
        "real sink lost: {got:?}"
    );
    assert!(
        !got.iter()
            .any(|p| p == "a" || p == "b" || p.contains("sort") || p.contains("grep")),
        "nested procsub body leaked: {got:?}"
    );
    // A fd-qualified `2>|file` attached form (the `>|` offset arm with a qualifier).
    assert_eq!(
        paths("svc 2>|/tmp/q.log"),
        vec![("/tmp/q.log".to_string(), ">")]
    );
    // A fd-qualified attached append `2>>file` and truncate `2>file` still slice the
    // path from the original at the post-qualifier offset.
    assert_eq!(
        paths("svc 2>>/tmp/a.log"),
        vec![("/tmp/a.log".to_string(), ">>")]
    );
    assert_eq!(
        paths("svc 2>/tmp/b.log"),
        vec![("/tmp/b.log".to_string(), ">")]
    );
}

#[test]
fn shell_mask_is_byte_length_preserving_with_multibyte_utf8() {
    // REGRESSION: the mask must be BYTE-length-identical to the input even with
    // accented-Latin / 3-byte / 4-byte-emoji chars inside (and outside) quotes — else
    // `masked_tokens` slices on a non-char boundary and panics (a dense-multibyte oracle).
    for cmd in [
        r#"echo "café € région > cover résumé" > /tmp/out.txt"#,
        r#"printf 'naïve >per-mode déjà' && touch /tmp/né.txt"#,
        "grep -nE 'cur > base' café.txt",
        r#"echo "🛠 build >done" > /tmp/x"#,
        "tee >(grep café) /tmp/real.log",
    ] {
        let m = shell_mask(cmd);
        assert_eq!(
            m.len(),
            cmd.len(),
            "mask byte-length must equal input for {cmd:?}"
        );
        // And the parse itself must not panic + must keep only real paths.
        let got = just_paths(cmd);
        assert!(
            got.iter()
                .all(|p| p.starts_with('/') || !p.chars().any(|c| c as u32 > 127)),
            "no non-ASCII-prose fragment should surface as a file: {got:?}"
        );
    }
    // The real redirect after a non-ASCII-bearing quoted arg is still caught.
    assert_eq!(
        just_paths(r#"echo "café > cover" > /tmp/nonascii-ok.txt"#),
        vec!["/tmp/nonascii-ok.txt".to_string()]
    );
}

#[test]
fn quote_aware_split_does_not_break_on_in_quote_sequencing() {
    // A `;`/`|`/`&&` INSIDE a quoted string no longer splits the segment (a side
    // benefit of the mask), so a real trailing redirect after the quote is still found
    // and the in-quote operator fabricates nothing.
    assert_eq!(
        paths(r#"echo "a; b | c && d" > /tmp/seq.log"#),
        vec![("/tmp/seq.log".to_string(), ">")]
    );
}

#[test]
fn shell_mask_and_helpers_direct_coverage() {
    // The mask masks quote interiors + procsub bodies, preserving byte length so offsets
    // line up with the original.
    let cmd = r#"echo "a>b" > /tmp/x"#;
    let m = shell_mask(cmd);
    assert_eq!(m.len(), cmd.len(), "mask is byte-length-preserving");
    // The in-quote `>` is masked; the real redirect `>` after the quote is intact.
    assert!(!m.contains("a>b"), "in-quote `>` should be masked: {m:?}");
    assert!(m.ends_with("> /tmp/x"), "real redirect kept: {m:?}");
    // A procsub body is fully masked (so its tokens drop).
    let m2 = shell_mask("tee >(grep foo) /tmp/x");
    assert!(!m2.contains("grep foo"), "procsub body masked: {m2:?}");
    assert!(m2.contains(">("), "the `>(` head is kept");
    // is_fully_masked: a body word is all-mask; a real word is not.
    let body = shell_mask(">(grep foo)"); // `>(` head kept, rest masked
    assert!(is_fully_masked(&body[2..]), "body after `>(` is all mask");
    assert!(!is_fully_masked("abc"));
    assert!(!is_fully_masked(""), "empty is not fully-masked");
    // has_unresolved_var now rejects a leading `~` as well as `$`.
    assert!(has_unresolved_var("~/x"));
    assert!(has_unresolved_var("$VAR"));
    assert!(!has_unresolved_var("/tmp/a~b")); // mid-path `~` is a literal
    assert!(!has_unresolved_var("/tmp/clean"));
}

#[test]
fn heredoc_here_string_is_not_a_heredoc() {
    // `<<<` is a here-STRING (no body line) — heredoc_delims must skip it, so a
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
    // A command with NO `<<` at all → strip_heredoc_bodies fast path (no change).
    assert_eq!(strip_heredoc_bodies("echo hi > /tmp/x"), "echo hi > /tmp/x");
}

// ── Backtick command-substitution: an inner redirect must not corrupt a path ──

#[test]
fn backtick_cmdsub_inner_redirect_not_fabricated() {
    // A `>` redirect INSIDE a backtick command substitution is masked, so neither the
    // redirect target NOR a backtick-glued path (`/tmp/bt.log\``) is ever emitted.
    assert!(just_paths("echo `date > /tmp/bt.log`").is_empty());
    // An assignment whose RHS is a backtick cmdsub with an inner `>>` likewise emits
    // nothing (the whole backtick body is invisible to redirect detection).
    assert!(just_paths("x=`wc -l < f >> /tmp/bt2.log`").is_empty());
}

#[test]
fn backtick_does_not_swallow_a_real_following_redirect() {
    // A real redirect OUTSIDE the backtick span is still detected: `echo \`date\` > f`
    // writes `f` (the backtick closes before the `>`).
    assert_eq!(
        just_paths("echo `date` > /tmp/real.log"),
        vec!["/tmp/real.log".to_string()]
    );
}

// ── Arithmetic `(( ))` and test `[[ ]]` comparison operators are not redirects ──

#[test]
fn arithmetic_comparison_does_not_fabricate_identifier() {
    // `(( a > b ))` is a comparison — the `>` is NOT a redirect, so the bare identifier
    // `b` must NOT be fabricated as a written file. (Before the arith-mask the `>` was
    // read as a redirect and `b` emitted.) The masked span emits NOTHING.
    assert!(
        just_paths("(( a > b ))").is_empty(),
        "arithmetic comparison must not fabricate a file"
    );
    // The same inside an `if … then … fi` wrapper: still no fabricated `b`.
    assert!(!just_paths("if (( a > b )); then echo hi; fi").contains(&"b".to_string()));
    // A numeric RHS likewise fabricates nothing.
    assert!(!just_paths("(( count > 5 ))").contains(&"5".to_string()));
    // A REAL redirect alongside the arithmetic is still detected (the `>` outside the
    // `(( ))` span writes `/tmp/r.log`).
    assert_eq!(
        just_paths("(( a > b )); echo done > /tmp/r.log"),
        vec!["/tmp/r.log".to_string()]
    );
}
