//! Rendered bodies + the agent-message richness signals.

use super::*;

/// The rendered form of a unit's text body (verbatim if ≤ cap, else middle-truncated)
/// plus the metadata a JSON consumer needs.
#[derive(Debug, Clone)]
pub(crate) struct RenderedUnit {
    pub(crate) body: String,
    pub(crate) rendered_chars: usize,
    pub(crate) truncated: bool,
    pub(crate) elided_chars: usize,
    pub(crate) elided_lines: usize,
}

/// Middle-truncate a unit to a cap (`cap_override` when set - the fixed-fleet `--slices` window
/// cap that keeps whole turns - else the unit's per-role cap), keeping head+tail, with an explicit
/// elided marker. A unit at or below the cap renders verbatim. The cut is on `char` boundaries
/// (never mid-codepoint). The `L lines elided` note is included only when the original text spanned
/// ≥1 newline.
pub(crate) fn render_unit_body(unit: &TurnUnit, cap_override: Option<usize>) -> RenderedUnit {
    let cap = cap_override.unwrap_or_else(|| unit.role.cap());
    let chars: Vec<char> = unit.text.chars().collect();
    let total = chars.len();
    if total <= cap {
        return RenderedUnit {
            body: unit.text.clone(),
            rendered_chars: total,
            truncated: false,
            elided_chars: 0,
            elided_lines: 0,
        };
    }
    let head_keep = ((cap as f64) * unit.role.head_frac()).round() as usize;
    let head_keep = head_keep.min(cap);
    let tail_keep = cap - head_keep;
    let head: String = chars[..head_keep].iter().collect();
    let tail: String = chars[total - tail_keep..].iter().collect();
    let elided_chars = total - cap;
    // Lines elided: original newline count, surfaced only for multi-line bodies. The
    // rendered one-line form has no newlines, so we report the original's count as the
    // magnitude the consumer should expect in the raw record.
    let elided_lines = unit.orig_newlines;
    let nl_note = if elided_lines > 0 {
        format!(", {elided_lines} lines elided")
    } else {
        String::new()
    };
    let body = format!("{head} … [+{elided_chars} chars{nl_note}] … {tail}");
    RenderedUnit {
        body,
        rendered_chars: cap,
        truncated: true,
        elided_chars,
        elided_lines,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Richness function (the content gate) + agent-message selection
// ─────────────────────────────────────────────────────────────────────────────

/// The fixed substance-noun set the number-of-substance signal looks near (Arm 2a).
pub(crate) const SUBSTANCE_NOUNS: &[&str] = &[
    "passed", "failed", "tests", "test", "errors", "error", "files", "file", "chars", "lines",
    "line", "ops", "cases", "case",
];

/// The fixed finding/decision lexeme set (Arm 2e), case-insensitive substring. Includes
/// a CJK set matched on the normalized String (codepoint-safe). A message carrying any
/// of these is a finding/decision worth keeping at any length.
pub(crate) const FINDING_LEXEMES: &[&str] = &[
    "found",
    "confirmed",
    "verified",
    "proven",
    "proof",
    "root cause",
    "root-cause",
    "defer",
    "deferred",
    "fails",
    "failed",
    "failure",
    "error",
    "bug",
    "correction",
    "corrected",
    "fix",
    "fixed",
    "regression",
];

/// The intent-verb openers that mark a PURE declaration (Arm of the drop predicate),
/// case-insensitive prefix on the first ~24 trimmed chars. A message that opens with one
/// of these, is short, and carries no signal is the only thing collapsed.
pub(crate) const INTENT_VERB_OPENERS: &[&str] = &[
    "let me", "i'll", "i will", "now i", "now let", "next i", "next,", "let's",
];

/// Does a normalized agent message carry important info? A SHORT-CIRCUIT OR of two keep
/// arms over the normalized one-line text: ARM 1 the length gate (kept on length alone
/// when ≥ `rich_min_chars`), ARM 2 the signal test (a number-of-substance / commit hash /
/// file:line ref / backtick code path / finding-or-decision lexeme). Keep-on-doubt: this
/// returns true on ANY signal; the separate [`agent_msg_is_droppable`] is what proves a
/// message is a pure declaration safe to collapse.
pub(crate) fn agent_msg_is_rich(text: &str, cfg: &RichnessCfg) -> bool {
    // ARM 1 - LENGTH GATE.
    if text.chars().count() >= cfg.rich_min_chars {
        return true;
    }
    // ARM 2 - SIGNAL TEST (first match wins; single cheap scan per arm).
    let lower = text.to_lowercase();
    signal_number_of_substance(&lower)
        || signal_commit_hash(&lower)
        || signal_file_line_ref(text)
        || signal_backtick_code(text)
        || signal_finding_lexeme(&lower)
}

/// Arm 2a - a NUMBER-OF-SUBSTANCE: a ≥2-digit run, or an `N / M` / `N of M` ratio, sitting
/// within a ±16-char window of one of [`SUBSTANCE_NOUNS`]. Byte scan for ASCII digits,
/// then a bounded-window noun check - never a full regex pass. Operates on lowercased text.
pub(crate) fn signal_number_of_substance(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        if bytes[i].is_ascii_digit() {
            // Extent of this digit run.
            let start = i;
            while i < n && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let run_len = i - start;
            // An "N / M" or "N of M" ratio: a digit run, optional spaces, '/' or "of",
            // optional spaces, another digit run → always substance, no noun needed.
            if ratio_follows(bytes, i) {
                return true;
            }
            // A ≥2-digit integer within a ±16-byte window of a substance noun. The window
            // bounds are BYTE offsets that may land mid-codepoint (CJK text), so snap `lo`
            // DOWN and `hi` UP to the nearest char boundary before slicing - slicing a
            // non-boundary index panics. The substance nouns are ASCII, so a slightly wider
            // (boundary-snapped) window never changes a match decision.
            if run_len >= 2 {
                let mut lo = start.saturating_sub(16);
                while lo > 0 && !lower.is_char_boundary(lo) {
                    lo -= 1;
                }
                let mut hi = (i + 16).min(n);
                while hi < n && !lower.is_char_boundary(hi) {
                    hi += 1;
                }
                let window = &lower[lo..hi];
                if SUBSTANCE_NOUNS.iter().any(|noun| window.contains(noun)) {
                    return true;
                }
            }
            continue;
        }
        i += 1;
    }
    false
}

/// True when, starting at byte `i` (just past a digit run), an `[/ ]` or `of` separator
/// then another digit run forms a ratio (`12/40`, `3 of 5`).
pub(crate) fn ratio_follows(bytes: &[u8], mut i: usize) -> bool {
    let n = bytes.len();
    while i < n && bytes[i] == b' ' {
        i += 1;
    }
    if i < n && bytes[i] == b'/' {
        i += 1;
    } else if i + 1 < n && &bytes[i..i + 2] == b"of" {
        i += 2;
    } else {
        return false;
    }
    while i < n && bytes[i] == b' ' {
        i += 1;
    }
    i < n && bytes[i].is_ascii_digit()
}

/// Arm 2b - a COMMIT-HASH-LIKE HEX: a maximal `[0-9a-f]` run of length 7..=40 containing
/// at least one a–f letter (excludes plain decimals already caught by Arm 2a). Operates
/// on lowercased text.
pub(crate) fn signal_commit_hash(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    let n = bytes.len();
    let is_hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    let mut i = 0;
    while i < n {
        // A run must not be glued to a longer alnum token (so `deadbeef` inside a word is
        // still a run, but `g1a2b3c` won't include the leading g). Start a run on a hex
        // byte whose predecessor is not alphanumeric.
        let prev_alnum = i > 0 && bytes[i - 1].is_ascii_alphanumeric();
        if is_hex(bytes[i]) && !prev_alnum {
            let start = i;
            let mut has_alpha = false;
            while i < n && is_hex(bytes[i]) {
                if bytes[i].is_ascii_alphabetic() {
                    has_alpha = true;
                }
                i += 1;
            }
            let len = i - start;
            // The run must END at a non-alphanumeric boundary too (reject `a1b2c3z...`).
            let next_alnum = i < n && bytes[i].is_ascii_alphanumeric();
            if (7..=40).contains(&len) && has_alpha && !next_alnum {
                return true;
            }
            continue;
        }
        i += 1;
    }
    false
}

/// Arm 2c - a FILE-AND-LINE REF: a `name.rs:NNN` shape (a token with a `.` + alpha
/// extension followed by `:` + digits) OR a `src/…` / `tests/…`-rooted path token.
/// Operates on the ORIGINAL (case-preserving) text - paths are case-sensitive.
pub(crate) fn signal_file_line_ref(text: &str) -> bool {
    for tok in text.split(|c: char| c.is_whitespace()) {
        let tok = tok.trim_matches(|c: char| matches!(c, '`' | '(' | ')' | ',' | ';' | '"'));
        // `name.ext:NNN` - a dot, an alpha extension, a colon, then ≥1 digit.
        if let Some(colon) = tok.rfind(':') {
            let (path, after) = tok.split_at(colon);
            let line_part = &after[1..];
            if !line_part.is_empty()
                && line_part.bytes().all(|b| b.is_ascii_digit())
                && path_has_alpha_extension(path)
            {
                return true;
            }
        }
        // A `src/…` or `tests/…`-rooted path token (a file ledger reference).
        if (tok.starts_with("src/") || tok.starts_with("tests/")) && tok.len() > 4 {
            return true;
        }
    }
    false
}

/// True when `path` ends in `.<alpha…>` (a file extension), e.g. `turns.rs`, `foo.py`.
pub(crate) fn path_has_alpha_extension(path: &str) -> bool {
    match path.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty() && !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphabetic())
        }
        None => false,
    }
}

/// Arm 2d - a BACKTICK CODE PATH: at least one backtick-delimited span (`` `code` ``).
pub(crate) fn signal_backtick_code(text: &str) -> bool {
    let first = match text.find('`') {
        Some(i) => i,
        None => return false,
    };
    text[first + 1..].contains('`')
}

/// Arm 2e - a FINDING/DECISION LEXEME (case-insensitive substring against the fixed
/// [`FINDING_LEXEMES`] set). Operates on lowercased text (the CJK lexemes are
/// substring-matched on the same normalized String, codepoint-safe).
pub(crate) fn signal_finding_lexeme(lower: &str) -> bool {
    FINDING_LEXEMES.iter().any(|lex| lower.contains(lex))
}

/// Is a normalized agent message a PROVEN pure declaration (safe to collapse)? Requires
/// ALL of: NOT rich, AND opens with an intent verb (case-insensitive prefix on the first
/// ~24 trimmed chars), AND short (`chars < declaration_max_chars`). A message that is
/// neither clearly rich nor a proven declaration (no opener verb, no signal, mid-length)
/// is KEPT - drop requires proof, keep is default.
pub(crate) fn agent_msg_is_droppable(text: &str, cfg: &RichnessCfg) -> bool {
    if agent_msg_is_rich(text, cfg) {
        return false;
    }
    if text.chars().count() >= cfg.declaration_max_chars {
        return false;
    }
    let head: String = text
        .trim_start()
        .chars()
        .take(24)
        .collect::<String>()
        .to_lowercase();
    INTENT_VERB_OPENERS.iter().any(|v| head.starts_with(v))
}
