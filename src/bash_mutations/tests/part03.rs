use super::*;

#[test]
fn unresolved_var_verb_operand_is_dropped() {
    // A `$VAR`-bearing operand of a normal verb is also dropped (no fabrication).
    assert!(paths("touch $TMPFILE").is_empty());
    assert!(paths("cp src $DEST/out").is_empty());
    assert!(paths("mkdir -p $WORK/sub").is_empty());
}

#[test]
fn unresolved_var_flag_and_dd_dropped() {
    // The precision rule applies to the new emitters too.
    assert!(paths("pytest --junit-xml=$REPORT").is_empty());
    assert!(paths("curl URL -o $OUT").is_empty());
    assert!(paths("dd if=/dev/zero of=$IMG").is_empty());
}

#[test]
fn flag_output_glob_is_dropped() {
    // A glob as a single output destination is a parse artifact → concrete_path drops.
    assert!(paths("tool --output=/tmp/*.json").is_empty());
    assert!(paths("dd if=x of=/tmp/?.bin").is_empty());
}

#[test]
fn concrete_path_helper_rules() {
    // Direct coverage: rejects globs + vars; keeps a concrete path.
    assert_eq!(concrete_path("/tmp/x.json").as_deref(), Some("/tmp/x.json"));
    assert!(concrete_path("/tmp/*.json").is_none());
    assert!(concrete_path("/tmp/$v").is_none());
    assert!(concrete_path("-flag").is_none());
}

#[test]
fn strip_fd_qualifier_rules() {
    // Direct coverage of the fd-prefix peeler.
    assert_eq!(strip_fd_qualifier("2>"), ">");
    assert_eq!(strip_fd_qualifier("1>>"), ">>");
    assert_eq!(strip_fd_qualifier("&>"), ">");
    assert_eq!(strip_fd_qualifier("12>file"), ">file");
    // No redirect after the qualifier → unchanged.
    assert_eq!(strip_fd_qualifier("&1"), "&1");
    assert_eq!(strip_fd_qualifier("2"), "2");
    assert_eq!(strip_fd_qualifier(">"), ">");
    assert_eq!(strip_fd_qualifier("plain"), "plain");
}

#[test]
fn previously_caught_idioms_all_still_caught() {
    // The verdict's CAUGHT column — a regression guard in one place.
    assert_eq!(just_paths("echo hi > /tmp/x"), vec!["/tmp/x"]);
    assert_eq!(just_paths("echo hi >> /tmp/x"), vec!["/tmp/x"]);
    assert_eq!(just_paths("cmd | tee /tmp/x"), vec!["/tmp/x"]);
    assert_eq!(just_paths("touch /tmp/x"), vec!["/tmp/x"]);
    assert_eq!(just_paths("mkdir -p /tmp/x"), vec!["/tmp/x"]);
    assert_eq!(just_paths("cp src /tmp/x"), vec!["/tmp/x"]);
    assert_eq!(just_paths("mv src /tmp/x"), vec!["/tmp/x", "src"]); // dest + mv-from
    assert_eq!(just_paths("sed -i s/a/b/ /tmp/x"), vec!["/tmp/x"]);
}

#[test]
fn heredoc_python_open_is_out_of_scope_documented() {
    // Fix E: an inline `python3 -c "open('/tmp/x','w')…"` body is NOT parsed — a
    // documented lexical-parser limitation. The precision contract still holds: the
    // miss produces NO wrong row (the `python3` verb is not in the allowlist and the
    // quoted body never resolves to a redirect/flag). This test PINS that contract:
    // a recall miss, never a precision violation.
    assert!(
        paths(r#"python3 -c "open('/tmp/out.json','w').write('x')""#).is_empty(),
        "heredoc/python body is out of scope — and must not fabricate a row"
    );
}

// ── R1: the DOMINANT garbage class — fd-redirect close-paren / process-sub leaks ──

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
fn process_substitution_redirect_emits_no_fragment() {
    // `> >(tee /tmp/x)` must NOT leak `>(tee` / `(tee` rows (the real inner path is a
    // documented recall miss, but precision is preserved — no garbage).
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
    // common macOS path shape — Library/Application Support, Google Drive, My Documents)
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

#[test]
fn redirect_metachar_and_fd_dup_tokens_are_not_paths() {
    // `2>&1` and a bare `>` never become file rows even when they reach path_operand.
    assert!(paths("pytest 2>&1").is_empty());
    assert_eq!(path_operand("2>&1"), None);
    assert_eq!(path_operand("/tmp/a>b"), None); // embedded redirect → noise.
}

// ── R1: cp/mv/install `-t DIR` correctness (source-vs-dest inversion) ──

#[test]
fn cp_target_directory_flag_dest_is_the_t_value_not_a_source() {
    // `cp -t DIR src…`: the DEST is DIR; the sources are reads, never reported as cp.
    assert_eq!(
        paths("cp -t /tmp/destdir /home/a.txt /home/b.txt"),
        vec![("/tmp/destdir".to_string(), "cp")]
    );
    // `--target-directory=DIR` inline form.
    assert_eq!(
        paths("cp --target-directory=/tmp/d /home/a.txt"),
        vec![("/tmp/d".to_string(), "cp")]
    );
    // install behaves the same.
    assert_eq!(
        paths("install -m755 -t /usr/local/bin a b"),
        vec![("/usr/local/bin".to_string(), "install")]
    );
}

#[test]
fn mv_target_directory_flag_dest_and_sources() {
    // `mv -t DIR a b`: DIR is the destination (mv); a + b are mv-from sources; DIR is
    // NOT also listed as a source.
    let got = paths("mv -t /tmp/destdir /home/a.txt /home/b.txt");
    assert!(
        got.contains(&("/tmp/destdir".to_string(), "mv")),
        "got: {got:?}"
    );
    assert!(
        got.contains(&("/home/a.txt".to_string(), "mv-from")),
        "got: {got:?}"
    );
    assert!(
        got.contains(&("/home/b.txt".to_string(), "mv-from")),
        "got: {got:?}"
    );
    assert!(
        !got.iter()
            .any(|(p, v)| p == "/tmp/destdir" && *v == "mv-from"),
        "the -t DIR must not also be a source: {got:?}"
    );
}

// ── R1: recall — ln / install / rsync / >| ──

#[test]
fn ln_install_rsync_destinations_caught() {
    assert_eq!(
        paths("ln -s /src /tmp/link"),
        vec![("/tmp/link".to_string(), "ln")]
    );
    assert_eq!(
        paths("install -m755 bin /usr/local/bin/tool"),
        vec![("/usr/local/bin/tool".to_string(), "install")]
    );
    assert_eq!(
        paths("rsync -a src/ /tmp/dest/"),
        vec![("/tmp/dest/".to_string(), "rsync")]
    );
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

// ── R7: a trailing OUTPUT redirect must not poison a positional-dest verb ──
// collect_redirections now REMOVES the redirect tokens (operator + its spaced path) from
// the operand stream before verb dispatch — symmetric to strip_input_redirects for `<`.
// Without this, the surviving `2>&1` / `2>/dev/null` / spaced `> file` token displaced the
// real cp/mv/ln/install/rsync destination (RECALL MISS), got mislabeled as a source
// (SEMANTIC LEAK), or double-emitted the redirect path (DOUBLE-EMIT).

#[test]
fn cp_dest_survives_trailing_fd_dup_redirect() {
    // `2>&1` is an fd-dup (no path); it must NOT become cp's positional.last() dest. The
    // REAL dest `/tmp/CP_DEST.txt` is emitted exactly once, the source is a read.
    assert_eq!(
        paths("cp src.txt /tmp/CP_DEST.txt 2>&1"),
        vec![("/tmp/CP_DEST.txt".to_string(), "cp")]
    );
}

#[test]
fn install_ln_rsync_dest_survive_trailing_redirect() {
    // install with `2>&1`: the real dest /etc/DEST.conf survives.
    assert_eq!(
        paths("install -m 644 s /etc/DEST.conf 2>&1"),
        vec![("/etc/DEST.conf".to_string(), "install")]
    );
    // ln -s with `2>/dev/null`: the link name survives (the dev-sink emits no row).
    assert_eq!(
        paths("ln -s t /tmp/LINKNAME 2>/dev/null"),
        vec![("/tmp/LINKNAME".to_string(), "ln")]
    );
    // rsync with `2>&1`: the dest dir survives.
    assert_eq!(
        paths("rsync -a src/ /tmp/RSYNC_DEST/ 2>&1"),
        vec![("/tmp/RSYNC_DEST/".to_string(), "rsync")]
    );
}

#[test]
fn mv_into_dir_not_leaked_as_source_by_trailing_redirect() {
    // `mv /a /b /tmp/DESTDIR/ 2>/dev/null`: the spaced/attached `2>/dev/null` is a dev-sink
    // (emits no row) AND is removed from operands, so DESTDIR stays the dest (verb `mv`) and
    // /a,/b stay sources (mv-from). Before the fix, `2>/dev/null` became the dest_tok
    // (rejected as a sink → NO mv dest) and ALL of /a,/b,DESTDIR leaked as mv-from reads.
    assert_eq!(
        paths("mv /a /b /tmp/DESTDIR/ 2>/dev/null"),
        vec![
            ("/tmp/DESTDIR/".to_string(), "mv"),
            ("/a".to_string(), "mv-from"),
            ("/b".to_string(), "mv-from")
        ]
    );
}

#[test]
fn cp_dest_survives_trailing_append_redirect_no_double_emit() {
    // `cp a b /tmp/CDEST/ >> /tmp/log.txt`: the `>>` and its spaced path are both consumed,
    // so /tmp/log.txt is emitted ONCE (by the redirect collector, verb `>>`) and the real
    // dest /tmp/CDEST/ stays cp's last positional — no double-emit, no dropped dest.
    assert_eq!(
        paths("cp a b /tmp/CDEST/ >> /tmp/log.txt"),
        vec![
            ("/tmp/log.txt".to_string(), ">>"),
            ("/tmp/CDEST/".to_string(), "cp")
        ]
    );
}

#[test]
fn cp_dest_survives_spaced_truncate_redirect() {
    // The spaced `> /tmp/log` form (operator + path are two tokens): both consumed.
    assert_eq!(
        paths("cp src /tmp/CP2/ > /tmp/log"),
        vec![
            ("/tmp/log".to_string(), ">"),
            ("/tmp/CP2/".to_string(), "cp")
        ]
    );
}

// ── R1: heredoc body is SKIPPED, not mis-reported (the never-mis-reported contract) ──

#[test]
fn heredoc_body_redirect_char_does_not_fabricate_a_row() {
    // A `>` inside a heredoc body must NOT become a redirect row, and the opener's own
    // real trailing redirect (if any) IS still caught.
    let body_only = "python3 - <<'PY'\nprint('a > b')\nopen('/tmp/real.json','w')\nPY";
    let got = just_paths(body_only);
    assert!(
        !got.iter()
            .any(|p| p.contains("b'") || p.contains('>') || p == "/tmp/real.json"),
        "heredoc body leaked a row: {got:?}"
    );
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
    // here-string `<<<` is NOT a heredoc (no body) — the command still parses normally.
    assert_eq!(
        just_paths("grep x <<< 'data' > /tmp/hs.txt"),
        vec!["/tmp/hs.txt".to_string()]
    );
}

#[test]
fn trim_structural_tail_and_syntax_noise_helpers() {
    // Direct coverage of the two precision helpers.
    assert_eq!(trim_structural_tail("/dev/null)"), "/dev/null");
    assert_eq!(trim_structural_tail("/tmp/x;"), "/tmp/x");
    assert_eq!(trim_structural_tail("/tmp/x))"), "/tmp/x");
    // A balanced paren in the name is left intact.
    assert_eq!(trim_structural_tail("/tmp/(x)"), "/tmp/(x)");
    assert!(has_syntax_noise("'/tmp/x")); // unbalanced single quote
    assert!(has_syntax_noise("\"/tmp/x")); // unbalanced double quote
    assert!(has_syntax_noise(">(tee")); // process-sub head (`>(`)
    assert!(has_syntax_noise("<(cat")); // process-sub head (`<(`)
    assert!(has_syntax_noise("(subshell")); // bare `(` head
    assert!(has_syntax_noise("/a>b")); // embedded `>` redirect
    assert!(has_syntax_noise("/a<b")); // embedded `<` redirect
    assert!(has_syntax_noise("/a\\b")); // backslash escape
    assert!(has_syntax_noise("a|b")); // pipe metachar
    assert!(has_syntax_noise("a^b")); // caret metachar
    assert!(has_syntax_noise("/tmp/[unbalanced")); // unbalanced bracket
    assert!(has_syntax_noise("/tmp/{unbalanced")); // unbalanced brace
    assert!(has_syntax_noise("=value")); // `=`-led fragment
    assert!(has_syntax_noise("12345")); // pure number
    assert!(has_syntax_noise("a,b")); // comma, no slash → code shard
    assert!(has_syntax_noise("label:")); // trailing colon, no slash
    assert!(!has_syntax_noise("/tmp/clean.txt"));
    assert!(!has_syntax_noise("*.tmp")); // a plain glob is NOT noise
    assert!(!has_syntax_noise("src/main.rs")); // a real relative path with `/`
    assert!(!has_syntax_noise("")); // empty is not (pure-number guard skips empty)
}
