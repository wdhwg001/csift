//! Shell masking: quotes/substitutions to MASK_CHAR, segment split, token walk.

/// A "shell mask" of a command: a byte-for-byte parallel string in which every character
/// that lives INSIDE a single/double-quoted span, or inside a `>(…)` / `<(…)` process-
/// substitution BODY, is replaced by [`MASK_CHAR`] (a control byte that occurs in no real
/// command and is whitespace to no tokenizer). The surrounding quotes / procsub head/tail
/// punctuation are themselves left intact, so token boundaries are preserved.
///
/// Why: the parser is otherwise quote-UNAWARE, so a `>`/`<` inside a quoted echo/printf
/// prose or grep regex (`echo "idle >8min"`, `grep 'cur > base'`) was read as a real
/// redirect and the next quoted word fabricated as a file; likewise a process-sub body
/// `tee >(grep foo) /real.log` leaked `foo` as a file. Masking the INTERIOR (not the bytes
/// length, so positions still line up with the original) lets operator/redirect detection
/// run on the mask — where those inner `>`/words are now [`MASK_CHAR`], invisible — while
/// the ORIGINAL bytes are still sliced for the emitted path (so a genuinely-quoted single
/// redirect target `> "/tmp/a.txt"` still resolves via `strip_quotes`). This kills the
/// quoted-`>` fabrication class without rewriting the whitespace tokenizer.
pub(crate) const MASK_CHAR: char = '\u{1}';

/// The masked byte: `0x01` (the `MASK_CHAR` codepoint, one ASCII byte). Used so the mask is
/// built byte-for-byte (BYTE-LENGTH-PRESERVING vs the input — critical for the parallel
/// offsets used by `masked_tokens` / `split_segments`).
pub(crate) const MASK_BYTE: u8 = 0x01;

/// Build the [`MASK_CHAR`] mask for a command (BYTE-length-identical to the input, so any
/// offset/slice valid on one is valid on the other). Quote state and several bracket spans
/// are tracked with a tiny lexical scanner:
/// - a QUOTE / BACKTICK-cmdsub span opens on an unescaped `'`/`"`/`` ` `` and closes on the
///   matching delimiter (a backtick command substitution is masked like a quote, so an
///   inner `>` redirect never leaks and the closing backtick never glues onto a path);
/// - a `>(`/`<(` PROCSUB body opens and its depth is balanced by `(`/`)`;
/// - a `((` ARITHMETIC span and a `[[` TEST span open on the double bracket and mask their
///   interior `>`/`<` (which are comparison operators there, never redirects, so
///   `(( a > b ))` / `[[ a > b ]]` neither fabricate a file nor read a redirect).
///
/// Inside ANY span, EVERY byte is masked to [`MASK_BYTE`] (so a multi-byte UTF-8 char inside
/// a span masks to N mask-bytes — the span boundaries are ASCII delimiters, which sit on
/// char boundaries, so the result is still valid UTF-8). Outside spans, the ORIGINAL bytes
/// are copied verbatim (the whole multi-byte char, never `byte as char` which would corrupt
/// the length). A single-paren subshell / `$(…)` command substitution is intentionally NOT
/// masked (its body may legitimately mutate files, e.g. `(touch f)`). Best-effort: it does
/// not model `\`-escapes outside double quotes precisely, acceptable for a heuristic.
pub(crate) fn shell_mask(command: &str) -> String {
    let bytes = command.as_bytes();
    let mut mask: Vec<u8> = Vec::with_capacity(command.len());
    let mut quote: Option<u8> = None; // the open quote byte (`'`/`"`/backtick), if in a span
    let mut procsub_depth: usize = 0; // `(` nesting inside an open `>(`/`<(` body
    let mut arith_depth: usize = 0; // `(` nesting inside an open `((` arithmetic span
    let mut test_depth: usize = 0; // `[` nesting inside an open `[[` test span
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            // Inside a quoted / backtick-command-substitution span: mask every interior byte;
            // the matching close (same byte) ends it. A backtick command substitution
            // (`` `date > f` ``) is masked exactly like a quote, so an embedded `>` redirect
            // INSIDE it never reaches `collect_redirections` — and the closing backtick never
            // survives glued onto a path (the `/tmp/bt.log\`` corruption class).
            if c == q {
                quote = None;
                mask.push(c); // the closing delimiter itself is structural, not masked.
            } else {
                mask.push(MASK_BYTE);
            }
            i += 1;
            continue;
        }
        if procsub_depth > 0 {
            // Inside a process-sub body: mask EVERY interior byte (incl. the matching close
            // `)`), tracking `(`/`)` nesting so the outermost close ends the span. Masking the
            // closing `)` too means a body token like `foo)` from `>(grep foo)` becomes fully
            // masked and is dropped — without it, the surviving `)` would let
            // `trim_structural_tail` peel back to `foo` and fabricate a file.
            match c {
                b'(' => procsub_depth += 1,
                b')' => procsub_depth -= 1,
                _ => {}
            }
            mask.push(MASK_BYTE);
            i += 1;
            continue;
        }
        if arith_depth > 0 {
            // Inside a `(( … ))` arithmetic span: the `>`/`<` are COMPARISON operators, never
            // redirects, and the operands are numbers/identifiers, never files. Mask every
            // interior byte (incl. the balancing parens) so `(( a > b ))` neither fabricates a
            // file `b` nor reads `>` as a redirect. Depth tracks `(`/`)` so a nested
            // `(( (a) > b ))` closes correctly.
            match c {
                b'(' => arith_depth += 1,
                b')' => arith_depth -= 1,
                _ => {}
            }
            mask.push(MASK_BYTE);
            i += 1;
            continue;
        }
        if test_depth > 0 {
            // Inside a `[[ … ]]` test span: `>`/`<` are lexicographic comparisons, not
            // redirects; mask the interior (incl. the balancing brackets) so a `[[ a > b ]]`
            // never surfaces a redirect/fabricated file. Depth tracks `[`/`]`.
            match c {
                b'[' => test_depth += 1,
                b']' => test_depth -= 1,
                _ => {}
            }
            mask.push(MASK_BYTE);
            i += 1;
            continue;
        }
        // Outside any span. An unquoted `#` at a WORD BOUNDARY opens a shell comment that
        // runs to end-of-line: mask the whole `# … \n` tail so the comment words never
        // become tokens (an in-comment `>`/`>>` redirect, a `;`/`|` operator, and every
        // comment word are all masked, exactly like a heredoc body). `#` is a comment ONLY
        // at a word start — preceded by start-of-input, whitespace, or a command separator
        // (`;` `|` `&` `(` `\n`); guarding on that keeps an IN-PATH `#` (`/tmp/a#b`,
        // `file#1`, where the prev byte is a path char) intact. The boundary set excludes
        // `<`/`>` so a literal redirect target `> #file` is not mistaken for a comment.
        if c == b'#'
            && (i == 0
                || matches!(
                    bytes[i - 1],
                    b' ' | b'\t' | b'\n' | b'\r' | b';' | b'|' | b'&' | b'('
                ))
        {
            // Mask `#` through the byte before the next newline (the newline itself is a
            // structural segment separator and stays unmasked so `split_segments` still
            // sees it). End-of-input ends the comment too.
            while i < bytes.len() && bytes[i] != b'\n' {
                mask.push(MASK_BYTE);
                i += 1;
            }
            continue;
        }
        // A `>(`/`<(` opens a process-sub body.
        if (c == b'>' || c == b'<') && bytes.get(i + 1) == Some(&b'(') {
            mask.push(c); // keep the `>(`/`<(` head so `has_syntax_noise` still sees it.
            mask.push(b'(');
            procsub_depth = 1;
            i += 2;
            continue;
        }
        // A `((` opens an arithmetic span (NOT a `$((` — but the leading `$` is copied
        // verbatim above this and the `((` still opens here, which is correct: the interior
        // is masked either way). The two parens are kept structural; the body is masked.
        if c == b'(' && bytes.get(i + 1) == Some(&b'(') {
            mask.push(b'(');
            mask.push(b'(');
            arith_depth = 1;
            i += 2;
            continue;
        }
        // A `[[` opens a test span.
        if c == b'[' && bytes.get(i + 1) == Some(&b'[') {
            mask.push(b'[');
            mask.push(b'[');
            test_depth = 1;
            i += 2;
            continue;
        }
        if c == b'\'' || c == b'"' || c == b'`' {
            quote = Some(c);
            mask.push(c); // the opening delimiter is structural.
            i += 1;
            continue;
        }
        mask.push(c); // copy the verbatim byte (whole multi-byte chars stay intact).
        i += 1;
    }
    // Valid UTF-8: unmasked bytes are copied verbatim and masked spans only ever cover whole
    // chars (their boundaries are ASCII quotes/parens). `from_utf8` therefore never fails;
    // the fallback keeps us panic-free even on a pathological input.
    String::from_utf8(mask).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// Split a command into segments on the shell sequencing/pipe operators and newlines,
/// QUOTE/PROCSUB-AWARE via `mask` (a parallel string where in-quote / in-procsub bytes are
/// [`MASK_CHAR`]). An operator is honored only where the mask still shows it — a `;`/`|`
/// inside a quoted string is masked, so it no longer splits (fixing the prior "a `;` inside
/// a quote splits" limitation as a side benefit). Each returned slice is taken from the
/// ORIGINAL `command` (so the segment text is verbatim).
pub(crate) fn split_segments<'a>(command: &'a str, mask: &str) -> Vec<&'a str> {
    let mbytes = mask.as_bytes();
    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < mbytes.len() {
        let two = mbytes.get(i..i + 2);
        // `>|` is bash's noclobber-OVERRIDE truncate redirect, NOT a pipe — the `|` must
        // not split the segment (else the redirect path is orphaned). Skip both bytes so
        // the `>|<path>` stays intact for `collect_redirections` to read.
        if matches!(two, Some(b">|")) {
            i += 2;
            continue;
        }
        let is_two_op = matches!(two, Some(b"&&") | Some(b"||"));
        let is_one_op = matches!(mbytes[i], b';' | b'|' | b'\n');
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

/// One whitespace-split token of a segment, paired with its MASK (same length): `orig` is
/// the verbatim text used for the emitted path, `masked` is what operator/redirect tests
/// read so an in-quote / in-procsub `>`/`<`/word is invisible. A token whose masked form is
/// ALL [`MASK_CHAR`] (e.g. the `foo` from `>(grep foo)`) carries no real operand and is
/// dropped before path emission.
#[derive(Clone, Copy)]
pub(crate) struct MaskedTok<'a> {
    pub(crate) orig: &'a str,
    pub(crate) masked: &'a str,
}

/// Tokenize a segment (and its parallel mask) on whitespace as the MASK sees it, returning
/// `(orig, masked)` token pairs. Whitespace is detected on the MASK bytes (NOT the original):
/// an interior space inside a quoted / backtick / procsub span is [`MASK_BYTE`] (`0x01`) in
/// the mask — a non-whitespace byte — so a quoted `"a b"` / `'/dest dir/x'` stays ONE token,
/// and the ORIGINAL (verbatim, with its real space) is sliced for the emitted path. Reading
/// the boundary off the ORIGINAL bytes here was the bug that severed a quoted space-bearing
/// path mid-filename (`"…/Application Support/…"` → fabricated `Support/…`); the mask is the
/// only byte-stream where an in-quote space is non-whitespace. The mask is byte-length-
/// identical to the segment, so `[start..i]` slices both safely.
pub(crate) fn masked_tokens<'a>(segment: &'a str, mask: &'a str) -> Vec<MaskedTok<'a>> {
    let mbytes = mask.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0usize;
    while i < mbytes.len() {
        if mbytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < mbytes.len() && !mbytes[i].is_ascii_whitespace() {
            i += 1;
        }
        toks.push(MaskedTok {
            orig: &segment[start..i],
            masked: &mask[start..i],
        });
    }
    toks
}
