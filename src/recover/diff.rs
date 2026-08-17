//! LCS diff + unified-patch formatting + small text utilities.

use super::*;

/// Compute a unified diff between two line vectors, emitting `@@ -a,b +c,d @@` hunks with
/// ` `/`-`/`+` prefixes. A compact LCS-based diff (O(n·m) DP) - fine for single-file
/// reconstruction sizes and fully unit-testable. Returns an empty string when identical.
/// `context` = number of equal lines to keep around each change. `usize::MAX` ⇒ FULL context:
/// every line of `old`/`new` is shown. `--patches` passes MAX on purpose - `old`/`new` are the
/// segment's READ-covered lines, and CC's strict Read-before-Edit means each of those lines was
/// genuinely observed, so showing them all is valid, high-quality context (a fully-read,
/// barely-edited file then reproduces in full, not just a 3-line window around the one change).
pub(crate) fn unified_diff(old: &[String], new: &[String], context: usize) -> String {
    if old == new {
        return String::new();
    }
    let ops = lcs_diff(old, new);
    format_unified(&ops, old, new, context)
}

/// A single edit op in the diff script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffOp {
    Equal,
    Delete,
    Insert,
}

/// LCS-based diff script over two line slices (classic DP backtrace).
pub(crate) fn lcs_diff(old: &[String], new: &[String]) -> Vec<(DiffOp, usize, usize)> {
    let n = old.len();
    let m = new.len();
    // dp[i][j] = LCS length of old[i..] and new[j..].
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old[i] == new[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if old[i] == new[j] {
            ops.push((DiffOp::Equal, i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push((DiffOp::Delete, i, j));
            i += 1;
        } else {
            ops.push((DiffOp::Insert, i, j));
            j += 1;
        }
    }
    while i < n {
        ops.push((DiffOp::Delete, i, j));
        i += 1;
    }
    while j < m {
        ops.push((DiffOp::Insert, i, j));
        j += 1;
    }
    ops
}

/// Format an LCS op-script as unified-diff hunks. `context` equal lines surround each change
/// (`usize::MAX` ⇒ full context: all lines emitted, every change merged into one spanning hunk).
pub(crate) fn format_unified(
    ops: &[(DiffOp, usize, usize)],
    old: &[String],
    new: &[String],
    context: usize,
) -> String {
    // Mark which op indices are changes vs equal.
    let is_change: Vec<bool> = ops.iter().map(|(o, _, _)| *o != DiffOp::Equal).collect();
    let mut out = String::new();

    let mut idx = 0usize;
    while idx < ops.len() {
        if !is_change[idx] {
            idx += 1;
            continue;
        }
        // A change run: extend backward/forward by CONTEXT equal ops.
        let mut start = idx;
        let mut ctx_back = 0;
        while start > 0 && !is_change[start - 1] && ctx_back < context {
            start -= 1;
            ctx_back += 1;
        }
        let mut end = idx;
        // Walk to the end of this hunk: include changes + up to CONTEXT trailing equals,
        // but merge adjacent change runs separated by ≤ 2*CONTEXT equals.
        while end < ops.len() {
            if is_change[end] {
                end += 1;
                continue;
            }
            // Count equals run; if a change follows within 2*CONTEXT, keep going.
            let mut run = end;
            while run < ops.len() && !is_change[run] {
                run += 1;
            }
            let equal_len = run - end;
            if run < ops.len() && equal_len <= context.saturating_mul(2) {
                end = run; // absorb the gap
            } else {
                end += equal_len.min(context);
                break;
            }
        }

        // Compute hunk header line numbers (1-based) from the first/last op in [start,end).
        let slice = &ops[start..end];
        let (mut old_lo, mut new_lo) = (usize::MAX, usize::MAX);
        let (mut old_count, mut new_count) = (0usize, 0usize);
        let mut body = String::new();
        for (op, oi, nj) in slice {
            match op {
                DiffOp::Equal => {
                    old_lo = old_lo.min(*oi);
                    new_lo = new_lo.min(*nj);
                    old_count += 1;
                    new_count += 1;
                    body.push_str(&format!(" {}\n", old[*oi]));
                }
                DiffOp::Delete => {
                    old_lo = old_lo.min(*oi);
                    old_count += 1;
                    body.push_str(&format!("-{}\n", old[*oi]));
                }
                DiffOp::Insert => {
                    new_lo = new_lo.min(*nj);
                    new_count += 1;
                    body.push_str(&format!("+{}\n", new[*nj]));
                }
            }
        }
        if old_lo == usize::MAX {
            old_lo = 0;
        }
        if new_lo == usize::MAX {
            new_lo = 0;
        }
        // Unified-diff headers are 1-based; a zero-length side uses the 0,0 form.
        let old_start = if old_count == 0 { old_lo } else { old_lo + 1 };
        let new_start = if new_count == 0 { new_lo } else { new_lo + 1 };
        out.push_str(&format!(
            "@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
        ));
        out.push_str(&body);
        idx = end;
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared text helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Split content into lines WITHOUT a trailing empty element for a final newline (so a
/// file `"a\nb\n"` is `["a","b"]`, matching how CC numbers lines).
pub(crate) fn split_lines(content: &str) -> Vec<String> {
    if content.is_empty() {
        return Vec::new();
    }
    let trimmed = content.strip_suffix('\n').unwrap_or(content);
    trimmed.split('\n').map(str::to_string).collect()
}

/// Count the lines a content blob represents (== `split_lines(content).len()`).
pub(crate) fn line_count(content: &str) -> usize {
    split_lines(content).len()
}

/// The first line of a (possibly multi-line) string, trimmed.
pub(crate) fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

/// Strip a leading line-number gutter from each line of a cat -n style snippet. Handles
/// BOTH the TAB gutter (`\d+\t<text>`, what current CC Read content uses) and the arrow
/// gutter (`\d+→<text>`, an older form). Returns `(file_line_no, text)` pairs; a line
/// with no recognizable gutter is skipped (we never fabricate a number).
pub(crate) fn strip_gutter(snippet: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for raw in snippet.split('\n') {
        let line = raw;
        // Find the gutter separator: a tab or the U+2192 arrow after leading digits.
        let trimmed = line.trim_start();
        let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            continue;
        }
        let rest = &trimmed[digits.len()..];
        let text = if let Some(t) = rest.strip_prefix('\t') {
            t
        } else if let Some(t) = rest.strip_prefix('\u{2192}') {
            t
        } else {
            continue;
        };
        if let Ok(n) = digits.parse::<usize>() {
            out.push((n, text.to_string()));
        }
    }
    out
}

/// Parse a `--turn START..END` into an inclusive 0-based `(lo, hi)` (shared parser).
pub(crate) fn parse_turn_range(s: &str) -> Result<crate::text::RangeSpec> {
    crate::text::parse_range_spec(s, "--turn", false)
}

/// Parse a `--file-lines` token into a [`RangeSpec`] (the shared grammar; 1-based, so a 0
/// start is rejected), resolved against the reconstructed file's line count in
/// [`apply_line_range`].
pub(crate) fn parse_line_range(s: &str) -> Result<crate::text::RangeSpec> {
    crate::text::parse_range_spec(s, "--file-lines", true)
}

/// Truncate to [`EXCERPT_MAX`] chars with the shared explicit `… (+N chars)` marker.
pub(crate) fn truncate_excerpt(s: &str) -> String {
    crate::text::truncate_excerpt(s, EXCERPT_MAX)
}

// ─────────────────────────────────────────────────────────────────────────────
// At-cutoff resolution
// ─────────────────────────────────────────────────────────────────────────────
