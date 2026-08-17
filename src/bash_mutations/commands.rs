//! Per-command emitters: cp/mv/rm/touch/tee/ln/sed and operand shaping.

use super::*;

/// Parse one segment (with its parallel mask) — handle redirection targets anywhere, then
/// dispatch on the leading command verb.
pub(crate) fn parse_segment(segment: &str, mask: &str, out: &mut Vec<BashMutation>) {
    let toks = masked_tokens(segment, mask);
    if toks.is_empty() {
        return;
    }

    // Redirection targets (`>`/`>>`, incl. fd-qualified `2>`/`1>`/`&>`) can appear after
    // ANY command; scan all tokens (operator detection reads the MASK so an in-quote `>`
    // is invisible; the emitted path is sliced from the original). The returned index set
    // is every token CONSUMED as redirect syntax (the operator AND, for a bare `> file`
    // form, the following path token) so those tokens are dropped before verb dispatch —
    // symmetric to how `strip_input_redirects` removes `<`/`<file`. Without this drop a
    // surviving `2>&1` / `> /tmp/log` token poisons every positional-dest verb
    // (`cp`/`mv`/`ln`/`install`/`rsync`): it can BECOME the bogus `positional.last()` dest
    // (real dest dropped / source mislabeled) or double-emit the redirect path.
    let redirect_consumed = collect_redirections(&toks, out);

    // Output-path FLAGS (`--junit-xml=…`, `--report-path …`) can likewise appear under
    // any command, so scan every segment for the allowlisted output flags.
    collect_flag_outputs(&toks, out);

    // Drop pure-mask tokens (a process-sub body word like `foo` from `>(grep foo)`) AND the
    // redirect-syntax tokens recorded above BEFORE verb dispatch — neither is a real command
    // operand. The remaining tokens carry at least one unmasked byte, so their original text
    // is a genuine command/operand.
    let cmd_all: Vec<&str> = toks
        .iter()
        .enumerate()
        .filter(|(idx, t)| !is_fully_masked(t.masked) && !redirect_consumed.contains(idx))
        .map(|(_, t)| t.orig)
        .collect();

    // Strip leading `sudo` and `env VAR=val` prefixes to find the real command verb.
    let cmd_tokens = strip_prefixes(&cmd_all);
    let Some((&verb_tok, operands)) = cmd_tokens.split_first() else {
        return;
    };

    match verb_tok {
        "rm" => emit_operands(operands, "rm", out),
        "mkdir" => emit_operands(operands, "mkdir", out),
        "touch" => emit_touch(operands, out),
        "tee" => emit_tee(operands, out),
        "cp" => emit_copy_like(operands, "cp", out),
        "mv" => emit_mv(operands, out),
        "install" => emit_copy_like(operands, "install", out),
        "ln" => emit_ln(operands, out),
        "rsync" => emit_last_operand(operands, "rsync", out),
        "sed" => emit_sed(operands, out),
        "git" => emit_git(operands, out),
        "curl" => emit_download_output(operands, "curl", out),
        "wget" => emit_download_output(operands, "wget", out),
        "dd" => emit_dd(operands, out),
        "zip" => emit_zip(operands, out),
        "tar" => emit_tar(operands, out),
        _ => {}
    }
}

/// Drop leading `sudo` and `env VAR=value` prefix tokens (best-effort). `env` consumes
/// following `KEY=VALUE` tokens; the first non-`KEY=VALUE` token is the real command.
pub(crate) fn strip_prefixes<'a>(tokens: &[&'a str]) -> Vec<&'a str> {
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
pub(crate) fn is_assignment(tok: &str) -> bool {
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
pub(crate) fn emit_operands(operands: &[&str], verb: &'static str, out: &mut Vec<BashMutation>) {
    let kept = strip_input_redirects(operands);
    for op in &kept {
        if let Some(path) = path_operand(op) {
            out.push(BashMutation { path, verb });
        }
    }
}

/// `touch [opts] file…` — like [`emit_operands`] but with `touch`'s VALUE-taking flags
/// stripped first. `touch -d DATE`, `--date=DATE`, `-r REFFILE`, `--reference=REFFILE`,
/// and `-t STAMP` each consume a FOLLOWING token that is NOT a created path: `-r`'s value
/// is a READ-ONLY reference file, `-d`/`-t`'s value is a timestamp string. Without this,
/// `touch -r /ref/file out` would fabricate `/ref/file` as a created path (a phantom
/// mutation indistinguishable from a real one). `-t`'s pure-digit stamp is also caught by
/// `has_syntax_noise`'s number filter, but `-d`/`-r` values are not, so the skip is needed.
pub(crate) fn emit_touch(operands: &[&str], out: &mut Vec<BashMutation>) {
    /// Touch flags whose VALUE is the next token (not a created path).
    const VALUE_FLAGS: &[&str] = &["-d", "--date", "-r", "--reference", "-t"];
    let kept = strip_value_flags(strip_input_redirects(operands), VALUE_FLAGS);
    for op in &kept {
        if let Some(path) = path_operand(op) {
            out.push(BashMutation {
                path,
                verb: "touch",
            });
        }
    }
}

/// `tee [opts] file…` — every non-flag operand is a WRITTEN sink. The append form
/// (`tee -a` / `tee --append`) does NOT truncate (the file may pre-exist), mirroring
/// `>>` vs `>`; emit it under the `tee-a` verb so [`bash_verb_is_create`] maps it to
/// `is_create=false` (the truncating `tee` stays a create). tee has no value-taking
/// flag that consumes a following path (`-a`/`-i` are booleans), so no value-flag skip.
pub(crate) fn emit_tee(operands: &[&str], out: &mut Vec<BashMutation>) {
    let append = operands.iter().any(|t| *t == "-a" || *t == "--append");
    let verb = if append { "tee-a" } else { "tee" };
    let kept = strip_input_redirects(operands);
    for op in &kept {
        if let Some(path) = path_operand(op) {
            out.push(BashMutation { path, verb });
        }
    }
}

/// Drop each VALUE-taking flag token AND the token that follows it. A flag is matched in
/// two shapes: the spaced form (`-r ref` → drop both `-r` and `ref`) and the inline
/// `--flag=value` form (`--reference=ref` → drop the single token). Used so a flag's
/// argument is never mistaken for a positional path operand.
pub(crate) fn strip_value_flags<'a>(operands: Vec<&'a str>, value_flags: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < operands.len() {
        let tok = operands[i];
        if value_flags.contains(&tok) {
            i += 2; // skip the flag AND its value token
            continue;
        }
        // The `--flag=value` inline form: drop the whole token (value rides on it).
        if let Some((name, _)) = tok.split_once('=') {
            if name.starts_with("--") && value_flags.contains(&name) {
                i += 1;
                continue;
            }
        }
        out.push(tok);
        i += 1;
    }
    out
}

/// Drop input-redirect operators (`<`, `<file`) AND the filename following a bare `<`
/// (an input file is READ, never mutated, so it must not be reported as a target).
pub(crate) fn strip_input_redirects<'a>(operands: &[&'a str]) -> Vec<&'a str> {
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
pub(crate) fn non_flag_operands<'a>(operands: &[&'a str]) -> Vec<&'a str> {
    strip_input_redirects(operands)
        .into_iter()
        .filter(|t| !t.starts_with('-'))
        .collect()
}

/// Emit only the destination of a last-operand-dest verb (`ln`, `rsync`) — the LAST
/// positional operand. Validating that specific token (not "the last token that happens
/// to be a valid path") means a `… $DEST/x` whose real destination is an unexpandable
/// `$VAR` emits NOTHING, rather than wrongly reporting an earlier source.
pub(crate) fn emit_last_operand(
    operands: &[&str],
    verb: &'static str,
    out: &mut Vec<BashMutation>,
) {
    let positional = non_flag_operands(operands);
    if let Some(dest_tok) = positional.last() {
        if let Some(path) = path_operand(dest_tok) {
            out.push(BashMutation { path, verb });
        }
    }
}

/// `ln` destination resolution, GNU `-t DIR` / `--target-directory` aware (the same
/// inversion `cp`/`mv` have). The default form (`ln [-s] target… linkname`) writes the
/// LAST positional (the link). But `ln -t DIR target…` / `--target-directory DIR` puts
/// the destination DIRECTORY after `-t` with the link TARGETS last — so blindly taking
/// the last positional would wrongly report a read-only source target as the created link
/// AND miss the real destination dir. When `-t` is present we emit ITS value as the
/// destination and treat every positional as a (read) source; else the last-positional
/// default applies.
pub(crate) fn emit_ln(operands: &[&str], out: &mut Vec<BashMutation>) {
    if let Some(dir) = target_directory_value(operands) {
        if let Some(path) = path_operand(dir) {
            out.push(BashMutation { path, verb: "ln" });
        }
        return; // sources are reads; the `-t` DIR is the sole written destination.
    }
    emit_last_operand(operands, "ln", out);
}

/// `cp` / `install` destination resolution, GNU `-t DIR` aware. The default form
/// (`cp src… dest`) writes the LAST positional. But `cp -t DIR src…` / `--target-directory
/// DIR` puts the destination right after `-t` with the SOURCES last — so blindly taking
/// the last positional would wrongly report a read-only SOURCE as written. When `-t`/
/// `--target-directory` is present we emit ITS value as the destination and treat every
/// positional as a source (read). `-T`/`--no-target-directory` forces the plain
/// 2-operand semantics (last positional is the dest), which the default already does.
pub(crate) fn emit_copy_like(operands: &[&str], verb: &'static str, out: &mut Vec<BashMutation>) {
    if let Some(dir) = target_directory_value(operands) {
        if let Some(path) = path_operand(dir) {
            out.push(BashMutation { path, verb });
        }
        return; // sources are reads; the `-t` DIR is the sole written destination.
    }
    emit_last_operand(operands, verb, out);
}

/// The value of a GNU `-t DIR` / `--target-directory DIR` / `--target-directory=DIR`
/// destination-directory flag among `operands`, or `None` if absent. Used by cp / mv /
/// install where the flag inverts the usual "last positional is the destination" rule.
pub(crate) fn target_directory_value<'a>(operands: &[&'a str]) -> Option<&'a str> {
    let mut i = 0usize;
    while i < operands.len() {
        let tok = operands[i];
        if tok == "-t" || tok == "--target-directory" {
            return operands.get(i + 1).copied();
        }
        if let Some(v) = tok.strip_prefix("--target-directory=") {
            return Some(v);
        }
        i += 1;
    }
    None
}

/// `mv` — the LAST positional operand is the destination (created/overwritten); earlier
/// positionals are sources (moved-from). The destination is taken POSITIONALLY then
/// validated, so a dropped `$VAR` destination suppresses only the `mv` dest row (the
/// real source moves are still reported as `mv-from`).
pub(crate) fn emit_mv(operands: &[&str], out: &mut Vec<BashMutation>) {
    // GNU `mv -t DIR src…`: the destination is the `-t` value, every positional is a
    // source — the same inversion `cp -t` has. Without `-t` the last positional is the
    // destination and earlier positionals are sources.
    if let Some(dir) = target_directory_value(operands) {
        if let Some(path) = path_operand(dir) {
            out.push(BashMutation { path, verb: "mv" });
        }
        // The `-t DIR` value is also a non-flag positional; exclude it from the sources.
        for src in non_flag_operands(operands) {
            if src == dir {
                continue;
            }
            if let Some(path) = path_operand(src) {
                out.push(BashMutation {
                    path,
                    verb: "mv-from",
                });
            }
        }
        return;
    }
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

/// `sed` mutates a file ONLY with an in-place flag (`-i`, `-i.bak`, `--in-place`,
/// `--in-place=.bak`); a `sed` without it streams to stdout and changes no file → record
/// nothing.
///
/// `sed [opts] '<script>' <file>…` — the FIRST non-option operand is the script (e.g.
/// `s/a/b/`), NOT a file, so it is skipped; every later non-option operand is a file.
///
/// Three operand-split subtleties a naive "first bare operand is the script" loop gets
/// wrong, each fixed here:
/// - **explicit-script flags** (`-e <expr>`, `--expression=<expr>`, `-f <file>`,
///   `--file=<file>`): when ANY of these is present the script is carried by the flag, so
///   there is NO positional script — EVERY remaining bare operand is an edited file. The
///   flag VALUES are stripped first ([`strip_value_flags`]) so a multi-`-e` script
///   (`-e 's/a/b/' -e 's/c/d/'`) never leaks its 2nd-and-later expressions as phantom
///   files.
/// - **BSD `-i ''` empty suffix**: macOS/BSD sed spells in-place as `-i ''` where the
///   following `''` token is the (here empty) backup suffix, NOT the script. An
///   empty-after-quote-strip operand is therefore never the script nor a file → skipped,
///   so the REAL script no longer slides into the script slot and gets emitted as a file.
pub(crate) fn emit_sed(operands: &[&str], out: &mut Vec<BashMutation>) {
    if !operands.iter().any(|t| is_sed_in_place_flag(t)) {
        return;
    }
    /// sed flags whose VALUE is a SCRIPT (or a script FILE), never an edited file. Their
    /// presence also means there is no positional script operand.
    const SCRIPT_FLAGS: &[&str] = &["-e", "--expression", "-f", "--file"];
    let has_explicit_script = operands
        .iter()
        .any(|t| SCRIPT_FLAGS.contains(t) || script_flag_inline(t));
    let kept = strip_value_flags(strip_input_redirects(operands), SCRIPT_FLAGS);
    // With an explicit `-e`/`-f` script the first bare operand is already a file (no
    // positional script to skip); otherwise the first bare operand IS the script.
    let mut seen_script = has_explicit_script;
    for op in &kept {
        if op.starts_with('-') {
            continue; // an option flag (incl. the in-place flag) — not the script/file.
        }
        if strip_quotes(op).is_empty() {
            continue; // a BSD `-i ''` empty backup-suffix token — not script nor file.
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

/// True for a sed inline `--expression=…` / `--file=…` script flag (the spaced `-e`/`-f`
/// and exact `--expression`/`--file` tokens are matched directly by [`emit_sed`]).
pub(crate) fn script_flag_inline(tok: &str) -> bool {
    matches!(tok.split_once('='), Some((name, _)) if name == "--expression" || name == "--file")
}

/// True for a `sed` in-place flag: `--in-place`, the GNU `--in-place=SUFFIX` backup form,
/// or any `-i…` short flag (`-i`, `-i.bak`, …). `-i<suffix>` / `--in-place=<suffix>` are
/// sed's in-place-with-backup forms.
pub(crate) fn is_sed_in_place_flag(tok: &str) -> bool {
    tok == "--in-place" || tok.starts_with("--in-place=") || tok.starts_with("-i")
}
