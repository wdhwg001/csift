//! The LEXICAL classifier of Claude Code generation 3 - `out`, which replaced
//! `hnt` at 2.1.261 (the new target regex's literal is absent at 2.1.260 and
//! present from 2.1.261 on). Offsets below are into the 2.1.268 build, which is
//! the newest one held here; they are NOT byte-checkable against the 2.1.258 build
//! the ledger anchors read.
//!
//! What changed, all of it visible in what the classifier can now reach:
//! - the `rm` guard is case-insensitive;
//! - private-use sentinels are stripped from the input up front;
//! - the clause-head regex is GONE, replaced by a token walk that skips shell
//!   keywords, assignments, `$`-led tokens, redirections and a seventeen-name
//!   prefix-command set - so `do rm`, `then rm`, `sudo rm` and `xargs rm` all now
//!   reach the target test, which is exactly the gap the old port hand-patched;
//! - `find -exec` / `-ok` / `-execdir` / `-okdir` are scanned, at most eight per
//!   clause, terminated by the find-predicate set;
//! - the target regex is widened (the post-slash set gains `?` `[` `{`, a
//!   `${VAR:-default}` form is accepted, an optional backslash may precede the
//!   slash) and a SECOND, positional form (`$1 $@ $* $!`) applies only when the
//!   clause carries no function definition and no `set --`;
//! - the paren fixpoint replaces with a sentinel instead of a space;
//! - a nested `sh -c '...'` / `eval` / backtick script is recursed into, depth 2.
//!
//! The nested recursion reads the script `iLo` @166795515 REBUILDS, never a slice
//! of the clause: the shield has already replaced the script's separators, so the
//! runs are reassembled and unmasked (@166795979) first, and the allowance the
//! positional target form runs under is derived from the script's own tail
//! (@166796336). One divergence in this neighbourhood is DECLINED, because it is
//! nothing the nested port needs and nothing the contract asked for: `MIn`
//! @166793250, the blanking mask the top-level function scan reads, decides a
//! backslash with `RNe` and emits two blanks where it blanks an escape pair, while
//! `mask` below treats every backslash outside a single quote as an escape and
//! emits one character per character. It is recorded here so the next pass over
//! this file starts from a known list rather than a rediscovery.

use crate::bash_danger_lexical::{
    amp_to_semicolon, escapes_next, mask, mask_specials, open_quote, paren_fixpoint,
    resolve_dquote_escapes, strip_backticks, unmask, CLAUSE_SPLIT, ESCAPED_SPACE, KCT, REDIR_OP,
    REDIR_START, VCT,
};
use regex::Regex;
use std::sync::LazyLock;

/// `JNo` @166791437 - the nested-shell recursion cap.
const NESTED_DEPTH: usize = 2;
/// The per-clause cap on scanned `find -exec` occurrences.
const FIND_EXEC_CAP: usize = 8;

/// `KNo` @166790679 - the named-variable target.
static TARGET_NAMED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r#"^[\u{E020}"']*\$(?:\{[A-Za-z_][A-Za-z0-9_]*(?::?-(?:["']{2}|"?\$\{?"#,
        r#"[A-Za-z_][A-Za-z0-9_]*\}?"?)?)?\}|[A-Za-z_][A-Za-z0-9_]*)"#,
        r#"[\u{E020}"']*\\?/(?:[*?\[{]|\$|/|["']|\u{E020}|$)"#
    ))
    .expect("KNo")
});

/// `YNo` @166790679 - the positional / special-parameter target.
static TARGET_POSITIONAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r#"^[\u{E020}"']*\$(?:\{(?:[0-9]+|[@*!])(?::?-(?:["']{2}|"?\$\{?"#,
        r#"[A-Za-z_][A-Za-z0-9_]*\}?"?)?)?\}|[0-9@*!])"#,
        r#"[\u{E020}"']*\\?/(?:[*?\[{]|\$|/|["']|\u{E020}|$)"#
    ))
    .expect("YNo")
});

/// `XNo` @166791026 - a function definition anywhere in the command.
static FUNCTION_SCAN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:^|[;&|(\n])[ \t]*(?:function[ \t]+[A-Za-z_][A-Za-z0-9_]*|[A-Za-z_][A-Za-z0-9_]*[ \t]*\([ \t]*\))",
    )
    .expect("XNo")
});
/// `QNo` @166791132 - a `set --` that assigns real positional parameters. The
/// JavaScript uses a negative lookahead the `regex` crate has not got, so the
/// exclusion is applied in code.
static SET_DASHDASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[;&|(\n])[ \t]*set[ \t]+--[ \t]+([^\s;&|])").expect("QNo"));
/// `nLo` @166791677 - the find predicates that terminate an `-exec` argument run.
static FIND_PREDICATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^-(?:exec|execdir|ok|okdir|name|iname|path|ipath|regex|prune|type|maxdepth|mindepth|newer|mtime|mmin|size|print0?|delete|o|a|not|and|or)$",
    )
    .expect("nLo")
});
/// The `-exec` / `-ok` / `-execdir` / `-okdir` introducers.
static FIND_EXEC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)-(?:exec|ok)(?:dir)?\s+").expect("find_exec"));
/// The entry guard, case-INSENSITIVE at this generation.
static RM_WORD_CI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\brm(?:dir)?\b").expect("rm_word_ci"));
/// `ZNo` @166791437 - a nested shell invoked with a `-c` flag and a quoted script.
static NESTED_SHELL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)(?:^|[\s;&|(])(?:[^\s=]*/)?(?:busybox\s+)?",
        r"(?:bash|sh|zsh|dash|ksh|mksh|ash|hush|rbash)\s+",
        r"(?:(?:--?|\+)[A-Za-z][A-Za-z0-9_-]*(?:\s+[A-Za-z0-9_][A-Za-z0-9_-]*)?\s+)*",
        r#"-[A-Za-z]*c[A-Za-z]*\s+(?:--\s+)?\$?(["'])"#
    ))
    .expect("ZNo")
});

/// `eLo` @166791437 - the shell keywords the token walk skips.
const KEYWORDS: &[&str] = &[
    "do", "then", "else", "elif", "if", "while", "until", "!", "{", "coproc",
];
/// `tLo` @166791437 - the prefix commands the token walk skips, which is where
/// `sudo rm` and `xargs rm` enter.
const PREFIXES: &[&str] = &[
    "sudo", "doas", "exec", "command", "env", "nice", "nohup", "busybox", "time", "timeout",
    "stdbuf", "setsid", "ionice", "pkexec", "runuser", "eval", "xargs",
];

/// `/^[A-Za-z_][A-Za-z0-9_]*\+?=/` - an assignment prefix.
static ASSIGNMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*\+?=").expect("assignment"));
/// @166796336 - a redirection and its operand, taken out of the nested script's
/// tail before its remaining words are counted.
static TAIL_REDIRECTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s)[\d&]*(?:>>?[|&]?|<<?<?|<>|&>)(?:\S+|\s+\S+)").expect("tail_redirection")
});
/// @166796336 - the find terminator dropped from the end of that same tail.
static TAIL_TERMINATOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*(?:\\;|\\|\+)\s*$").expect("tail_terminator"));

/// What `out` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutHit {
    pub command: &'static str,
    pub target: String,
}

/// `out(command)` @166793748 - two passes over the statements, the first
/// quote-aware and carrying the positional-form flag, the second raw without it.
pub(crate) fn out(command: &str) -> Option<OutHit> {
    if !command.contains('$') || !RM_WORD_CI.is_match(command) {
        return None;
    }
    let cleaned: String = command
        .chars()
        .filter(|c| !('\u{E000}'..='\u{F8FF}').contains(c))
        .collect();
    let positional_ok =
        !FUNCTION_SCAN.is_match(&mask(&cleaned, true)) && !has_set_dashdash(&cleaned);
    if let Some(hit) = clause_pass(&cleaned, positional_ok, 0, true, false) {
        return Some(hit);
    }
    clause_pass(&cleaned, false, 0, false, false)
}

/// `QNo` with its negative lookahead applied in code: a `set --` whose first
/// argument is an empty quoted string or a `$`-led word does not count.
fn has_set_dashdash(s: &str) -> bool {
    let masked = mask(s, false);
    for c in SET_DASHDASH.captures_iter(&masked) {
        let Some(m) = c.get(1) else { continue };
        let rest = &masked[m.start()..];
        if rest.starts_with("''") || rest.starts_with("\"\"") {
            let after = rest[2..].chars().next();
            if after.is_none()
                || matches!(after, Some(c) if c.is_whitespace() || c == ';' || c == '&' || c == '|')
            {
                continue;
            }
        }
        if rest.starts_with('$') || rest.starts_with("'$") || rest.starts_with("\"$") {
            continue;
        }
        return true;
    }
    false
}

/// `Xct` @166794... - one pass over the statements of a script.
fn clause_pass(
    text: &str,
    positional_ok: bool,
    depth: usize,
    quote_aware: bool,
    in_dquote: bool,
) -> Option<OutHit> {
    let p = text.replace("\\\r\n", " ").replace("\\\n", " ");
    let masked = if quote_aware { mask_specials(&p) } else { p };
    let mut s = strip_backticks(&masked);
    s = s.trim_start().to_string();
    while s.starts_with('(') || s.starts_with('{') {
        s = s[1..].trim_start().to_string();
    }
    s = paren_fixpoint(&s);
    s = amp_to_semicolon(&s);
    for clause in CLAUSE_SPLIT.split(&s) {
        let c = clause.trim_start_matches(|ch: char| ch.is_whitespace() || ch == VCT);
        let mut candidates: Vec<Invocation> = Vec::new();
        if let Some(inv) = token_walk(c) {
            candidates.push(inv);
        }
        for m in FIND_EXEC.find_iter(c) {
            if candidates.len() >= FIND_EXEC_CAP {
                break;
            }
            if let Some(inv) = token_walk(&c[m.end()..]) {
                candidates.push(truncate_at_find_predicate(inv));
            }
        }
        for inv in &candidates {
            if let Some(hit) = operand_walk(inv, positional_ok, in_dquote) {
                return Some(hit);
            }
        }
        if quote_aware && depth < NESTED_DEPTH {
            if let Some(hit) = nested_shell(c, depth + 1, positional_ok || depth == 0) {
                return Some(hit);
            }
        }
    }
    None
}

/// One `rm`/`rmdir` invocation the token walk found, with the tokens after it.
#[derive(Debug, Clone)]
struct Invocation {
    command: &'static str,
    tokens: Vec<String>,
}

/// `ENe` @166791677 - is this token an `rm`/`rmdir` verb, after dropping a
/// redirection tail, a leading backslash and a path prefix, case-folded?
fn verb_of(token: &str) -> Option<&'static str> {
    let no_redir = token.split(['<', '>']).next().unwrap_or(token);
    let no_escape = no_redir.strip_prefix('\\').unwrap_or(no_redir);
    let base = match no_escape.rfind('/') {
        Some(i) if !no_escape[..i].contains(char::is_whitespace) => &no_escape[i + 1..],
        _ => no_escape,
    };
    match base.to_ascii_lowercase().as_str() {
        "rm" => Some("rm"),
        "rmdir" => Some("rmdir"),
        _ => None,
    }
}

fn is_prefix_command(token: &str) -> bool {
    let no_escape = token.strip_prefix('\\').unwrap_or(token);
    let base = match no_escape.rfind('/') {
        Some(i) if !no_escape[..i].contains(char::is_whitespace) => &no_escape[i + 1..],
        _ => no_escape,
    };
    PREFIXES.contains(&base.to_ascii_lowercase().as_str())
}

/// `EIn` @166793036 - walk a clause's tokens to the removal verb, skipping shell
/// keywords, assignments, `$`-led words, redirections and prefix commands.
fn token_walk(clause: &str) -> Option<Invocation> {
    let n: Vec<&str> = clause.split_whitespace().collect();
    let mut r = 0usize;
    while r < n.len() {
        let p = n[r];
        if verb_of(p).is_some() {
            break;
        }
        if KEYWORDS.contains(&p.to_ascii_lowercase().as_str())
            || ASSIGNMENT.is_match(p)
            || p.starts_with('$')
            || p.starts_with(VCT)
        {
            r += 1;
        } else if REDIR_START.is_match(p) {
            r += 1;
            if REDIR_OP.is_match(p) {
                r += 1;
            }
        } else {
            break;
        }
    }
    while r < n.len() && is_prefix_command(n[r]) {
        r += 1;
        while r < n.len() {
            let p = n[r];
            if verb_of(p).is_some() || is_prefix_command(p) {
                break;
            }
            if REDIR_START.is_match(p) {
                r += 1;
                if REDIR_OP.is_match(p) {
                    r += 1;
                }
                continue;
            }
            if p.starts_with('-') {
                r += 1;
                if let Some(next) = n.get(r) {
                    if !next.starts_with('-')
                        && !next.contains('=')
                        && verb_of(next).is_none()
                        && !is_prefix_command(next)
                    {
                        r += 1;
                    }
                }
                continue;
            }
            if ASSIGNMENT.is_match(p)
                || p.starts_with(|c: char| c.is_ascii_digit())
                || p.starts_with('$')
                || p.starts_with(VCT)
            {
                r += 1;
                continue;
            }
            break;
        }
    }
    let command = verb_of(n.get(r)?)?;
    Some(Invocation {
        command,
        tokens: n[r + 1..].iter().map(|t| (*t).to_string()).collect(),
    })
}

/// The `-exec` argument run ends at the first `\;`, `;`, `+` after `{}`, or an
/// unshielded find predicate.
fn truncate_at_find_predicate(mut inv: Invocation) -> Invocation {
    let mut saw_dashdash = false;
    let mut cut: Option<usize> = None;
    for (i, tok) in inv.tokens.iter().enumerate() {
        let t = unmask(&tok.replace(['\'', '"'], ""));
        if t == "--" {
            saw_dashdash = true;
        }
        let prev_brace = i
            .checked_sub(1)
            .and_then(|p| inv.tokens.get(p))
            .map(|p| unmask(&p.replace(['\'', '"'], "")) == "{}")
            .unwrap_or(false);
        if t == "\\;" || t == ";" || t == "\\" || (t == "+" && prev_brace) {
            cut = Some(i);
            break;
        }
        if !saw_dashdash && FIND_PREDICATE.is_match(&t) {
            cut = Some(i);
            break;
        }
    }
    if let Some(i) = cut {
        inv.tokens.truncate(i);
    }
    inv
}

/// `rLo` @166794... - the operand walk and the two target tests.
fn operand_walk(inv: &Invocation, positional_ok: bool, in_dquote: bool) -> Option<OutHit> {
    let mut p = 0usize;
    while p < inv.tokens.len() {
        let t = inv.tokens[p].trim_end_matches([')', ']', '}']);
        if t.is_empty() || t.starts_with('-') {
            p += 1;
            continue;
        }
        // The skip is gated on whether this script came out of a DOUBLE-quoted
        // nested shell, not on the pass: both top-level passes carry the flag
        // false, so both skip a token that opens a single quote and ends on `$`.
        if !in_dquote && t.starts_with('\'') && !t[1..].contains('\'') && t.ends_with('$') {
            p += 1;
            continue;
        }
        if REDIR_START.is_match(t) {
            if REDIR_OP.is_match(t) {
                p += 1;
            }
            p += 1;
            continue;
        }
        if TARGET_NAMED.is_match(t) || (positional_ok && TARGET_POSITIONAL.is_match(t)) {
            return Some(OutHit {
                command: inv.command,
                target: unmask(t),
            });
        }
        p += 1;
    }
    None
}

/// `iLo` @166795515 - recurse into a nested `sh -c '<script>'`.
///
/// The script is NOT a slice of the clause. `sLo` shielded every separator inside
/// the quotes, so the clause carries stand-ins where the script's spaces were, and
/// a slice handed to the walk arrives as one token. `iLo` rebuilds the script from
/// its quoted and unquoted runs, resolves the double-quoted escapes, and UNMASKS
/// the result (@166795979) before recursing.
///
/// The recursion's positional allowance is DERIVED (@166796336), not inherited:
/// arguments after the script, or an `xargs` ahead of it, supply `$1`, so the
/// positional form is not dangerous there. Only the double-quoted arm consults the
/// caller's allowance at all.
fn nested_shell(clause: &str, depth: usize, inherited_positional: bool) -> Option<OutHit> {
    let chars: Vec<char> = clause.chars().collect();
    let balanced = open_quote(&chars, chars.len()).is_none();
    for m in NESTED_SHELL.find_iter(clause) {
        // A head that starts inside a quoted run of a balanced clause is text, not
        // an invocation.
        if balanced && open_quote(&chars, clause[..m.start()].chars().count()).is_some() {
            continue;
        }
        let quote = clause[m.start()..m.end()].chars().last()?;
        let (runs, end) = quoted_runs(&chars, clause[..m.end()].chars().count() - 1);
        let script = unmask(&reassemble(&runs));
        let tail: String = chars[end..].iter().collect();
        let supplied = arguments_supplied(&tail, &clause[..m.start()]);
        let (text, positional_ok) = if quote == '"' {
            let t = if supplied {
                script
            } else {
                script.replace(KCT, "$")
            };
            (t, inherited_positional || !supplied)
        } else {
            (strip_wrapping_quotes(&script), !supplied)
        };
        if let Some(hit) = clause_pass(&text, positional_ok, depth, true, quote == '"') {
            return Some(OutHit {
                command: hit.command,
                target: hit.target.replace(KCT, "\\$"),
            });
        }
    }
    None
}

/// The run split `iLo` reads the script token as: unquoted and quoted runs in
/// order, starting ON the opening quote and ending at the first unquoted
/// whitespace. Returns the runs and the index the walk stopped at.
fn quoted_runs(chars: &[char], start: usize) -> (Vec<(Option<char>, String)>, usize) {
    let mut runs: Vec<(Option<char>, String)> = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut i = start;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && escapes_next(chars, i, quote) {
            cur.push(c);
            if let Some(n) = chars.get(i + 1) {
                cur.push(*n);
            }
            i += 2;
            continue;
        }
        if quote.is_none() {
            if c.is_whitespace() {
                break;
            }
            if c == '"' || c == '\'' {
                if !cur.is_empty() {
                    runs.push((None, std::mem::take(&mut cur)));
                }
                quote = Some(c);
                i += 1;
                continue;
            }
        } else if Some(c) == quote {
            runs.push((quote, std::mem::take(&mut cur)));
            quote = None;
            i += 1;
            continue;
        }
        cur.push(c);
        i += 1;
    }
    if !cur.is_empty() {
        runs.push((quote, cur));
    }
    (runs, i)
}

/// The runs joined back into one script @166795979: a double-quoted run resolves
/// its escapes, a single-quoted run keeps its quotes, an unquoted run is verbatim.
fn reassemble(runs: &[(Option<char>, String)]) -> String {
    runs.iter()
        .map(|(q, text)| match q {
            Some('"') => resolve_dquote_escapes(text),
            Some('\'') => format!("'{text}'"),
            _ => text.clone(),
        })
        .collect()
}

/// `replace(/^'([\s\S]*)'$/,"$1")` - the single-quoted arm undoes the quotes the
/// reassembly put back.
fn strip_wrapping_quotes(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() >= 2 && b[0] == b'\'' && b[b.len() - 1] == b'\'' {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

/// @166796336 - does anything supply the nested script's positional parameters?
/// Either the words after it, once a redirection and a find terminator are taken
/// out, come to two or more; or an `xargs` ahead of it will append them.
fn arguments_supplied(tail: &str, before: &str) -> bool {
    let t = tail.replace("\\ ", ESCAPED_SPACE);
    let t = TAIL_REDIRECTION.replace_all(&t, " ");
    let t = TAIL_TERMINATOR.replace(&t, "");
    t.split_whitespace().count() >= 2 || xargs_appends(before)
}

/// `/(?:^|\s)xargs(?:\s+(?!-[In]\b)\S+)*\s*$/` - the text before the nested shell
/// ends with an `xargs` whose remaining words are neither `-I` nor `-n`.
fn xargs_appends(before: &str) -> bool {
    for (i, _) in before.match_indices("xargs") {
        if i > 0 && !before[..i].ends_with(char::is_whitespace) {
            continue;
        }
        let rest = &before[i + "xargs".len()..];
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            continue;
        }
        if !rest.split_whitespace().any(holds_xargs_arguments) {
            return true;
        }
    }
    false
}

/// `-I` or `-n` at a word boundary: the two options that stop xargs appending.
fn holds_xargs_arguments(token: &str) -> bool {
    let Some(rest) = token
        .strip_prefix("-I")
        .or_else(|| token.strip_prefix("-n"))
    else {
        return false;
    };
    !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
#[path = "bash_danger_out_tests.rs"]
mod tests;
