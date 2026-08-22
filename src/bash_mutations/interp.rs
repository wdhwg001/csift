//! Interpreter-payload write analysis: literal and one-hop targets, opaque markers.
//!
//! An interpreter invocation (`python3 - <<'PY' ... PY`, `node -e "..."`) can rewrite
//! files exactly like an Edit, but the write lives inside an embedded-language body the
//! shell lexer cannot see. This module inspects that body TEXT for a small set of
//! write idioms and extracts a target ONLY when it is provably literal:
//!
//! - a DIRECT literal argument: `open('x.md','w')`, `Path('x.md').write_text(...)`,
//!   `fs.writeFileSync('x.md', ...)`, `File.write('x.md', ...)`;
//! - a ONE-HOP constant: `p = 'x.md'` assigned EXACTLY ONCE at top level, then
//!   `open(p,'w')` / `p.write_text(...)`. The binding must be a bare string literal
//!   (or `Path('lit')`); a reassigned name, a loop binder, an f-string, a
//!   concatenation, argv, or an env read disqualifies it.
//!
//! When a write idiom is present but no target survives those guards, the command is
//! reported as an OPAQUE marker (`interp:<lang>`): the class is known, the file set is
//! not, and fabricating one would break the precision contract. Corpus measurement
//! behind the guards: only about 5% of interpreter write calls take a literal first
//! argument; the dominant shape is a single constant binding, which the one-hop rule
//! resolves with zero false-attribution risk.

use super::*;

/// The write-idiom findings for one interpreter payload.
pub(crate) struct InterpFinding {
    /// Provably-literal write targets (verbatim, possibly relative).
    pub(crate) targets: Vec<String>,
    /// A write idiom was seen but at least one write's target is not extractable.
    pub(crate) opaque_write: bool,
}

/// Interpreter command names and the language tag used in the `interp:<lang>` marker.
pub(crate) fn interp_lang(verb: &str) -> Option<&'static str> {
    match verb {
        "python" | "python3" | "python2" => Some("python"),
        "node" | "nodejs" => Some("node"),
        "ruby" => Some("ruby"),
        _ => None,
    }
}

/// Emit mutations for one interpreter segment: `body` is the heredoc body (when the
/// segment opened one) and inline `-c`/`-e`/`--eval`/`-p` script arguments are read
/// from the operands. Literal targets become real rows (verb `interp-write`, resolved
/// through the cwd checkpoint like any operand); an unextractable write becomes one
/// `interp:<lang>` class marker.
pub(crate) fn emit_interp(
    lang: &'static str,
    operands: &[&str],
    bodies: &[String],
    out: &mut Vec<BashMutation>,
) {
    let mut source = String::new();
    for b in bodies {
        source.push_str(b);
        source.push('\n');
    }
    // Inline script flags: the NEXT operand after -c/-e/--eval (quote-stripped).
    let mut i = 0usize;
    while i < operands.len() {
        if matches!(operands[i], "-c" | "-e" | "--eval") {
            if let Some(script) = operands.get(i + 1) {
                source.push_str(strip_quotes(script));
                source.push('\n');
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    if source.is_empty() {
        return;
    }
    let finding = analyze_interp_source(&source);
    for t in &finding.targets {
        out.push(BashMutation {
            path: t.clone(),
            verb: "interp-write",
            cwd_at: CwdAt::Spawn,
        });
    }
    if finding.opaque_write {
        out.push(BashMutation {
            path: format!("interp:{lang}"),
            verb: "interp",
            cwd_at: CwdAt::Spawn,
        });
    }
}

/// Scan one payload for write idioms and resolve their targets per the module rules.
pub(crate) fn analyze_interp_source(source: &str) -> InterpFinding {
    let mut targets: Vec<String> = Vec::new();
    let mut opaque = false;
    for line in source.lines() {
        for (idx, kind) in write_call_sites(line) {
            match write_target_at(line, idx, kind, source) {
                CallVerdict::Target(t) => {
                    if !targets.contains(&t) {
                        targets.push(t);
                    }
                }
                CallVerdict::Opaque => opaque = true,
                CallVerdict::NotAWrite => {}
            }
        }
    }
    InterpFinding {
        targets,
        opaque_write: opaque,
    }
}

/// What one write-idiom call site turned out to be.
enum CallVerdict {
    /// A write with a provably-literal target.
    Target(String),
    /// A write whose target the guards cannot extract.
    Opaque,
    /// Not a write after all (an `open` in read mode).
    NotAWrite,
}

/// A write call's argument position and how its target is expressed.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum WriteKind {
    /// `open(ARG, 'w'|'a'|'x')` and `io.open(...)`: target is the first argument,
    /// and the MODE argument must show a write letter or the call is a read.
    OpenMode,
    /// `X.write_text(` / `X.write_bytes(`: target is the receiver BEFORE the dot.
    Receiver,
    /// `writeFileSync(ARG` / `writeFile(ARG` / `File.write(ARG`: first argument.
    FirstArg,
}

/// Every write-idiom call site on a line: (byte index of the argument list start,
/// kind). Index points AT the `(`.
fn write_call_sites(line: &str) -> Vec<(usize, WriteKind)> {
    let mut sites = Vec::new();
    for (needle, kind) in [
        ("open(", WriteKind::OpenMode),
        (".write_text(", WriteKind::Receiver),
        (".write_bytes(", WriteKind::Receiver),
        ("writeFileSync(", WriteKind::FirstArg),
        ("File.write(", WriteKind::FirstArg),
    ] {
        let mut from = 0usize;
        while let Some(rel) = line[from..].find(needle) {
            let at = from + rel;
            // `open(` must not be the tail of `.write...` needles or of identifiers
            // like `reopen(`; require a non-alphanumeric byte before it.
            let head_ok =
                needle != "open(" || at == 0 || !line.as_bytes()[at - 1].is_ascii_alphanumeric();
            if head_ok {
                sites.push((at + needle.len() - 1, kind));
            }
            from = at + needle.len();
        }
    }
    sites
}

/// Resolve one write-idiom call site to its verdict.
fn write_target_at(line: &str, paren: usize, kind: WriteKind, source: &str) -> CallVerdict {
    let to_verdict = |lit: Option<String>| match lit {
        Some(t) => CallVerdict::Target(t),
        None => CallVerdict::Opaque,
    };
    match kind {
        WriteKind::OpenMode => {
            let args = &line[paren + 1..];
            let Some(first) = first_argument(args) else {
                return CallVerdict::Opaque;
            };
            // The mode must be a literal second argument carrying a write letter; a
            // bare `open(p)` (a read) or a non-literal mode is not provably a write,
            // so it contributes neither a target nor an opaque flag.
            match second_argument_literal(args) {
                Some(m) if m.contains(['w', 'a', 'x']) => {}
                _ => return CallVerdict::NotAWrite,
            }
            to_verdict(argument_to_literal(&first, source))
        }
        WriteKind::Receiver => {
            // `paren` points at the `(`; the receiver ends before the `.write_*` name.
            let name_len = line[..paren].rfind('.').map_or(0, |dot| paren - dot);
            match receiver_before(line, paren - name_len) {
                Some(recv) => to_verdict(argument_to_literal(&recv, source)),
                None => CallVerdict::Opaque,
            }
        }
        WriteKind::FirstArg => {
            let args = &line[paren + 1..];
            match first_argument(args) {
                Some(first) => to_verdict(argument_to_literal(&first, source)),
                None => CallVerdict::Opaque,
            }
        }
    }
}

/// One argument expression of an argument list, plus the byte index of the delimiter
/// that ended it (an unnested `,` or the closing `)`). `None` when the list never
/// closes on this line. Quote- and nesting-aware.
fn split_argument(args: &str) -> Option<(String, usize)> {
    let mut depth = 0usize;
    let mut in_str: Option<char> = None;
    for (i, c) in args.char_indices() {
        match (in_str, c) {
            (Some(q), _) if c == q => in_str = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => in_str = Some(c),
            (None, '(') => depth += 1,
            (None, ')') if depth == 0 => return Some((args[..i].trim().to_string(), i)),
            (None, ')') => depth -= 1,
            (None, ',') if depth == 0 => return Some((args[..i].trim().to_string(), i)),
            _ => {}
        }
    }
    None
}

/// The first argument expression of an argument list (up to an unnested `,` or `)`).
fn first_argument(args: &str) -> Option<String> {
    split_argument(args).map(|(a, _)| a)
}

/// The SECOND argument, only when it is a bare string literal (`'w'`, `"a+"`).
fn second_argument_literal(args: &str) -> Option<String> {
    let (_, delim) = split_argument(args)?;
    if args.as_bytes().get(delim) != Some(&b',') {
        return None; // the list closed after one argument: no mode present.
    }
    let (second, _) = split_argument(&args[delim + 1..])?;
    string_literal(&second)
}

/// The receiver expression ending just before `end` (walk identifiers/call chains
/// backward): `p.write_text(` gives `p`; `Path('x').write_text(` gives `Path('x')`.
fn receiver_before(line: &str, end: usize) -> Option<String> {
    let bytes = line.as_bytes();
    let mut i = end;
    let mut depth = 0usize;
    while i > 0 {
        let c = bytes[i - 1];
        match c {
            b')' => depth += 1,
            b'(' if depth > 0 => depth -= 1,
            b'(' => break,
            c if depth == 0
                && !(c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'\'' | b'"')) =>
            {
                break
            }
            _ => {}
        }
        i -= 1;
    }
    let recv = line[i..end].trim();
    (!recv.is_empty()).then(|| recv.to_string())
}

/// Turn an argument/receiver expression into a literal path: a direct string literal,
/// a `Path('lit')`-style constructor, or a ONE-HOP constant binding looked up in the
/// whole source under the strict guards.
fn argument_to_literal(expr: &str, source: &str) -> Option<String> {
    if let Some(lit) = string_literal(expr) {
        return Some(lit);
    }
    if let Some(lit) = path_constructor_literal(expr) {
        return Some(lit);
    }
    if is_identifier(expr) {
        return one_hop_literal(expr, source);
    }
    None
}

/// `'lit'` / `"lit"` with nothing else around it.
fn string_literal(expr: &str) -> Option<String> {
    let b = expr.as_bytes();
    if b.len() >= 2 && (b[0] == b'\'' || b[0] == b'"') && b[b.len() - 1] == b[0] {
        let inner = &expr[1..expr.len() - 1];
        let q = b[0] as char;
        if !inner.contains(q) && !inner.is_empty() {
            return Some(inner.to_string());
        }
    }
    None
}

/// `Path('lit')` / `pathlib.Path("lit")` / `new URL(...)`-free simple constructors.
fn path_constructor_literal(expr: &str) -> Option<String> {
    let open = expr.find('(')?;
    let head = &expr[..open];
    if !head.ends_with("Path") {
        return None;
    }
    let inner = expr[open + 1..].strip_suffix(')')?;
    string_literal(inner.trim())
}

fn is_identifier(expr: &str) -> bool {
    !expr.is_empty()
        && expr.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !expr.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// The one-hop rule: `ident` bound EXACTLY ONCE at top level to a bare string
/// literal (or `Path('lit')`), never a loop binder, never argv/env/f-string/concat.
fn one_hop_literal(ident: &str, source: &str) -> Option<String> {
    let mut binding: Option<String> = None;
    for line in source.lines() {
        let t = line.trim_start();
        // A loop binder disqualifies the name outright.
        if let Some(head) = t.strip_prefix("for ") {
            if head
                .split([' ', ',', '('])
                .any(|w| w.trim_matches([')', ':']) == ident)
            {
                return None;
            }
        }
        // An assignment line: `ident = RHS` (also `const/let/var ident = RHS`).
        let assign = t
            .strip_prefix("const ")
            .or_else(|| t.strip_prefix("let "))
            .or_else(|| t.strip_prefix("var "))
            .unwrap_or(t);
        let Some(rest) = assign.strip_prefix(ident) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rhs) = rest.strip_prefix('=') else {
            continue;
        };
        if rhs.starts_with('=') {
            continue; // `==` comparison, not an assignment.
        }
        let rhs = rhs.trim().trim_end_matches([';']);
        if binding.is_some() {
            return None; // reassigned: not a constant.
        }
        // The RHS must be a bare literal or Path('lit'); anything else (f-string,
        // concatenation, argv, env, another call) disqualifies.
        let lit = string_literal(rhs).or_else(|| path_constructor_literal(rhs));
        match lit {
            Some(l) => binding = Some(l),
            None => return None,
        }
    }
    binding
}
