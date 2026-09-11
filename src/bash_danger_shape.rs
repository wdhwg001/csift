//! The decomposition branch: WHICH checker the harness would run for a command.
//!
//! Claude Code parses every Bash command with tree-sitter and decomposes it
//! (`Dfe`, 2.1.258 @159477546). A cleanly parsed command - a plain command, a
//! pipeline, a list, a redirection, or a command substitution whose inner commands
//! are decomposed and appended - is `{kind:"simple"}` and goes to the STRUCTURED
//! removal checker `_9`. Everything else is `{kind:"too-complex"}` (the builder
//! `xe` @159523867) and only that arm reaches the LEXICAL classifier.
//!
//! csift has no tree-sitter, so this is a conservative SHAPE test over a
//! quote-aware statement split. A statement it cannot classify with confidence
//! takes the lexical path and says so, because the lexical path is the one that
//! can still produce an immune ask: routing a structured command to it would
//! over-claim, routing a too-complex one away from it would under-claim, and the
//! disclosure on the verdict names which happened.

use regex::Regex;
use std::sync::LazyLock;

/// `ls` @159382823 (`var ls=1e4`): the byte ceiling of the tree-sitter parse. Over
/// it the parse returns the abort symbol and `Dfe` answers `PARSE_ABORT`.
pub(crate) const PARSE_LIMIT: usize = 10_000;

/// Which checker the harness would reach, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Branch {
    /// The parse was clean: the structured removal checker `_9` decides.
    Structured,
    /// Too complex to decompose: the lexical classifier decides. The payload is
    /// the harness's own reason (a tree-sitter node type, or one of `Dfe`'s text
    /// prechecks), or `shape unknown` where csift declines to guess.
    Lexical(&'static str),
}

impl Branch {
    /// The one spelling of the path disclosure.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Branch::Structured => "structured".to_string(),
            Branch::Lexical(SHAPE_UNKNOWN) => "lexical (shape unknown)".to_string(),
            Branch::Lexical(r) => format!("lexical (too-complex: {r})"),
        }
    }
}

/// csift's own verdict, with no harness counterpart: the shape test could not
/// decide, so the conservative lexical path runs and the verdict says so.
pub(crate) const SHAPE_UNKNOWN: &str = "shape unknown";

/// `Jbn` @159475405 - a control character anywhere.
static CONTROL_CHARS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\x00-\x08\x0B-\x1F\x7F]").expect("Jbn"));
/// `of` @159475513 - a Unicode whitespace character anywhere.
static UNICODE_WS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        "[\u{00A0}\u{1680}\u{2000}-\u{200B}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]",
    )
    .expect("of")
});
/// `Zbn` @159475582 - a backslash-escaped space/tab, or a continuation shape.
static ESCAPED_WS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\\[ \t]|(?:^|[^ \t\\])(?:\\\\)*\\\n|[ \t](?:\\\\)+\\\n").expect("Zbn")
});
/// `OGt` @159475705 - zsh `~[` dynamic directory syntax.
static ZSH_TILDE_BRACKET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"~\[").expect("OGt"));
/// `DGt` @159475715 - zsh `=cmd` equals expansion.
static ZSH_EQUALS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[\s;&|])=[a-zA-Z_]").expect("DGt"));
/// `eTn` @159475745 - zsh `<N-M>` numeric-range glob.
static ZSH_RANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<\d*-\d*>").expect("eTn"));
/// `sf` @159475761 - a brace carrying a quote character (expansion obfuscation).
static BRACE_QUOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\{[^}]*['\"]").expect("sf"));
/// A `{a,b}` word not led by `$`: tree-sitter calls it `brace_expression`, which
/// is in the too-complex name set `Aa` @159474800.
static BRACE_EXPRESSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{[^{}\s]*,[^{}\s]*\}").expect("brace_expression"));
/// A function definition head: `function name` or `name ()`.
static FUNCTION_DEF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:function\s+[A-Za-z_][A-Za-z0-9_]*|[A-Za-z_][A-Za-z0-9_]*\s*\(\s*\))")
        .expect("function_definition")
});

/// The quote state `af` carries across one command string.
#[derive(Debug, Default)]
struct BraceMask {
    single: bool,
    double: bool,
    backtick: bool,
    /// Whether the next character opens a word, which is the only thing that
    /// makes a `#` a comment.
    word_start: bool,
}

/// A brace inside a quote is blanked; every other character is kept.
fn push_masked(out: &mut String, w: char) {
    out.push(if w == '{' { ' ' } else { w });
}

impl BraceMask {
    fn step_backtick(&mut self, e: &[char], i: usize, out: &mut String) -> usize {
        let w = e[i];
        if w == '\\' && matches!(e.get(i + 1), Some('`' | '\\' | '$')) {
            out.push(w);
            out.push(e[i + 1]);
            return i + 2;
        }
        if w == '`' {
            self.backtick = false;
        }
        push_masked(out, w);
        i + 1
    }

    fn step_single(&mut self, e: &[char], i: usize, out: &mut String) -> usize {
        let w = e[i];
        if w == '\'' {
            self.single = false;
        }
        push_masked(out, w);
        i + 1
    }

    fn step_double(&mut self, e: &[char], i: usize, out: &mut String) -> usize {
        let w = e[i];
        if w == '\\' && matches!(e.get(i + 1), Some('"' | '\\' | '`')) {
            out.push(w);
            out.push(e[i + 1]);
            return i + 2;
        }
        // A backtick inside double quotes opens a substitution, not a word.
        if w == '`' {
            self.backtick = true;
            out.push(w);
            return i + 1;
        }
        if w == '"' {
            self.double = false;
        }
        push_masked(out, w);
        i + 1
    }

    fn step_plain(&mut self, e: &[char], i: usize, out: &mut String) -> usize {
        let w = e[i];
        if w == '\\' && i + 1 < e.len() {
            out.push(w);
            out.push(e[i + 1]);
            if e[i + 1] != '\n' {
                self.word_start = false;
            }
            return i + 2;
        }
        if w == '#' && self.word_start {
            let mut j = i;
            while j < e.len() && e[j] != '\n' {
                out.push(e[j]);
                j += 1;
            }
            self.word_start = true;
            return j;
        }
        if w == '`' {
            self.backtick = true;
            self.word_start = false;
            out.push(w);
            return i + 1;
        }
        if w == '\'' {
            self.single = true;
        } else if w == '"' {
            self.double = true;
        }
        self.word_start = matches!(
            w,
            ' ' | '\t' | '\n' | ';' | '|' | '&' | '(' | ')' | '<' | '>'
        );
        // Outside a quote a brace is kept: that is the shape `sf` looks for.
        out.push(w);
        i + 1
    }
}

/// `af` @159476428 - the transform the brace-quote precheck is tested against.
/// A command with no brace is returned unchanged; otherwise every brace inside a
/// single-quoted, double-quoted or backtick span becomes a space, and a `#`
/// comment is copied verbatim to the end of its line.
pub(crate) fn mask_quoted_braces(command: &str) -> String {
    if !command.contains('{') {
        return command.to_string();
    }
    let e: Vec<char> = command.chars().collect();
    let mut out = String::with_capacity(command.len());
    let mut st = BraceMask {
        word_start: true,
        ..BraceMask::default()
    };
    let mut i = 0usize;
    while i < e.len() {
        i = if st.backtick {
            st.step_backtick(&e, i, &mut out)
        } else if st.single {
            st.step_single(&e, i, &mut out)
        } else if st.double {
            st.step_double(&e, i, &mut out)
        } else {
            st.step_plain(&e, i, &mut out)
        };
    }
    out
}

/// The tree-sitter statement keywords whose node type `xe` reports verbatim.
const KEYWORD_STATEMENTS: &[(&str, &str)] = &[
    ("for", "for_statement"),
    ("while", "while_statement"),
    ("until", "until_statement"),
    ("if", "if_statement"),
    ("case", "case_statement"),
    ("select", "for_statement"),
];

/// Decide the branch for one command string.
pub(crate) fn branch_of(command: &str) -> Branch {
    if command.len() > PARSE_LIMIT {
        return Branch::Lexical("PARSE_ABORT");
    }
    // `Dfe`'s text prechecks, in its own order. Each is a `too-complex` answer
    // taken BEFORE any node walk, so a command tripping one never reaches `_9`.
    for (re, reason) in [
        (&*CONTROL_CHARS, "Contains control characters"),
        (&*UNICODE_WS, "Contains Unicode whitespace"),
        (&*ESCAPED_WS, "Contains backslash-escaped whitespace"),
        (
            &*ZSH_TILDE_BRACKET,
            "Contains zsh ~[ dynamic directory syntax",
        ),
        (&*ZSH_EQUALS, "Contains zsh =cmd equals expansion"),
        (&*ZSH_RANGE, "Contains zsh <N-M> numeric-range glob"),
    ] {
        if re.is_match(command) {
            return Branch::Lexical(reason);
        }
    }
    // The seventh precheck is the only one that does NOT read the raw command:
    // the harness tests `sf` against `af(e)` (@159478267), and `af` blanks every
    // brace INSIDE a quote. So the arm fires on an UNQUOTED brace group carrying
    // a quote character - the obfuscation shape - and not on a brace that merely
    // sits inside a quoted word.
    if BRACE_QUOTE.is_match(&mask_quoted_braces(command)) {
        return Branch::Lexical("Contains brace with quote character (expansion obfuscation)");
    }
    if command.trim().is_empty() {
        return Branch::Structured;
    }
    // Argument shapes that `ze`'s walk hands to the too-complex builder wherever
    // they appear, not only at a statement head.
    for (needle, node) in [
        ("<<<", "herestring_redirect"),
        ("<<", "heredoc_redirect"),
        ("[[", "test_command"),
        ("$'", "ansi_c_string"),
        ("$\"", "translated_string"),
    ] {
        if command.contains(needle) {
            return Branch::Lexical(node);
        }
    }
    if BRACE_EXPRESSION.is_match(command) && !command.contains("${") {
        return Branch::Lexical("brace_expression");
    }
    let Some(statements) = split_statements(command) else {
        // Unbalanced quotes or parens: tree-sitter would answer ERROR, whose
        // `xe` reason is `Parse error`.
        return Branch::Lexical("Parse error");
    };
    for stmt in statements {
        let s = stmt.trim();
        if s.is_empty() {
            continue;
        }
        if let Some(node) = statement_node(s) {
            return Branch::Lexical(node);
        }
    }
    Branch::Structured
}

/// The node type a statement HEAD forces, or `None` when the head is an ordinary
/// command (which `ze` decomposes).
fn statement_node(s: &str) -> Option<&'static str> {
    if s.starts_with('(') {
        return Some("subshell");
    }
    if s.starts_with('{') && s[1..].starts_with(|c: char| c.is_whitespace()) {
        return Some("compound_statement");
    }
    if FUNCTION_DEF.is_match(s) {
        return Some("function_definition");
    }
    let head = s.split_whitespace().next().unwrap_or_default();
    for (kw, node) in KEYWORD_STATEMENTS {
        if head == *kw {
            return Some(node);
        }
    }
    // A bare loop/branch keyword that is neither a head nor a plain command word
    // (`do`, `then`, `fi`, `done`, `esac`, `elif`, `else`) can only appear inside
    // one of the shapes above, so reaching one here means the split lost track.
    if matches!(
        head,
        "do" | "then" | "fi" | "done" | "esac" | "elif" | "else"
    ) {
        return Some(SHAPE_UNKNOWN);
    }
    None
}

/// Split a command into top-level statements, honoring quotes and nesting.
/// `None` when the quoting or nesting is unbalanced (the harness's ERROR arm).
pub(crate) fn split_statements(command: &str) -> Option<Vec<&str>> {
    let b = command.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<u8> = None;
    let mut depth: i32 = 0;
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == b'\\' && q == b'"' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'\\' => {
                i += 2;
                continue;
            }
            b'\'' | b'"' => quote = Some(c),
            b'(' | b'{' => depth += 1,
            b')' | b'}' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            b';' | b'\n' | b'&' | b'|' if depth == 0 => {
                out.push(&command[start..i]);
                // Consume a two-byte operator whole.
                let two = i + 1 < b.len() && b[i + 1] == c;
                i += if two { 2 } else { 1 };
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    if quote.is_some() || depth != 0 {
        return None;
    }
    out.push(&command[start.min(command.len())..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_brace_mask_blanks_a_brace_only_inside_a_quote() {
        // No brace at all is the fast path, returned unchanged.
        assert_eq!(mask_quoted_braces("echo plain"), "echo plain");
        // Inside each of the three quote spans the brace becomes a space; the
        // closing brace is not a brace and stays.
        assert_eq!(mask_quoted_braces("echo \"a{b}\""), "echo \"a b}\"");
        assert_eq!(mask_quoted_braces("echo 'a{b}'"), "echo 'a b}'");
        assert_eq!(mask_quoted_braces("echo `a{b}`"), "echo `a b}`");
        // Outside a quote it is kept, because that is the shape the precheck
        // is looking for.
        assert_eq!(mask_quoted_braces("rm -rf /{a,b}"), "rm -rf /{a,b}");
        // An escaped quote does not open a span, so the brace after it is still
        // outside one.
        assert_eq!(mask_quoted_braces("echo \\\"{a}"), "echo \\\"{a}");
        // A comment is copied verbatim, brace and all.
        assert_eq!(mask_quoted_braces("ls # {a}"), "ls # {a}");
    }

    #[test]
    fn the_brace_quote_precheck_reads_the_masked_command() {
        // An UNQUOTED brace group carrying a quote character is the obfuscation
        // the arm exists for.
        assert!(matches!(
            branch_of("rm -rf /{'',}tmp"),
            Branch::Lexical("Contains brace with quote character (expansion obfuscation)")
        ));
        // A brace that merely sits inside a quoted word is NOT: testing the raw
        // command here would send an ordinary echo to the lexical classifier.
        assert!(matches!(
            branch_of("echo \"set {a: 'b'}\""),
            Branch::Structured
        ));
    }
}
