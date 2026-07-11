//! Shared text helpers — the ONE implementation of "show an excerpt, mark the elision
//! explicitly" and the inclusive `START..END` range parser, both shared
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

/// The ONE canonical "malformed lines skipped" note fragment, e.g.
/// `3 malformed line(s) skipped`. Every subcommand surfaces the same malformed-line event;
/// it was previously phrased three different ways (`(N malformed line(s) skipped)` on
/// files/recover/search, a `note`-prefixed form on list/agents, and a reordered
/// `(skipped N malformed jsonl line(s))` on turns), which made a scripted grep over the note
/// fragile. Callers wrap this in their own delimiter (a parenthesized standalone line, or a
/// `note`-prefixed header row) but the WORDING is now singular. Returns the bare fragment
/// (no surrounding parens) so each caller controls its own framing.
#[must_use]
pub fn malformed_note(n: usize) -> String {
    format!("{n} malformed line(s) skipped")
}

/// The canonical SCOPE-span text fragment, e.g. `4 sessions in scope (1 top-level + 3
/// subagent)`. EVERY subagent-spanning subcommand (`list`/`files`/`search`/`recover`/`turns`)
/// reports a bare-uuid fan-out with this SAME wording so an agent reads one format. `turns`
/// prefixes `scope  ` and appends its budget clause; the others use [`emit_scope_banner`].
#[must_use]
pub fn scope_span_fragment(top: usize, sub: usize) -> String {
    let total = top + sub;
    format!(
        "{total} session{} in scope ({top} top-level + {sub} subagent)",
        if total == 1 { "" } else { "s" }
    )
}

/// Emit the `scope  N sessions in scope (X top-level + Y subagent)` banner + a trailing blank
/// line to stdout — but ONLY when the resolved set actually spans ≥1 subagent (`sub > 0`).
/// This is the ONE emit site for the four non-turns spanning subcommands
/// (`list`/`files`/`search`/`recover`): a bare `csift <cmd> <uuid>` that silently balloons
/// from 1 transcript to N must announce the fan-out up front, identically across surfaces,
/// and stay silent (no banner) under `--no-subagents` or a genuinely single-transcript scope.
/// `turns` keeps its own richer banner (it folds in the per-session budget math) but reuses
/// [`scope_span_fragment`] for the wording.
pub fn emit_scope_banner(top: usize, sub: usize) {
    if sub > 0 {
        println!("scope  {}", scope_span_fragment(top, sub));
        println!();
    }
}

/// envelope v2 (SPEC §8) — EVERY `--format json` stream is exactly three parts:
///   `{"kind":"header","command":"<cmd>", …}`   the FIRST line, ALWAYS emitted;
///   `{"kind":"<row-kind>", …}` × N             command-specific kind-tagged rows;
///   `{"kind":"summary", …}`                    the LAST line, ALWAYS emitted (even all-zero).
/// ONE reading idiom therefore serves every command: `jq 'select(.kind=="…")'` — no
/// per-command envelope knowledge, no conditional first line, no shape-varied trailer.
/// These two builders are the ONLY way a module makes its header/summary line, so the
/// invariant cannot drift per command.
#[must_use]
pub fn envelope_header(command: &str, extra: serde_json::Value) -> serde_json::Value {
    merge_into(
        serde_json::json!({"kind": "header", "command": command}),
        extra,
    )
}

/// [`envelope_header`] for a SPAN command: adds the scope fields
/// (`sessions_in_scope`/`top_level_sessions`/`subagent_sessions`) every spanning
/// surface discloses identically.
#[must_use]
pub fn envelope_scope_header(
    command: &str,
    top: usize,
    sub: usize,
    extra: serde_json::Value,
) -> serde_json::Value {
    merge_into(
        serde_json::json!({
            "kind": "header",
            "command": command,
            "sessions_in_scope": top + sub,
            "top_level_sessions": top,
            "subagent_sessions": sub,
        }),
        extra,
    )
}

/// The closing `{"kind":"summary", …}` line (see [`envelope_header`]).
#[must_use]
pub fn envelope_summary(extra: serde_json::Value) -> serde_json::Value {
    merge_into(serde_json::json!({"kind": "summary"}), extra)
}

/// Merge `extra`'s object fields into `base` (base keys win are-not — extra never
/// overrides the `kind`/`command` discriminators by construction of the callers).
fn merge_into(mut base: serde_json::Value, extra: serde_json::Value) -> serde_json::Value {
    if let (Some(b), serde_json::Value::Object(e)) = (base.as_object_mut(), extra) {
        for (k, v) in e {
            b.entry(k).or_insert(v);
        }
    }
    base
}

/// One endpoint of an index range, parsed but NOT yet resolved against a concrete domain
/// length (open / from-the-end endpoints need the length to materialize).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// An explicit index exactly as written (0- or 1-based per the flag's domain).
    At(usize),
    /// `k`-th index counting from the END, 1-based: `FromEnd(1)` = the last element,
    /// `FromEnd(3)` = third-from-last. Written `-k` (`-1`, `-3`).
    FromEnd(usize),
    /// Open — the natural extreme: a START endpoint resolves to the first index, an END
    /// endpoint to the last. Written by omitting the side (`N..`, `..N`, `..`).
    Open,
}

/// A parsed inclusive index range with possibly-open / from-the-end endpoints. THE range
/// grammar, shared by every range flag (`show --line`/`--turn` · `--turn` ·
/// `--file-lines`): `N` (single) · `A..B` (closed) · `N..` (to the end) · `..N` (from the
/// start) · `..` (all) · negative `-k` = `k`-th from the end, so `-3..` = the last 3 and `-1`
/// = the last. Resolve against a concrete domain length with [`RangeSpec::resolve`]. Both
/// sides trimmed; the dash-form `A-B` is a HARD error that teaches the `..` spelling; `END <
/// START` after resolution is an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeSpec {
    pub start: Endpoint,
    pub end: Endpoint,
}

/// Parse one `..`-split, trimmed endpoint token. Empty ⇒ [`Endpoint::Open`]; `-k` ⇒
/// [`Endpoint::FromEnd`]; digits ⇒ [`Endpoint::At`]. A dash inside a number-bearing token
/// (`495-500`) is the removed `A-B` spelling → a teaching error.
fn parse_endpoint(tok: &str, label: &str) -> anyhow::Result<Endpoint> {
    use anyhow::bail;
    if tok.is_empty() {
        return Ok(Endpoint::Open);
    }
    if let Some(rest) = tok.strip_prefix('-') {
        if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
            let k: usize = rest
                .parse()
                .map_err(|_| anyhow::anyhow!("{label}: {tok:?} is not a valid from-end index"))?;
            if k == 0 {
                bail!("{label}: -0 is not a from-end index (use -1 for the last element)");
            }
            return Ok(Endpoint::FromEnd(k));
        }
    }
    if !tok.is_empty() && tok.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(Endpoint::At(tok.parse().map_err(|_| {
            anyhow::anyhow!("{label} index {tok:?} is out of range")
        })?));
    }
    if tok.contains('-') && tok.bytes().any(|b| b.is_ascii_digit()) {
        bail!(
            "{label} range is START..END (e.g. 495..500); a leading -N counts from the end \
             (-3.. = the last 3). got {tok:?}"
        );
    }
    bail!("{label} endpoint must be N, -N (from the end), or empty (open); got {tok:?}")
}

/// Parse an index-range token into a [`RangeSpec`] (unresolved). `one_based` only affects a
/// literal `At(0)` (rejected for a 1-based LINE domain, allowed for a 0-based TURN domain);
/// open/negative forms resolve against the domain length in [`RangeSpec::resolve`].
pub fn parse_range_spec(s: &str, label: &str, one_based: bool) -> anyhow::Result<RangeSpec> {
    let t = s.trim();
    let spec = if let Some((a, b)) = t.split_once("..") {
        RangeSpec {
            start: parse_endpoint(a.trim(), label)?,
            end: parse_endpoint(b.trim(), label)?,
        }
    } else {
        let ep = parse_endpoint(t, label)?;
        if ep == Endpoint::Open {
            anyhow::bail!("{label} must be N, A..B, N.., ..N, or -N (from the end); got {s:?}");
        }
        RangeSpec { start: ep, end: ep }
    };
    if one_based {
        if let Endpoint::At(0) = spec.start {
            anyhow::bail!("{label} start must be ≥ 1 (this domain is 1-based)");
        }
        if let Endpoint::At(0) = spec.end {
            anyhow::bail!("{label} end must be ≥ 1 (this domain is 1-based)");
        }
    }
    // Statically-detectable reversal (both endpoints explicit) errors up front — the common
    // `9..3` mistake. A len-dependent reversal (from-end / open) can only be judged after
    // resolution, where it resolves to an empty range that simply matches nothing.
    if let (Endpoint::At(lo), Endpoint::At(hi)) = (spec.start, spec.end) {
        if hi < lo {
            anyhow::bail!("{label} end ({hi}) is before start ({lo})");
        }
    }
    Ok(spec)
}

impl RangeSpec {
    /// Resolve to a concrete inclusive `(lo, hi)` in a domain of `len` elements — `one_based`
    /// ⇒ indices 1..=len, else 0..=len-1. Open/from-end endpoints materialize against `len`
    /// (a from-end index past the start clamps to the first index, so `-100..` of 5 = all 5);
    /// an explicit `At` is returned as-written (the caller clamps/validates a bare
    /// out-of-range index, matching each flag's existing behavior). Infallible: a
    /// len-dependent reversal (`hi < lo`) is returned as-is — an empty range that matches
    /// nothing — since the statically-detectable reversal is already caught at parse time.
    #[must_use]
    pub fn resolve(&self, len: usize, one_based: bool) -> (usize, usize) {
        let first = usize::from(one_based);
        let last = if len == 0 {
            first
        } else if one_based {
            len
        } else {
            len - 1
        };
        let from_end = |k: usize| -> usize {
            if one_based {
                (len + 1).saturating_sub(k).max(first)
            } else {
                len.saturating_sub(k).max(first)
            }
        };
        let resolve_ep = |ep: Endpoint, open_to: usize| -> usize {
            match ep {
                Endpoint::At(n) => n,
                Endpoint::FromEnd(k) => from_end(k),
                Endpoint::Open => open_to,
            }
        };
        let lo = resolve_ep(self.start, first);
        let hi = resolve_ep(self.end, last);
        (lo, hi)
    }
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
        // A 3-byte codepoint likewise.
        let three_byte = "€".repeat(203);
        assert!(truncate_excerpt(&three_byte, 200).ends_with("… (+3 chars)"));
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
    fn malformed_note_is_canonical() {
        assert_eq!(malformed_note(3), "3 malformed line(s) skipped");
        assert_eq!(malformed_note(0), "0 malformed line(s) skipped");
        // No SURROUNDING parens / `note` prefix / `jsonl` token — callers frame it. (The
        // `(s)` plural-marker parens ARE part of the canonical wording.)
        assert!(!malformed_note(1).starts_with('('));
        assert!(!malformed_note(1).contains("jsonl"));
        assert!(!malformed_note(1).contains("note"));
    }

    fn rng(s: &str, one_based: bool, len: usize) -> (usize, usize) {
        parse_range_spec(s, "--r", one_based)
            .unwrap()
            .resolve(len, one_based)
    }

    #[test]
    fn range_closed_bare_and_open() {
        // Closed A..B and bare N are explicit (len-independent).
        assert_eq!(rng("0..5", false, 100), (0, 5));
        assert_eq!(rng(" 3 .. 9 ", false, 100), (3, 9));
        assert_eq!(rng("7..7", false, 100), (7, 7));
        assert_eq!(rng("270", false, 100), (270, 270));
        assert_eq!(rng(" 42 ", true, 100), (42, 42));
        assert_eq!(rng("0", false, 10), (0, 0));
        // Open ends materialize against len. 0-based turn domain of 10 → 0..=9.
        assert_eq!(rng("3..", false, 10), (3, 9)); // to the end
        assert_eq!(rng("..4", false, 10), (0, 4)); // from the start
        assert_eq!(rng("..", false, 10), (0, 9)); // all
                                                  // 1-based line domain of 200 → 1..=200.
        assert_eq!(rng("5..", true, 200), (5, 200));
        assert_eq!(rng("..50", true, 200), (1, 50));
    }

    #[test]
    fn range_from_end_negative() {
        // -k = k-th from the end. Tail-peek: last 3 turns of a 10-turn (0-based) session.
        assert_eq!(rng("-3..", false, 10), (7, 9));
        assert_eq!(rng("-1", false, 10), (9, 9)); // the last element
        assert_eq!(rng("-1..", false, 10), (9, 9)); // last 1
        assert_eq!(rng("3..-1", false, 10), (3, 9)); // turn 3 → the last
                                                     // 1-based lines: last 20 of a 200-line file.
        assert_eq!(rng("-20..", true, 200), (181, 200));
        assert_eq!(rng("-1", true, 200), (200, 200));
        // A from-end index past the start clamps to the first (last 100 of 5 = all 5).
        assert_eq!(rng("-100..", false, 5), (0, 4));
        assert_eq!(rng("-100..", true, 5), (1, 5));
    }

    #[test]
    fn range_dash_form_teaches_the_dotdot_grammar() {
        // `A-B` is the removed spelling — the error hands back both correct forms.
        let err = parse_range_spec("495-500", "--line", true).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("START..END"), "got: {msg}");
        assert!(msg.contains("495-500"), "got: {msg}");
    }

    #[test]
    fn range_one_based_rejects_zero() {
        assert!(parse_range_spec("0..5", "--line", true).is_err());
        assert!(parse_range_spec("0", "--line", true).is_err());
        let err = parse_range_spec("0..5", "--line", true).unwrap_err();
        assert!(err.to_string().contains("≥ 1"), "got: {err}");
    }

    #[test]
    fn range_errors() {
        // Non-integer bound.
        assert!(parse_range_spec("a..5", "--turn", false).is_err());
        // A statically-detectable reversal (both explicit) errors AT PARSE.
        let err = parse_range_spec("9..3", "--turn", false).unwrap_err();
        assert!(err.to_string().contains("before start"), "got: {err}");
        // The label appears in a parse error.
        let err2 = parse_range_spec("x", "--my-range", false).unwrap_err();
        assert!(err2.to_string().contains("--my-range"));
        // -0 is not a valid from-end index.
        assert!(parse_range_spec("-0..", "--r", false).is_err());
    }
}
