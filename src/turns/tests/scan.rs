//! Scan candidates, turn/time windows, scope summary, slice windows.

use super::*;

#[test]
fn line_is_turn_candidate_superset_of_assistant_text() {
    // A pure-text assistant record (no Edit/Write/Read/Bash) must pass — the broadened
    // prefilter is the design's required deviation.
    let pure_asst = br#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"just prose, no tools"}]}}"#;
    assert!(line_is_turn_candidate(pure_asst));
    let user = br#"{"type":"user","message":{"role":"user","content":"hi"}}"#;
    assert!(line_is_turn_candidate(user));
    let summary =
        br#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"s"}}"#;
    assert!(line_is_turn_candidate(summary));
    // A system metrics record with none of the markers → skipped.
    let sys = br#"{"type":"system","subtype":"turn_duration","durationMs":5}"#;
    assert!(!line_is_turn_candidate(sys));
}

#[test]
fn window_admits_turn_range_and_time() {
    let tr = Some((2usize, 5usize));
    let unbounded = TimeWindow::default();
    assert!(window_admits(
        3,
        Some("2026-06-07T05:00:00Z"),
        tr,
        &unbounded
    ));
    assert!(!window_admits(
        1,
        Some("2026-06-07T05:00:00Z"),
        tr,
        &unbounded
    ));
    assert!(!window_admits(6, None, tr, &unbounded));
    // No turn, bounded time excludes timestamp-less.
    let bounded = TimeWindow::from_args(Some("2026-06-01"), None).unwrap();
    assert!(!window_admits(0, None, None, &bounded));
}

#[test]
fn parse_turn_range_parses_and_rejects() {
    assert_eq!(
        parse_turn_range("2..5").unwrap().resolve(100, false),
        (2, 5)
    );
    assert!(parse_turn_range("5..2").is_err());
    assert!(parse_turn_range("noformat").is_err());
    assert!(parse_turn_range("a..b").is_err());
}

// ── Prefilter arm-by-arm + build None arms ──

#[test]
fn line_is_turn_candidate_each_arm_in_isolation() {
    // Each arm matched by a line that triggers ONLY that probe (so the OR short-circuits
    // at a different position each time → every arm's true side is exercised).
    assert!(line_is_turn_candidate(br#"x "role":"user" x"#)); // arm 1
    assert!(line_is_turn_candidate(br#"x "role":"assistant" x"#)); // arm 2
    assert!(line_is_turn_candidate(br#"x "type":"assistant" x"#)); // arm 3
    assert!(line_is_turn_candidate(b"x isCompactSummary x")); // arm 4
    assert!(line_is_turn_candidate(b"x tool_use x")); // arm 5
                                                      // A line matching NONE → all arms false.
    assert!(!line_is_turn_candidate(
        b"{\"type\":\"system\",\"subtype\":\"x\"}"
    ));
    assert!(!line_is_turn_candidate(b""));
}

#[test]
fn parse_turn_range_valid_range_hi_ge_lo() {
    // hi >= lo → the `hi < lo` FALSE arm (the success path, distinct from the reject).
    assert_eq!(
        parse_turn_range("0..0").unwrap().resolve(100, false),
        (0, 0)
    );
    assert_eq!(
        parse_turn_range("10..20").unwrap().resolve(100, false),
        (10, 20)
    );
}

// ── Fan-out scope summary + top-level/subagent id classification ──

#[test]
fn shared_uuid_validator_distinguishes_uuid_from_bare_hex() {
    // turns no longer rolls its own `is_top_level_session_id`; it now discriminates via the
    // authoritative path-derived `is_subagent` field, and any id-SHAPE check reuses the ONE
    // canonical validator `path::is_session_uuid` (the DRY fix). Spot-check that validator's
    // contract holds for the id forms turns' tests construct.
    assert!(crate::path::is_session_uuid(
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
    ));
    assert!(!crate::path::is_session_uuid("a00ea52f023afd9ce"));
    // Wrong group shape / non-hex → not a uuid.
    assert!(!crate::path::is_session_uuid("0a1b2c3d-4e5f-4a6b-8c7d"));
    assert!(!crate::path::is_session_uuid(
        "zzzzzzzz-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
    ));
}

/// A ScanResult with a chosen session id + a single trivial round-trip turn (so its plan
/// is non-empty under any budget).
fn scan_named(session_id: &str) -> ScanResult {
    // Keep the id-domain fields self-consistent with the chosen id form: a non-uuid (bare
    // hex) id is a subagent transcript. parent_session_id is the id itself for a top-level
    // session (a bare-hex test id has no real parent path, so it points at itself here).
    let is_subagent = !crate::path::is_session_uuid(session_id);
    ScanResult {
        session_id: session_id.to_string(),
        is_subagent,
        parent_session_id: session_id.to_string(),
        turns: vec![mk_turn(0, Some("ask"), Some("reply"), 1, 0)],
        summaries: Vec::new(),
        skipped_lines: 0,
    }
}

#[test]
fn scope_summary_counts_top_level_and_subagents() {
    // One top-level uuid + two bare-hex subagents, all rendering.
    let sessions = vec![
        scan_named("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"),
        scan_named("a00ea52f023afd9ce"),
        scan_named("a01086fb826e1ab0e"),
    ];
    let plans: Vec<SessionPlan> = sessions
        .iter()
        .map(|sr| plan_session(sr, 8000, 0.5, 0, &cfg()))
        .collect();
    let sc = scope_summary(&sessions, &plans);
    assert_eq!(sc.in_scope, 3);
    assert_eq!(sc.in_scope_top, 1);
    assert_eq!(sc.in_scope_sub, 2);
    assert_eq!(sc.rendered, 3);
}

#[test]
fn scope_summary_reports_true_scope_not_rendered() {
    // CRITICAL: a session whose plan selects nothing (budget too small) is STILL counted in
    // the TRUE scope and its top-level/subagent split — only `rendered` shrinks. This is what
    // stops a targeted top-level uuid from reading as `0 top-level` and a budget knob from
    // silently rewriting "scope".
    let mut empty = scan_named("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d");
    empty.turns.clear(); // its plan will be empty (nothing fits)
    let rendered_sub = scan_named("a00ea52f023afd9ce");
    let sessions = vec![empty, rendered_sub];
    let plans: Vec<SessionPlan> = sessions
        .iter()
        .map(|sr| plan_session(sr, 8000, 0.5, 0, &cfg()))
        .collect();
    let sc = scope_summary(&sessions, &plans);
    // Both sessions are in scope; the top-level one is counted even though it rendered nothing.
    assert_eq!(sc.in_scope, 2);
    assert_eq!(
        sc.in_scope_top, 1,
        "targeted top-level must NOT read as 0 top-level"
    );
    assert_eq!(sc.in_scope_sub, 1);
    assert_eq!(sc.rendered, 1, "only the subagent fit the budget");
}

// ── --slice chunked output (slice_into_windows) ──

#[test]
fn slice_windows_concatenate_back_to_the_source() {
    let doc = "line one\nline two\nthree\nfour five six\n";
    let chunks = slice_into_windows(doc, 12);
    assert_eq!(chunks.concat(), doc, "lossless reassembly across slices");
    for c in &chunks {
        assert!(c.chars().count() <= 12, "chunk over window: {c:?}");
    }
    assert!(chunks.len() > 1, "doc spans multiple windows");
}

#[test]
fn slice_windows_count_chars_not_bytes() {
    // Each `🛠` is 4 BYTES but 1 CHARACTER. A 6-char window fits 5 wrenches + newline
    // (21 bytes), proving the window counts Unicode scalars — the unit Claude Code's
    // additionalContext cap uses — not bytes (a byte budget would split after the first).
    let line = "🛠🛠🛠🛠🛠\n"; // 6 chars, 21 bytes
    let chunks = slice_into_windows(line, 6);
    assert_eq!(
        chunks.len(),
        1,
        "6 chars fit one 6-char window despite 21 bytes"
    );
    assert_eq!(chunks[0], line);
}

#[test]
fn slice_windows_hard_split_an_oversized_line_on_char_boundaries() {
    // A single line longer than the window is hard-split so NO chunk exceeds it — and never
    // mid-`🛠` (char boundary). Window 2, line of 5 wrenches (no trailing newline).
    let line = "🛠🛠🛠🛠🛠";
    let chunks = slice_into_windows(line, 2);
    assert_eq!(chunks.concat(), line, "lossless even when hard-splitting");
    for c in &chunks {
        assert!(c.chars().count() <= 2);
        assert!(c.chars().all(|ch| ch == '🛠'), "no broken char: {c:?}");
    }
    assert_eq!(chunks, vec!["🛠🛠", "🛠🛠", "🛠"]);
}

#[test]
fn slice_windows_empty_input_yields_no_chunks() {
    assert!(slice_into_windows("", 10).is_empty());
}
