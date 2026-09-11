//! The LEXICAL classifier of Claude Code generations 1 and 2 - `hnt` at 2.1.258
//! @162773520, byte-identical at 2.1.142 @192802095, 2.1.193 @205414799 (where it
//! is named `Ywa`) and 2.1.207 @219697578, and GONE from 2.1.261 on (the rewrite
//! lives in `bash_danger_out`).
//!
//! It runs on the TOO-COMPLEX arm only (`Hno` @162867544 calls `$no` @162861871,
//! which calls this). It is purely lexical: it never checks whether the variable is
//! actually empty, and it touches no filesystem. Mirror it exactly.
//!
//! Thirteen regexes and nothing else: eleven in the function body and the two
//! module-level named ones defined immediately above it at @162773345. Two of the
//! eleven need lookaround the `regex` crate has not got (`(?<!\$)\(...\)` and the
//! lone-`&` rule), so those two are hand-ported byte scans.

use regex::Regex;
use std::sync::LazyLock;

/// `_to` @162773345, the clause head: zero or more `NAME=value` / `NAME+=value`
/// assignments, an optional single backslash, an optional `anything-without-space-
/// or-equals/` path prefix, then `rm` or `rmdir` followed by whitespace or end.
/// Capture 1 selects the verb. No keyword skipping and no `sudo`/`env`/`xargs`
/// skipping: this generation cannot see `do rm`, `then rm` or `sudo rm`.
static CLAUSE_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[A-Za-z_][A-Za-z0-9_]*\+?=[^\s]*\s+)*\\?(?:[^\s=]*/)?(rm|rmdir)(?:\s|$)")
        .expect("_to")
});

/// `yto` @162773345, the dangerous target: an optional `"`, then `$NAME` or
/// `${NAME}`, an optional `"`, then `/`, then one of `*` `$` `/` `"` `'` end.
static TARGET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^"?\$(?:\{[A-Za-z_][A-Za-z0-9_]*\}|[A-Za-z_][A-Za-z0-9_]*)"?/(?:\*|\$|/|["']|$)"#)
        .expect("yto")
});

/// The entry guard `/\brm(?:dir)?\b/` (case-sensitive at this generation).
pub(crate) static RM_WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\brm(?:dir)?\b").expect("rm_word"));

/// `/^[\d&]*[<>]/` - a token that STARTS like a redirection.
pub(crate) static REDIR_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\d&]*[<>]").expect("redir"));

/// `/^(?:[0-9]+|&)?(?:>>?[|&]?|<<?<?|<>)$/` - a token that IS a bare redirect
/// operator, so the NEXT token is its target and must be skipped too.
pub(crate) static REDIR_OP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[0-9]+|&)?(?:>>?[|&]?|<<?<?|<>)$").expect("redir_op"));

/// `/\\\r?\n/` - a line continuation.
static LINE_CONT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\\r?\n").expect("line_cont"));
/// `` /`[^`]*`/ `` - a backtick command substitution.
pub(crate) static BACKTICKS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`[^`]*`").expect("backticks"));
/// `/\$\([^()]*\)/` - an innermost `$(...)` command substitution.
pub(crate) static DOLLAR_PAREN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\([^()]*\)").expect("dpar"));
/// `/\([^()]*\)/` - an innermost `(...)` group. The `(?<!\$)` guard is in code.
static PLAIN_PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^()]*\)").expect("ppar"));
/// `/[;|\n\r]|&&/` - the clause separator set.
pub(crate) static CLAUSE_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[;|\n\r]|&&").expect("clause"));

/// What `hnt` returns: `{command, target}` or nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LexicalHit {
    pub command: &'static str,
    pub target: String,
}

/// `hnt(command)` - the first dangerous `rm`/`rmdir` removal, or `None`.
///
/// csift's one standing deviation: the harness splits statements with tree-sitter
/// (`ep` @162265300); csift processes the whole string as ONE statement, which is
/// `ep`'s own documented fallback for unparseable or over-long input. The clause
/// split below recovers the separated clauses; a clause the split cannot reach is
/// exactly what the harness's own head regex cannot reach either, because that
/// regex skips no shell keyword.
pub(crate) fn hnt(command: &str) -> Option<LexicalHit> {
    if !command.contains('$') || !RM_WORD.is_match(command) {
        return None;
    }
    let n = LINE_CONT.replace_all(command, " ");
    let n = BACKTICKS.replace_all(&n, " ");
    let mut n = n.trim_start().to_string();
    while n.starts_with('(') || n.starts_with('{') {
        n = n[1..].trim_start().to_string();
    }
    n = strip_paren_groups(&n);
    n = amp_to_semicolon(&n);
    for clause in CLAUSE_SPLIT.split(&n) {
        let o = clause.trim_start();
        let Some(caps) = CLAUSE_HEAD.captures(o) else {
            continue;
        };
        let verb: &'static str = if &caps[1] == "rmdir" { "rmdir" } else { "rm" };
        let rest = &o[caps.get(0).map_or(0, |m| m.end())..];
        let args: Vec<&str> = rest.split_whitespace().collect();
        if let Some(target) = scan_operands(&args) {
            return Some(LexicalHit {
                command: verb,
                target,
            });
        }
    }
    None
}

/// The operand walk: trailing-bracket trim, the empty / `-` / `'` skip, the
/// redirect skip (a bare operator also consumes its target), then the target test.
fn scan_operands(args: &[&str]) -> Option<String> {
    let mut l = 0usize;
    while l < args.len() {
        let c = args[l].trim_end_matches([')', ']', '}']);
        if c.is_empty() || c.starts_with('-') || c.starts_with('\'') {
            l += 1;
            continue;
        }
        if REDIR_START.is_match(c) {
            if REDIR_OP.is_match(c) {
                l += 1;
            }
            l += 1;
            continue;
        }
        if TARGET.is_match(c) {
            return Some(c.to_string());
        }
        l += 1;
    }
    None
}

/// The `$(...)` / `(...)` FIXPOINT @162773751:
/// `for(let o="";o!==r;)o=r,r=r.replace(/\$\([^()]*\)/g," ").replace(/(?<!\$)\([^()]*\)/g," ")`.
/// Unbounded by design - it terminates when the string stops changing.
pub(crate) fn strip_paren_groups(s: &str) -> String {
    let mut n = s.to_string();
    loop {
        let prev = n.clone();
        n = DOLLAR_PAREN.replace_all(&n, " ").into_owned();
        n = remove_plain_groups(&n);
        if n == prev {
            break;
        }
    }
    n
}

/// `replace(/(?<!\$)\([^()]*\)/g," ")` - the lookbehind applied by checking the
/// byte before each match; a `$`-preceded group is kept for the other pass.
fn remove_plain_groups(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for m in PLAIN_PAREN.find_iter(s) {
        let start = m.start();
        let preceded_by_dollar = start > 0 && bytes[start - 1] == b'$';
        out.push_str(&s[last..start]);
        if preceded_by_dollar {
            out.push_str(m.as_str());
        } else {
            out.push(' ');
        }
        last = m.end();
    }
    out.push_str(&s[last..]);
    out
}

/// `replace(/(?<![<>&])&(?![<>&])/g,";")` @162773842 - a lone `&` becomes `;`,
/// while `&&`, `<&`, `>&`, `&>` and `&<` survive. The special bytes are all ASCII
/// and never occur inside a UTF-8 multibyte sequence, so a byte scan that copies
/// every other byte verbatim leaves multibyte content untouched.
pub(crate) fn amp_to_semicolon(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    for i in 0..b.len() {
        if b[i] == b'&' {
            let prev_bad = i > 0 && matches!(b[i - 1], b'<' | b'>' | b'&');
            let next_bad = i + 1 < b.len() && matches!(b[i + 1], b'<' | b'>' | b'&');
            if !prev_bad && !next_bad {
                out.push(b';');
                continue;
            }
        }
        out.push(b[i]);
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}
