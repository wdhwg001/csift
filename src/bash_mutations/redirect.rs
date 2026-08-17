//! Redirections: fd qualifiers, targets, structural trimming, path operands.

use super::*;

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
pub(crate) fn collect_redirections(
    tokens: &[MaskedTok],
    out: &mut Vec<BashMutation>,
) -> std::collections::BTreeSet<usize> {
    let mut consumed: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    let mut i = 0usize;
    while i < tokens.len() {
        let tok = tokens[i];
        // Operator detection reads the MASK: an in-quote / in-procsub `>` is [`MASK_CHAR`]
        // there, so it never matches a redirect operator. The emitted PATH is sliced from
        // the original (next) token so a genuinely-quoted target still resolves.
        let body = strip_fd_qualifier(tok.masked);
        // A bash noclobber-override `>|` is a plain truncate redirect (the `|` only
        // disables noclobber): `>|` reads as `>`, `>|file` as `>file`.
        if body == ">|" {
            // Bare operator: it AND its following path token are both redirect syntax, not
            // command operands — record both indices so the verb dispatch never sees them.
            consumed.insert(i);
            if let Some(next) = tokens.get(i + 1) {
                push_redirect_target(next.orig, ">", out);
                consumed.insert(i + 1);
            }
            i += 1;
        } else if body.starts_with(">|") {
            // The operator is `>|` (possibly fd-qualified like `2>|file`); the attached
            // path follows it in the ORIGINAL token at the qualifier+`>|` byte offset.
            let off = tok.masked.len() - body.len() + 2;
            push_redirect_target(&tok.orig[off..], ">", out);
            consumed.insert(i);
            i += 1;
        } else if body == ">>" || body == ">" {
            // A bare (mask-confirmed) operator: its path is the NEXT token. BOTH the operator
            // and its path token are consumed redirect syntax (even when the target is an
            // fd-dup `&1` / `/dev/null` sink that emits no path — it is still NOT a command
            // operand and must not poison a positional-dest verb like `cp`/`mv`/`ln`).
            let verb = if body == ">>" { ">>" } else { ">" };
            consumed.insert(i);
            if let Some(next) = tokens.get(i + 1) {
                push_redirect_target(next.orig, verb, out);
                consumed.insert(i + 1);
            }
            i += 1;
        } else if body.starts_with(">>") {
            // An attached append `…>>file` (incl. `2>>file`). Slice the original token at
            // the offset the fd-qualifier+`>>` consumed in the MASK (the qualifier and the
            // operator are ASCII single-byte, so the mask offset is the original offset).
            let off = tok.masked.len() - body.len() + 2;
            push_redirect_target(&tok.orig[off..], ">>", out);
            consumed.insert(i);
            i += 1;
        } else if body == ">&" {
            // A bare `>&` operator. Its NEXT token is the target: a bare fd-NUMBER is an
            // fd-dup (`>& 2`, no path), but a WORD is a file (`make >& build.log`, the
            // bash combined-stream redirect, equivalent to `&>file`). Either way both the
            // operator and its target token are redirect syntax, so consume both.
            consumed.insert(i);
            if let Some(next) = tokens.get(i + 1) {
                if !is_fd_number(next.orig) {
                    push_redirect_target(next.orig, ">", out);
                }
                consumed.insert(i + 1);
            }
            i += 1;
        } else if body.starts_with(">&") {
            // An attached `>&TARGET`. `>&1`/`>&2` is an fd-dup (no path); `>&file` is the
            // combined-stream file redirect. Slice the original after the `>&` and classify.
            let off = tok.masked.len() - body.len() + 2;
            let tail = &tok.orig[off..];
            if !is_fd_number(tail) {
                push_redirect_target(tail, ">", out);
            }
            consumed.insert(i);
            i += 1;
        } else if body.starts_with('>') {
            // An attached truncate `…>file` (incl. `2>file`, `&>file`). A bare `2>&1` fd-dup
            // leaves the rest = `&1`, which `push_redirect_target` rejects — but the WHOLE
            // attached token (`2>&1`, `2>/dev/null`) is still redirect syntax, so consume it.
            let off = tok.masked.len() - body.len() + 1;
            push_redirect_target(&tok.orig[off..], ">", out);
            consumed.insert(i);
            i += 1;
        } else {
            i += 1;
        }
    }
    consumed
}

/// Strip an optional leading fd qualifier (`&`, or a run of ASCII digits) from a token,
/// exposing the bare redirect operator/body. `2>` → `>`, `1>>` → `>>`, `&>` → `>`,
/// `12>file` → `>file`; a token with no such prefix (or `>`/`>>` itself) is returned
/// unchanged. Only strips when a `>` actually follows the qualifier, so a plain numeric
/// or `&`-leading token that is NOT a redirect is left intact.
pub(crate) fn strip_fd_qualifier(tok: &str) -> &str {
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

/// True when a `>&` redirect target is a bare file-DESCRIPTOR number (`>&1`, `>&2`, `>& 2`)
/// — an fd-dup that writes NO file — rather than a filename. A `-` close (`>&-`) is also an
/// fd op, not a path. Only an all-ASCII-digit (or `-`) target is an fd-dup; any other word
/// is a file (`make >& build.log`).
pub(crate) fn is_fd_number(tail: &str) -> bool {
    let t = strip_quotes(tail);
    t == "-" || (!t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
}

/// Emit a redirect target if it resolves to a concrete path — dropping a `/dev/null`-
/// class sink and any fd-dup remnant (`&1`) that `path_operand` rejects.
pub(crate) fn push_redirect_target(tail: &str, verb: &'static str, out: &mut Vec<BashMutation>) {
    if is_dev_sink(tail) || tail.starts_with('&') {
        return; // a discard sink or an fd-dup (`>&1`/`2>&1`): no real path written.
    }
    if let Some(path) = path_operand(tail) {
        out.push(BashMutation { path, verb });
    }
}

/// True for a redirect sink that is NOT a real created file: `/dev/null`, `/dev/stderr`,
/// `/dev/stdout` (and their quote-stripped forms). These are ubiquitous noise targets.
pub(crate) fn is_dev_sink(tail: &str) -> bool {
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
pub(crate) fn path_operand(token: &str) -> Option<String> {
    let stripped = strip_quotes(token);
    // Peel trailing shell-structural punctuation glued on by a command/process
    // substitution or sequencing (`…2>/dev/null)`, `…>file;`) BEFORE the path tests, so
    // the sink/var/syntax filters see the bare path. A leading `'`/`"` that survived an
    // unbalanced split is also handled by [`has_syntax_noise`] below.
    let stripped = trim_structural_tail(stripped);
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
        return None; // an unexpandable `$VAR`/`~` pseudo-path — never fabricate it.
    }
    // After the trailing-tail trim, the sink test must run AGAIN: `2>/dev/null)` trims to
    // `/dev/null`, the dominant fabricated-path class (a `)` glued on by a command
    // substitution). A bare fd-dup remnant (`&1`) is likewise not a path.
    if is_dev_sink(stripped) || stripped.starts_with('&') {
        return None;
    }
    // Reject a token still carrying shell SYNTAX NOISE that no real path contains — an
    // unbalanced quote/paren (a quote-unaware split severed a quoted operand), an
    // embedded redirect operator (`/1>`, `2>&1`), a process-substitution head (`>(`,
    // `<(`), or a regex/escape metachar. These are parse artifacts, never files.
    if has_syntax_noise(stripped) {
        return None;
    }
    Some(stripped.to_string())
}

/// Strip trailing shell-STRUCTURAL punctuation a quote-unaware lexer may have glued onto
/// a path: the close-delimiters of a command/process substitution (`)`/`}`), a statement
/// terminator (`;`/`,`), and a trailing unmatched quote. Applied repeatedly (a token can
/// end in several, e.g. `/dev/null))`). A `)` is only trimmed when the token has NO
/// matching `(` (an unbalanced close glued on by `$(… )`); a balanced `(…)` is left so a
/// genuinely parenthesized name is untouched. This is the single fix for the dominant
/// `/dev/null)` / `>(tee` garbage class.
pub(crate) fn trim_structural_tail(token: &str) -> &str {
    let mut s = token;
    loop {
        // Each matching arm strips exactly one trailing byte; `_ => break` ends the loop.
        s = match s.as_bytes().last() {
            Some(b';' | b',') => &s[..s.len() - 1],
            Some(b'"' | b'\'') => &s[..s.len() - 1],
            Some(b')') if s.matches('(').count() < s.matches(')').count() => &s[..s.len() - 1],
            Some(b'}') if s.matches('{').count() < s.matches('}').count() => &s[..s.len() - 1],
            _ => break,
        };
    }
    s
}

/// True when a candidate path still carries shell SYNTAX a real filesystem path never
/// has — proof the token is a lexer artifact, not a file:
/// - an unbalanced surrounding quote (`'/tmp/x` from a severed quoted operand);
/// - an embedded redirect operator (`>` / a `<(`/`>(` process-substitution head);
/// - a regex/escape metachar (`\`, `(?`, `|`, `^`, a stray `?`/`*` mixed with `)` or `]`);
/// - a comparison/value fragment (`=1.94`, a `=`-led token), a pure number (`7000`), or an
///   embedded-code shard (a `,` or a trailing `:` inside a token with no `/` separator).
///
/// Conservative: a plain glob (`*.tmp`, `?.bin`) is NOT noise — it is still a real
/// touched set and is kept verbatim by `path_operand` (only `concrete_path` drops it). A
/// genuine RELATIVE path (`src/main.rs`, `Cargo.toml`, `paper.pdf`) is NOT noise either —
/// the code-shard tests only fire on tokens with NO `/` (a bare relative FILE is allowed).
pub(crate) fn has_syntax_noise(token: &str) -> bool {
    // An unbalanced single/double quote anywhere (the quote-split tell).
    if token.matches('"').count() % 2 == 1 || token.matches('\'').count() % 2 == 1 {
        return true;
    }
    // A process-substitution head (`>(` / `<(`) or a bare `(` — each maps to a distinct
    // precision-contract bullet, so keep them explicit (1:1 with the doc above).
    if token.starts_with(">(") || token.starts_with("<(") || token.starts_with('(') {
        return true;
    }
    // An embedded redirect operator inside a "path" (`/1>`, `2>&1`, `b>c`).
    if token.contains('>') || token.contains('<') {
        return true;
    }
    // Regex / escape metacharacters that never appear in a real path token.
    if token.contains('\\') || token.contains('|') || token.contains('^') {
        return true;
    }
    // A BACKTICK command substitution used as a redirect/operand target (`> `mktemp``, a
    // `cmd 2> `mktemp -t err``): `shell_mask` masks the backtick BODY but leaves the
    // structural backticks in the original slice, so the verbatim `` `mktemp` `` token can
    // reach here. A command substitution is never a literal on-disk path — reject any token
    // bearing a backtick (the verb-dispatch path already drops a FULLY-masked token, but a
    // redirect target is sliced from the original and never passes through that guard).
    if token.contains('`') {
        return true;
    }
    // An unbalanced bracket/paren/brace REMAINING after the tail trim → still structural.
    if token.matches('(').count() != token.matches(')').count()
        || token.matches('[').count() != token.matches(']').count()
        || token.matches('{').count() != token.matches('}').count()
    {
        return true;
    }
    // A `=`-led comparison/value fragment (`=1.94`, `=2980`, `=`) is never a path.
    if token.starts_with('=') {
        return true;
    }
    // A pure number (`0`, `7000`) is never a path (a bare integer reaching the path slot
    // is always a mis-attributed numeric arg, e.g. `head -c 9`).
    if !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    // An embedded-CODE shard: a `,` (function-call arg list) or a trailing `:` (a dict /
    // label) inside a token that has NO `/` separator — a real bare relative FILE never
    // carries these. Tokens WITH a `/` are paths and are left alone (a path may legally
    // contain a `,` or `:` in rare cases).
    if !token.contains('/') && (token.contains(',') || token.ends_with(':')) {
        return true;
    }
    false
}

/// Stricter sibling of [`path_operand`] for the precision-sensitive NEW emitters
/// (`--flag=<path>`, `dd of=`, `curl -o`): in addition to every [`path_operand`]
/// rejection, it ALSO drops a glob (`*`/`?`/`[`) — those output paths are written by a
/// single tool to ONE concrete destination, so a wildcard there is a parse artifact,
/// not a real touched set. Returns only a concrete, resolvable path.
pub(crate) fn concrete_path(token: &str) -> Option<String> {
    let path = path_operand(token)?;
    if path.contains(['*', '?', '[']) {
        return None; // a glob is not a concrete single destination here.
    }
    Some(path)
}

/// True when a token carries an UNRESOLVED shell expansion we cannot perform — a variable
/// reference (`$NAME`, `${NAME}`, `$1`, …) OR a leading `~`/`~user` home expansion (`~/x`,
/// `~`). Such a token can never be turned into the real on-disk path without the runtime
/// environment, so emitting it verbatim would fabricate a path that does not exist as
/// written (the module doc's precision contract, line 39). The `~` test fires only on a
/// LEADING `~` (a mid-path `~` like `/tmp/a~b` is a literal backup-file char, kept).
pub(crate) fn has_unresolved_var(token: &str) -> bool {
    token.contains('$') || token.starts_with('~')
}

/// Strip a single matched pair of surrounding single or double quotes.
pub(crate) fn strip_quotes(token: &str) -> &str {
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
