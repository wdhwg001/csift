//! Operand classification: flags, assignments, unresolved vars, noise rejection, helpers.

use super::*;

#[test]
fn segment_with_only_prefix_no_command_is_empty() {
    // A segment that is ONLY a `sudo`/`env …` prefix with no command → after
    // strip_prefixes the cmd_tokens are empty (the split_first None arm).
    assert!(paths("sudo").is_empty());
    assert!(paths("env FOO=1").is_empty());
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
fn no_mutation_commands_return_empty() {
    assert!(paths("ls -la").is_empty());
    assert!(paths("cat file.txt").is_empty());
    assert!(paths("grep -r foo .").is_empty());
    assert!(paths("echo hello").is_empty());
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
fn is_assignment_recognizes_and_rejects() {
    assert!(is_assignment("FOO=bar"));
    assert!(is_assignment("A_B=1"));
    assert!(!is_assignment("-flag"));
    assert!(!is_assignment("=noname"));
    assert!(!is_assignment("plain"));
}

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
    // The verdict's CAUGHT column - a regression guard in one place.
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

#[test]
fn residual_noise_classes_rejected_real_relpaths_kept() {
    // `=`-led value fragments, pure numbers, and code shards (comma / trailing colon
    // with no `/`) are rejected - the residual `by-file` garbage classes.
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
