//! The LEXICAL classifier of Claude Code generations 1 and 2, and the TEXT LAYER
//! every generation's classifier runs its input through.
//!
//! The classifier is `hnt` at 2.1.258 @162773520, byte-identical at 2.1.142
//! @192802095, 2.1.193 @205414799 (where it is named `Ywa`) and 2.1.207
//! @219697578, and GONE from 2.1.261 on (the rewrite lives in `bash_danger_out`).
//!
//! It runs on the TOO-COMPLEX arm only (`Hno` @162867544 calls `$no` @162861871,
//! which calls this). It is purely lexical: it never checks whether the variable is
//! actually empty, and it touches no filesystem. Mirror it exactly.
//!
//! Thirteen regexes and nothing else: eleven in the function body and the two
//! module-level named ones defined immediately above it at @162773345. Two of the
//! eleven need lookaround the `regex` crate has not got (`(?<!\$)\(...\)` and the
//! lone-`&` rule), so those two are hand-ported byte scans.
//!
//! Below the classifier sit the transforms it and the generation-3 walk share:
//! the substitution strips, the two masks and the private-use stand-in alphabet
//! the masks map onto. Each generation has its own spelling of several of them -
//! generation 1 and 2 blank a paren group to a space, generation 3 to a sentinel -
//! so the pairs live side by side here rather than one calling the other. Offsets
//! on the generation-3 half are into the 2.1.268 build; the rest are into 2.1.258.

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
pub(crate) fn scan_operands(args: &[&str]) -> Option<String> {
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
pub(crate) fn remove_plain_groups(s: &str) -> String {
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

// ---------------------------------------------------------------------------
// The generation-3 text layer: the stand-in alphabet, the quote-state primitives
// the harness reads a backslash with, and the transforms `out` runs before its
// token walk sees a clause.
// ---------------------------------------------------------------------------

/// `Vct` @166791013 - the sentinel the paren fixpoint and the backtick strip
/// substitute in.
pub(crate) const VCT: char = '\u{E020}';
/// `Kct` @166795502 - the stand-in a double-quoted nested script's escaped `$`
/// leaves behind, so a literal `$1` cannot read as a positional parameter.
pub(crate) const KCT: char = '\u{E010}';
/// @166796336 - the stand-in for a backslash-escaped space, so the tail that
/// decides a nested script's positional allowance does not count one operand as
/// two.
pub(crate) const ESCAPED_SPACE: &str = "\u{E022}";
/// `ANe` @166794789 - the characters `sLo` shields inside quotes, in its order.
const SPECIALS: &[char] = &[';', '|', '&', '\n', '\r', '(', ')', '`', ' ', '\t'];
/// `Wfe` @166794789 - the first private-use code point the shield maps onto.
const SHIELD_BASE: u32 = 57345;

/// `RNe` @166796663 - does the backslash at `i` escape the next character? Never
/// inside a single quote, always outside quotes, and inside a double quote only
/// before one of the four characters bash lets it escape there.
pub(crate) fn escapes_next(chars: &[char], i: usize, quote: Option<char>) -> bool {
    match quote {
        Some('\'') => false,
        None => true,
        Some(_) => matches!(chars.get(i + 1), Some('"' | '\\' | '$' | '`')),
    }
}

/// `Qct` @166796780 - the quote still open after the first `n` characters, or
/// `None` when every quote in that prefix is closed.
pub(crate) fn open_quote(chars: &[char], n: usize) -> Option<char> {
    let mut quote: Option<char> = None;
    let mut i = 0usize;
    while i < n {
        let c = chars[i];
        if c == '\\' && escapes_next(chars, i, quote) {
            i += 2;
            continue;
        }
        match quote {
            None if c == '"' || c == '\'' => quote = Some(c),
            Some(q) if c == q => quote = None,
            _ => {}
        }
        i += 1;
    }
    quote
}

/// `/\\[ \t]/` - a backslash-escaped space or tab, which the `sLo` guard admits
/// even in a command carrying no quote at all. Both bytes are ASCII and neither
/// occurs inside a UTF-8 multibyte sequence, so a byte scan cannot misread one.
fn has_escaped_blank(s: &str) -> bool {
    s.as_bytes()
        .windows(2)
        .any(|w| w[0] == b'\\' && (w[1] == b' ' || w[1] == b'\t'))
}

/// `sLo` @166794908 - replace each special character INSIDE a quoted run with a
/// private-use stand-in, so the clause split cannot cut a quoted string apart. An
/// escaped space or tab OUTSIDE quotes is shielded too, which is why the guard
/// admits a quoteless command carrying one; an UNBALANCED quote bails the whole
/// pass, because the shield cannot know where the run ends.
pub(crate) fn mask_specials(s: &str) -> String {
    if !s.contains('"') && !s.contains('\'') && !has_escaped_blank(s) {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    if open_quote(&chars, chars.len()).is_some() {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut quote: Option<char> = None;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && escapes_next(&chars, i, quote) {
            out.push(c);
            if let Some(n) = chars.get(i + 1).copied() {
                match SPECIALS.iter().position(|s| *s == n) {
                    Some(k) if quote.is_none() && (n == ' ' || n == '\t') => out.push(shield(k)),
                    _ => out.push(n),
                }
            }
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

pub(crate) fn shield(k: usize) -> char {
    char::from_u32(SHIELD_BASE + k as u32).unwrap_or('\u{E001}')
}

/// `sut` @166795380 - undo the shield.
pub(crate) fn unmask(s: &str) -> String {
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

/// The three replaces a double-quoted nested script's run goes through
/// @166795979. `\"`, `\\` and `` \` `` lose their backslash; `\$` becomes `KCT`,
/// except that a `KCT` naming a variable is restored to `$`. Those two fuse into
/// one walk: nothing the first replace emits is a letter, `_` or `{`, so the
/// character after a `\$` decides the same way before and after it. The third
/// replace then drops a backslash run standing directly before a `$`.
pub(crate) fn resolve_dquote_escapes(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '\\' {
            match chars.get(i + 1) {
                Some('$') => {
                    let named = names_a_variable(&chars, i + 2);
                    out.push(if named { '$' } else { KCT });
                    i += 2;
                    continue;
                }
                Some(c @ ('"' | '\\' | '`')) => {
                    out.push(*c);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    drop_backslashes_before_dollar(&out)
}

/// `(?=[A-Za-z_]|\{[A-Za-z_])` - what must follow a `$` for it to name a variable.
fn names_a_variable(chars: &[char], i: usize) -> bool {
    let ident = |c: Option<&char>| matches!(c, Some(c) if c.is_ascii_alphabetic() || *c == '_');
    ident(chars.get(i)) || (chars.get(i) == Some(&'{') && ident(chars.get(i + 1)))
}

/// `replace(/\\+(?=\$)/g,"")`. The run is maximal and every character in it is a
/// backslash, so a shorter match could never be followed by the `$` either.
fn drop_backslashes_before_dollar(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '\\' {
            let mut j = i;
            while chars.get(j) == Some(&'\\') {
                j += 1;
            }
            if chars.get(j) != Some(&'$') {
                out.extend(chars[i..j].iter());
            }
            i = j;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `MIn` @166793250 - blank out quoted runs (and comments) so a scan cannot read
/// inside them.
pub(crate) fn mask(s: &str, blank: bool) -> String {
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

/// The generation-3 backtick strip, which substitutes the sentinel rather than a
/// space: `` replace(/(?<!\\)`[^`]*`/g,Vct) ``.
pub(crate) fn strip_backticks(s: &str) -> String {
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

/// The generation-3 paren fixpoint, with the tightened lookbehinds:
/// `_.replace(/\$\([^()]*\)/g,Vct).replace(/(?<![$\\])\([^()]*(?<!\\)\)/g,Vct)`.
pub(crate) fn paren_fixpoint(s: &str) -> String {
    let mut cur = s.to_string();
    loop {
        let prev = cur.clone();
        cur = DOLLAR_PAREN
            .replace_all(&cur, VCT.to_string().as_str())
            .into_owned();
        cur = replace_plain_parens(&cur);
        if cur == prev {
            return cur;
        }
    }
}

pub(crate) fn replace_plain_parens(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `Qct` reads a backslash and the character it escapes as ONE unit, so the `"`
    /// a backslash escapes never opens a quote. Without the backslash the same
    /// quote is still open when the prefix runs out.
    #[test]
    fn a_backslash_escape_steps_the_quote_scan_over_both_characters() {
        let escaped: Vec<char> = "x\\\"y".chars().collect();
        assert_eq!(open_quote(&escaped, escaped.len()), None);
        let bare: Vec<char> = "x\"y".chars().collect();
        assert_eq!(open_quote(&bare, bare.len()), Some('"'));
    }

    /// The probe `sLo`'s guard asks: is there a BLANK the backslash escapes? Both
    /// bytes are required and in that order, so an unescaped blank is not one and
    /// neither is a backslash before anything else.
    #[test]
    fn the_escaped_blank_probe_wants_a_backslash_before_the_blank() {
        assert!(has_escaped_blank("a\\ b"));
        assert!(has_escaped_blank("a\\\tb"));
        assert!(!has_escaped_blank("rm -rf $D/*"));
        assert!(!has_escaped_blank("a b"));
    }

    /// `sLo`'s guard admits a command with NO quote at all when it carries a
    /// backslash-escaped blank, and the walk then stands that blank in so the clause
    /// split cannot cut one operand into two. A command carrying neither comes back
    /// unchanged.
    #[test]
    fn an_escaped_blank_admits_a_quoteless_command_to_the_shield() {
        assert_eq!(mask_specials("a\\ b"), format!("a\\{}b", shield(8)));
        assert_eq!(mask_specials("rm -rf $D/*"), "rm -rf $D/*");
    }

    /// The shield stands in for a special INSIDE a quoted run and keeps the same
    /// character outside one: that difference is the whole point of the pass.
    #[test]
    fn a_special_is_shielded_only_inside_a_quoted_run() {
        assert_eq!(
            mask_specials("echo \"a;b\""),
            format!("echo \"a{}b\"", shield(0))
        );
        assert_eq!(mask_specials("echo a;b"), "echo a;b");
    }

    /// Outside a quote only a blank the backslash escapes is stood in for; every
    /// other escaped special keeps its own character, which is what lets the clause
    /// split still read an escaped separator as escaped.
    #[test]
    fn an_escaped_special_that_is_not_a_blank_keeps_its_character() {
        assert_eq!(mask_specials("a\\ b\\;c"), format!("a\\{}b\\;c", shield(8)));
    }

    /// `sut` maps back only the code points the shield alphabet owns. The one just
    /// past its end is content and is left alone.
    #[test]
    fn the_unshield_leaves_the_code_point_past_the_alphabet_alone() {
        assert_eq!(unmask(&format!("a{}b", shield(9))), "a\tb");
        assert_eq!(unmask("a\u{E00B}b"), "a\u{E00B}b");
    }
}
