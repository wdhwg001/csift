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
//! Beyond the verb allowlist (`rm`/`mkdir`/`touch`/`tee`/`cp`/`mv`/`install`/`ln`/`rsync`/
//! `sed -i`/`git`) and plain `>`/`>>` redirects, it also reads:
//! - **fd-qualified redirects** — `2>`/`1>`/`&>` (+ `>>` forms), attached (`2>/tmp/x`)
//!   and spaced (`2> /tmp/x`), and the noclobber-override `>|`; a bare `2>&1` fd-dup and
//!   `/dev/null|stderr|stdout` sinks are NOT paths and are skipped.
//! - **`-t DIR` destination flag** — `cp`/`mv`/`install -t DIR src…` puts the written
//!   destination right after `-t` (sources LAST), so the `-t` value is the dest and every
//!   positional is a read source (without `-t`, the last positional is the dest).
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
//! above guarantees they never produce a WRONG row — heredoc BODY lines are lexically
//! skipped ([`strip_heredoc_bodies`]) BEFORE redirect/verb scanning, so a `>` or quote
//! inside the body can no longer be mis-read as a redirect (only the opener LINE, which
//! may carry a real trailing `> file`, is scanned).
//!
//! A trailing `# …` SHELL COMMENT is likewise lexically masked ([`shell_mask`]) before any
//! token/redirect scan: an unquoted `#` at a word boundary opens a comment to end-of-line,
//! so neither the comment words, an in-comment `>` redirect target, nor an in-comment `;`/`|`
//! operator can fabricate a row OR displace a real cp/mv/ln destination (`cp src dst # note`
//! reports `dst`, not `note`). An IN-PATH `#` (`/tmp/a#b`) is preserved — `#` masks only when
//! it starts a token.

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
    // Strip heredoc BODY lines first: a `<<DELIM` body is opaque TEXT (often containing a
    // `>` or quote that a lexer would mis-read as a redirect, fabricating a path — a
    // DOUBLE failure since the real write inside the body is still missed). The opener
    // LINE is kept (a `… <<DELIM > file` carries a real redirect on the opener itself).
    let command = strip_heredoc_bodies(command);
    // Build a parallel QUOTE/PROCSUB mask once, then split + tokenize against it so an
    // in-quote / in-procsub `>`/`<`/word can never be read as a redirect operator or
    // fabricated as a file (the dominant remaining precision leak). See [`shell_mask`].
    let mask = shell_mask(&command);
    let mut out = Vec::new();
    for (segment, seg_mask) in split_segments(&command, &mask)
        .into_iter()
        .zip(split_segments(&mask, &mask))
    {
        parse_segment(segment, seg_mask, &mut out);
    }
    out
}

/// True when every byte of a token's mask is [`MASK_CHAR`] — i.e. the whole token
/// originated inside a quoted span or a process-sub body, so it is not a real operand.
fn is_fully_masked(masked: &str) -> bool {
    !masked.is_empty() && masked.chars().all(|c| c == MASK_CHAR)
}

/// Remove heredoc BODY lines from a multi-line command, keeping every non-body line
/// (including each heredoc OPENER line, which may carry its own trailing `> file`
/// redirect). A heredoc opens on a `<<DELIM` / `<<-DELIM` / `<<'DELIM'` / `<<"DELIM"`
/// token (quoted or not) and closes on a line whose trimmed content equals `DELIM` (a
/// `<<-` opener also accepts a tab-indented closer). Multiple heredocs on one line open
/// in left-to-right order. This is a lexical best-effort — sufficient to stop the body's
/// `>`/quote characters from fabricating redirect rows.
fn strip_heredoc_bodies(command: &str) -> String {
    if !command.contains("<<") {
        return command.to_string(); // fast path: no heredoc at all.
    }
    let mut out = String::with_capacity(command.len());
    let mut pending: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut active: Option<String> = None;
    let mut first = true;
    for line in command.split('\n') {
        if !first {
            out.push('\n');
        }
        first = false;
        if let Some(delim) = &active {
            // Inside a heredoc body: drop the line; the closer (trimmed == delim) ends it.
            if line.trim() == delim.as_str() {
                active = pending.pop_front();
            }
            continue; // body line (and the closer line) are not commands.
        }
        // Not inside a body: this is a command/opener line — keep it, and queue any
        // heredoc delimiters it opens so the FOLLOWING lines are dropped as bodies.
        out.push_str(line);
        for delim in heredoc_delims(line) {
            pending.push_back(delim);
        }
        if active.is_none() {
            active = pending.pop_front();
        }
    }
    out
}

/// The heredoc delimiters opened on one line, in order. Recognizes `<<WORD`, `<<-WORD`,
/// and quoted `<<'WORD'` / `<<"WORD"` (the quotes are stripped from the closer-comparison
/// delimiter, matching bash). A `<<<` here-STRING is NOT a heredoc (no body line) and is
/// ignored.
fn heredoc_delims(line: &str) -> Vec<String> {
    let mut delims = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'<' && bytes[i + 1] == b'<' {
            // `<<<` is a here-string, not a heredoc — skip it.
            if bytes.get(i + 2) == Some(&b'<') {
                i += 3;
                continue;
            }
            let mut j = i + 2;
            if bytes.get(j) == Some(&b'-') {
                j += 1; // `<<-` strips leading tabs from the closer (delim text unchanged).
            }
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            // The delimiter WORD: a quoted `'WORD'`/`"WORD"` or a bare run up to whitespace
            // or a shell metacharacter.
            let (delim, next) = read_heredoc_word(&line[j..]);
            if !delim.is_empty() {
                delims.push(delim);
            }
            i = j + next;
        } else {
            i += 1;
        }
    }
    delims
}

/// Read one heredoc delimiter word starting at `s`, returning (delimiter, bytes-consumed).
/// A quoted word strips its surrounding quotes (bash compares the closer to the unquoted
/// text); a bare word runs until whitespace or a shell metacharacter (`;|&<>` ).
fn read_heredoc_word(s: &str) -> (String, usize) {
    let bytes = s.as_bytes();
    if let Some(&q) = bytes.first() {
        if q == b'\'' || q == b'"' {
            if let Some(end_rel) = s[1..].find(q as char) {
                let word = &s[1..1 + end_rel];
                return (word.to_string(), 1 + end_rel + 1);
            }
        }
    }
    let mut k = 0usize;
    while k < bytes.len() {
        let c = bytes[k];
        if c.is_ascii_whitespace() || matches!(c, b';' | b'|' | b'&' | b'<' | b'>' | b'(' | b')') {
            break;
        }
        k += 1;
    }
    (s[..k].to_string(), k)
}

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
const MASK_CHAR: char = '\u{1}';

/// The masked byte: `0x01` (the `MASK_CHAR` codepoint, one ASCII byte). Used so the mask is
/// built byte-for-byte (BYTE-LENGTH-PRESERVING vs the input — critical for the parallel
/// offsets used by `masked_tokens` / `split_segments`).
const MASK_BYTE: u8 = 0x01;

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
fn shell_mask(command: &str) -> String {
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
fn split_segments<'a>(command: &'a str, mask: &str) -> Vec<&'a str> {
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
struct MaskedTok<'a> {
    orig: &'a str,
    masked: &'a str,
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
fn masked_tokens<'a>(segment: &'a str, mask: &'a str) -> Vec<MaskedTok<'a>> {
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

/// Parse one segment (with its parallel mask) — handle redirection targets anywhere, then
/// dispatch on the leading command verb.
fn parse_segment(segment: &str, mask: &str, out: &mut Vec<BashMutation>) {
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

/// `touch [opts] file…` — like [`emit_operands`] but with `touch`'s VALUE-taking flags
/// stripped first. `touch -d DATE`, `--date=DATE`, `-r REFFILE`, `--reference=REFFILE`,
/// and `-t STAMP` each consume a FOLLOWING token that is NOT a created path: `-r`'s value
/// is a READ-ONLY reference file, `-d`/`-t`'s value is a timestamp string. Without this,
/// `touch -r /ref/file out` would fabricate `/ref/file` as a created path (a phantom
/// mutation indistinguishable from a real one). `-t`'s pure-digit stamp is also caught by
/// `has_syntax_noise`'s number filter, but `-d`/`-r` values are not, so the skip is needed.
fn emit_touch(operands: &[&str], out: &mut Vec<BashMutation>) {
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
fn emit_tee(operands: &[&str], out: &mut Vec<BashMutation>) {
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
fn strip_value_flags<'a>(operands: Vec<&'a str>, value_flags: &[&str]) -> Vec<&'a str> {
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

/// Emit only the destination of a last-operand-dest verb (`ln`, `rsync`) — the LAST
/// positional operand. Validating that specific token (not "the last token that happens
/// to be a valid path") means a `… $DEST/x` whose real destination is an unexpandable
/// `$VAR` emits NOTHING, rather than wrongly reporting an earlier source.
fn emit_last_operand(operands: &[&str], verb: &'static str, out: &mut Vec<BashMutation>) {
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
fn emit_ln(operands: &[&str], out: &mut Vec<BashMutation>) {
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
fn emit_copy_like(operands: &[&str], verb: &'static str, out: &mut Vec<BashMutation>) {
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
fn target_directory_value<'a>(operands: &[&'a str]) -> Option<&'a str> {
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
fn emit_mv(operands: &[&str], out: &mut Vec<BashMutation>) {
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

/// `tar` with a CREATE flag writes an archive; emit that archive path (the inputs that
/// follow are READ, never mutated, so only the destination is reported — symmetric with
/// `zip`). Three idioms are handled:
/// - bundled short flags WITHOUT a dash: `tar czf <archive> …` — a create+file token like
///   `czf`/`cJf`/`tzf`(no `c`→skip). The archive is the NEXT operand;
/// - bundled short flags WITH a dash: `tar -czf <archive> …` — same, the archive is next;
/// - long flags: `tar --create --file=<archive>` (inline) or `--create --file <archive>`
///   (spaced).
///
/// A `-f<archive>`/`f<archive>` form where the path is glued to the flag bundle is also
/// supported (the path is the bundle tail after `f`). Only emits when a `c`/`--create` flag
/// is present, so `tar -xzf …` (extract) and `tar -tzf …` (list) write nothing.
fn emit_tar(operands: &[&str], out: &mut Vec<BashMutation>) {
    // Pass 1: detect a create flag and find where the archive path is.
    let mut has_create = false;
    let mut file_long_inline: Option<&str> = None; // `--file=<archive>`
    let mut want_file_next = false; // a `-…f` / `czf` / `--file` expects the NEXT operand
    let mut glued_file: Option<&str> = None; // `-czfARCHIVE` / `czfARCHIVE` tail after `f`
    let mut archive: Option<&str> = None;

    for op in operands {
        if want_file_next {
            archive = Some(op);
            want_file_next = false;
            continue;
        }
        if let Some(rest) = op.strip_prefix("--file=") {
            file_long_inline = Some(rest);
            continue;
        }
        if *op == "--file" {
            want_file_next = true;
            continue;
        }
        if *op == "--create" {
            has_create = true;
            continue;
        }
        if op.starts_with("--") {
            continue; // another long flag (`--gzip`, `--verbose`, …)
        }
        // A bundled short-flag group: `-czf`, `czf`, `-cf`, `cJf`, possibly with a glued
        // archive tail after `f` (`-czfARCHIVE`). Strip an optional leading `-`.
        let bundle = op.strip_prefix('-').unwrap_or(op);
        // Only treat it as a flag bundle if it is all flag letters (or has an `f`-glued
        // path). A bare input path (`src/`) has a `/` or is not a known flag-letter run.
        if let Some(fpos) = bundle.find('f') {
            // Letters before `f` are flags; bytes after `f` are a glued archive path (if any).
            let flags_part = &bundle[..fpos];
            if flags_part.contains('c') {
                has_create = true;
            }
            let tail = &bundle[fpos + 1..];
            if tail.is_empty() {
                want_file_next = true;
            } else {
                glued_file = Some(tail);
            }
        } else if bundle.chars().all(|c| c.is_ascii_alphabetic()) && bundle.contains('c') {
            // A create bundle with no `f` (archive goes to stdout) — nothing to emit, but
            // record the create so a separate `--file` could still apply.
            has_create = true;
        }
    }

    if !has_create {
        return;
    }
    let dest = archive.or(glued_file).or(file_long_inline);
    if let Some(d) = dest {
        if let Some(path) = concrete_path(d) {
            out.push(BashMutation { path, verb: "tar" });
        }
    }
}

/// Scan every token for an allowlisted output FLAG and emit its path. Two shapes:
/// `--name=<path>` (inline) and `--name <path>` (the value is the next token). Only the
/// [`OUTPUT_FLAGS`] names qualify, keeping precision tight. Independent of the segment's
/// leading verb (a test runner names its report path the same way regardless of verb).
fn collect_flag_outputs(tokens: &[MaskedTok], out: &mut Vec<BashMutation>) {
    let mut i = 0usize;
    while i < tokens.len() {
        let tok = tokens[i];
        // Flag NAME detection reads the MASK so a `--output` that is part of a quoted prose
        // arg is never treated as a real output flag; the path VALUE is sliced from the
        // original token (a quoted destination stays intact).
        if let Some(name_eq) = tok.masked.strip_prefix("--") {
            if let Some((name, _)) = name_eq.split_once('=') {
                // `--name=<path>` inline form. The `--name=` prefix is unmasked ASCII, so
                // its byte length is the same in the original — slice the value from there.
                if OUTPUT_FLAGS.contains(&name) {
                    let value = &tok.orig[name.len() + 3..]; // `--` + name + `=`
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
                    if !next.masked.starts_with('-') {
                        if let Some(path) = concrete_path(next.orig) {
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
fn collect_redirections(
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
fn trim_structural_tail(token: &str) -> &str {
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
fn has_syntax_noise(token: &str) -> bool {
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
fn concrete_path(token: &str) -> Option<String> {
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
fn has_unresolved_var(token: &str) -> bool {
    token.contains('$') || token.starts_with('~')
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
    fn touch_value_flags_skip_their_arguments() {
        // `-r REFFILE`: the reference file is READ-ONLY — only the real target is created.
        assert_eq!(
            paths("touch -r /ref/file /tmp/out.txt"),
            vec![("/tmp/out.txt".to_string(), "touch")],
            "the -r reference file must not be fabricated as a created path"
        );
        // `-d DATE`: the date string must not be emitted as a path.
        assert_eq!(
            paths("touch -d '2020-01-01' /tmp/out.txt"),
            vec![("/tmp/out.txt".to_string(), "touch")]
        );
        // `--reference=REF` inline form.
        assert_eq!(
            paths("touch --reference=/ref/f /tmp/out.txt"),
            vec![("/tmp/out.txt".to_string(), "touch")]
        );
        // `-t STAMP`: the timestamp is dropped (also caught by the digit-noise filter, but
        // the explicit skip is the precise reason).
        assert_eq!(
            paths("touch -t 202001010000 /tmp/out.txt"),
            vec![("/tmp/out.txt".to_string(), "touch")]
        );
    }

    #[test]
    fn tee_append_is_not_a_create() {
        // `tee -a` / `tee --append` do not truncate (mirrors `>>` vs `>`): verb `tee-a`.
        assert_eq!(
            paths("echo hi | tee -a /tmp/a.log"),
            vec![("/tmp/a.log".to_string(), "tee-a")]
        );
        assert_eq!(
            paths("echo hi | tee --append /tmp/b.log"),
            vec![("/tmp/b.log".to_string(), "tee-a")]
        );
        // Plain `tee` (truncate) stays a `tee` create.
        assert_eq!(
            paths("echo hi | tee /tmp/c.log"),
            vec![("/tmp/c.log".to_string(), "tee")]
        );
    }

    #[test]
    fn ln_target_directory_emits_dest_not_source() {
        // `ln -s -t DIR target`: the real destination is DIR; `target` is the read source.
        assert_eq!(
            paths("ln -s -t /tmp/linkdir /target"),
            vec![("/tmp/linkdir".to_string(), "ln")],
            "the -t DIR is the link destination; the source target must not be reported"
        );
        // `--target-directory=DIR` inline form.
        assert_eq!(
            paths("ln --target-directory=/tmp/d /src"),
            vec![("/tmp/d".to_string(), "ln")]
        );
        // Plain `ln -s target linkname`: last positional (the link) is the destination.
        assert_eq!(
            paths("ln -s /target /tmp/link"),
            vec![("/tmp/link".to_string(), "ln")]
        );
    }

    #[test]
    fn tar_create_emits_archive() {
        // `tar czf` creates the archive (verb `tar`); the source dir is not a written path.
        assert_eq!(
            paths("tar czf /tmp/a.tar.gz src/"),
            vec![("/tmp/a.tar.gz".to_string(), "tar")]
        );
    }

    #[test]
    fn trailing_comment_words_are_not_fabricated_paths() {
        // A `# words` comment after a real mutation must NOT leak its words (nor the bare
        // `#`) as touched paths — only the genuine operand survives.
        assert_eq!(
            paths("touch /tmp/real.txt  # create the marker file"),
            vec![("/tmp/real.txt".to_string(), "touch")]
        );
        assert_eq!(
            paths("rm -rf /tmp/build && mkdir /tmp/build  # rebuild from scratch"),
            vec![
                ("/tmp/build".to_string(), "rm"),
                ("/tmp/build".to_string(), "mkdir")
            ]
        );
    }

    #[test]
    fn in_comment_redirect_does_not_fabricate_a_path() {
        // A `>`/`>>` that appears INSIDE a trailing comment is masked with the comment, so
        // it is not read as a real redirect — only the genuine `mkdir` dir survives.
        assert_eq!(
            paths("mkdir -p /tmp/d # comment > /tmp/fabricated-by-comment-redirect.txt"),
            vec![("/tmp/d".to_string(), "mkdir")]
        );
    }

    #[test]
    fn comment_does_not_displace_cp_destination() {
        // The comment must not steal the cp/mv/ln destination: the LAST positional after the
        // comment is masked away, so the true dest is the surviving last operand.
        assert_eq!(
            paths("cp /tmp/src.txt /tmp/real-dest.bak  # x"),
            vec![("/tmp/real-dest.bak".to_string(), "cp")]
        );
        // `mv a b` reports the dest (`mv`) AND the source (`mv-from`); the `# note` tail must
        // be masked away so it neither displaces the dest nor adds a phantom source.
        assert_eq!(
            paths("mv /tmp/a /tmp/b  # note"),
            vec![
                ("/tmp/b".to_string(), "mv"),
                ("/tmp/a".to_string(), "mv-from")
            ]
        );
    }

    #[test]
    fn in_path_hash_is_not_a_comment() {
        // A `#` that is part of a path (prev byte is a path char, not whitespace/separator)
        // is NOT a comment and the whole path is preserved.
        assert_eq!(
            paths("rm -f /tmp/a#b.txt"),
            vec![("/tmp/a#b.txt".to_string(), "rm")]
        );
        assert_eq!(
            paths("touch file#1.log"),
            vec![("file#1.log".to_string(), "touch")]
        );
    }

    #[test]
    fn comment_after_separator_without_space_is_masked() {
        // `cmd;# c` / `cmd|# c`: a `#` directly after a command separator (no space) still
        // opens a comment (the separator is a word boundary).
        assert_eq!(
            paths("touch /tmp/x.txt;# trailing"),
            vec![("/tmp/x.txt".to_string(), "touch")]
        );
    }

    #[test]
    fn full_line_leading_comment_emits_nothing() {
        // A line that is ENTIRELY a comment yields no rows (the verb itself is masked away).
        assert_eq!(paths("# just a note, nothing here"), vec![]);
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

    // ── R1: the DOMINANT garbage class — fd-redirect close-paren / process-sub leaks ──

    #[test]
    fn devnull_with_glued_close_paren_is_dropped() {
        // `$(… 2>/dev/null)` glues a `)` onto the sink → `/dev/null)`. The single most
        // common idiom in real sessions; it must NOT fabricate a `/dev/null)` row.
        assert!(paths(r#"RESOLVED="$(readlink -f x 2>/dev/null || true)""#).is_empty());
        assert!(paths("x=$(cmd 2>/dev/null)").is_empty());
        assert!(paths("diff <(a) <(b 2>/dev/null)").is_empty());
        // Doubled close parens (nested substitution) also drop.
        assert!(paths("y=$(f $(g 2>/dev/null))").is_empty());
    }

    #[test]
    fn real_redirect_path_with_trailing_struct_punct_kept_clean() {
        // A genuine redirect path with a glued statement terminator keeps the CLEAN path.
        assert_eq!(
            paths("echo x > /tmp/real.log;"),
            vec![("/tmp/real.log".to_string(), ">")]
        );
        assert_eq!(
            paths("(echo x > /tmp/sub.log)"),
            vec![("/tmp/sub.log".to_string(), ">")]
        );
    }

    #[test]
    fn process_substitution_redirect_emits_no_fragment() {
        // `> >(tee /tmp/x)` must NOT leak `>(tee` / `(tee` rows (the real inner path is a
        // documented recall miss, but precision is preserved — no garbage).
        let got = just_paths("cmd > >(tee /tmp/ps.log)");
        assert!(
            !got.iter()
                .any(|p| p.contains("tee") || p.contains('(') || p.contains('>')),
            "process-sub fragments leaked: {got:?}"
        );
        let got2 = just_paths("make 2> >(tee /tmp/err.log >&2)");
        assert!(
            !got2.iter().any(|p| p.contains("tee") || p.contains('(')),
            "process-sub fragments leaked: {got2:?}"
        );
    }

    #[test]
    fn quoted_path_with_metachar_or_space_does_not_leak_fragment() {
        // The mask now makes a quoted in-path `;` and space invisible to the tokenizer, so a
        // quoted span stays ONE token and the WHOLE path is recovered (no `'…`/`"…` fragment,
        // and no mid-filename split). Both forms below resolve to their full quoted content.
        let got = just_paths("echo x >> '/tmp/quoted; path.txt'");
        assert_eq!(got, vec!["/tmp/quoted; path.txt".to_string()]);
        let got2 = just_paths("cmd > 'output (final).txt'");
        assert_eq!(got2, vec!["output (final).txt".to_string()]);
        for got in [&got, &got2] {
            assert!(
                !got.iter()
                    .any(|p| p.starts_with('\'') || p.starts_with('"')),
                "quoted-split fragment leaked: {got:?}"
            );
        }
    }

    #[test]
    fn quoted_space_bearing_path_stays_one_token_not_a_partial() {
        // CRITICAL precision fix: a quoted redirect/operand path CONTAINING A SPACE (the most
        // common macOS path shape — Library/Application Support, Google Drive, My Documents)
        // must stay ONE token and be emitted WHOLE, never split mid-filename into a fabricated
        // partial (the prior `"…/Application Support/x"` → `Support/x` bug). Whitespace is now
        // read off the MASK, where an in-quote space is `0x01` (non-whitespace).
        assert_eq!(
            paths("cp config.json \"/Users/me/Library/Application Support/app/config.json\""),
            vec![(
                "/Users/me/Library/Application Support/app/config.json".to_string(),
                "cp"
            )]
        );
        assert_eq!(
            paths("rm -rf \"/Users/me/My Project/build\""),
            vec![("/Users/me/My Project/build".to_string(), "rm")]
        );
        assert_eq!(
            paths("mkdir -p \"/Users/me/Google Drive/notes\""),
            vec![("/Users/me/Google Drive/notes".to_string(), "mkdir")]
        );
        // Single-quoted redirect target with a space → full path via the `>` redirect verb.
        assert_eq!(
            paths("echo x > '/tmp/has space.txt'"),
            vec![("/tmp/has space.txt".to_string(), ">")]
        );
        // The bare `rm '/tmp/has space.txt'` operand also recovers the whole path.
        assert_eq!(
            paths("rm '/tmp/has space.txt'"),
            vec![("/tmp/has space.txt".to_string(), "rm")]
        );
    }

    #[test]
    fn backtick_command_substitution_target_is_not_a_path() {
        // A backtick command substitution used as a redirect/operand TARGET is never an
        // on-disk path. `shell_mask` masks the backtick body but the structural backticks
        // survive in the original slice, so `has_syntax_noise` must reject any backtick-
        // bearing token. No fabricated `` `mktemp` `` / `` `mktemp `` pseudo-path rows.
        assert!(paths("echo x > `mktemp`").is_empty());
        assert!(paths("cmd 2> `mktemp -t err`").is_empty());
        // A real redirect to a normal file alongside the backtick command is unaffected.
        assert_eq!(
            paths("echo `date` > /tmp/real.log"),
            vec![("/tmp/real.log".to_string(), ">")]
        );
    }

    #[test]
    fn redirect_metachar_and_fd_dup_tokens_are_not_paths() {
        // `2>&1` and a bare `>` never become file rows even when they reach path_operand.
        assert!(paths("pytest 2>&1").is_empty());
        assert_eq!(path_operand("2>&1"), None);
        assert_eq!(path_operand("/tmp/a>b"), None); // embedded redirect → noise.
    }

    // ── R1: cp/mv/install `-t DIR` correctness (source-vs-dest inversion) ──

    #[test]
    fn cp_target_directory_flag_dest_is_the_t_value_not_a_source() {
        // `cp -t DIR src…`: the DEST is DIR; the sources are reads, never reported as cp.
        assert_eq!(
            paths("cp -t /tmp/destdir /home/a.txt /home/b.txt"),
            vec![("/tmp/destdir".to_string(), "cp")]
        );
        // `--target-directory=DIR` inline form.
        assert_eq!(
            paths("cp --target-directory=/tmp/d /home/a.txt"),
            vec![("/tmp/d".to_string(), "cp")]
        );
        // install behaves the same.
        assert_eq!(
            paths("install -m755 -t /usr/local/bin a b"),
            vec![("/usr/local/bin".to_string(), "install")]
        );
    }

    #[test]
    fn mv_target_directory_flag_dest_and_sources() {
        // `mv -t DIR a b`: DIR is the destination (mv); a + b are mv-from sources; DIR is
        // NOT also listed as a source.
        let got = paths("mv -t /tmp/destdir /home/a.txt /home/b.txt");
        assert!(
            got.contains(&("/tmp/destdir".to_string(), "mv")),
            "got: {got:?}"
        );
        assert!(
            got.contains(&("/home/a.txt".to_string(), "mv-from")),
            "got: {got:?}"
        );
        assert!(
            got.contains(&("/home/b.txt".to_string(), "mv-from")),
            "got: {got:?}"
        );
        assert!(
            !got.iter()
                .any(|(p, v)| p == "/tmp/destdir" && *v == "mv-from"),
            "the -t DIR must not also be a source: {got:?}"
        );
    }

    // ── R1: recall — ln / install / rsync / >| ──

    #[test]
    fn ln_install_rsync_destinations_caught() {
        assert_eq!(
            paths("ln -s /src /tmp/link"),
            vec![("/tmp/link".to_string(), "ln")]
        );
        assert_eq!(
            paths("install -m755 bin /usr/local/bin/tool"),
            vec![("/usr/local/bin/tool".to_string(), "install")]
        );
        assert_eq!(
            paths("rsync -a src/ /tmp/dest/"),
            vec![("/tmp/dest/".to_string(), "rsync")]
        );
    }

    #[test]
    fn noclobber_override_redirect_caught() {
        // `>|` is a force-truncate redirect; the `|` must not split off the path.
        assert_eq!(
            paths("echo x >| /tmp/forced.txt"),
            vec![("/tmp/forced.txt".to_string(), ">")]
        );
        // Attached form.
        assert_eq!(
            paths("echo x >|/tmp/forced2.txt"),
            vec![("/tmp/forced2.txt".to_string(), ">")]
        );
        // fd-qualified `2>|`.
        assert_eq!(
            paths("svc 2>| /tmp/e.log"),
            vec![("/tmp/e.log".to_string(), ">")]
        );
    }

    // ── R7: a trailing OUTPUT redirect must not poison a positional-dest verb ──
    // collect_redirections now REMOVES the redirect tokens (operator + its spaced path) from
    // the operand stream before verb dispatch — symmetric to strip_input_redirects for `<`.
    // Without this, the surviving `2>&1` / `2>/dev/null` / spaced `> file` token displaced the
    // real cp/mv/ln/install/rsync destination (RECALL MISS), got mislabeled as a source
    // (SEMANTIC LEAK), or double-emitted the redirect path (DOUBLE-EMIT).

    #[test]
    fn cp_dest_survives_trailing_fd_dup_redirect() {
        // `2>&1` is an fd-dup (no path); it must NOT become cp's positional.last() dest. The
        // REAL dest `/tmp/CP_DEST.txt` is emitted exactly once, the source is a read.
        assert_eq!(
            paths("cp src.txt /tmp/CP_DEST.txt 2>&1"),
            vec![("/tmp/CP_DEST.txt".to_string(), "cp")]
        );
    }

    #[test]
    fn install_ln_rsync_dest_survive_trailing_redirect() {
        // install with `2>&1`: the real dest /etc/DEST.conf survives.
        assert_eq!(
            paths("install -m 644 s /etc/DEST.conf 2>&1"),
            vec![("/etc/DEST.conf".to_string(), "install")]
        );
        // ln -s with `2>/dev/null`: the link name survives (the dev-sink emits no row).
        assert_eq!(
            paths("ln -s t /tmp/LINKNAME 2>/dev/null"),
            vec![("/tmp/LINKNAME".to_string(), "ln")]
        );
        // rsync with `2>&1`: the dest dir survives.
        assert_eq!(
            paths("rsync -a src/ /tmp/RSYNC_DEST/ 2>&1"),
            vec![("/tmp/RSYNC_DEST/".to_string(), "rsync")]
        );
    }

    #[test]
    fn mv_into_dir_not_leaked_as_source_by_trailing_redirect() {
        // `mv /a /b /tmp/DESTDIR/ 2>/dev/null`: the spaced/attached `2>/dev/null` is a dev-sink
        // (emits no row) AND is removed from operands, so DESTDIR stays the dest (verb `mv`) and
        // /a,/b stay sources (mv-from). Before the fix, `2>/dev/null` became the dest_tok
        // (rejected as a sink → NO mv dest) and ALL of /a,/b,DESTDIR leaked as mv-from reads.
        assert_eq!(
            paths("mv /a /b /tmp/DESTDIR/ 2>/dev/null"),
            vec![
                ("/tmp/DESTDIR/".to_string(), "mv"),
                ("/a".to_string(), "mv-from"),
                ("/b".to_string(), "mv-from")
            ]
        );
    }

    #[test]
    fn cp_dest_survives_trailing_append_redirect_no_double_emit() {
        // `cp a b /tmp/CDEST/ >> /tmp/log.txt`: the `>>` and its spaced path are both consumed,
        // so /tmp/log.txt is emitted ONCE (by the redirect collector, verb `>>`) and the real
        // dest /tmp/CDEST/ stays cp's last positional — no double-emit, no dropped dest.
        assert_eq!(
            paths("cp a b /tmp/CDEST/ >> /tmp/log.txt"),
            vec![
                ("/tmp/log.txt".to_string(), ">>"),
                ("/tmp/CDEST/".to_string(), "cp")
            ]
        );
    }

    #[test]
    fn cp_dest_survives_spaced_truncate_redirect() {
        // The spaced `> /tmp/log` form (operator + path are two tokens): both consumed.
        assert_eq!(
            paths("cp src /tmp/CP2/ > /tmp/log"),
            vec![
                ("/tmp/log".to_string(), ">"),
                ("/tmp/CP2/".to_string(), "cp")
            ]
        );
    }

    // ── R1: heredoc body is SKIPPED, not mis-reported (the never-mis-reported contract) ──

    #[test]
    fn heredoc_body_redirect_char_does_not_fabricate_a_row() {
        // A `>` inside a heredoc body must NOT become a redirect row, and the opener's own
        // real trailing redirect (if any) IS still caught.
        let body_only = "python3 - <<'PY'\nprint('a > b')\nopen('/tmp/real.json','w')\nPY";
        let got = just_paths(body_only);
        assert!(
            !got.iter()
                .any(|p| p.contains("b'") || p.contains('>') || p == "/tmp/real.json"),
            "heredoc body leaked a row: {got:?}"
        );
        // Opener-line redirect survives heredoc-body stripping.
        let with_redirect = "cat <<EOF > /tmp/out.txt\nbody > not a redirect\nEOF";
        assert_eq!(
            just_paths(with_redirect),
            vec!["/tmp/out.txt".to_string()],
            "opener-line redirect must be caught; body `>` must not"
        );
    }

    #[test]
    fn heredoc_quoted_and_dash_delim_forms() {
        // Quoted delimiter `<<'EOF'` and tab-stripping `<<-EOF` both close correctly.
        let q = "cat <<'EOF'\n> garbage > here\nEOF\ntouch /tmp/after.txt";
        let got = just_paths(q);
        assert!(
            got.contains(&"/tmp/after.txt".to_string()),
            "post-heredoc cmd lost: {got:?}"
        );
        assert!(
            !got.iter().any(|p| p.contains("garbage")),
            "body leaked: {got:?}"
        );
        // here-string `<<<` is NOT a heredoc (no body) — the command still parses normally.
        assert_eq!(
            just_paths("grep x <<< 'data' > /tmp/hs.txt"),
            vec!["/tmp/hs.txt".to_string()]
        );
    }

    #[test]
    fn trim_structural_tail_and_syntax_noise_helpers() {
        // Direct coverage of the two precision helpers.
        assert_eq!(trim_structural_tail("/dev/null)"), "/dev/null");
        assert_eq!(trim_structural_tail("/tmp/x;"), "/tmp/x");
        assert_eq!(trim_structural_tail("/tmp/x))"), "/tmp/x");
        // A balanced paren in the name is left intact.
        assert_eq!(trim_structural_tail("/tmp/(x)"), "/tmp/(x)");
        assert!(has_syntax_noise("'/tmp/x")); // unbalanced single quote
        assert!(has_syntax_noise("\"/tmp/x")); // unbalanced double quote
        assert!(has_syntax_noise(">(tee")); // process-sub head (`>(`)
        assert!(has_syntax_noise("<(cat")); // process-sub head (`<(`)
        assert!(has_syntax_noise("(subshell")); // bare `(` head
        assert!(has_syntax_noise("/a>b")); // embedded `>` redirect
        assert!(has_syntax_noise("/a<b")); // embedded `<` redirect
        assert!(has_syntax_noise("/a\\b")); // backslash escape
        assert!(has_syntax_noise("a|b")); // pipe metachar
        assert!(has_syntax_noise("a^b")); // caret metachar
        assert!(has_syntax_noise("/tmp/[unbalanced")); // unbalanced bracket
        assert!(has_syntax_noise("/tmp/{unbalanced")); // unbalanced brace
        assert!(has_syntax_noise("=value")); // `=`-led fragment
        assert!(has_syntax_noise("12345")); // pure number
        assert!(has_syntax_noise("a,b")); // comma, no slash → code shard
        assert!(has_syntax_noise("label:")); // trailing colon, no slash
        assert!(!has_syntax_noise("/tmp/clean.txt"));
        assert!(!has_syntax_noise("*.tmp")); // a plain glob is NOT noise
        assert!(!has_syntax_noise("src/main.rs")); // a real relative path with `/`
        assert!(!has_syntax_noise("")); // empty is not (pure-number guard skips empty)
    }

    #[test]
    fn residual_noise_classes_rejected_real_relpaths_kept() {
        // `=`-led value fragments, pure numbers, and code shards (comma / trailing colon
        // with no `/`) are rejected — the residual `by-file` garbage classes.
        assert!(has_syntax_noise("=1.94"));
        assert!(has_syntax_noise("=2980"));
        assert!(has_syntax_noise("="));
        assert!(has_syntax_noise("7000"));
        assert!(has_syntax_noise("0"));
        assert!(has_syntax_noise("o.get('x',0):")); // comma + trailing colon, no slash → code shard
        assert!(has_syntax_noise("turn,t.get")); // comma, no slash → code shard
        assert!(has_syntax_noise("label:")); // trailing colon, no slash
                                             // Genuine RELATIVE paths must survive (no `/`-free code-shard false positive).
        assert!(!has_syntax_noise("src/main.rs"));
        assert!(!has_syntax_noise("Cargo.toml"));
        assert!(!has_syntax_noise("paper.pdf"));
        assert!(!has_syntax_noise("err.log"));
        assert!(!has_syntax_noise("build_turns_fixture.py"));
        // A path containing a colon/comma WITH a slash is left alone (rare but legal).
        assert!(!has_syntax_noise("/tmp/a:b"));
    }

    #[test]
    fn version_compare_and_numeric_args_do_not_become_files() {
        // End-to-end: a `=`-led or numeric token never surfaces as a redirect/operand row.
        assert!(just_paths("python -c 'x' > =1.94").is_empty());
        // A real path on the same redirect still surfaces.
        assert_eq!(
            just_paths("echo x > /tmp/ok.txt"),
            vec!["/tmp/ok.txt".to_string()]
        );
    }

    #[test]
    fn path_operand_post_trim_rejections() {
        // After the trailing-tail trim, the re-checked sink + fd-dup-remnant + syntax-noise
        // rejections each fire (the `/dev/null)` family, a `&1` remnant, a noisy token).
        assert_eq!(path_operand("2>/dev/null)"), None); // trims `)` → sink → None
        assert_eq!(path_operand("/dev/stderr)"), None);
        assert_eq!(path_operand("&1"), None); // fd-dup remnant
        assert_eq!(path_operand("'/tmp/x"), None); // unbalanced quote noise
                                                   // A clean path passes (with a trailing `;` peeled).
        assert_eq!(path_operand("/tmp/clean;").as_deref(), Some("/tmp/clean"));
        // A bare `-` (stdin/stdout) and an empty token are not paths.
        assert_eq!(path_operand("-"), None);
        assert_eq!(path_operand(""), None);
    }

    #[test]
    fn trim_structural_tail_brace_and_balanced_cases() {
        // The `}` unbalanced-brace arm (a `${VAR}`-substitution close glued on).
        assert_eq!(trim_structural_tail("/tmp/x}"), "/tmp/x");
        // A balanced `{…}` is left intact (no over-trim).
        assert_eq!(trim_structural_tail("/tmp/{a}"), "/tmp/{a}");
        // Mixed trailing punctuation peeled in sequence.
        assert_eq!(trim_structural_tail("/tmp/x;}"), "/tmp/x");
        // A clean path with no trailing structure is returned unchanged.
        assert_eq!(trim_structural_tail("/tmp/clean"), "/tmp/clean");
    }

    #[test]
    fn target_directory_value_all_forms() {
        // `-t DIR`, `--target-directory DIR`, `--target-directory=DIR`, and absent.
        assert_eq!(target_directory_value(&["-t", "/d", "a"]), Some("/d"));
        assert_eq!(
            target_directory_value(&["--target-directory", "/d", "a"]),
            Some("/d")
        );
        assert_eq!(
            target_directory_value(&["--target-directory=/d", "a"]),
            Some("/d")
        );
        assert_eq!(target_directory_value(&["a", "b"]), None);
        // `-t` at the very end with no value → None (the `get(i+1)` None arm).
        assert_eq!(target_directory_value(&["a", "-t"]), None);
    }

    #[test]
    fn cp_install_without_t_flag_uses_last_operand() {
        // The non-`-t` path of emit_copy_like (last positional is the dest).
        assert_eq!(paths("cp a b /dest"), vec![("/dest".to_string(), "cp")]);
        assert_eq!(
            paths("install -m644 src /etc/conf"),
            vec![("/etc/conf".to_string(), "install")]
        );
        // `-T`/`--no-target-directory` (forces 2-operand) → default last-operand path.
        assert_eq!(
            paths("cp -T src /dest/file"),
            vec![("/dest/file".to_string(), "cp")]
        );
    }

    #[test]
    fn heredoc_multiple_delims_and_unclosed_quote() {
        // Two heredocs opened on one line: both bodies dropped, in order.
        let multi = "cat <<A <<B\nbodyA\nA\nbodyB\nB\ntouch /tmp/end.txt";
        let got = just_paths(multi);
        assert!(
            got.contains(&"/tmp/end.txt".to_string()),
            "trailing cmd lost: {got:?}"
        );
        assert!(
            !got.iter().any(|p| p.contains("body")),
            "body leaked: {got:?}"
        );
        // A simple heredoc whose body carries a stray `>` → no real path, no fabrication.
        let unterminated = "cat <<EOF\nx > y\nEOF";
        assert!(just_paths(unterminated).is_empty(), "no real path here");
    }

    #[test]
    fn mv_target_dir_when_dir_token_repeats_in_sources() {
        // The `src == dir` skip arm of emit_mv's `-t` path: the `-t` value is excluded
        // from the mv-from sources even though it is also a non-flag positional.
        let got = paths("mv -t /tmp/d /tmp/a /tmp/b");
        // /tmp/d is the destination (mv); a + b are sources; /tmp/d is NOT a source.
        assert!(got.contains(&("/tmp/d".to_string(), "mv")));
        assert!(got.contains(&("/tmp/a".to_string(), "mv-from")));
        assert!(got.contains(&("/tmp/b".to_string(), "mv-from")));
        assert_eq!(
            got.iter().filter(|(p, _)| p == "/tmp/d").count(),
            1,
            "the -t DIR appears exactly once (as mv), never as a source: {got:?}"
        );
    }

    #[test]
    fn heredoc_body_then_more_commands_after_closer() {
        // A heredoc whose closer is followed by MORE commands exercises the active→None
        // transition inside strip_heredoc_bodies (pop the body, resume scanning).
        let cmd =
            "cat <<EOF\nline one > fake\nline two\nEOF\nmkdir -p /tmp/after && touch /tmp/also";
        let got = just_paths(cmd);
        assert!(
            got.contains(&"/tmp/after".to_string()),
            "post-closer cmd lost: {got:?}"
        );
        assert!(
            got.contains(&"/tmp/also".to_string()),
            "post-closer cmd lost: {got:?}"
        );
        assert!(
            !got.iter().any(|p| p.contains("fake")),
            "body leaked: {got:?}"
        );
    }

    // ── R2: quote-aware redirect detection (the dominant remaining fabrication class) ──

    #[test]
    fn quoted_inline_redirect_does_not_fabricate_a_file() {
        // The MUST-FIX regression: a `>` inside a quoted echo/printf prose or a quoted regex
        // is NOT a real redirect, and the next word inside the quote is NOT a file. The
        // exact oracle commands that fabricated `*dt` / `8min` / `cover` / `base`.
        // A real redirect AFTER the quoted arg is still caught.
        assert_eq!(
            paths(r#"echo "  wf transcript idle >8min OR gone" > /tmp/realfile.txt"#),
            vec![("/tmp/realfile.txt".to_string(), ">")],
            "in-quote `>8min` must not fabricate `8min`; the real redirect is kept"
        );
        // A quoted grep -E regex carrying `> base` / `cur > base` fabricated `base` before.
        assert!(
            just_paths(r#"grep -nE 'cur > base' "$JSONL""#).is_empty(),
            "in-quote regex `>` must not fabricate a file"
        );
        assert!(
            just_paths(r#"grep -rnE "...|> *dt|interval" file.txt"#).is_empty(),
            "in-quote regex `> *dt` must not fabricate `*dt`"
        );
        // printf prose with a `>` (`x > cover`) — the em-dash / CJK-bearing class.
        assert!(
            just_paths(r#"printf 'layout x > cover scaled x'"#).is_empty(),
            "in-quote prose `>` must not fabricate a file"
        );
        // The captured monitor's `echo ">>> NO compact"` / `echo "  >> ABSENT"` family.
        assert!(just_paths(r#"echo ">>> NO compact""#).is_empty());
        assert!(just_paths(r#"echo "  >> ABSENT""#).is_empty());
        assert!(just_paths(r#"echo "  >> NOT assigned""#).is_empty());
    }

    #[test]
    fn process_sub_body_operand_does_not_leak() {
        // `tee >(grep foo) /tmp/real.log`: the real sink is kept; the procsub BODY arg `foo`
        // must NOT be fabricated as a `tee` file (its surviving `)` previously let
        // `trim_structural_tail` peel it back to `foo`).
        let got = paths("tee >(grep foo) /tmp/real.log");
        assert!(
            got.contains(&("/tmp/real.log".to_string(), "tee")),
            "real sink lost: {got:?}"
        );
        assert!(
            !got.iter().any(|(p, _)| p == "foo" || p.contains("grep")),
            "process-sub body leaked: {got:?}"
        );
        // Nested / fd-qualified procsub bodies likewise contribute no file.
        assert!(
            !just_paths("make 2> >(tee /tmp/e.log >&2) 1> >(cat)")
                .iter()
                .any(|p| p == "cat" || p.contains("tee>") || p == "foo"),
            "nested procsub body leaked"
        );
    }

    #[test]
    fn tilde_home_path_is_dropped_per_contract() {
        // The module-doc (line 39) precision contract: a `~`-bearing token yields nothing
        // (the shell would have expanded `~`; the literal `~/x` is not the real on-disk
        // path). Previously `has_unresolved_var` checked only `$`, so these leaked.
        assert!(paths("echo x > ~/notes.txt").is_empty());
        assert!(paths("cp a.txt ~/dest.txt").is_empty());
        assert!(paths("touch ~/scratch").is_empty());
        assert!(paths("dd if=/dev/zero of=~/img.bin").is_empty());
        // A LEADING `~` only: a mid-path literal `~` (a backup-file char) is still a path.
        assert_eq!(
            paths("rm /tmp/file~"),
            vec![("/tmp/file~".to_string(), "rm")]
        );
    }

    #[test]
    fn shell_mask_input_procsub_and_quote_arms() {
        // An INPUT process-sub `<(…)` (the `<` open arm of the mask) is masked just like
        // `>(…)`, so a `diff <(a) <(b)` leaks no inner word.
        let got = just_paths("diff <(sort x) <(sort y) > /tmp/diff.out");
        assert!(
            got.contains(&"/tmp/diff.out".to_string()),
            "real redirect lost: {got:?}"
        );
        assert!(
            !got.iter()
                .any(|p| p == "x" || p == "y" || p.contains("sort")),
            "input-procsub body leaked: {got:?}"
        );
        // A single-quoted span inside a double-quoted command and vice-versa: the OUTER
        // quote governs (the inner quote byte is just masked content), so neither inner
        // `>` fabricates a file.
        assert!(just_paths(r#"echo "it's a > test""#).is_empty());
        assert!(just_paths(r#"echo 'say "a > b"'"#).is_empty());
        // The mask preserves a real redirect that follows a fully-quoted argument.
        assert_eq!(
            paths(r#"printf '%s' "a > b" > /tmp/p.txt"#),
            vec![("/tmp/p.txt".to_string(), ">")]
        );
    }

    #[test]
    fn shell_mask_nested_procsub_and_fd_qualified_forms() {
        // A NESTED process-sub `>( … >(…) …)` exercises the `procsub_depth += 1` arm; the
        // whole nested body is masked, so no inner word leaks.
        let got = just_paths("tee >(grep a >(sort) b) /tmp/real.log");
        assert!(
            got.contains(&"/tmp/real.log".to_string()),
            "real sink lost: {got:?}"
        );
        assert!(
            !got.iter()
                .any(|p| p == "a" || p == "b" || p.contains("sort") || p.contains("grep")),
            "nested procsub body leaked: {got:?}"
        );
        // A fd-qualified `2>|file` attached form (the `>|` offset arm with a qualifier).
        assert_eq!(
            paths("svc 2>|/tmp/q.log"),
            vec![("/tmp/q.log".to_string(), ">")]
        );
        // A fd-qualified attached append `2>>file` and truncate `2>file` still slice the
        // path from the original at the post-qualifier offset.
        assert_eq!(
            paths("svc 2>>/tmp/a.log"),
            vec![("/tmp/a.log".to_string(), ">>")]
        );
        assert_eq!(
            paths("svc 2>/tmp/b.log"),
            vec![("/tmp/b.log".to_string(), ">")]
        );
    }

    #[test]
    fn shell_mask_is_byte_length_preserving_with_multibyte_utf8() {
        // REGRESSION: the mask must be BYTE-length-identical to the input even with CJK /
        // emoji inside (and outside) quotes — else `masked_tokens` slices on a non-char
        // boundary and panics (caught by the captured session, which is dense CJK).
        for cmd in [
            r#"echo "x x > cover x" > /tmp/out.txt"#,
            r#"printf 'x >per-mode x' && touch /tmp/x.txt"#,
            "grep -nE 'cur > base' x.txt",
            r#"echo "🛠 build >done" > /tmp/x"#,
            "tee >(grep x) /tmp/real.log",
        ] {
            let m = shell_mask(cmd);
            assert_eq!(
                m.len(),
                cmd.len(),
                "mask byte-length must equal input for {cmd:?}"
            );
            // And the parse itself must not panic + must keep only real paths.
            let got = just_paths(cmd);
            assert!(
                got.iter()
                    .all(|p| p.starts_with('/') || !p.chars().any(|c| c as u32 > 127)),
                "no CJK-prose fragment should surface as a file: {got:?}"
            );
        }
        // The real redirect after a CJK-bearing quoted arg is still caught.
        assert_eq!(
            just_paths(r#"echo "x > cover" > /tmp/cjk-ok.txt"#),
            vec!["/tmp/cjk-ok.txt".to_string()]
        );
    }

    #[test]
    fn quote_aware_split_does_not_break_on_in_quote_sequencing() {
        // A `;`/`|`/`&&` INSIDE a quoted string no longer splits the segment (a side
        // benefit of the mask), so a real trailing redirect after the quote is still found
        // and the in-quote operator fabricates nothing.
        assert_eq!(
            paths(r#"echo "a; b | c && d" > /tmp/seq.log"#),
            vec![("/tmp/seq.log".to_string(), ">")]
        );
    }

    #[test]
    fn shell_mask_and_helpers_direct_coverage() {
        // The mask masks quote interiors + procsub bodies, preserving byte length so offsets
        // line up with the original.
        let cmd = r#"echo "a>b" > /tmp/x"#;
        let m = shell_mask(cmd);
        assert_eq!(m.len(), cmd.len(), "mask is byte-length-preserving");
        // The in-quote `>` is masked; the real redirect `>` after the quote is intact.
        assert!(!m.contains("a>b"), "in-quote `>` should be masked: {m:?}");
        assert!(m.ends_with("> /tmp/x"), "real redirect kept: {m:?}");
        // A procsub body is fully masked (so its tokens drop).
        let m2 = shell_mask("tee >(grep foo) /tmp/x");
        assert!(!m2.contains("grep foo"), "procsub body masked: {m2:?}");
        assert!(m2.contains(">("), "the `>(` head is kept");
        // is_fully_masked: a body word is all-mask; a real word is not.
        let body = shell_mask(">(grep foo)"); // `>(` head kept, rest masked
        assert!(is_fully_masked(&body[2..]), "body after `>(` is all mask");
        assert!(!is_fully_masked("abc"));
        assert!(!is_fully_masked(""), "empty is not fully-masked");
        // has_unresolved_var now rejects a leading `~` as well as `$`.
        assert!(has_unresolved_var("~/x"));
        assert!(has_unresolved_var("$VAR"));
        assert!(!has_unresolved_var("/tmp/a~b")); // mid-path `~` is a literal
        assert!(!has_unresolved_var("/tmp/clean"));
    }

    #[test]
    fn heredoc_here_string_is_not_a_heredoc() {
        // `<<<` is a here-STRING (no body line) — heredoc_delims must skip it, so a
        // following command on the next line is NOT swallowed as a body.
        let hs = "grep x <<< 'data'\ntouch /tmp/post.txt";
        assert!(just_paths(hs).contains(&"/tmp/post.txt".to_string()));
    }

    #[test]
    fn heredoc_delims_direct_coverage() {
        // Direct coverage of the delimiter scanner's shapes.
        assert_eq!(heredoc_delims("cat <<EOF"), vec!["EOF".to_string()]);
        assert_eq!(heredoc_delims("cat <<-EOF"), vec!["EOF".to_string()]);
        assert_eq!(heredoc_delims("cat <<'Q'"), vec!["Q".to_string()]);
        assert_eq!(heredoc_delims("cat <<\"D\""), vec!["D".to_string()]);
        assert!(heredoc_delims("cat <<< here-string").is_empty());
        assert!(heredoc_delims("no heredoc here").is_empty());
        // `read_heredoc_word` bare-word stop at a metachar.
        assert_eq!(read_heredoc_word("EOF;rest").0, "EOF");
        // An unterminated quote has no closing `'`, so it falls through to the bare-word
        // scan, which keeps the leading `'` and runs to the end (no metachar).
        assert_eq!(read_heredoc_word("'unterminated").0, "'unterminated");
        // A command with NO `<<` at all → strip_heredoc_bodies fast path (no change).
        assert_eq!(strip_heredoc_bodies("echo hi > /tmp/x"), "echo hi > /tmp/x");
    }

    // ── Backtick command-substitution: an inner redirect must not corrupt a path ──

    #[test]
    fn backtick_cmdsub_inner_redirect_not_fabricated() {
        // A `>` redirect INSIDE a backtick command substitution is masked, so neither the
        // redirect target NOR a backtick-glued path (`/tmp/bt.log\``) is ever emitted.
        assert!(just_paths("echo `date > /tmp/bt.log`").is_empty());
        // An assignment whose RHS is a backtick cmdsub with an inner `>>` likewise emits
        // nothing (the whole backtick body is invisible to redirect detection).
        assert!(just_paths("x=`wc -l < f >> /tmp/bt2.log`").is_empty());
    }

    #[test]
    fn backtick_does_not_swallow_a_real_following_redirect() {
        // A real redirect OUTSIDE the backtick span is still detected: `echo \`date\` > f`
        // writes `f` (the backtick closes before the `>`).
        assert_eq!(
            just_paths("echo `date` > /tmp/real.log"),
            vec!["/tmp/real.log".to_string()]
        );
    }

    // ── Arithmetic `(( ))` and test `[[ ]]` comparison operators are not redirects ──

    #[test]
    fn arithmetic_comparison_does_not_fabricate_identifier() {
        // `(( a > b ))` is a comparison — the `>` is NOT a redirect, so the bare identifier
        // `b` must NOT be fabricated as a written file. (Before the arith-mask the `>` was
        // read as a redirect and `b` emitted.) The masked span emits NOTHING.
        assert!(
            just_paths("(( a > b ))").is_empty(),
            "arithmetic comparison must not fabricate a file"
        );
        // The same inside an `if … then … fi` wrapper: still no fabricated `b`.
        assert!(!just_paths("if (( a > b )); then echo hi; fi").contains(&"b".to_string()));
        // A numeric RHS likewise fabricates nothing.
        assert!(!just_paths("(( count > 5 ))").contains(&"5".to_string()));
        // A REAL redirect alongside the arithmetic is still detected (the `>` outside the
        // `(( ))` span writes `/tmp/r.log`).
        assert_eq!(
            just_paths("(( a > b )); echo done > /tmp/r.log"),
            vec!["/tmp/r.log".to_string()]
        );
    }

    #[test]
    fn test_double_bracket_comparison_is_not_a_redirect() {
        // `[[ a > b ]]` is a lexicographic comparison; the `>` is not a redirect and `y`
        // must not be fabricated. The masked test span emits nothing.
        assert!(
            just_paths("[[ x > y ]]").is_empty(),
            "[[ ]] comparison must not fabricate a file"
        );
        assert!(!just_paths("if [[ x > y ]]; then echo hi; fi").contains(&"y".to_string()));
    }

    // ── tar archive creation recall ──

    #[test]
    fn tar_create_emits_archive_dest() {
        // `-czf <archive>` (dashed bundle, spaced archive).
        assert_eq!(
            paths("tar -czf /tmp/arch.tar.gz src/"),
            vec![("/tmp/arch.tar.gz".to_string(), "tar")]
        );
        // `czf <archive>` (bundle without a leading dash).
        assert_eq!(
            paths("tar czf backup.tar.gz ."),
            vec![("backup.tar.gz".to_string(), "tar")]
        );
        // `-cf <archive>` (no compression flag).
        assert_eq!(
            paths("tar -cf out.tar a b c"),
            vec![("out.tar".to_string(), "tar")]
        );
        // Long-flag inline + spaced forms.
        assert_eq!(
            paths("tar --create --file=/tmp/x.tar dir/"),
            vec![("/tmp/x.tar".to_string(), "tar")]
        );
        assert_eq!(
            paths("tar --create --file /tmp/y.tar dir/"),
            vec![("/tmp/y.tar".to_string(), "tar")]
        );
        // A glued archive (`-czfARCHIVE`).
        assert_eq!(
            paths("tar -czf/tmp/glued.tgz src/"),
            vec![("/tmp/glued.tgz".to_string(), "tar")]
        );
    }

    #[test]
    fn tar_extract_or_list_writes_nothing() {
        // No create flag → no archive is written, so nothing is emitted.
        assert!(just_paths("tar -xzf /tmp/arch.tar.gz").is_empty());
        assert!(just_paths("tar -tzf /tmp/arch.tar.gz").is_empty());
        assert!(just_paths("tar tf archive.tar").is_empty());
    }

    #[test]
    fn tar_create_to_stdout_emits_nothing() {
        // A create bundle with NO `f` (archive → stdout, e.g. piped) writes no named file.
        assert!(just_paths("tar cz src/").is_empty());
        // `--create` long flag with no `--file` likewise has no destination to emit.
        assert!(just_paths("tar --create --gzip dir/").is_empty());
    }

    #[test]
    fn tar_file_without_create_writes_nothing() {
        // `-f <archive>` but NO create flag (`-rf` append is not a create) → no emit, since
        // `has_create` is false.
        assert!(just_paths("tar -rf /tmp/x.tar extra").is_empty());
        // A spaced `--file <archive>` with no `--create` likewise emits nothing.
        assert!(just_paths("tar --list --file /tmp/x.tar").is_empty());
    }
}
