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
//!
//! ## What it catches (recall)
//!
//! Beyond the verb allowlist (`rm`/`mkdir`/`touch`/`tee`/`cp`/`mv`/`sed -i`/`git`) and
//! plain `>`/`>>` redirects, it also reads:
//! - **fd-qualified redirects** — `2>`/`1>`/`&>` (+ `>>` forms), attached (`2>/tmp/x`)
//!   and spaced (`2> /tmp/x`); a bare `2>&1` fd-dup and `/dev/null|stderr|stdout` sinks
//!   are NOT paths and are skipped.
//! - **`curl`/`wget` output flags** — `-o <path>` / `--output <path>` / `--output=…`
//!   (and `wget -O <path>`). A `curl -O` that derives the name from the URL has no
//!   deterministic local path and is skipped.
//! - **flag-specified outputs** — `--<name>=<path>` / `--<name> <path>` for a small
//!   allowlist (`junit-xml`, `junitxml`, `report-path`, `output`, `out-file`, …),
//!   `dd of=<path>`, and a `zip <dest>` archive.
//!
//! ## Precision contract (no fabricated rows)
//!
//! Only a **concrete, resolvable** path is ever emitted. A token that does not name a
//! real path is DROPPED, never surfaced as a noisy pseudo-row: an unresolved `$VAR` /
//! `${VAR}` / `~`-or-`$()`-bearing token (we cannot expand it, so a row would be a
//! fabricated path), a `/dev/null`-class sink, and a mis-parsed redirect tail all yield
//! nothing. (Globs like `*.tmp` remain the one informative non-concrete exception, kept
//! verbatim, because they still name a real touched set and the heuristic label is
//! explicit.) The `git:<sub>` coarse pseudo-path is intentional and unaffected.
//!
//! ## Out of scope (documented limitation)
//!
//! Write calls inside an EMBEDDED-LANGUAGE body are NOT parsed — a heredoc
//! (`python3 - <<'PY' … PY`), an inline `python3 -c "open('/tmp/x','w')…"`, a `Path(…)
//! .write_text(…)`, etc. This is a deliberate limit of a lexical (non-shell, non-Python)
//! parser: the body is opaque command TEXT, and reliably parsing arbitrary embedded code
//! is out of scope. Such writes are missed (a recall gap), but the precision contract
//! above guarantees they never produce a WRONG row.

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

    // Redirection targets (`>`/`>>`, incl. fd-qualified `2>`/`1>`/`&>`) can appear after
    // ANY command; scan all tokens.
    collect_redirections(&tokens, out);

    // Output-path FLAGS (`--junit-xml=…`, `--report-path …`) can likewise appear under
    // any command, so scan every segment for the allowlisted output flags.
    collect_flag_outputs(&tokens, out);

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
        "curl" => emit_download_output(operands, "curl", out),
        "wget" => emit_download_output(operands, "wget", out),
        "dd" => emit_dd(operands, out),
        "zip" => emit_zip(operands, out),
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

/// The positional non-flag, non-input-redirect operand TOKENS, in order. Unlike a
/// `filter_map(path_operand)` collapse, this preserves POSITION across a token that
/// `path_operand` will later drop (a `$VAR` pseudo-path), so `cp`/`mv` can identify the
/// destination POSITIONALLY (the last token) and validate THAT token — instead of
/// silently promoting an earlier source to "destination" when the real dest is dropped.
fn non_flag_operands<'a>(operands: &[&'a str]) -> Vec<&'a str> {
    strip_input_redirects(operands)
        .into_iter()
        .filter(|t| !t.starts_with('-'))
        .collect()
}

/// Emit only the destination of `cp` — the LAST positional operand. Validating that
/// specific token (not "the last token that happens to be a valid path") means a `cp src
/// $DEST/x` whose real destination is an unexpandable `$VAR` emits NOTHING, rather than
/// wrongly reporting `src` as the destination.
fn emit_last_operand(operands: &[&str], verb: &'static str, out: &mut Vec<BashMutation>) {
    let positional = non_flag_operands(operands);
    if let Some(dest_tok) = positional.last() {
        if let Some(path) = path_operand(dest_tok) {
            out.push(BashMutation { path, verb });
        }
    }
}

/// `mv` — the LAST positional operand is the destination (created/overwritten); earlier
/// positionals are sources (moved-from). The destination is taken POSITIONALLY then
/// validated, so a dropped `$VAR` destination suppresses only the `mv` dest row (the
/// real source moves are still reported as `mv-from`).
fn emit_mv(operands: &[&str], out: &mut Vec<BashMutation>) {
    let positional = non_flag_operands(operands);
    let Some((dest_tok, source_toks)) = positional.split_last() else {
        return;
    };
    if let Some(path) = path_operand(dest_tok) {
        out.push(BashMutation { path, verb: "mv" });
    }
    for src in source_toks {
        if let Some(path) = path_operand(src) {
            out.push(BashMutation {
                path,
                verb: "mv-from",
            });
        }
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

/// The output-flag NAMES (without the leading `--`) whose value is a written path. Kept
/// CONSERVATIVE so a flag whose value is NOT a path (a number, a format name) is never
/// misread as a creation. Matched both as `--name=<path>` and `--name <path>`.
const OUTPUT_FLAGS: &[&str] = &[
    "junit-xml",
    "junitxml",
    "report-path",
    "output",
    "out-file",
    "outfile",
    "out",
    "logfile",
    "log-file",
];

/// `curl`/`wget` write to a LOCAL path only via an explicit output flag: `-o <path>`,
/// `--output <path>`/`--output=<path>` (curl + wget), or `wget -O <path>`. A `curl -O`
/// (uppercase, no path arg — the name is derived from the URL) has NO deterministic
/// local path, so it is intentionally skipped. The destination is emitted under the
/// download verb (`curl`/`wget`), is_create true.
fn emit_download_output(operands: &[&str], verb: &'static str, out: &mut Vec<BashMutation>) {
    // The long `--output` / `--output=` forms are owned by the generic
    // [`collect_flag_outputs`] scan (which runs on EVERY segment, `output` is in its
    // allowlist), so this arm handles ONLY the SHORT flags the generic `--name` scan
    // cannot see: curl `-o <path>`, and wget `-O <path>` / `--output-document <path>`.
    // (curl's `-O` derives the name from the URL → no deterministic local path → skip.)
    let mut i = 0usize;
    while i < operands.len() {
        let tok = operands[i];
        let takes_next =
            tok == "-o" || (verb == "wget" && matches!(tok, "-O" | "--output-document"));
        if takes_next {
            if let Some(next) = operands.get(i + 1) {
                if let Some(path) = path_operand(next) {
                    out.push(BashMutation { path, verb });
                }
                i += 2;
                continue;
            }
        }
        i += 1;
    }
}

/// `dd … of=<path>` — the output file is named by the `of=` operand. `path_operand`
/// rejects a `KEY=VALUE` token, so `of=` is parsed specially here (its value after the
/// `=` is the path). `of=/dev/null`-class sinks are dropped.
fn emit_dd(operands: &[&str], out: &mut Vec<BashMutation>) {
    for op in operands {
        if let Some(rest) = strip_quotes(op).strip_prefix("of=") {
            if is_dev_sink(rest) {
                continue;
            }
            // `rest` is a bare path (no KEY=VALUE wrapper now) — concrete-path filter.
            if let Some(path) = concrete_path(rest) {
                out.push(BashMutation { path, verb: "dd" });
            }
        }
    }
}

/// `zip [opts] <dest.zip> <input…>` — the FIRST non-flag operand is the archive being
/// created/updated (the only path `zip` writes). Later operands are inputs (read), so
/// only the destination is emitted.
fn emit_zip(operands: &[&str], out: &mut Vec<BashMutation>) {
    for op in operands {
        if op.starts_with('-') {
            continue; // a zip option flag (`-r`, `-9`, …).
        }
        if let Some(path) = path_operand(op) {
            out.push(BashMutation { path, verb: "zip" });
        }
        return; // only the first non-flag operand (the archive dest).
    }
}

/// Scan every token for an allowlisted output FLAG and emit its path. Two shapes:
/// `--name=<path>` (inline) and `--name <path>` (the value is the next token). Only the
/// [`OUTPUT_FLAGS`] names qualify, keeping precision tight. Independent of the segment's
/// leading verb (a test runner names its report path the same way regardless of verb).
fn collect_flag_outputs(tokens: &[&str], out: &mut Vec<BashMutation>) {
    let mut i = 0usize;
    while i < tokens.len() {
        let tok = tokens[i];
        if let Some(name_eq) = tok.strip_prefix("--") {
            if let Some((name, value)) = name_eq.split_once('=') {
                // `--name=<path>` inline form.
                if OUTPUT_FLAGS.contains(&name) {
                    if let Some(path) = concrete_path(value) {
                        out.push(BashMutation {
                            path,
                            verb: "flag-output",
                        });
                    }
                }
                i += 1;
                continue;
            }
            // `--name <path>` spaced form: the value is the NEXT token — but only when
            // that token is NOT itself a flag (so `--output --verbose` does not consume
            // `--verbose` as a fabricated path and skip a real flag).
            if OUTPUT_FLAGS.contains(&name_eq) {
                if let Some(next) = tokens.get(i + 1) {
                    if !next.starts_with('-') {
                        if let Some(path) = concrete_path(next) {
                            out.push(BashMutation {
                                path,
                                verb: "flag-output",
                            });
                        }
                        i += 2;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
}

/// Collect redirection targets from a token list. Handles the plain (`>`/`>>`) AND the
/// fd-qualified forms (`2>`, `1>`, `&>`, and their `>>` appends), in both the spaced
/// (`cmd 2> file`) and attached (`cmd 2>file`) shapes. The token FOLLOWING a bare
/// operator, or the suffix of an attached `OP file`, is the written path.
///
/// A token's optional leading fd qualifier — `&` or a run of ASCII digits — is stripped
/// BEFORE the `>`/`>>` test, so `2>`, `1>`, `12>`, `&>` all match. A bare `2>&1`-style
/// fd-DUP (the redirect target is another fd, not a path) and the `/dev/null`-class
/// sinks carry no real path and emit nothing (handled by [`path_operand`] +
/// [`is_dev_sink`]).
fn collect_redirections(tokens: &[&str], out: &mut Vec<BashMutation>) {
    let mut i = 0usize;
    while i < tokens.len() {
        let tok = tokens[i];
        // Peel an optional leading fd qualifier (`&` or digits) to expose a `>`/`>>`.
        let body = strip_fd_qualifier(tok);
        if body == ">>" || body == ">" {
            // A bare (possibly fd-qualified) operator: its path is the NEXT token.
            let verb = if body == ">>" { ">>" } else { ">" };
            if let Some(next) = tokens.get(i + 1) {
                push_redirect_target(next, verb, out);
            }
            i += 1;
        } else if let Some(rest) = body.strip_prefix(">>") {
            // An attached append `…>>file` (incl. `2>>file`); `rest` is the path.
            push_redirect_target(rest, ">>", out);
            i += 1;
        } else if let Some(rest) = body.strip_prefix('>') {
            // An attached truncate `…>file` (incl. `2>file`, `&>file`); `rest` is the
            // path. A bare `2>&1` fd-dup leaves `rest = "&1"`, which is not a path.
            push_redirect_target(rest, ">", out);
            i += 1;
        } else {
            i += 1;
        }
    }
}

/// Strip an optional leading fd qualifier (`&`, or a run of ASCII digits) from a token,
/// exposing the bare redirect operator/body. `2>` → `>`, `1>>` → `>>`, `&>` → `>`,
/// `12>file` → `>file`; a token with no such prefix (or `>`/`>>` itself) is returned
/// unchanged. Only strips when a `>` actually follows the qualifier, so a plain numeric
/// or `&`-leading token that is NOT a redirect is left intact.
fn strip_fd_qualifier(tok: &str) -> &str {
    let bytes = tok.as_bytes();
    let mut k = 0usize;
    if bytes.first() == Some(&b'&') {
        k = 1;
    } else {
        while k < bytes.len() && bytes[k].is_ascii_digit() {
            k += 1;
        }
    }
    // Only treat the prefix as an fd qualifier if a redirect operator follows it AND we
    // actually consumed something (k>0). `&1`, `2` alone, etc. stay untouched.
    if k > 0 && bytes.get(k) == Some(&b'>') {
        &tok[k..]
    } else {
        tok
    }
}

/// Emit a redirect target if it resolves to a concrete path — dropping a `/dev/null`-
/// class sink and any fd-dup remnant (`&1`) that `path_operand` rejects.
fn push_redirect_target(tail: &str, verb: &'static str, out: &mut Vec<BashMutation>) {
    if is_dev_sink(tail) || tail.starts_with('&') {
        return; // a discard sink or an fd-dup (`>&1`/`2>&1`): no real path written.
    }
    if let Some(path) = path_operand(tail) {
        out.push(BashMutation { path, verb });
    }
}

/// True for a redirect sink that is NOT a real created file: `/dev/null`, `/dev/stderr`,
/// `/dev/stdout` (and their quote-stripped forms). These are ubiquitous noise targets.
fn is_dev_sink(tail: &str) -> bool {
    matches!(
        strip_quotes(tail),
        "/dev/null" | "/dev/stderr" | "/dev/stdout"
    )
}

/// Normalize a single operand to a reported path, or `None` when it is not a path:
/// strip surrounding quotes; reject options (`-…`), `KEY=VALUE` operands, an empty
/// token, a bare `-` (stdin/stdout), and an UNRESOLVED-variable pseudo-path (any token
/// bearing a `$`, e.g. `$OUT`, `${DIR}/x`, `/tmp/$run.log` — we cannot expand it, so a
/// row would fabricate a path; precision rule, dropped). A glob we cannot expand
/// (`*.tmp`) is KEPT verbatim — it still names a real touched set and the heuristic
/// label makes that clear.
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
    if has_unresolved_var(stripped) {
        return None; // an unexpandable `$VAR` pseudo-path — never fabricate it.
    }
    Some(stripped.to_string())
}

/// Stricter sibling of [`path_operand`] for the precision-sensitive NEW emitters
/// (`--flag=<path>`, `dd of=`, `curl -o`): in addition to every [`path_operand`]
/// rejection, it ALSO drops a glob (`*`/`?`/`[`) — those output paths are written by a
/// single tool to ONE concrete destination, so a wildcard there is a parse artifact,
/// not a real touched set. Returns only a concrete, resolvable path.
fn concrete_path(token: &str) -> Option<String> {
    let path = path_operand(token)?;
    if path.contains(['*', '?', '[']) {
        return None; // a glob is not a concrete single destination here.
    }
    Some(path)
}

/// True when a token carries an UNRESOLVED shell variable reference (`$NAME`,
/// `${NAME}`, `$1`, …) we cannot expand. Such a token can never be turned into a real
/// path without the runtime environment, so emitting it would fabricate a pseudo-path.
fn has_unresolved_var(token: &str) -> bool {
    token.contains('$')
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

    // ────────────────────────────────────────────────────────────────────────────
    // Regression oracle: the synthetic IDIOM MATRIX from the files-attribution
    // verdict (csift-files-attribution-verdict.md). Every idiom the verdict marked
    // CAUGHT must stay caught; every idiom it marked MISSED (Fixes A–C) must now be
    // caught; the precision cases (Fix D) must stay DROPPED.
    // ────────────────────────────────────────────────────────────────────────────

    /// Convenience: the set of just the PATHS a command yields (verb-agnostic), for
    /// idiom tests that only care that the destination surfaced.
    fn just_paths(cmd: &str) -> Vec<String> {
        parse_bash_mutations(cmd)
            .into_iter()
            .map(|m| m.path)
            .collect()
    }

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
}
