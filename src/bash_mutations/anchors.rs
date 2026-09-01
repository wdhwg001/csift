//! Content-ANCHOR classification of one Bash command (the recover layer's v0.9.4
//! "bash reads are reads, bash writes are writes" upgrade).
//!
//! A small closed set of shell shapes carries DETERMINISTIC file content in the
//! transcript itself: a quoted-delimiter heredoc's body is byte-verbatim in the
//! tool_use input; `cat <file>` / `head -n N <file>` / `sed -n 'A,Bp' <file>` stdout
//! (under the caller's completeness gate) IS the file window; `echo`/`printf` with
//! purely literal arguments write known bytes; `truncate -s 0` writes the empty file.
//! Everything else stays in the boundary/heuristic lanes - correctness first, but
//! honesty about the decidable subset is not surrender on it.
//!
//! ADMISSION LAWS (each refusal falls back to today's behavior, never a wrong anchor):
//! - A READ anchor demands a SINGLE simple segment: a compound command's stdout is a
//!   concatenation nothing can attribute to one file.
//! - A WRITE anchor is SEGMENT-scoped: the dominant real shape is "write the file,
//!   then run it" (`cat > f <<'EOF' … EOF` followed by chained commands), so a write
//!   segment anchors inside a compound command - but the caller must then hold the
//!   whole command to a CLEAN result (exit ok AND empty stderr: a failing
//!   cat/tee/echo always writes stderr, so a clean echo proves the write landed even
//!   in a `;`/newline chain whose exit code only reflects the LAST command). The
//!   anchor's own segment must also be free of substitutions/subshells, and no OTHER
//!   part of the command may touch the same resolved path (`same_path_hits`).
//! - Every operand and every content token must be LITERAL: single-quoted verbatim,
//!   double-quoted with no `$`/backtick/backslash, or bare with no expansion/glob
//!   characters. A variable target or interpolated body is never guessed.
//! - A heredoc anchors only when its CONSUMER is `cat` or `tee` (a plain local file
//!   write). An interpreter heredoc (`python3 <<EOF`) is a SCRIPT, not file content,
//!   and an `ssh`-fed heredoc writes on a REMOTE filesystem - the consumer gate
//!   excludes both, which also enforces nesting depth 0 by construction.
//! - An unquoted heredoc delimiter admits the body only when the body is free of
//!   `$`, backticks, and backslashes (bash would expand them).
//!
//! The caller (recover's per-turn extraction) supplies the transcript-side gates:
//! result pairing, cwd resolution (via the segment's own mutation row, so a leading
//! `cd` chain resolves through the existing checkpoint machinery), and the
//! same-path collision check.

use super::*;

/// One admissible content-anchor command shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AnchorCmd {
    /// `cat <file>`: stdout under the completeness gate is the WHOLE file.
    ReadFull { operand: String },
    /// `head -n N <file>` (start 1) or `sed -n 'A[,B|,$]p' <file>`: stdout lines are
    /// file lines `start..`; `end: None` = to EOF.
    ReadWindow {
        operand: String,
        start: usize,
        end: Option<usize>,
    },
    /// A byte-known full write (`>` truncate): quoted heredoc via cat/tee, literal
    /// echo/printf, `truncate -s 0`.
    WriteFull {
        operand: String,
        content: String,
        heredoc: bool,
    },
    /// A byte-known APPEND (`>>` / `tee -a`): placeable only onto a complete buffer
    /// (the replay layer decides; unplaceable degrades to a disclosed boundary).
    Append {
        operand: String,
        content: String,
        heredoc: bool,
    },
}

/// Every content anchor one command carries.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct BashAnchors {
    /// The read anchor (single-segment commands only).
    pub(crate) read: Option<AnchorCmd>,
    /// Write/append anchors, one per admissible write segment.
    pub(crate) writes: Vec<AnchorCmd>,
    /// True when the command has more than one segment - the caller must then hold
    /// the whole command to a CLEAN result echo before admitting any write.
    pub(crate) multi_segment: bool,
}

/// Classify `command`'s content anchors. An empty result means the existing
/// heuristic layers keep full authority.
pub(crate) fn bash_anchors(command: &str) -> BashAnchors {
    let mut out = BashAnchors::default();
    let (stripped, bodies) = strip_heredoc_bodies_keeping(command);
    let mask = shell_mask(&stripped);
    let segments = split_segments(&stripped, &mask);
    let non_empty = segments.iter().filter(|s| !s.trim().is_empty()).count();
    out.multi_segment = non_empty > 1;

    let mut body_at = 0usize;
    for seg_raw in &segments {
        let seg = seg_raw.trim();
        // Slice this segment's heredoc bodies by OPENER count (the two passes read
        // the same `<<` tokens, so the counts agree by construction).
        let openers = heredoc_delims(seg_raw).len();
        let seg_bodies = &bodies[body_at..(body_at + openers).min(bodies.len())];
        body_at += openers;
        if seg.is_empty() {
            continue;
        }
        let off = seg.as_ptr() as usize - stripped.as_ptr() as usize;
        let seg_mask = &mask[off..off + seg.len()];
        let Some(parts) = segment_parts(seg, seg_mask) else {
            continue; // substitution / extra redirects / unparseable: this segment anchors nothing.
        };
        if seg_bodies.len() > 1 {
            continue; // multiple heredocs in one segment: not a plain write.
        }
        if let Some(body) = seg_bodies.first() {
            if let Some(a) = classify_heredoc(seg_raw, &parts.words, parts.redirect, body) {
                out.writes.push(a);
            }
            continue;
        }
        match classify_plain(&parts.words, parts.redirect) {
            Some(a @ (AnchorCmd::WriteFull { .. } | AnchorCmd::Append { .. })) => {
                out.writes.push(a);
            }
            Some(a) if non_empty == 1 => out.read = Some(a),
            _ => {}
        }
    }
    out
}

/// A parsed simple segment: word tokens + at most one `>`/`>>` redirect.
struct SegmentParts<'a> {
    words: Vec<MaskedTok<'a>>,
    redirect: Option<(bool, String)>,
}

/// Tokenize one segment, separating the single allowed redirect and skipping the
/// heredoc opener. `None` = the segment cannot anchor (substitution, subshell,
/// background, fd-form redirects, more than one redirect).
fn segment_parts<'a>(seg: &'a str, seg_mask: &'a str) -> Option<SegmentParts<'a>> {
    if seg_mask.contains('`')
        || seg_mask.contains("$(")
        || seg_mask.contains('(')
        || seg_mask.contains('&')
    {
        return None;
    }
    let toks = masked_tokens(seg, seg_mask);
    let mut words: Vec<MaskedTok> = Vec::new();
    let mut redirect: Option<(bool, String)> = None;
    let mut skip_next_as_delim = false;
    let mut i = 0usize;
    while i < toks.len() {
        let t = toks[i];
        i += 1;
        if skip_next_as_delim {
            skip_next_as_delim = false;
            continue;
        }
        let m = t.masked;
        if m.starts_with("<<<") {
            return None; // a here-STRING feeds stdin - its operand is DATA, not a file.
        }
        if m.starts_with("<<") {
            // Heredoc opener: `<<'EOF'` attached, or `<<` with the word following.
            let rest = m.trim_start_matches("<<").trim_start_matches('-');
            if rest.is_empty() {
                skip_next_as_delim = true;
            }
            continue;
        }
        if m == ">" || m == ">>" {
            // Detached redirect: the next token is the target.
            let target = toks.get(i)?;
            i += 1;
            if redirect.is_some() {
                return None; // more than one redirect: refuse.
            }
            redirect = Some((m == ">>", literal_token(*target)?));
            continue;
        }
        if let Some(rest) = m.strip_prefix(">>").or_else(|| m.strip_prefix('>')) {
            if !rest.is_empty() {
                if redirect.is_some() {
                    return None;
                }
                let orig_rest = &t.orig[t.orig.len() - rest.len()..];
                redirect = Some((
                    m.starts_with(">>"),
                    literal_token(MaskedTok {
                        orig: orig_rest,
                        masked: rest,
                    })?,
                ));
                continue;
            }
        }
        if m.contains('<') || m.contains('>') {
            return None; // any other redirect shape (fd forms, here-strings): refuse.
        }
        words.push(t);
    }
    Some(SegmentParts { words, redirect })
}

/// Heredoc consumer gate + body admissibility.
fn classify_heredoc(
    seg_raw: &str,
    words: &[MaskedTok],
    redirect: Option<(bool, String)>,
    body: &str,
) -> Option<AnchorCmd> {
    if !heredoc_delimiter_quoted(seg_raw) && body.contains(['$', '`', '\\']) {
        return None; // an unquoted delimiter expands these - the body is not literal.
    }
    let content = if body.is_empty() {
        String::new()
    } else {
        format!("{body}\n")
    };
    let names: Vec<&str> = words.iter().map(|w| w.orig).collect();
    match (names.as_slice(), redirect) {
        (["cat"], Some((append, target))) => Some(if append {
            AnchorCmd::Append {
                operand: target,
                content,
                heredoc: true,
            }
        } else {
            AnchorCmd::WriteFull {
                operand: target,
                content,
                heredoc: true,
            }
        }),
        (["tee", t], None) if !t.starts_with('-') => Some(AnchorCmd::WriteFull {
            operand: literal_token(*words.last()?)?,
            content,
            heredoc: true,
        }),
        (["tee", "-a", t], None) if !t.starts_with('-') => Some(AnchorCmd::Append {
            operand: literal_token(*words.last()?)?,
            content,
            heredoc: true,
        }),
        _ => None, // interpreter / ssh / anything else consuming the heredoc: refuse.
    }
}

/// Non-heredoc shapes: cat / head / sed reads, echo / printf / truncate writes.
fn classify_plain(words: &[MaskedTok], redirect: Option<(bool, String)>) -> Option<AnchorCmd> {
    let first = words.first()?.orig;
    match first {
        "cat" | "head" | "sed" => {
            if redirect.is_some() {
                return None; // a redirected read's stdout is not the tool result.
            }
            classify_read(first, words)
        }
        "echo" | "printf" => {
            let (append, target) = redirect?;
            let content = literal_output(first, &words[1..])?;
            Some(if append {
                AnchorCmd::Append {
                    operand: target,
                    content,
                    heredoc: false,
                }
            } else {
                AnchorCmd::WriteFull {
                    operand: target,
                    content,
                    heredoc: false,
                }
            })
        }
        "truncate" => {
            let names: Vec<&str> = words.iter().map(|w| w.orig).collect();
            if let ["truncate", "-s", "0", _] = names.as_slice() {
                Some(AnchorCmd::WriteFull {
                    operand: literal_token(*words.last()?)?,
                    content: String::new(),
                    heredoc: false,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `cat f` / `head -n N f` / `sed -n 'A[,B]p' f`.
fn classify_read(cmd: &str, words: &[MaskedTok]) -> Option<AnchorCmd> {
    match cmd {
        "cat" if words.len() == 2 => Some(AnchorCmd::ReadFull {
            operand: literal_token(words[1])?,
        }),
        "head" => {
            let (n, op) = match words.len() {
                4 if words[1].orig == "-n" => (words[2].orig.parse::<usize>().ok()?, words[3]),
                3 => (
                    words[1].orig.strip_prefix("-n")?.parse::<usize>().ok()?,
                    words[2],
                ),
                _ => return None,
            };
            let operand = literal_token(op)?;
            (n >= 1).then_some(AnchorCmd::ReadWindow {
                operand,
                start: 1,
                end: Some(n),
            })
        }
        "sed" if words.len() == 4 && words[1].orig == "-n" => {
            let script = strip_quotes(words[2].orig);
            let (start, end) = parse_sed_window(script)?;
            Some(AnchorCmd::ReadWindow {
                operand: literal_token(words[3])?,
                start,
                end,
            })
        }
        _ => None,
    }
}

/// `A p` / `A,B p` / `A,$ p` sed print scripts (the windowed-read idiom). Returns
/// `(start, end)`; `end: None` = to EOF.
fn parse_sed_window(script: &str) -> Option<(usize, Option<usize>)> {
    let body = script.strip_suffix('p')?;
    match body.split_once(',') {
        None => {
            let a: usize = body.parse().ok()?;
            (a >= 1).then_some((a, Some(a)))
        }
        Some((a, b)) => {
            let a: usize = a.parse().ok()?;
            if a < 1 {
                return None;
            }
            if b == "$" {
                return Some((a, None));
            }
            let b: usize = b.parse().ok()?;
            (b >= a).then_some((a, Some(b)))
        }
    }
}

/// The literal stdout `echo`/`printf` would produce, or `None` when any token could
/// expand. `echo`: args joined with single spaces + `\n` (`-n` drops it; `-e` does
/// escape processing and is refused; `-E` is the no-escape default and is dropped).
/// `printf`: the bare format string only when it holds no `%` directive and no `\`
/// escape (then it prints verbatim, with NO added newline).
fn literal_output(cmd: &str, args: &[MaskedTok]) -> Option<String> {
    let mut rest = args;
    let mut newline = true;
    if cmd == "echo" {
        while let Some(first) = rest.first() {
            match first.orig {
                "-n" => {
                    newline = false;
                    rest = &rest[1..];
                }
                "-E" => rest = &rest[1..],
                "-e" | "-ne" | "-en" => return None,
                _ => break,
            }
        }
        let mut parts: Vec<String> = Vec::with_capacity(rest.len());
        for t in rest {
            parts.push(literal_token(*t)?);
        }
        let mut out = parts.join(" ");
        if newline {
            out.push('\n');
        }
        return Some(out);
    }
    // printf: exactly one literal, %-free, escape-free format operand.
    if rest.len() != 1 {
        return None;
    }
    let fmt = literal_token(rest[0])?;
    (!fmt.contains('%') && !fmt.contains('\\')).then_some(fmt)
}

/// The token's literal value: single-quoted verbatim; double-quoted with no
/// `$`/backtick/backslash; bare with no expansion/glob characters. `None` = the
/// shell could rewrite it, so nothing is anchored on it.
fn literal_token(t: MaskedTok) -> Option<String> {
    let orig = t.orig;
    let b = orig.as_bytes();
    if b.len() >= 2 && (b[0] == b'\'' || b[0] == b'"') && b[b.len() - 1] == b[0] {
        let inner = &orig[1..orig.len() - 1];
        if inner.as_bytes().contains(&b[0]) {
            return None; // adjacent quoted spans (`'a'x'b'`): not one literal.
        }
        if b[0] == b'"' && inner.contains(['$', '`', '\\']) {
            return None;
        }
        return Some(inner.to_string());
    }
    if orig.contains(['$', '`', '\\', '*', '?', '[', ']', '{', '}', '~', '\'', '"']) {
        return None;
    }
    Some(orig.to_string())
}

/// True when the segment's heredoc opener uses a QUOTED delimiter (`<<'EOF'` /
/// `<<"EOF"`), so bash takes the body verbatim.
fn heredoc_delimiter_quoted(seg_raw: &str) -> bool {
    let first_line = seg_raw.split('\n').next().unwrap_or(seg_raw);
    let Some(pos) = first_line.find("<<") else {
        return false;
    };
    let rest = first_line[pos + 2..]
        .trim_start_matches('-')
        .trim_start_matches(' ');
    matches!(rest.as_bytes().first(), Some(b'\'') | Some(b'"'))
}
