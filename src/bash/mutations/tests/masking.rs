//! Shell masking: quotes, comments, command/process substitution, non-redirect operators.

use super::*;

#[test]
fn trailing_comment_words_are_not_fabricated_paths() {
    // A `# words` comment after a real mutation must NOT leak its words (nor the bare
    // `#`) as touched paths - only the genuine operand survives.
    assert_eq!(
        paths("touch /tmp/real.txt  # create the marker file"),
        vec![("/tmp/real.txt".to_string(), "touch")]
    );
    assert_eq!(
        paths("rm -rf /tmp/build && mkdir /tmp/build  # rebuild from scratch"),
        vec![
            ("/tmp/build".to_string(), "rm"),
            ("/tmp/build".to_string(), "mkdir")
        ]
    );
}

#[test]
fn in_comment_redirect_does_not_fabricate_a_path() {
    // A `>`/`>>` that appears INSIDE a trailing comment is masked with the comment, so
    // it is not read as a real redirect - only the genuine `mkdir` dir survives.
    assert_eq!(
        paths("mkdir -p /tmp/d # comment > /tmp/fabricated-by-comment-redirect.txt"),
        vec![("/tmp/d".to_string(), "mkdir")]
    );
}

#[test]
fn comment_does_not_displace_cp_destination() {
    // The comment must not steal the cp/mv/ln destination: the LAST positional after the
    // comment is masked away, so the true dest is the surviving last operand.
    assert_eq!(
        paths("cp /tmp/src.txt /tmp/real-dest.bak  # café note"),
        vec![("/tmp/real-dest.bak".to_string(), "cp")]
    );
    // `mv a b` reports the dest (`mv`) AND the source (`mv-from`); the `# note` tail must
    // be masked away so it neither displaces the dest nor adds a phantom source.
    assert_eq!(
        paths("mv /tmp/a /tmp/b  # note"),
        vec![
            ("/tmp/b".to_string(), "mv"),
            ("/tmp/a".to_string(), "mv-from")
        ]
    );
}

#[test]
fn in_path_hash_is_not_a_comment() {
    // A `#` that is part of a path (prev byte is a path char, not whitespace/separator)
    // is NOT a comment and the whole path is preserved.
    assert_eq!(
        paths("rm -f /tmp/a#b.txt"),
        vec![("/tmp/a#b.txt".to_string(), "rm")]
    );
    assert_eq!(
        paths("touch file#1.log"),
        vec![("file#1.log".to_string(), "touch")]
    );
}

#[test]
fn comment_after_separator_without_space_is_masked() {
    // `cmd;# c` / `cmd|# c`: a `#` directly after a command separator (no space) still
    // opens a comment (the separator is a word boundary).
    assert_eq!(
        paths("touch /tmp/x.txt;# trailing"),
        vec![("/tmp/x.txt".to_string(), "touch")]
    );
}

#[test]
fn full_line_leading_comment_emits_nothing() {
    // A line that is ENTIRELY a comment yields no rows (the verb itself is masked away).
    assert_eq!(paths("# just a note, nothing here"), vec![]);
}

#[test]
fn quoted_paths_are_stripped() {
    assert_eq!(
        paths("rm \"a-file.txt\""),
        vec![("a-file.txt".to_string(), "rm")]
    );
    assert_eq!(
        paths("touch 'single.txt'"),
        vec![("single.txt".to_string(), "touch")]
    );
}

#[test]
fn strip_quotes_unmatched_left_intact() {
    // An unmatched single quote is left intact (the no-strip arm).
    assert_eq!(strip_quotes("\"only-left"), "\"only-left");
    assert_eq!(strip_quotes("x"), "x");
    assert_eq!(strip_quotes(""), "");
}

#[test]
fn process_substitution_redirect_emits_no_fragment() {
    // `> >(tee /tmp/x)` must NOT leak `>(tee` / `(tee` rows (the real inner path is a
    // documented recall miss, but precision is preserved - no garbage).
    let got = just_paths("cmd > >(tee /tmp/ps.log)");
    assert!(
        !got.iter()
            .any(|p| p.contains("tee") || p.contains('(') || p.contains('>')),
        "process-sub fragments leaked: {got:?}"
    );
    let got2 = just_paths("make 2> >(tee /tmp/err.log >&2)");
    assert!(
        !got2.iter().any(|p| p.contains("tee") || p.contains('(')),
        "process-sub fragments leaked: {got2:?}"
    );
}

#[test]
fn quoted_path_with_metachar_or_space_does_not_leak_fragment() {
    // The mask now makes a quoted in-path `;` and space invisible to the tokenizer, so a
    // quoted span stays ONE token and the WHOLE path is recovered (no `'…`/`"…` fragment,
    // and no mid-filename split). Both forms below resolve to their full quoted content.
    let got = just_paths("echo x >> '/tmp/quoted; path.txt'");
    assert_eq!(got, vec!["/tmp/quoted; path.txt".to_string()]);
    let got2 = just_paths("cmd > 'output (final).txt'");
    assert_eq!(got2, vec!["output (final).txt".to_string()]);
    for got in [&got, &got2] {
        assert!(
            !got.iter()
                .any(|p| p.starts_with('\'') || p.starts_with('"')),
            "quoted-split fragment leaked: {got:?}"
        );
    }
}

#[test]
fn quoted_space_bearing_path_stays_one_token_not_a_partial() {
    // CRITICAL precision fix: a quoted redirect/operand path CONTAINING A SPACE (the most
    // common macOS path shape - Library/Application Support, Google Drive, My Documents)
    // must stay ONE token and be emitted WHOLE, never split mid-filename into a fabricated
    // partial (the prior `"…/Application Support/x"` → `Support/x` bug). Whitespace is now
    // read off the MASK, where an in-quote space is `0x01` (non-whitespace).
    assert_eq!(
        paths("cp config.json \"/Users/me/Library/Application Support/app/config.json\""),
        vec![(
            "/Users/me/Library/Application Support/app/config.json".to_string(),
            "cp"
        )]
    );
    assert_eq!(
        paths("rm -rf \"/Users/me/My Project/build\""),
        vec![("/Users/me/My Project/build".to_string(), "rm")]
    );
    assert_eq!(
        paths("mkdir -p \"/Users/me/Google Drive/notes\""),
        vec![("/Users/me/Google Drive/notes".to_string(), "mkdir")]
    );
    // Single-quoted redirect target with a space → full path via the `>` redirect verb.
    assert_eq!(
        paths("echo x > '/tmp/has space.txt'"),
        vec![("/tmp/has space.txt".to_string(), ">")]
    );
    // The bare `rm '/tmp/has space.txt'` operand also recovers the whole path.
    assert_eq!(
        paths("rm '/tmp/has space.txt'"),
        vec![("/tmp/has space.txt".to_string(), "rm")]
    );
}

#[test]
fn backtick_command_substitution_target_is_not_a_path() {
    // A backtick command substitution used as a redirect/operand TARGET is never an
    // on-disk path. `shell_mask` masks the backtick body but the structural backticks
    // survive in the original slice, so `has_syntax_noise` must reject any backtick-
    // bearing token. No fabricated `` `mktemp` `` / `` `mktemp `` pseudo-path rows.
    assert!(paths("echo x > `mktemp`").is_empty());
    assert!(paths("cmd 2> `mktemp -t err`").is_empty());
    // A real redirect to a normal file alongside the backtick command is unaffected.
    assert_eq!(
        paths("echo `date` > /tmp/real.log"),
        vec![("/tmp/real.log".to_string(), ">")]
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
    // printf prose with a `>` (`café > cover`) - the non-ASCII-bearing class.
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
    // accented-Latin / 3-byte / 4-byte-emoji chars inside (and outside) quotes - else
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
    // has_unresolved_var rejects `$` only; a leading `~` is KEPT (verbatim policy -
    // the resolver classes it `unresolved` instead of dropping the row).
    assert!(!has_unresolved_var("~/x"));
    assert!(has_unresolved_var("$VAR"));
    assert!(!has_unresolved_var("/tmp/a~b")); // mid-path `~` is a literal
    assert!(!has_unresolved_var("/tmp/clean"));
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
    // `(( a > b ))` is a comparison - the `>` is NOT a redirect, so the bare identifier
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

#[test]
fn test_double_bracket_comparison_is_not_a_redirect() {
    // `[[ a > b ]]` is a lexicographic comparison; the `>` is not a redirect and `y`
    // must not be fabricated. The masked test span emits nothing.
    assert!(
        just_paths("[[ x > y ]]").is_empty(),
        "[[ ]] comparison must not fabricate a file"
    );
    assert!(!just_paths("if [[ x > y ]]; then echo hi; fi").contains(&"y".to_string()));
}
