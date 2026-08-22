//! Heredoc body stripping (delimiters, quoted words, exact closers).

/// Remove heredoc BODY lines from a multi-line command, keeping every non-body line
/// (including each heredoc OPENER line, which may carry its own trailing `> file`
/// redirect). A heredoc opens on a `<<DELIM` / `<<-DELIM` / `<<'DELIM'` / `<<"DELIM"`
/// token (quoted or not) and closes on a line whose trimmed content equals `DELIM` (a
/// `<<-` opener also accepts a tab-indented closer). Multiple heredocs on one line open
/// in left-to-right order. This is a lexical best-effort - sufficient to stop the body's
/// `>`/quote characters from fabricating redirect rows.
///
/// Besides the stripped command, the dropped body TEXTS are returned, one per heredoc
/// in OPENER order (the same left-to-right, line-by-line order in which the `<<DELIM`
/// tokens appear). The closer line belongs to neither the command stream nor the body.
/// Callers pair the bodies back to segments by counting each stripped segment's openers
/// with [`heredoc_delims`] - the two passes read the same tokens, so the counts agree
/// by construction.
pub(crate) fn strip_heredoc_bodies_keeping(command: &str) -> (String, Vec<String>) {
    if !command.contains("<<") {
        return (command.to_string(), Vec::new()); // fast path: no heredoc at all.
    }
    let mut out = String::with_capacity(command.len());
    let mut bodies: Vec<String> = Vec::new();
    // Each queued opener carries its body's slot index, assigned at OPEN time so the
    // returned order is opener order even when heredocs nest their body ranges.
    let mut pending: std::collections::VecDeque<(String, usize)> =
        std::collections::VecDeque::new();
    let mut active: Option<(String, usize)> = None;
    let mut first = true;
    for line in command.split('\n') {
        if !first {
            out.push('\n');
        }
        first = false;
        if let Some((delim, slot)) = active.as_ref() {
            // Inside a heredoc body: collect the line; the closer (trimmed == delim)
            // ends it.
            if line.trim() == delim.as_str() {
                active = pending.pop_front();
            } else {
                let body = &mut bodies[*slot];
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(line);
            }
            continue; // body line (and the closer line) are not commands.
        }
        // Not inside a body: this is a command/opener line - keep it, and queue any
        // heredoc delimiters it opens so the FOLLOWING lines are dropped as bodies.
        out.push_str(line);
        for delim in heredoc_delims(line) {
            let slot = bodies.len();
            bodies.push(String::new());
            pending.push_back((delim, slot));
        }
        if active.is_none() {
            active = pending.pop_front();
        }
    }
    (out, bodies)
}

/// The heredoc delimiters opened on one line, in order. Recognizes `<<WORD`, `<<-WORD`,
/// and quoted `<<'WORD'` / `<<"WORD"` (the quotes are stripped from the closer-comparison
/// delimiter, matching bash). A `<<<` here-STRING is NOT a heredoc (no body line) and is
/// ignored.
pub(crate) fn heredoc_delims(line: &str) -> Vec<String> {
    let mut delims = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'<' && bytes[i + 1] == b'<' {
            // `<<<` is a here-string, not a heredoc - skip it.
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
pub(crate) fn read_heredoc_word(s: &str) -> (String, usize) {
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
