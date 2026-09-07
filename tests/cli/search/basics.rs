//! search basics: round-trip exchanges, envelopes, empty patterns, totals.

use crate::harness::*;

#[test]
fn search_text_returns_round_trip_exchange() {
    let h = populated_home();
    let out = h.run(&["search", "carry"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("{}·t", &SESS[..8])),
        "the id-prefix header token:\n{}",
        out.stdout
    );
    assert!(out.stdout.contains("matched"));
    assert!(out.stdout.contains("malformed line(s) skipped"));
}

#[test]
fn search_json_emits_hits_and_summary() {
    let h = populated_home();
    let out = h.run(&["search", "carry", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert!(!lines.is_empty());
    // Last line is the trailing summary object with matched/dropped/skipped.
    let summary: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert!(
        summary.get("matched").is_some(),
        "no summary: {:?}",
        lines.last()
    );
    assert!(summary.get("skipped_lines").is_some());
}

#[test]
fn search_no_match_reports_zero() {
    let h = populated_home();
    let out = h.run(&["search", "zzz-no-such-token-zzz"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no matching exchanges"),
        "got: {}",
        out.stdout
    );
    // Even with no matches, the skipped-line note still surfaces.
    assert!(out.stdout.contains("malformed line(s) skipped"));
}

#[test]
fn search_empty_pattern_warns_then_emits() {
    let h = populated_home();
    let out = h.run(&["search", ""]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The unbounded empty-pattern warning goes to stderr.
    assert!(
        out.stderr.contains("empty pattern with no category"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn search_empty_pattern_with_session_target_only_does_not_warn() {
    // Empty pattern + ONLY an `@<uuid>` session target (no category/time/turn filter) → the
    // warning's `has_session_filter` operand (a `pins_single_session` target) is TRUE → warning
    // suppressed.
    let h = populated_home();
    let out = h.run(&["search", "", at(SESS).as_str(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("empty pattern with no category"),
        "an @<uuid> session scope must suppress the warning; stderr: {}",
        out.stderr
    );
}

#[test]
fn search_empty_pattern_with_category_does_not_warn() {
    // An empty pattern but WITH a `-t` category → the warning's
    // `args.categories.is_empty()` operand is FALSE, so the warning is suppressed.
    let h = populated_home();
    let out = h.run(&[
        "search",
        "",
        "-t",
        "user",
        "--no-subagents",
        at(SESS).as_str(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("empty pattern with no category"),
        "category filter must suppress the warning; stderr: {}",
        out.stderr
    );
}

#[test]
fn search_empty_pattern_with_uuid_positional_does_not_warn() {
    // A bare-uuid POSITIONAL routes to the SAME session filter as `--session` (via
    // resolve_session_files), so the empty-pattern warning - which claims "no session
    // filter" - must be SUPPRESSED. Previously the gate only inspected `--session` and
    // printed the misleading warning here.
    let h = populated_home();
    let out = h.run(&["search", "", at(SESS).as_str(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("empty pattern with no category"),
        "a bare-uuid positional scopes to one session and must suppress the warning; \
         stderr: {}",
        out.stderr
    );
}

#[test]
fn search_footer_always_reports_match_and_session_totals() {
    let h = populated_home();
    let out = h.run(&["search", "carry", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("· 1 session ·"),
        "the footer carries the distinct-session total: {}",
        out.stdout
    );
    // JSON footer gains `sessions` alongside `matched`.
    let j = h.run(&["search", "carry", "--no-subagents", "--format", "json"]);
    let footer: serde_json::Value = serde_json::from_str(
        j.stdout
            .lines()
            .filter(|l| !l.is_empty())
            .next_back()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(footer["sessions"], 1);
    assert!(footer["matched"].as_u64().unwrap() >= 1);
}

#[test]
fn search_text_output_is_token_lean() {
    let h = populated_home();
    let out = h.run(&["search", "carry", at(SESS).as_str(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Every exchange header opens with the STABLE id-prefix token (`<first-8>·t<n>`) - no
    // per-invocation `sN` ordinal, no `sN = <uuid>` legend block anywhere in the output.
    assert!(
        out.stdout.contains(&format!("{}·t", &SESS[..8])),
        "id-prefix header token: {}",
        out.stdout
    );
    assert!(
        !out.stdout.lines().any(|l| l.starts_with("s1 = ")),
        "no legend line: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("s1·t"),
        "no ordinal header: {}",
        out.stdout
    );
    // The old heavyweight header is gone: no `═══` rule, no uppercase `SESSION `/`TURN `.
    assert!(
        !out.stdout.contains("═══"),
        "no rule glyphs: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("TURN "),
        "no uppercase TURN: {}",
        out.stdout
    );
    // The FULL uuid never repeats per exchange - the 8-char prefix token carries each header.
    assert_eq!(
        out.stdout.matches(SESS).count(),
        0,
        "the full uuid is never printed; the prefix token references it: {}",
        out.stdout
    );
    // Timestamps are single local+offset (no `(<UTC>)` second copy on the turn header).
    assert!(
        !out.stdout.contains(" (2026-"),
        "no parenthesised UTC copy: {}",
        out.stdout
    );
}

#[test]
fn search_invalid_regex_errors() {
    let h = populated_home();
    let out = h.run(&["search", "(unclosed"]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("invalid regex"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn search_help_mentions_regex_dialect_boundaries() {
    let h = Home::new();
    let out = h.run(&["search", "--help"]);
    assert!(out.success);
    assert!(
        out.stdout.contains("linear-time"),
        "dialect block: {}",
        out.stdout
    );
    assert!(out.stdout.contains("backreference"));
    assert!(out.stdout.contains("lookahead") || out.stdout.contains("lookbehind"));
}

/// Help IS surface (the five-document contract), so the C-27 diff line is pinned in
/// BOTH manuals: the taxonomy's `.unsent` entry in `search --help`, its own section in
/// `show --help`. Each must name the metric, the "can exceed 100" caveat and the JSON
/// keys, so a help edit that drops one fails here instead of silently thinning the
/// manual against SKILL.
#[test]
fn help_pins_the_unsent_diff_line_in_both_manuals() {
    let h = Home::new();
    for (cmd, keys) in [
        ("search", "diff_chars, diff_pct, diff_exact"),
        ("show", "diff_chars, diff_pct and diff_exact"),
    ] {
        let out = h.run(&[cmd, "--help"]);
        assert!(out.success, "{cmd} --help: {}", out.stderr);
        assert!(
            out.stdout
                .contains("differs from the sent message in N chars"),
            "{cmd} --help states the diff line:\n{}",
            out.stdout
        );
        assert!(
            out.stdout.contains("CHARACTER edit script")
                && out.stdout.contains("never a length difference"),
            "{cmd} --help names the metric:\n{}",
            out.stdout
        );
        assert!(
            out.stdout.contains("exceed 100"),
            "{cmd} --help keeps the over-100 caveat:\n{}",
            out.stdout
        );
        assert!(
            out.stdout.contains("superseding_line, superseding_uuid") && out.stdout.contains(keys),
            "{cmd} --help names the JSON keys:\n{}",
            out.stdout
        );
    }
}

#[test]
fn regex_alternation_extracts_needles_and_still_matches() {
    // The v0.9.4 required-needle gate: a metachar alternation is prefiltered by the
    // union of its branches' safe literals - matches survive, needle-less files gate.
    let h = Home::new();
    let enc = "-Users-dev-example-project";
    let mk = |id: &str, text: &str| {
        h.write(
            &format!("{enc}/{id}.jsonl"),
            &format!(
                "{}\n",
                format_args!(
                    r#"{{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"{text}"}}}}"#
                )
            ),
        );
    };
    mk(
        "aaaa0001-0000-4000-8000-000000000001",
        "Harborx lights the beacon",
    );
    mk(
        "aaaa0001-0000-4000-8000-000000000002",
        "the beacon calls, Lanternz answers",
    );
    mk(
        "aaaa0001-0000-4000-8000-000000000003",
        "nothing relevant here",
    );
    let out = h.run(&["search", "Harborx.*beacon|beacon.*Lanternz", enc, "-c"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert_eq!(
        out.stdout.trim(),
        "2",
        "both branch shapes match:\n{}",
        out.stdout
    );
    // Same query case-insensitively (caseless alternation prefilter path).
    let ci = h.run(&["search", "harborx.*beacon|beacon.*lanternz", enc, "-c"]);
    assert_eq!(ci.stdout.trim(), "2", "{}", ci.stdout);
}

#[test]
fn the_unbounded_scan_warning_is_for_an_empty_pattern_alone() {
    // The advisory fires on the one query that really does match everything in scope. A
    // pattern narrows the result by definition, so warning about it would train a reader
    // to ignore the line that matters.
    let h = populated_home();
    let bare = h.run(&["search", ""]);
    assert!(bare.success, "stderr: {}", bare.stderr);
    assert!(
        bare.stderr.contains("matches every exchange in scope"),
        "an empty pattern with no filter is warned about: {}",
        bare.stderr
    );
    let pattern = h.run(&["search", "carry"]);
    assert!(pattern.success, "stderr: {}", pattern.stderr);
    assert!(
        !pattern.stderr.contains("matches every exchange in scope"),
        "a real pattern is not an unbounded scan: {}",
        pattern.stderr
    );
    // A filtered empty pattern is bounded too.
    let filtered = h.run(&["search", "", "-t", "user.message"]);
    assert!(
        !filtered.stderr.contains("matches every exchange in scope"),
        "a label filter bounds it: {}",
        filtered.stderr
    );
}
