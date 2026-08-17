//! BashMutation + parse_bash_mutations: the lexical shell-mutation entrypoint.

use super::*;

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
pub(crate) const GIT_MUTATING: &[&str] = &[
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
pub(crate) fn is_fully_masked(masked: &str) -> bool {
    !masked.is_empty() && masked.chars().all(|c| c == MASK_CHAR)
}
