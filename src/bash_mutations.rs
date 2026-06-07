//! Heuristic, regex-free Bash file-mutation parser.
//!
//! Bash tool_use records carry `input.command` but their `toolUseResult` is
//! `{stdout, stderr, interrupted, isImage, noOutputExpected}` — **no path field**. So
//! unlike the structured Write/Edit/MultiEdit/NotebookEdit tools (which name an exact
//! `file_path`), a Bash file mutation can only be inferred from the command STRING.
//!
//! This is a **best-effort LEXICAL** parse, NOT a shell parser: it splits the command
//! on `;`, `&&`, `||`, `|`, and newlines into segments, then inspects each segment's
//! leading command token against a conservative allowlist of mutating verbs. It does
//! not expand variables, globs, command substitutions, or aliases, and it does not
//! touch the filesystem. Every mutation it reports is therefore **labelled heuristic**
//! everywhere it surfaces (text output, JSON, help, SKILL) — see
//! [`crate::model::FileOp::is_heuristic`]. Relative paths are reported VERBATIM (the
//! session's cwd at command time is not reliably known, so absolutizing would
//! fabricate a path).

/// One heuristically-detected Bash file mutation. `verb` is from the fixed allowlist
/// below (it is the lexical command/operator that touched the path), and `path` is the
/// operand exactly as it appeared (quote-stripped, otherwise verbatim).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BashMutation {
    pub path: String,
    pub verb: &'static str,
}

/// Git subcommands that mutate the working tree / index / refs (the conservative
/// mutating set). A `git <sub>` not in this set (e.g. `status`, `log`, `diff`) records
/// nothing — git does not name a clean file list lexically, so a mutating subcommand
/// is recorded coarsely as a single `git:<sub>` pseudo-path, flagged heuristic.
const GIT_MUTATING: &[&str] = &[
    "add", "commit", "checkout", "reset", "rm", "mv", "restore", "stash", "merge", "rebase",
    "apply", "clean",
];

/// Parse a Bash command string into the file mutations it heuristically performs.
///
/// Splits into segments on `;`, `&&`, `||`, `|`, and newlines, then for each segment:
/// strips leading `env VAR=…` / `sudo` prefixes, inspects the first token against the
/// mutating-verb allowlist, and additionally scans every segment for `>`/`>>`
/// redirection targets. Non-mutating commands (`ls`, `cat`, `grep`, `sed` without an
/// in-place flag, `git status`, …) contribute nothing.
#[must_use]
pub fn parse_bash_mutations(command: &str) -> Vec<BashMutation> {
    let mut out = Vec::new();
    for segment in split_segments(command) {
        parse_segment(segment, &mut out);
    }
    out
}

/// Split a command into segments on the shell sequencing/pipe operators and newlines.
/// This is lexical: it does NOT respect quoting of an operator (a `;` inside a quoted
/// string would still split) — acceptable for a heuristic, and rare in practice.
fn split_segments(command: &str) -> Vec<&str> {
    // Replace the multi-char operators with a single sentinel byte we then split on,
    // without allocating per-segment: walk and cut. We do a simple manual scan.
    let bytes = command.as_bytes();
    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let two = bytes.get(i..i + 2);
        let is_two_op = matches!(two, Some(b"&&") | Some(b"||"));
        let is_one_op = matches!(bytes[i], b';' | b'|' | b'\n');
        if is_two_op {
            segments.push(&command[start..i]);
            i += 2;
            start = i;
        } else if is_one_op {
            segments.push(&command[start..i]);
            i += 1;
            start = i;
        } else {
            i += 1;
        }
    }
    segments.push(&command[start..]);
    segments
}

/// Parse one segment: handle redirection targets anywhere, then dispatch on the leading
/// command verb.
fn parse_segment(segment: &str, out: &mut Vec<BashMutation>) {
    // Tokenize on ASCII whitespace (lexical — no quote-aware splitting; a quoted path
    // with spaces is handled by `strip_quotes` only when it is a single token, which is
    // the common `"a file"` → one token case after the shell would have parsed it; for
    // a heuristic we accept the limitation and still strip surrounding quotes).
    let tokens: Vec<&str> = segment.split_whitespace().collect();
    if tokens.is_empty() {
        return;
    }

    // Redirection targets (`>`/`>>`) can appear after any command; scan all tokens.
    collect_redirections(&tokens, out);

    // Strip leading `sudo` and `env VAR=val` prefixes to find the real command verb.
    let cmd_tokens = strip_prefixes(&tokens);
    let Some((&verb_tok, operands)) = cmd_tokens.split_first() else {
        return;
    };

    match verb_tok {
        "rm" => emit_operands(operands, "rm", out),
        "mkdir" => emit_operands(operands, "mkdir", out),
        "touch" => emit_operands(operands, "touch", out),
        "tee" => emit_operands(operands, "tee", out),
        "cp" => emit_last_operand(operands, "cp", out),
        "mv" => emit_mv(operands, out),
        "sed" => emit_sed(operands, out),
        "git" => emit_git(operands, out),
        _ => {}
    }
}

/// Drop leading `sudo` and `env VAR=value` prefix tokens (best-effort). `env` consumes
/// following `KEY=VALUE` tokens; the first non-`KEY=VALUE` token is the real command.
fn strip_prefixes<'a>(tokens: &[&'a str]) -> Vec<&'a str> {
    let mut idx = 0usize;
    loop {
        match tokens.get(idx) {
            Some(&"sudo") => idx += 1,
            Some(&"env") => {
                idx += 1;
                // Skip KEY=VALUE assignments that follow `env`.
                while matches!(tokens.get(idx), Some(t) if is_assignment(t)) {
                    idx += 1;
                }
            }
            _ => break,
        }
    }
    tokens[idx..].to_vec()
}

/// True for a `KEY=VALUE` env-assignment token (a name, then `=`, with no leading dash).
fn is_assignment(tok: &str) -> bool {
    if tok.starts_with('-') {
        return false;
    }
    match tok.split_once('=') {
        Some((name, _)) => {
            !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        }
        None => false,
    }
}

/// Emit every non-flag operand as a path with the given verb, skipping an input
/// redirect (`< file`) — its file is READ, not mutated.
fn emit_operands(operands: &[&str], verb: &'static str, out: &mut Vec<BashMutation>) {
    let kept = strip_input_redirects(operands);
    for op in &kept {
        if let Some(path) = path_operand(op) {
            out.push(BashMutation { path, verb });
        }
    }
}

/// Drop input-redirect operators (`<`, `<file`) AND the filename following a bare `<`
/// (an input file is READ, never mutated, so it must not be reported as a target).
fn strip_input_redirects<'a>(operands: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < operands.len() {
        let tok = operands[i];
        if tok == "<" {
            i += 2; // skip `<` and its (read-only) filename
        } else if tok.starts_with('<') {
            i += 1; // an attached `<file` input redirect
        } else {
            out.push(tok);
            i += 1;
        }
    }
    out
}

/// Emit only the LAST non-flag operand (the destination of `cp`).
fn emit_last_operand(operands: &[&str], verb: &'static str, out: &mut Vec<BashMutation>) {
    let kept = strip_input_redirects(operands);
    let paths: Vec<String> = kept.iter().filter_map(|o| path_operand(o)).collect();
    if let Some(dest) = paths.last() {
        out.push(BashMutation {
            path: dest.clone(),
            verb,
        });
    }
}

/// `mv` — the last operand is the destination (created/overwritten); earlier operands
/// are sources (moved-from). Emit the destination as `mv` and, with 2+ operands, each
/// source as `mv-from`.
fn emit_mv(operands: &[&str], out: &mut Vec<BashMutation>) {
    let kept = strip_input_redirects(operands);
    let paths: Vec<String> = kept.iter().filter_map(|o| path_operand(o)).collect();
    let Some((dest, sources)) = paths.split_last() else {
        return;
    };
    out.push(BashMutation {
        path: dest.clone(),
        verb: "mv",
    });
    for src in sources {
        out.push(BashMutation {
            path: src.clone(),
            verb: "mv-from",
        });
    }
}

/// `sed` mutates a file ONLY with an in-place flag (`-i`, `-i.bak`, `--in-place`); a
/// `sed` without it streams to stdout and changes no file → record nothing.
///
/// `sed [opts] '<script>' <file>…` — the FIRST non-option operand is the script (e.g.
/// `s/a/b/`), NOT a file, so it is skipped; every later non-option operand is a file.
/// (An `-e <expr>` / `-f <file>` flag would carry its script separately, but the bare
/// `sed -i s/a/b/ file` form is the dominant case; for a heuristic we handle that.)
fn emit_sed(operands: &[&str], out: &mut Vec<BashMutation>) {
    if !operands.iter().any(|t| is_sed_in_place_flag(t)) {
        return;
    }
    let kept = strip_input_redirects(operands);
    let mut seen_script = false;
    for op in &kept {
        if op.starts_with('-') {
            continue; // an option flag (incl. the in-place flag) — not the script/file.
        }
        if !seen_script {
            seen_script = true; // the first bare operand is the sed script, not a file.
            continue;
        }
        if let Some(path) = path_operand(op) {
            out.push(BashMutation {
                path,
                verb: "sed-i",
            });
        }
    }
}

/// True for a `sed` in-place flag: `--in-place`, or any `-i…` short flag (`-i`,
/// `-i.bak`, …). `-i<suffix>` is GNU sed's in-place-with-backup form.
fn is_sed_in_place_flag(tok: &str) -> bool {
    tok == "--in-place" || tok.starts_with("-i")
}

/// `git <sub> …` — record a single coarse `git:<sub>` pseudo-path ONLY when `<sub>` is
/// in the mutating allowlist. Git does not name a clean per-file list lexically, so we
/// never try to enumerate files; the entry is flagged heuristic like everything here.
fn emit_git(operands: &[&str], out: &mut Vec<BashMutation>) {
    if let Some(sub) = git_subcommand(operands) {
        if GIT_MUTATING.contains(&sub) {
            out.push(BashMutation {
                path: format!("git:{sub}"),
                verb: "git",
            });
        }
    }
}

/// The git SUBCOMMAND token, skipping leading global options. The value-taking global
/// flags `-c <key=val>` and `-C <path>` consume the FOLLOWING token, so it must not be
/// mistaken for the subcommand (the `git -c user.name=x commit` case).
fn git_subcommand<'a>(operands: &[&'a str]) -> Option<&'a str> {
    let mut i = 0usize;
    while i < operands.len() {
        let tok = operands[i];
        if tok == "-c" || tok == "-C" {
            i += 2; // skip the flag AND its value
        } else if tok.starts_with('-') {
            i += 1; // a bare option flag (e.g. `--no-pager`)
        } else {
            return Some(tok); // the first non-option token is the subcommand
        }
    }
    None
}

/// Collect `>`/`>>` redirection targets from a token list. Handles both the spaced
/// form (`cmd > file`) and the attached form (`cmd >file` / `cmd >>file`). The token
/// FOLLOWING a bare `>`/`>>`, or the suffix of an attached `>file`, is the written path.
fn collect_redirections(tokens: &[&str], out: &mut Vec<BashMutation>) {
    let mut i = 0usize;
    while i < tokens.len() {
        let tok = tokens[i];
        if tok == ">>" || tok == ">" {
            let verb = if tok == ">>" { ">>" } else { ">" };
            if let Some(next) = tokens.get(i + 1) {
                if let Some(path) = path_operand(next) {
                    out.push(BashMutation { path, verb });
                }
            }
            i += 1;
        } else if let Some(rest) = tok.strip_prefix(">>") {
            if let Some(path) = path_operand(rest) {
                out.push(BashMutation { path, verb: ">>" });
            }
            i += 1;
        } else if let Some(rest) = tok.strip_prefix('>') {
            if let Some(path) = path_operand(rest) {
                out.push(BashMutation { path, verb: ">" });
            }
            i += 1;
        } else {
            i += 1;
        }
    }
}

/// Normalize a single operand to a reported path, or `None` when it is not a path:
/// strip surrounding quotes; reject options (`-…`), `KEY=VALUE` operands, an empty
/// token, and a bare `-` (stdin/stdout). A glob we cannot expand (`*.tmp`) is KEPT
/// verbatim — it is still informative and the heuristic label makes that clear.
fn path_operand(token: &str) -> Option<String> {
    let stripped = strip_quotes(token);
    if stripped.is_empty() || stripped == "-" {
        return None;
    }
    if stripped.starts_with('-') {
        return None; // an option flag, not a path.
    }
    if is_assignment(stripped) {
        return None; // a KEY=VALUE operand, not a path.
    }
    Some(stripped.to_string())
}

/// Strip a single matched pair of surrounding single or double quotes.
fn strip_quotes(token: &str) -> &str {
    let b = token.as_bytes();
    if b.len() >= 2 {
        let first = b[0];
        let last = b[b.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &token[1..token.len() - 1];
        }
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(cmd: &str) -> Vec<(String, &'static str)> {
        parse_bash_mutations(cmd)
            .into_iter()
            .map(|m| (m.path, m.verb))
            .collect()
    }

    #[test]
    fn rm_each_non_flag_operand() {
        assert_eq!(
            paths("rm -rf /tmp/a /tmp/b"),
            vec![("/tmp/a".to_string(), "rm"), ("/tmp/b".to_string(), "rm")]
        );
    }

    #[test]
    fn mkdir_p_creates_each_dir() {
        assert_eq!(
            paths("mkdir -p /tmp/x /tmp/y"),
            vec![
                ("/tmp/x".to_string(), "mkdir"),
                ("/tmp/y".to_string(), "mkdir")
            ]
        );
    }

    #[test]
    fn touch_and_tee() {
        assert_eq!(paths("touch a.txt"), vec![("a.txt".to_string(), "touch")]);
        assert_eq!(paths("tee out.log"), vec![("out.log".to_string(), "tee")]);
    }

    #[test]
    fn cp_records_only_destination() {
        assert_eq!(
            paths("cp src.txt dst.txt"),
            vec![("dst.txt".to_string(), "cp")]
        );
        // With flags, the LAST non-flag operand is the destination.
        assert_eq!(
            paths("cp -r a/ b/ /dest/"),
            vec![("/dest/".to_string(), "cp")]
        );
    }

    #[test]
    fn mv_destination_and_sources() {
        // `mv a b c`: c is the destination (mv), a + b are moved-from sources (mv-from).
        assert_eq!(
            paths("mv a b c"),
            vec![
                ("c".to_string(), "mv"),
                ("a".to_string(), "mv-from"),
                ("b".to_string(), "mv-from")
            ]
        );
        // Two-operand mv: one destination, one source.
        assert_eq!(
            paths("mv old new"),
            vec![("new".to_string(), "mv"), ("old".to_string(), "mv-from")]
        );
        // Single operand (degenerate) → just a destination, no source.
        assert_eq!(paths("mv onlyone"), vec![("onlyone".to_string(), "mv")]);
    }

    #[test]
    fn sed_in_place_vs_streaming() {
        // sed WITHOUT -i mutates nothing.
        assert!(paths("sed s/a/b/ file.txt").is_empty());
        // sed -i mutates the file.
        assert_eq!(
            paths("sed -i s/a/b/ file.txt"),
            vec![("file.txt".to_string(), "sed-i")]
        );
        // sed -i.bak (a backup-suffix in-place flag).
        assert_eq!(
            paths("sed -i.bak s/a/b/ file.txt"),
            vec![("file.txt".to_string(), "sed-i")]
        );
        // sed --in-place long form.
        assert_eq!(
            paths("sed --in-place s/a/b/ file.txt"),
            vec![("file.txt".to_string(), "sed-i")]
        );
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
}
