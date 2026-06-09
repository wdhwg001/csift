//! Shared text helpers — the ONE implementation of "show an excerpt, mark the elision
//! explicitly" and the inclusive `START..END` range parser, both previously copy-pasted
//! across `list`/`search`/`recover`/`files`/`turns`/`agents`.
//!
//! ## Why one place
//!
//! The never-silent-truncation contract (SPEC §0, §8.1) requires EVERY content excerpt to
//! mark dropped characters with an explicit `… (+N chars)` count. That algorithm lived in
//! three byte-identical `truncate_excerpt` copies (differing only by a per-file cap) plus a
//! DIVERGENT fourth in `agents::one_line` that omitted the count (a real contract
//! violation). Folding them here keeps the algorithm — and the marker — singular; callers
//! still pass their own cap (200 for the scannable `list`/`agents` previews, 400 for the
//! context-rich `search`/`recover` excerpts), so the legitimately-different caps stay.

/// Truncate `s` to at most `max` CHARACTERS, marking any elision with an explicit
/// `… (+N chars)` suffix (N = dropped char count). Counts CHARACTERS, never bytes, so
/// multi-byte UTF-8 (CJK / emoji) truncates on a codepoint boundary. NEVER silent: a string
/// longer than `max` always carries the count. A string within `max` is returned unchanged.
#[must_use]
pub fn truncate_excerpt(s: &str, max: usize) -> String {
    let total = s.chars().count();
    if total <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max).collect();
    format!("{kept}… (+{} chars)", total - max)
}

/// Collapse internal whitespace runs to single spaces, then [`truncate_excerpt`] to `max`.
/// CODEPOINT-SAFE (collapses + truncates on char boundaries). Used by the `agents`
/// one-line returned-message preview, which both flattens multi-line content AND must mark
/// its elision with the same `… (+N chars)` count as every other excerpt path (the old
/// `agents::one_line` dropped the count — a silent-truncation contract violation).
#[must_use]
pub fn collapse_and_truncate(s: &str, max: usize) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_excerpt(&collapsed, max)
}

/// Parse an inclusive `START..END` index range. Both bounds parse as `usize`; `END < START`
/// is an error. When `one_based` is true the start must be ≥ 1 (file LINE ranges are
/// 1-based); when false a 0 start is allowed (TURN ranges are 0-based). `label` names the
/// flag in the error messages (e.g. `--turn-range` / `--line-range`). This is the single
/// implementation behind the four byte-identical `parse_turn_range` copies + the
/// `parse_line_range` near-clone.
pub fn parse_range(s: &str, label: &str, one_based: bool) -> anyhow::Result<(usize, usize)> {
    use anyhow::{bail, Context};
    let int_kind = if one_based {
        "positive"
    } else {
        "non-negative"
    };
    let (a, b) = s
        .split_once("..")
        .with_context(|| format!("{label} must be START..END, got {s:?}"))?;
    let lo: usize = a
        .trim()
        .parse()
        .with_context(|| format!("{label} start is not a {int_kind} integer: {a:?}"))?;
    let hi: usize = b
        .trim()
        .parse()
        .with_context(|| format!("{label} end is not a {int_kind} integer: {b:?}"))?;
    if one_based && lo == 0 {
        bail!("{label} start must be ≥ 1 (file lines are 1-based)");
    }
    if hi < lo {
        bail!("{label} end ({hi}) is before start ({lo})");
    }
    Ok((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_is_unchanged() {
        assert_eq!(truncate_excerpt("hello", 200), "hello");
        assert_eq!(truncate_excerpt("", 10), "");
        // Exactly at the cap → unchanged (boundary).
        assert_eq!(truncate_excerpt("abcde", 5), "abcde");
    }

    #[test]
    fn truncate_long_marks_dropped_count() {
        let s = "x".repeat(205);
        let out = truncate_excerpt(&s, 200);
        assert!(out.ends_with("… (+5 chars)"), "got: {out}");
        assert!(out.starts_with(&"x".repeat(200)));
        // A different cap (the search/recover 400) still marks the count.
        let s2 = "y".repeat(410);
        assert!(truncate_excerpt(&s2, 400).ends_with("… (+10 chars)"));
    }

    #[test]
    fn truncate_counts_chars_not_bytes() {
        // A 4-byte emoji repeated past the cap — the count is CHARS, not bytes.
        let s = "🛠".repeat(202);
        let out = truncate_excerpt(&s, 200);
        assert!(out.ends_with("… (+2 chars)"), "got: {out}");
        // CJK likewise.
        let cjk = "x".repeat(203);
        assert!(truncate_excerpt(&cjk, 200).ends_with("… (+3 chars)"));
    }

    #[test]
    fn collapse_and_truncate_flattens_then_marks() {
        // Multi-line / multi-space input collapses to single spaces.
        assert_eq!(
            collapse_and_truncate("a\n  b\t c", 200),
            "a b c",
            "whitespace runs collapse to single spaces"
        );
        // And the SAME count marker fires past the cap (the one_line bug-fix: not silent).
        let long = "word ".repeat(60); // 300 chars collapsed
        let out = collapse_and_truncate(&long, 200);
        assert!(
            out.contains("… (+") && out.ends_with("chars)"),
            "collapse path must mark the elision count, not drop silently: {out}"
        );
    }

    #[test]
    fn parse_range_zero_based_turn() {
        assert_eq!(parse_range("0..5", "--turn-range", false).unwrap(), (0, 5));
        assert_eq!(
            parse_range(" 3 .. 9 ", "--turn-range", false).unwrap(),
            (3, 9)
        );
        // Equal bounds are a valid single-index range.
        assert_eq!(parse_range("7..7", "--turn-range", false).unwrap(), (7, 7));
    }

    #[test]
    fn parse_range_one_based_line_rejects_zero_start() {
        assert_eq!(
            parse_range("1..200", "--line-range", true).unwrap(),
            (1, 200)
        );
        let err = parse_range("0..5", "--line-range", true).unwrap_err();
        assert!(err.to_string().contains("≥ 1"), "got: {err}");
    }

    #[test]
    fn parse_range_errors() {
        // Missing `..`.
        assert!(parse_range("5", "--turn-range", false).is_err());
        // Non-integer bound.
        assert!(parse_range("a..5", "--turn-range", false).is_err());
        // end < start.
        let err = parse_range("9..3", "--turn-range", false).unwrap_err();
        assert!(err.to_string().contains("before start"), "got: {err}");
        // The label appears in the message.
        let err2 = parse_range("x", "--my-range", false).unwrap_err();
        assert!(err2.to_string().contains("--my-range"));
    }
}
