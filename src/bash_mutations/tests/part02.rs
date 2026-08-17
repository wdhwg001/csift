use super::*;

#[test]
fn git_mutating_vs_readonly_subcommand() {
    assert_eq!(paths("git add ."), vec![("git:add".to_string(), "git")]);
    assert_eq!(
        paths("git commit -m wip"),
        vec![("git:commit".to_string(), "git")]
    );
    // Read-only subcommands record nothing.
    assert!(paths("git status").is_empty());
    assert!(paths("git log --oneline").is_empty());
    assert!(paths("git diff HEAD").is_empty());
}

#[test]
fn git_skips_leading_dash_c_option() {
    // `git -c key=val commit` → the subcommand is the first non-flag token.
    assert_eq!(
        paths("git -c user.name=x commit"),
        vec![("git:commit".to_string(), "git")]
    );
    // A bare global option (`--no-pager`) is skipped to find the subcommand.
    assert_eq!(
        paths("git --no-pager add ."),
        vec![("git:add".to_string(), "git")]
    );
    // `-C <path>` also consumes its value before the subcommand.
    assert_eq!(
        paths("git -C /repo commit"),
        vec![("git:commit".to_string(), "git")]
    );
    // git with ONLY options and no subcommand → git_subcommand returns None.
    assert!(paths("git --version").is_empty());
    assert!(paths("git -c a=b").is_empty());
}

#[test]
fn segment_with_only_prefix_no_command_is_empty() {
    // A segment that is ONLY a `sudo`/`env …` prefix with no command → after
    // strip_prefixes the cmd_tokens are empty (the split_first None arm).
    assert!(paths("sudo").is_empty());
    assert!(paths("env FOO=1").is_empty());
}

#[test]
fn mv_with_only_flags_has_no_paths() {
    // `mv -f` (only a flag, no operands) → emit_mv's split_last None arm.
    assert!(paths("mv -f").is_empty());
}

#[test]
fn redirection_operator_with_no_following_token() {
    // A trailing bare `>` with no filename after it → the `tokens.get(i+1)` None
    // arm (no path emitted), and the bare `>` is not itself a path.
    assert!(paths("echo hi >").is_empty());
    assert!(paths("echo hi >>").is_empty());
}

#[test]
fn cp_with_only_flags_emits_nothing() {
    // `cp -r` (no operands) → emit_last_operand finds no paths.
    assert!(paths("cp -r").is_empty());
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
fn compound_command_across_operators() {
    // A real compound: mkdir, tee, sed -i across &&/|. Each segment parsed; the
    // sed script `s/a/b/` and the input redirect `< z` must NOT become targets.
    let got = paths("mkdir -p x && tee y < z | sed -i s/a/b/ w");
    assert!(got.contains(&("x".to_string(), "mkdir")), "got: {got:?}");
    assert!(got.contains(&("y".to_string(), "tee")), "got: {got:?}");
    assert!(got.contains(&("w".to_string(), "sed-i")), "got: {got:?}");
    // The read-only input file `z`, the `<` operator, and the sed script are NOT
    // reported as mutations.
    assert!(
        !got.iter()
            .any(|(p, _)| p == "z" || p == "<" || p == "s/a/b/"),
        "input-redirect file / operator / sed-script leaked: {got:?}"
    );
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

#[test]
fn no_mutation_commands_return_empty() {
    assert!(paths("ls -la").is_empty());
    assert!(paths("cat file.txt").is_empty());
    assert!(paths("grep -r foo .").is_empty());
    assert!(paths("echo hello").is_empty());
}

#[test]
fn sudo_and_env_prefixes_are_stripped() {
    // sudo rm → still detected.
    assert_eq!(paths("sudo rm /etc/x"), vec![("/etc/x".to_string(), "rm")]);
    // env VAR=1 touch → the real verb is `touch` after stripping the env assignment.
    assert_eq!(
        paths("env FOO=1 BAR=2 touch out.txt"),
        vec![("out.txt".to_string(), "touch")]
    );
}

#[test]
fn flags_and_bare_dash_and_assignments_skipped_as_paths() {
    // rm with only flags + a bare `-` → no path operands.
    assert!(paths("rm -rf -").is_empty());
    // A KEY=VALUE operand is not a path.
    assert!(paths("touch FOO=bar").is_empty());
}

#[test]
fn glob_operand_kept_verbatim() {
    // A glob we cannot expand is kept verbatim (still informative; heuristic label).
    assert_eq!(paths("rm *.tmp"), vec![("*.tmp".to_string(), "rm")]);
}

#[test]
fn empty_command_is_empty() {
    assert!(paths("").is_empty());
    assert!(paths("   ").is_empty());
    // A segment that is only an operator yields empty segments → nothing.
    assert!(paths(" && || ; |").is_empty());
}

#[test]
fn strip_quotes_unmatched_left_intact() {
    // An unmatched single quote is left intact (the no-strip arm).
    assert_eq!(strip_quotes("\"only-left"), "\"only-left");
    assert_eq!(strip_quotes("x"), "x");
    assert_eq!(strip_quotes(""), "");
}

#[test]
fn is_assignment_recognizes_and_rejects() {
    assert!(is_assignment("FOO=bar"));
    assert!(is_assignment("A_B=1"));
    assert!(!is_assignment("-flag"));
    assert!(!is_assignment("=noname"));
    assert!(!is_assignment("plain"));
}

// ────────────────────────────────────────────────────────────────────────────
// Regression oracle: the synthetic IDIOM MATRIX from the files-attribution
// verdict (csift-files-attribution-verdict.md). Every idiom the verdict marked
// CAUGHT must stay caught; every idiom it marked MISSED (Fixes A–C) must now be
// caught; the precision cases (Fix D) must stay DROPPED.
// ────────────────────────────────────────────────────────────────────────────

// ── Fix A — fd-qualified redirects (the dominant previously-missed class) ──

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
    // `1>/tmp/x.log` — the stdout fd-redirect form.
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
    // `&>/tmp/x.log` — both-streams redirect (attached + spaced).
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
    // `2>>/tmp/e.err` and `&>>/tmp/x.log` — the fd-qualified APPEND forms, verb ">>".
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

// ── Fix B — curl / wget output flags ──

#[test]
fn curl_dash_o_output_caught() {
    // `curl -s URL -o /tmp/x.json` — the dominant Smain miss (7/7).
    assert_eq!(
        paths("curl -s https://api.example.com/d -o /tmp/x.json"),
        vec![("/tmp/x.json".to_string(), "curl")]
    );
}

#[test]
fn curl_long_output_flag_both_forms() {
    // `--output /tmp/x` (spaced) and `--output=/tmp/x` (inline). The LONG `--output`
    // forms are owned by the generic flag-output scan (verb `flag-output`, NOT
    // double-emitted under `curl`); only the path is load-bearing. Exactly ONE row.
    assert_eq!(
        paths("curl URL --output /tmp/a.json"),
        vec![("/tmp/a.json".to_string(), "flag-output")]
    );
    assert_eq!(
        paths("curl URL --output=/tmp/b.json"),
        vec![("/tmp/b.json".to_string(), "flag-output")]
    );
}

#[test]
fn curl_capital_o_no_path_is_skipped() {
    // `curl -O URL` derives the local name from the URL → no deterministic path.
    assert!(paths("curl -O https://example.com/file.tar.gz").is_empty());
    // `curl -sO https://… /tmp/x` — the bundled `-sO` is not our `-O`-takes-next
    // form (curl's -O takes no path), so no fabricated path either.
    assert!(paths("curl -sO https://example.com/x").is_empty());
}

#[test]
fn wget_capital_o_output_caught() {
    // `wget -O /tmp/x.bin URL` — wget's capital-O DOES take a path.
    assert_eq!(
        paths("wget -O /tmp/x.bin https://example.com/x"),
        vec![("/tmp/x.bin".to_string(), "wget")]
    );
}

#[test]
fn wget_output_document_caught() {
    assert_eq!(
        paths("wget --output-document /tmp/y.bin https://example.com/y"),
        vec![("/tmp/y.bin".to_string(), "wget")]
    );
}

// ── Fix C — flag-specified outputs, dd, zip ──

#[test]
fn junit_xml_flag_both_dashes_caught() {
    // `--junit-xml=/tmp/x.xml` and `--junitxml=/tmp/x.xml` (the two pytest spellings).
    assert_eq!(
        paths("pytest --junit-xml=/tmp/r.xml"),
        vec![("/tmp/r.xml".to_string(), "flag-output")]
    );
    assert_eq!(
        paths("pytest --junitxml=/tmp/r2.xml"),
        vec![("/tmp/r2.xml".to_string(), "flag-output")]
    );
}

#[test]
fn report_path_flag_spaced_caught() {
    // `gitleaks --report-path /tmp/x.json` (spaced value form).
    assert_eq!(
        paths("gitleaks detect --report-path /tmp/leaks.json"),
        vec![("/tmp/leaks.json".to_string(), "flag-output")]
    );
}

#[test]
fn generic_output_flags_caught() {
    // `--output=/tmp/o` under a non-curl/wget verb still resolves via the generic scan.
    assert_eq!(
        paths("sometool --output=/tmp/o.txt"),
        vec![("/tmp/o.txt".to_string(), "flag-output")]
    );
    assert_eq!(
        paths("sometool --logfile /tmp/run.log"),
        vec![("/tmp/run.log".to_string(), "flag-output")]
    );
}

#[test]
fn dd_of_output_caught() {
    // `dd if=/dev/zero of=/tmp/x.bin` — `of=` parsed specially (KEY=VALUE otherwise
    // rejected); `if=` (input) is NOT emitted.
    assert_eq!(
        paths("dd if=/dev/zero of=/tmp/x.bin bs=1M count=4"),
        vec![("/tmp/x.bin".to_string(), "dd")]
    );
}

#[test]
fn dd_of_dev_null_dropped() {
    // `of=/dev/null` is a sink, not a created file.
    assert!(paths("dd if=/tmp/src of=/dev/null").is_empty());
}

#[test]
fn zip_dest_is_first_operand_only() {
    // `zip /tmp/x.zip a b` — only the archive dest, NOT the input members.
    assert_eq!(
        paths("zip /tmp/x.zip a b c"),
        vec![("/tmp/x.zip".to_string(), "zip")]
    );
    // With flags before the dest, the flag is skipped and the first non-flag wins.
    assert_eq!(
        paths("zip -r /tmp/y.zip dir/"),
        vec![("/tmp/y.zip".to_string(), "zip")]
    );
}

// ── Fix D — PRECISION: noisy pseudo-paths are DROPPED, never fabricated ──

#[test]
fn unresolved_var_redirect_is_dropped() {
    // `> $OUT` / `>${DIR}/x` — an unexpandable variable pseudo-path is dropped.
    assert!(paths("echo hi > $OUT").is_empty());
    assert!(paths("echo hi >${DIR}/x.log").is_empty());
    assert!(paths("svc 2>/tmp/$run.err").is_empty());
}
