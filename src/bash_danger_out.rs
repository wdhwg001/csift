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

use regex::Regex;
use std::sync::LazyLock;

/// `Vct` @166791013 - the sentinel the paren fixpoint and the backtick strip
/// substitute in.
const VCT: char = '\u{E020}';
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
    if let Some(hit) = clause_pass(&cleaned, positional_ok, 0, true) {
        return Some(hit);
    }
    clause_pass(&cleaned, false, 0, false)
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
fn clause_pass(text: &str, positional_ok: bool, depth: usize, quote_aware: bool) -> Option<OutHit> {
    let p = text.replace("\\\r\n", " ").replace("\\\n", " ");
    let masked = if quote_aware { mask_specials(&p) } else { p };
    let mut s = strip_backticks(&masked);
    s = s.trim_start().to_string();
    while s.starts_with('(') || s.starts_with('{') {
        s = s[1..].trim_start().to_string();
    }
    s = paren_fixpoint(&s);
    s = crate::bash_danger_lexical::amp_to_semicolon(&s);
    for clause in crate::bash_danger_lexical::CLAUSE_SPLIT.split(&s) {
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
            if let Some(hit) = operand_walk(inv, positional_ok, quote_aware) {
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
        } else if crate::bash_danger_lexical::REDIR_START.is_match(p) {
            r += 1;
            if crate::bash_danger_lexical::REDIR_OP.is_match(p) {
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
            if crate::bash_danger_lexical::REDIR_START.is_match(p) {
                r += 1;
                if crate::bash_danger_lexical::REDIR_OP.is_match(p) {
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
fn operand_walk(inv: &Invocation, positional_ok: bool, quote_aware: bool) -> Option<OutHit> {
    let mut p = 0usize;
    while p < inv.tokens.len() {
        let t = inv.tokens[p].trim_end_matches([')', ']', '}']);
        if t.is_empty() || t.starts_with('-') {
            p += 1;
            continue;
        }
        if !quote_aware && t.starts_with('\'') && !t[1..].contains('\'') && t.ends_with('$') {
            p += 1;
            continue;
        }
        if crate::bash_danger_lexical::REDIR_START.is_match(t) {
            if crate::bash_danger_lexical::REDIR_OP.is_match(t) {
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

/// `iLo` @166794... - recurse into a nested `sh -c '<script>'`.
fn nested_shell(clause: &str, depth: usize, positional_ok: bool) -> Option<OutHit> {
    for m in NESTED_SHELL.find_iter(clause) {
        let quote = clause[m.start()..m.end()].chars().last()?;
        let body_start = m.end();
        let rest = &clause[body_start..];
        let end = rest.find(quote).unwrap_or(rest.len());
        let script = &rest[..end];
        if script.is_empty() {
            continue;
        }
        if let Some(hit) = clause_pass(script, positional_ok, depth, true) {
            return Some(hit);
        }
    }
    None
}

/// `ANe` @166794... - the characters `sLo` shields inside quotes, in its order.
const SPECIALS: &[char] = &[';', '|', '&', '\n', '\r', '(', ')', '`', ' ', '\t'];
/// `Wfe` @166794... - the first private-use code point the shield maps onto.
const SHIELD_BASE: u32 = 57345;

/// `sLo` - replace each special character INSIDE a quoted run with a private-use
/// stand-in, so the clause split cannot cut a quoted string apart.
fn mask_specials(s: &str) -> String {
    if !s.contains('"') && !s.contains('\'') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut quote: Option<char> = None;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && quote != Some('\'') && i + 1 < chars.len() {
            out.push(c);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        match quote {
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                }
                out.push(c);
            }
            Some(q) if c == q => {
                quote = None;
                out.push(c);
            }
            Some(_) => match SPECIALS.iter().position(|s| *s == c) {
                Some(k) => out.push(shield(k)),
                None => out.push(c),
            },
        }
        i += 1;
    }
    out
}

fn shield(k: usize) -> char {
    char::from_u32(SHIELD_BASE + k as u32).unwrap_or('\u{E001}')
}

/// `sut` - undo the shield.
fn unmask(s: &str) -> String {
    s.chars()
        .map(|c| {
            let v = c as u32;
            if v >= SHIELD_BASE && v < SHIELD_BASE + SPECIALS.len() as u32 {
                SPECIALS[(v - SHIELD_BASE) as usize]
            } else {
                c
            }
        })
        .collect()
}

/// `MIn` - blank out quoted runs (and comments) so a scan cannot read inside them.
fn mask(s: &str, blank: bool) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut quote: Option<char> = None;
    let mut comment = false;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if comment {
            if c == '\n' {
                comment = false;
                out.push(c);
            } else {
                out.push(' ');
            }
            i += 1;
            continue;
        }
        if quote.is_none() {
            if c == '\\' {
                out.push(c);
                if let Some(n) = chars.get(i + 1) {
                    out.push(*n);
                }
                i += 2;
                continue;
            }
            if c == '#'
                && (i == 0
                    || matches!(
                        chars[i - 1],
                        ' ' | '\t' | '\n' | '\r' | ';' | '&' | '|' | '(' | ')'
                    ))
            {
                comment = true;
                out.push(' ');
                i += 1;
                continue;
            }
            if c == '"' || c == '\'' {
                quote = Some(c);
            }
            out.push(c);
            i += 1;
            continue;
        }
        if Some(c) == quote {
            quote = None;
            out.push(c);
            i += 1;
            continue;
        }
        out.push(if blank { ' ' } else { c });
        i += 1;
    }
    out
}

/// The backtick strip, which at this generation substitutes the sentinel rather
/// than a space: `` replace(/(?<!\\)`[^`]*`/g,Vct) ``.
fn strip_backticks(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            out.push(chars[i]);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if chars[i] == '`' {
            if let Some(end) = chars[i + 1..].iter().position(|c| *c == '`') {
                out.push(VCT);
                i += end + 2;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// The paren fixpoint with the tightened lookbehinds:
/// `_.replace(/\$\([^()]*\)/g,Vct).replace(/(?<![$\\])\([^()]*(?<!\\)\)/g,Vct)`.
fn paren_fixpoint(s: &str) -> String {
    let mut cur = s.to_string();
    loop {
        let prev = cur.clone();
        cur = crate::bash_danger_lexical::DOLLAR_PAREN
            .replace_all(&cur, VCT.to_string().as_str())
            .into_owned();
        cur = replace_plain_parens(&cur);
        if cur == prev {
            return cur;
        }
    }
}

fn replace_plain_parens(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '(' && !(i > 0 && (chars[i - 1] == '$' || chars[i - 1] == '\\')) {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '(' && chars[j] != ')' {
                j += 1;
            }
            if j < chars.len() && chars[j] == ')' && chars[j - 1] != '\\' {
                out.push(VCT);
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}
