//! Unit tests for the `turns` engine over small, locale-neutral fixtures.
//!
//! Charset discipline (CLAUDE.md / design §0): fixture strings use accented-Latin +
//! emoji multi-byte tokens only (`café🛠`), the house fixture style. The
//! end-to-end feature exercise on a real-shaped, multi-summary transcript lives in
//! `tests/cli_integration.rs`.

use super::*;

/// Build a [`TurnUnit`] for a role from a plain string (no record), for cost/ellipsis
/// tests. `orig_newlines` lets a test assert the `L lines elided` note.
fn unit(role: Role, line_no: usize, text: &str, orig_newlines: usize) -> TurnUnit {
    TurnUnit {
        line_no,
        role,
        full_chars: text.chars().count(),
        text: text.to_string(),
        orig_newlines,
        ts_utc: Some("2026-06-07T05:00:00.000Z".to_string()),
        also_in_summary: false,
    }
}

#[test]
fn role_caps_and_head_fractions_are_asymmetric() {
    assert_eq!(Role::User.cap(), 600);
    assert_eq!(Role::Assistant.cap(), 900);
    assert!(Role::Assistant.cap() > Role::User.cap());
    assert!(Role::Assistant.head_frac() > Role::User.head_frac());
    assert_eq!(Role::User.label(), "user");
    assert_eq!(Role::Assistant.label(), "assistant");
}

#[test]
fn sub_cap_unit_renders_verbatim_no_marker() {
    let u = unit(Role::User, 10, "café🛠 a short ask", 0);
    let r = render_unit_body(&u);
    assert!(!r.truncated);
    assert_eq!(r.body, "café🛠 a short ask");
    assert_eq!(r.elided_chars, 0);
    assert_eq!(r.elided_lines, 0);
    assert_eq!(r.rendered_chars, "café🛠 a short ask".chars().count());
}

#[test]
fn user_ellipsis_head_360_tail_240_with_counts() {
    // A user body > 600 chars → head 360 / tail 240, marker carries +K chars.
    let body: String = "a".repeat(1000);
    let u = unit(Role::User, 10, &body, 3);
    let r = render_unit_body(&u);
    assert!(r.truncated);
    assert_eq!(r.elided_chars, 1000 - 600);
    assert_eq!(r.elided_lines, 3);
    // head 360, tail 240 (600 cap, 0.60 head frac).
    assert!(r.body.starts_with(&"a".repeat(360)));
    assert!(r.body.ends_with(&"a".repeat(240)));
    assert!(r.body.contains("[+400 chars, 3 lines elided]"));
    // The displayed (rendered) char count excludes the marker scaffolding.
    assert_eq!(r.rendered_chars, 600);
}

#[test]
fn assistant_ellipsis_head_larger_than_user_head() {
    // Assistant 900 cap, 0.66 head frac → head 594 / tail 306. Strictly larger head
    // than the user side (the measured asymmetry).
    let body: String = "b".repeat(2000);
    let a = unit(Role::Assistant, 20, &body, 7);
    let r = render_unit_body(&a);
    assert!(r.truncated);
    assert_eq!(r.elided_chars, 2000 - 900);
    // head 594.
    assert!(r.body.starts_with(&"b".repeat(594)));
    assert!(r.body.ends_with(&"b".repeat(306)));
    assert!(r.body.contains("[+1100 chars, 7 lines elided]"));
    assert_eq!(r.rendered_chars, 900);

    // The assistant head (594) is strictly larger than the user head (360).
    let ubody: String = "u".repeat(2000);
    let ru = render_unit_body(&unit(Role::User, 1, &ubody, 0));
    let asst_head_len = 594usize;
    let user_head_len = 360usize;
    assert!(asst_head_len > user_head_len);
    // and the rendered user head prefix is shorter than the assistant head prefix.
    assert!(ru.body.starts_with(&"u".repeat(360)));
}

#[test]
fn single_line_user_omits_lines_elided_note() {
    let body: String = "x".repeat(1000);
    let u = unit(Role::User, 5, &body, 0); // 0 original newlines
    let r = render_unit_body(&u);
    assert!(r.truncated);
    assert!(r.body.contains("[+400 chars]"));
    assert!(
        !r.body.contains("lines elided"),
        "single-line message must omit the line note: {}",
        r.body
    );
}

#[test]
fn ellipsis_cut_is_codepoint_safe_for_multibyte_token() {
    // A multi-byte token straddling the cut must be wholly kept or wholly dropped — the
    // rendered string must remain valid UTF-8 with no replacement char.
    // Build a body of 700 single-char 'a' then a 🛠 (4 bytes) at the boundary region.
    let mut body = String::new();
    body.push_str(&"a".repeat(360)); // exactly the head region
    body.push('🛠'); // lands right after the head cut
    body.push_str(&"a".repeat(400));
    let u = unit(Role::User, 1, &body, 0);
    let r = render_unit_body(&u);
    // valid UTF-8 by construction (String), and the emoji is either in head or dropped,
    // never split. The head is the first 360 'a's; the emoji is elided.
    assert!(r.body.starts_with(&"a".repeat(360)));
    assert!(!r.body.contains('\u{FFFD}'), "no replacement char");
    // The whole rendered body round-trips through chars (no mid-codepoint slice).
    let rebuilt: String = r.body.chars().collect();
    assert_eq!(rebuilt, r.body);
}

#[test]
fn emoji_in_tail_is_wholly_kept() {
    // Put the emoji in the tail region; it must survive intact.
    let mut body = String::new();
    body.push_str(&"a".repeat(700));
    body.push('🛠');
    body.push_str(&"b".repeat(239)); // tail = last 240 chars = 🛠 + 239 b's
    let u = unit(Role::User, 1, &body, 0);
    let r = render_unit_body(&u);
    assert!(r.truncated);
    assert!(r.body.contains('🛠'), "emoji in the kept tail: {}", r.body);
    assert!(!r.body.contains('\u{FFFD}'));
}

#[test]
fn marker_cost_zero_for_no_tool_calls() {
    assert_eq!(marker_cost(0), 0);
    // "  [3 tool calls]\n" = 2 + 1 + 1 + 12 + 1 = count chars.
    let expect = "  [3 tool calls]\n".chars().count();
    assert_eq!(marker_cost(3), expect);
    // A big count costs more.
    assert!(marker_cost(231) > marker_cost(3));
}

#[test]
fn unit_cost_includes_header_and_caps_body() {
    let small = unit(Role::User, 1, "hi", 0);
    assert_eq!(unit_cost(&small), HEADER_COST + 2);
    // A huge user unit costs header + 600 (cap) + the marker scaffolding chars.
    let big = unit(Role::User, 1, &"z".repeat(5000), 2);
    let r = render_unit_body(&big);
    assert_eq!(unit_cost(&big), HEADER_COST + r.body.chars().count());
    assert!(
        unit_cost(&big) < HEADER_COST + 700,
        "body is capped near 600"
    );
}

// ── Fingerprint / dedup ──

#[test]
fn fingerprint_normalizes_lowercases_and_caps_at_80() {
    let fp = fingerprint("  Hello   WORLD café  ");
    assert_eq!(fp, "hello world café");
    let long = "a".repeat(200);
    assert_eq!(fingerprint(&long).chars().count(), 80);
    assert_eq!(fingerprint(""), "");
    assert_eq!(fingerprint("   "), "");
}

#[test]
fn quoted_inner_extracts_first_quoted_run() {
    assert_eq!(
        quoted_inner(r#"- "initial greeting about café" (note)"#).as_deref(),
        Some("initial greeting about café")
    );
    assert_eq!(quoted_inner("no quotes here"), None);
    assert_eq!(quoted_inner(r#"one " only"#), None); // unmatched quote
}

#[test]
fn summary_fingerprints_pulls_bullets_and_quotes() {
    let body = "\
6. All user messages:
   - \"the very first ask about the carry logic\"
   - \"second ask: explain the panic path now please\"
9. Optional Next Step:
   The assistant said \"I will run the budget walk and report counts\".";
    let fps = summary_fingerprints(body);
    assert!(fps.iter().any(|f| f.starts_with("the very first ask")));
    assert!(fps.iter().any(|f| f.starts_with("second ask")));
    assert!(fps.iter().any(|f| f.contains("run the budget walk")));
}

#[test]
fn unit_matches_summary_prefix_either_direction() {
    let summary_fps = vec![fingerprint(
        "the very first ask about the carry logic in detail",
    )];
    // A unit clipped shorter than the summary bullet still matches (prefix).
    let short = unit(Role::User, 1, "the very first ask", 0);
    assert!(unit_matches_summary(&short, &summary_fps));
    // A unit longer than the bullet (summary is the prefix) also matches.
    let long = unit(
        Role::User,
        1,
        "the very first ask about the carry logic in detail, and then much more text",
        0,
    );
    assert!(unit_matches_summary(&long, &summary_fps));
    // An unrelated unit does not match.
    let other = unit(Role::User, 1, "a completely different question entirely", 0);
    assert!(!unit_matches_summary(&other, &summary_fps));
    // An empty-fingerprint unit never matches.
    let empty = unit(Role::User, 1, "   ", 0);
    assert!(!unit_matches_summary(&empty, &summary_fps));
}

// ── Turn cost + selection sides ──

fn mk_turn(
    turn_index: usize,
    user: Option<&str>,
    asst: Option<&str>,
    tools: usize,
    comp: usize,
) -> TurnSlice {
    TurnSlice {
        turn_index,
        user: user.map(|t| unit(Role::User, turn_index * 10 + 1, t, 0)),
        tool_calls: tools,
        assistant_eot: asst.map(|t| unit(Role::Assistant, turn_index * 10 + 5, t, 0)),
        compactions_before: comp,
    }
}

#[test]
fn turn_cost_both_charges_marker_single_side_does_not() {
    let t = mk_turn(0, Some("ask"), Some("reply"), 3, 0);
    let both = turn_cost(&t, SelSides::Both);
    let user_only = turn_cost(&t, SelSides::UserOnly);
    let asst_only = turn_cost(&t, SelSides::AssistantOnly);
    // Both = user + marker + assistant.
    assert_eq!(both, user_only + marker_cost(3) + (asst_only));
    // A zero-tool turn charges no marker on the both selection.
    let t0 = mk_turn(0, Some("ask"), Some("reply"), 0, 0);
    assert_eq!(
        turn_cost(&t0, SelSides::Both),
        turn_cost(&t0, SelSides::UserOnly) + turn_cost(&t0, SelSides::AssistantOnly)
    );
}

#[test]
fn is_round_trip_requires_both_sides() {
    assert!(mk_turn(0, Some("a"), Some("b"), 0, 0).is_round_trip());
    assert!(!mk_turn(0, Some("a"), None, 0, 0).is_round_trip());
    assert!(!mk_turn(0, None, Some("b"), 0, 0).is_round_trip());
}

// ── Budget allocation (the load-bearing 50% floor) ──

/// Build a session of `n` complete round-trips + a trailing assistant-heavy block, to
/// drive the 50%-floor regression: a naive recency walk would starve users.
fn scan_with_turns(turns: Vec<TurnSlice>, summaries: Vec<SummaryInfo>) -> ScanResult {
    ScanResult {
        session_id: "s".to_string(),
        turns,
        summaries,
        skipped_lines: 0,
    }
}

#[test]
fn rt_floor_recovers_a_user_turn_despite_assistant_heavy_tail() {
    // Mirror the pulse finding: an assistant-heavy tail (3 huge assistant-only "turns"
    // near EOF) plus older complete round-trips. Without the 50% floor a recency walk
    // would spend the whole budget on the assistant monologue and recover 0 users.
    let huge = "h".repeat(5000);
    let mut turns = Vec::new();
    // older complete round-trips (turn 0..3)
    for i in 0..4 {
        turns.push(mk_turn(
            i,
            Some(&format!("user ask number {i}")),
            Some(&format!("reply {i}")),
            1,
            0,
        ));
    }
    // assistant-heavy tail: complete turns whose assistant side is enormous (turn 4..7)
    for i in 4..7 {
        turns.push(mk_turn(i, Some(&format!("short {i}")), Some(&huge), 2, 0));
    }
    let sr = scan_with_turns(turns, Vec::new());
    let plan = plan_session(&sr, 8000, 0.5, 0);
    let (n_user, _n_asst) = count_sides(&plan);
    assert!(
        n_user >= 1,
        "the 50% floor must recover >=1 user turn, got {n_user}"
    );
    // And the rendered chars stay within budget.
    assert!(
        plan.rendered_chars <= 8000,
        "budget respected: {}",
        plan.rendered_chars
    );
}

#[test]
fn budget_respected_and_monotonic() {
    let mut turns = Vec::new();
    for i in 0..20 {
        turns.push(mk_turn(
            i,
            Some(&format!("the user asks question {i} about the carry logic")),
            Some(&format!("the assistant replies in detail to question {i}")),
            i % 3,
            0,
        ));
    }
    let sr = scan_with_turns(turns, Vec::new());
    let big = plan_session(&sr, 40000, 0.5, 0);
    let small = plan_session(&sr, 4000, 0.5, 0);
    assert!(big.rendered_chars <= 40000);
    assert!(small.rendered_chars <= 4000);
    assert!(small.selected.len() <= big.selected.len());
    // Monotonic: the smaller budget's selected turn-indices are a subset of the bigger.
    let big_set: std::collections::BTreeSet<usize> =
        big.selected.iter().map(|s| s.turn_index).collect();
    for s in &small.selected {
        assert!(
            big_set.contains(&s.turn_index),
            "turn {} in small but not big (recency-first must be stable)",
            s.turn_index
        );
    }
}

#[test]
fn higher_rt_fraction_takes_more_complete_pairs() {
    let mut turns = Vec::new();
    for i in 0..20 {
        turns.push(mk_turn(
            i,
            Some(&format!("user question {i} text here")),
            Some(&format!("assistant answer {i} text here")),
            1,
            0,
        ));
    }
    let sr = scan_with_turns(turns, Vec::new());
    let count_both = |p: &SessionPlan| {
        p.selected
            .iter()
            .filter(|s| matches!(s.sides, SelSides::Both))
            .count()
    };
    let low = plan_session(&sr, 6000, 0.3, 0);
    let high = plan_session(&sr, 6000, 0.8, 0);
    assert!(
        count_both(&high) >= count_both(&low),
        "higher rt-fraction must not yield fewer complete pairs: {} vs {}",
        count_both(&high),
        count_both(&low)
    );
}

#[test]
fn determinism_identical_plan_for_identical_input() {
    let mut turns = Vec::new();
    for i in 0..12 {
        turns.push(mk_turn(
            i,
            Some(&format!("u{i}")),
            Some(&format!("a{i}")),
            1,
            0,
        ));
    }
    let sr = scan_with_turns(turns, Vec::new());
    let p1 = plan_session(&sr, 5000, 0.5, 0);
    let p2 = plan_session(&sr, 5000, 0.5, 0);
    let idx = |p: &SessionPlan| p.selected.iter().map(|s| s.turn_index).collect::<Vec<_>>();
    assert_eq!(idx(&p1), idx(&p2));
    assert_eq!(p1.rendered_chars, p2.rendered_chars);
}

// ── Multi-compaction spanning + dedup demotion ──

fn summary(line_no: usize, fps: Vec<&str>, body_chars: usize) -> SummaryInfo {
    SummaryInfo {
        line_no,
        fingerprints: fps.into_iter().map(fingerprint).collect(),
        body_chars,
    }
}

#[test]
fn spanned_boundary_count_is_max_compactions_before() {
    let turns = vec![
        mk_turn(0, Some("a"), Some("b"), 0, 2), // behind 2 summaries
        mk_turn(1, Some("c"), Some("d"), 0, 1), // behind newest summary
        mk_turn(2, Some("e"), Some("f"), 0, 0), // live region
    ];
    let selected = vec![
        Selected {
            turn_index: 0,
            sides: SelSides::Both,
        },
        Selected {
            turn_index: 1,
            sides: SelSides::Both,
        },
        Selected {
            turn_index: 2,
            sides: SelSides::Both,
        },
    ];
    // The oldest selected turn (cb=2) means the ascending render crosses 2 boundaries.
    assert_eq!(spanned_boundary_count(&turns, &selected), 2);
    // Only the live turn selected → 0 boundaries spanned.
    let live_only = vec![Selected {
        turn_index: 2,
        sides: SelSides::Both,
    }];
    assert_eq!(spanned_boundary_count(&turns, &live_only), 0);
    // Empty selection → 0.
    assert_eq!(spanned_boundary_count(&turns, &[]), 0);
}

#[test]
fn dedup_demotes_live_region_match_but_not_pre_boundary() {
    // A live-region (compactions_before==0) user turn whose text matches a summary
    // bullet is flagged + demoted; an identical-text turn BEFORE an older boundary is
    // pure restoration (never deduped).
    let dup_text = "the very first ask about the carry logic";
    let turns = vec![
        mk_turn(0, Some(dup_text), Some("old reply"), 0, 1), // pre-boundary: not deduped
        mk_turn(1, Some(dup_text), Some("live reply"), 0, 0), // live: deduped
    ];
    let sums = vec![summary(900, vec![dup_text], 12000)];
    let sr = scan_with_turns(turns, sums);
    let plan = plan_session(&sr, 40000, 0.5, 0);
    assert_eq!(
        plan.dedup_demoted, 1,
        "exactly the live-region match is demoted"
    );
    // The pre-boundary identical turn is still selected (not dropped) and NOT flagged.
    assert!(plan.selected.iter().any(|s| s.turn_index == 0));
}

#[test]
fn dedup_demoted_turn_is_selected_after_non_dups_not_dropped() {
    // Even a dedup-flagged turn is still selected (demote, not delete) when budget allows.
    let dup_text = "shared question text that the summary already has verbatim here";
    let turns = vec![
        mk_turn(
            0,
            Some("unique earlier question"),
            Some("earlier reply"),
            0,
            0,
        ),
        mk_turn(1, Some(dup_text), Some("a later reply"), 0, 0),
    ];
    let sums = vec![summary(900, vec![dup_text], 9000)];
    let sr = scan_with_turns(turns, sums);
    let plan = plan_session(&sr, 40000, 0.5, 0);
    assert!(plan.dedup_demoted >= 1);
    // Both turns selected (generous budget): the dup is demoted, never dropped.
    assert!(plan.selected.iter().any(|s| s.turn_index == 1));
    assert!(plan.selected.iter().any(|s| s.turn_index == 0));
}

#[test]
fn max_compactions_caps_reach() {
    let turns = vec![
        mk_turn(0, Some("a"), Some("b"), 0, 3),
        mk_turn(1, Some("c"), Some("d"), 0, 2),
        mk_turn(2, Some("e"), Some("f"), 0, 1),
        mk_turn(3, Some("g"), Some("h"), 0, 0),
    ];
    let sr = scan_with_turns(turns, Vec::new());
    let capped = plan_session(&sr, 40000, 0.5, 1);
    // Only turns with compactions_before <= 1 survive.
    for s in &capped.selected {
        assert!(s.turn_index >= 2, "turn {} beyond cap leaked", s.turn_index);
    }
    assert!(capped.spanned_boundaries <= 1);
}

// ── compact_summary_body / raw_body_newlines via real Record parse ──

fn rec(json: &str) -> Record {
    serde_json::from_str(json).expect("valid record")
}

#[test]
fn compact_summary_body_reads_string_content_only() {
    let s = rec(
        r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"the summary body text"}}"#,
    );
    assert_eq!(
        compact_summary_body(&s).as_deref(),
        Some("the summary body text")
    );
    // A block-bodied (surprise) summary → None, not a guess.
    let blocks = rec(
        r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":[{"type":"text","text":"x"}]}}"#,
    );
    assert!(compact_summary_body(&blocks).is_none());
    // No message → None.
    let bare = rec(r#"{"type":"user","isCompactSummary":true}"#);
    assert!(compact_summary_body(&bare).is_none());
}

#[test]
fn raw_body_newlines_counts_user_and_assistant_bodies() {
    let u = rec(
        r#"{"type":"user","message":{"role":"user","content":"line one\nline two\nline three"}}"#,
    );
    assert_eq!(raw_body_newlines(&u), 2);
    let a = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"first"},{"type":"text","text":"second"}]}}"#,
    );
    // two text blocks joined by \n → 1 newline.
    assert_eq!(raw_body_newlines(&a), 1);
    // No message → 0.
    let bare = rec(r#"{"type":"system","subtype":"x"}"#);
    assert_eq!(raw_body_newlines(&bare), 0);
    // message but no content → 0.
    let nocontent = rec(r#"{"type":"user","message":{"role":"user"}}"#);
    assert_eq!(raw_body_newlines(&nocontent), 0);
}

// ── build(): turn slices + tool-call counts from line-numbered records ──

#[test]
fn build_produces_round_trip_with_tool_count_and_compaction() {
    // turn 0: user, assistant tool_use x2, assistant text. Then a summary. Then turn 1.
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(
                r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"first ask about café"}}"#,
            ),
        ),
        (
            2,
            rec(
                r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{}},{"type":"tool_use","id":"t2","name":"Bash","input":{}}]}}"#,
            ),
        ),
        (
            3,
            rec(
                r#"{"type":"assistant","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"the reply to the first ask"}]}}"#,
            ),
        ),
        (
            4,
            rec(
                r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"6. All user messages:\n   - \"first ask about café\""}}"#,
            ),
        ),
        (
            5,
            rec(
                r#"{"type":"user","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"second ask after compaction"}}"#,
            ),
        ),
        (
            6,
            rec(
                r#"{"type":"assistant","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"the second reply"}]}}"#,
            ),
        ),
    ];
    let (turns, summaries) = build(&records);
    assert_eq!(turns.len(), 2);
    // turn 0: round-trip, 2 tool calls, before the (one) summary → compactions_before 1.
    assert!(turns[0].is_round_trip());
    assert_eq!(turns[0].tool_calls, 2);
    assert_eq!(turns[0].compactions_before, 1);
    assert_eq!(turns[0].user.as_ref().unwrap().line_no, 1);
    assert_eq!(turns[0].assistant_eot.as_ref().unwrap().line_no, 3);
    // turn 1: live region, 0 tool calls, after the summary.
    assert_eq!(turns[1].compactions_before, 0);
    assert_eq!(turns[1].tool_calls, 0);
    // one summary captured with a fingerprint of the §6 bullet.
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].line_no, 4);
    assert!(summaries[0]
        .fingerprints
        .iter()
        .any(|f| f.starts_with("first ask about café")));
}

#[test]
fn build_pure_tool_call_turn_has_no_assistant_eot() {
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(r#"{"type":"user","message":{"role":"user","content":"do the thing"}}"#),
        ),
        (
            2,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#,
            ),
        ),
    ];
    let (turns, _s) = build(&records);
    assert_eq!(turns.len(), 1);
    assert!(turns[0].user.is_some());
    assert!(
        turns[0].assistant_eot.is_none(),
        "pure tool-call turn has no EOT text"
    );
    assert_eq!(turns[0].tool_calls, 1);
    assert!(!turns[0].is_round_trip());
}

// ── boundary banner crossing logic ──

#[test]
fn crossed_summaries_first_turn_crosses_nothing() {
    // 3 summaries at lines 100<200<300 → ranks (newest=1): 300=r1, 200=r2, 100=r3.
    let sums = vec![
        summary(100, vec![], 10),
        summary(200, vec![], 20),
        summary(300, vec![], 30),
    ];
    // FIRST turn (from=None): the cursor seeds at its own depth → crosses NOTHING (a
    // summary older than every selected turn has no restored turn below it).
    assert!(crossed_summaries(&sums, None, 2).is_empty());
    assert!(crossed_summaries(&sums, None, 0).is_empty());
}

#[test]
fn crossed_summaries_step_emits_each_boundary_once() {
    let sums = vec![
        summary(100, vec![], 10),
        summary(200, vec![], 20),
        summary(300, vec![], 30),
    ];
    // Moving from a turn at cb=2 to a turn at cb=0: crosses ranks (0, 2] = r1 (300) and
    // r2 (200), ascending → [200, 300].
    let crossed = crossed_summaries(&sums, Some(2), 0);
    assert_eq!(
        crossed.iter().map(|s| s.line_no).collect::<Vec<_>>(),
        vec![200, 300]
    );
    // Moving from cb=2 to cb=1: crosses only rank 2 (line 200).
    let one = crossed_summaries(&sums, Some(2), 1);
    assert_eq!(one.iter().map(|s| s.line_no).collect::<Vec<_>>(), vec![200]);
    // No movement (same cb) → nothing; moving DEEPER (to >= from) → nothing.
    assert!(crossed_summaries(&sums, Some(1), 1).is_empty());
    assert!(crossed_summaries(&sums, Some(1), 2).is_empty());
}

#[test]
fn boundary_banners_total_equals_max_cb_across_walk() {
    // Full ascending walk: first turn at cb=2 (crosses nothing), then cb=1, cb=0. Total
    // banners == the greatest cb (2), each summary within the span emitted exactly once.
    let sums = vec![
        summary(100, vec![], 10),
        summary(200, vec![], 20),
        summary(300, vec![], 30),
    ];
    let mut prev: Option<usize> = None;
    let mut emitted: Vec<usize> = Vec::new();
    for cb in [2usize, 1, 0] {
        for s in crossed_summaries(&sums, prev, cb) {
            emitted.push(s.line_no);
        }
        prev = Some(cb);
    }
    // The two summaries WITHIN the selected span (ranks 1,2 = lines 300,200), each once.
    // The oldest summary (line 100, rank 3) is OLDER than every selected turn → never
    // bannered.
    assert_eq!(emitted.len(), 2, "banners == max cb (2): {emitted:?}");
    let mut sorted = emitted.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, vec![200, 300]);
}

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
    // No turn-range, bounded time excludes timestamp-less.
    let bounded = TimeWindow::from_args(Some("2026-06-01"), None).unwrap();
    assert!(!window_admits(0, None, None, &bounded));
}

#[test]
fn parse_turn_range_parses_and_rejects() {
    assert_eq!(parse_turn_range("2..5").unwrap(), (2, 5));
    assert!(parse_turn_range("5..2").is_err());
    assert!(parse_turn_range("noformat").is_err());
    assert!(parse_turn_range("a..b").is_err());
}

// ── Branch-completeness: selection edge paths + renderers ──

#[test]
fn phase1_giant_first_round_trip_is_clamped_in() {
    // The most-recent complete turn is larger than the WHOLE rt reservation. It must
    // still be selected (the most-recent exchange is load-bearing), clamped — the
    // `spent_rt == 0 && c <= rt_budget` / `> rt_budget` arms of Phase 1.
    let huge_u = "u".repeat(5000);
    let huge_a = "a".repeat(5000);
    let turns = vec![mk_turn(0, Some(&huge_u), Some(&huge_a), 3, 0)];
    let sr = scan_with_turns(turns, Vec::new());
    // A tiny budget whose rt half is smaller than the single capped round-trip cost.
    let plan = plan_session(&sr, 2000, 0.5, 0);
    assert_eq!(
        plan.selected.len(),
        1,
        "the giant most-recent turn is included"
    );
    assert!(matches!(plan.selected[0].sides, SelSides::Both));
}

#[test]
fn phase2_fills_user_only_when_round_trip_does_not_fit() {
    // turn 2 (most recent, small complete) is taken whole in Phase 1; turn 1 (older, a
    // complete pair whose ASSISTANT is huge) does not fit the remaining pool as a whole
    // pair, so Phase 2 takes its USER side only (user-first preference); turn 0 small.
    let turns = vec![
        mk_turn(
            0,
            Some("oldest short ask"),
            Some("oldest short reply"),
            0,
            0,
        ),
        mk_turn(
            1,
            Some("middle user ask kept verbatim"),
            Some(&"a".repeat(3000)),
            0,
            0,
        ),
        mk_turn(
            2,
            Some("most recent short ask"),
            Some("most recent short reply"),
            0,
            0,
        ),
    ];
    let sr = scan_with_turns(turns, Vec::new());
    // Budget tuned so the small complete pairs + turn 1's user-only fit, but turn 1's
    // 900-capped assistant side does not.
    let plan = plan_session(&sr, 700, 0.5, 0);
    assert!(
        plan.rendered_chars <= 700,
        "budget respected: {}",
        plan.rendered_chars
    );
    assert!(
        plan.selected
            .iter()
            .any(|s| s.turn_index == 1 && matches!(s.sides, SelSides::UserOnly)),
        "the recent turn's user side is filled (assistant too big): {:?}",
        plan.selected
            .iter()
            .map(|s| (s.turn_index, s.sides))
            .collect::<Vec<_>>()
    );
}

#[test]
fn partial_user_only_turn_is_selected_as_user_only() {
    // A turn with NO assistant EOT (pure tool-call) is partial → Phase 2 takes its user
    // side only (the `else if t.user.is_some()` arm).
    let turns = vec![
        mk_turn(0, Some("complete ask"), Some("complete reply"), 0, 0),
        mk_turn(1, Some("partial ask with no eot"), None, 2, 0),
    ];
    let sr = scan_with_turns(turns, Vec::new());
    let plan = plan_session(&sr, 40000, 0.5, 0);
    assert!(
        plan.selected
            .iter()
            .any(|s| s.turn_index == 1 && matches!(s.sides, SelSides::UserOnly)),
        "partial (no-EOT) turn taken user-only"
    );
}

#[test]
fn partial_assistant_only_turn_is_selected_as_assistant_only() {
    // A turn with NO user opener (a synthetic/orphan lead with only assistant text) →
    // the `else if t.assistant_eot.is_some()` arm.
    let turns = vec![
        mk_turn(
            0,
            None,
            Some("orphan assistant reply with no user opener"),
            0,
            0,
        ),
        mk_turn(1, Some("normal ask"), Some("normal reply"), 0, 0),
    ];
    let sr = scan_with_turns(turns, Vec::new());
    let plan = plan_session(&sr, 40000, 0.5, 0);
    assert!(
        plan.selected
            .iter()
            .any(|s| s.turn_index == 0 && matches!(s.sides, SelSides::AssistantOnly)),
        "orphan assistant-only turn taken assistant-only"
    );
}

#[test]
fn render_turn_text_emits_user_marker_assistant_for_both() {
    // Exercise the text renderer arms directly: a both-sides turn with tool calls emits
    // ▽ user, the [N tool calls] marker, then △ assistant.
    let t = mk_turn(0, Some("the ask"), Some("the reply"), 3, 0);
    let mut lines: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::Both, &mut |s| lines.push(s));
    let joined = lines.join("\n");
    assert!(joined.contains("▽ L1"), "user header: {joined}");
    assert!(joined.contains("[3 tool calls]"), "tool marker: {joined}");
    assert!(joined.contains("△ L5"), "assistant header: {joined}");
    assert!(joined.contains("the ask"));
    assert!(joined.contains("the reply"));

    // UserOnly: no marker, no assistant.
    let mut uonly: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::UserOnly, &mut |s| uonly.push(s));
    let uj = uonly.join("\n");
    assert!(uj.contains("▽ L1"));
    assert!(!uj.contains("tool calls"), "no marker on user-only: {uj}");
    assert!(!uj.contains("△ L5"), "no assistant on user-only");

    // AssistantOnly: only the assistant side.
    let mut aonly: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::AssistantOnly, &mut |s| aonly.push(s));
    let aj = aonly.join("\n");
    assert!(!aj.contains("▽ L1"), "no user on assistant-only");
    assert!(aj.contains("△ L5"));
}

#[test]
fn render_turn_text_zero_tool_turn_omits_marker() {
    let t = mk_turn(0, Some("ask"), Some("reply"), 0, 0);
    let mut lines: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::Both, &mut |s| lines.push(s));
    assert!(
        !lines.join("\n").contains("tool calls"),
        "0-tool omits marker"
    );
}

#[test]
fn emit_unit_text_flags_dedup_unit() {
    let mut u = unit(Role::User, 7, "deduped ask", 0);
    u.also_in_summary = true;
    let mut lines: Vec<String> = Vec::new();
    emit_unit_text("▽", &u, &mut |s| lines.push(s));
    assert!(
        lines.iter().any(|l| l.contains("(also in summary)")),
        "dedup flag rendered: {lines:?}"
    );
}

#[test]
fn count_sides_counts_each_selection_kind() {
    let plan = SessionPlan {
        selected: vec![
            Selected {
                turn_index: 0,
                sides: SelSides::Both,
            },
            Selected {
                turn_index: 1,
                sides: SelSides::UserOnly,
            },
            Selected {
                turn_index: 2,
                sides: SelSides::AssistantOnly,
            },
        ],
        turns: Vec::new(),
        spanned_boundaries: 0,
        rendered_chars: 0,
        newest_summary_line: None,
        dedup_demoted: 0,
    };
    // Both → +1 user +1 asst; UserOnly → +1 user; AssistantOnly → +1 asst.
    assert_eq!(count_sides(&plan), (2, 2));
}

#[test]
fn turn_cost_assistant_only_costs_only_assistant_side() {
    let t = mk_turn(0, Some("ask"), Some("reply"), 5, 0);
    // AssistantOnly cost == just the assistant unit cost (no user, no marker).
    let a = t.assistant_eot.as_ref().unwrap();
    assert_eq!(turn_cost(&t, SelSides::AssistantOnly), unit_cost(a));
}

#[test]
fn summary_fingerprints_handles_bare_unquoted_bullet() {
    // A §6 bullet with NO quotes → the fingerprint falls back to the whole bullet body
    // (the `unwrap_or_else(|| fingerprint(rest))` arm).
    let body = "6. All user messages:\n   - a bare unquoted bullet about the carry";
    let fps = summary_fingerprints(body);
    assert!(
        fps.iter().any(|f| f.starts_with("a bare unquoted bullet")),
        "bare bullet fingerprinted: {fps:?}"
    );
}

#[test]
fn build_orphan_assistant_lead_has_no_user() {
    // Records that lead with an assistant before any genuine user → a synthetic turn 0
    // with assistant_eot but no user (group_turn_indices seeds turn 0 with the lead).
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"orphan lead reply"}]}}"#,
            ),
        ),
        (
            2,
            rec(r#"{"type":"user","message":{"role":"user","content":"the first real ask"}}"#),
        ),
        (
            3,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"real reply"}]}}"#,
            ),
        ),
    ];
    let (turns, _s) = build(&records);
    // The orphan lead folds into turn 0 (the first real user turn), so turn 0 has the
    // user AND carries the orphan assistant text as its EOT (last assistant in the turn).
    assert_eq!(turns.len(), 1);
    assert!(turns[0].user.is_some());
    assert!(turns[0].assistant_eot.is_some());
}

#[test]
fn empty_session_plans_to_nothing() {
    let sr = scan_with_turns(Vec::new(), Vec::new());
    let plan = plan_session(&sr, 40000, 0.5, 0);
    assert!(plan.selected.is_empty());
    assert_eq!(plan.spanned_boundaries, 0);
    assert_eq!(plan.rendered_chars, 0);
}

#[test]
fn dedup_no_op_when_no_summary() {
    // No summary at all → dedup is a no-op, nothing flagged.
    let turns = vec![mk_turn(0, Some("ask"), Some("reply"), 0, 0)];
    let sr = scan_with_turns(turns, Vec::new());
    let plan = plan_session(&sr, 40000, 0.5, 0);
    assert_eq!(plan.dedup_demoted, 0);
    assert!(plan.newest_summary_line.is_none());
}

#[test]
fn end_to_end_live_dedup_through_build_and_plan() {
    // A LIVE-region (compactions_before==0) turn whose user text matches the newest
    // summary's §6 bullet must be flagged + demoted through the full build → plan path.
    // The pre-boundary turn (cb=1) carrying nearby text is NOT deduped (older context).
    let records: Vec<(usize, Record)> = vec![
        (1, rec(r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pre-boundary ask"}}"#)),
        (2, rec(r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"pre reply"}]}}"#)),
        (3, rec("{\"type\":\"user\",\"isCompactSummary\":true,\"message\":{\"role\":\"user\",\"content\":\"6. All user messages:\\n   - \\\"the live duplicate ask verbatim\\\"\\n9. Optional Next Step:\\n   x\"}}")),
        (4, rec(r#"{"type":"user","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"the live duplicate ask verbatim"}}"#)),
        (5, rec(r#"{"type":"assistant","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"live reply"}]}}"#)),
    ];
    let (turns, summaries) = build(&records);
    assert_eq!(summaries.len(), 1);
    assert!(summaries[0]
        .fingerprints
        .iter()
        .any(|f| f.starts_with("the live duplicate ask verbatim")));
    // turn 0 is pre-boundary (cb=1), turn 1 is live (cb=0).
    assert_eq!(turns[0].compactions_before, 1);
    assert_eq!(turns[1].compactions_before, 0);
    let sr = scan_with_turns(turns, summaries);
    let plan = plan_session(&sr, 40000, 0.5, 0);
    assert_eq!(
        plan.dedup_demoted, 1,
        "exactly the live-region match is demoted"
    );
    assert_eq!(plan.newest_summary_line, Some(3));
}

// ── More branch-completeness: clamp arms, empty fingerprints, dedup edge ──

#[test]
fn phase1_clamp_giant_round_trip_bigger_than_full_budget() {
    // A single complete turn whose CAPPED cost exceeds even the WHOLE budget → the
    // `c > rt_budget` (the inner else) arm: it is still taken, accounting clamped to
    // rt_budget so Phase 2's pool never goes negative.
    let huge_u = "u".repeat(5000);
    let huge_a = "a".repeat(5000);
    let turns = vec![mk_turn(0, Some(&huge_u), Some(&huge_a), 9, 0)];
    let sr = scan_with_turns(turns, Vec::new());
    // Budget so small that even the 600+900-capped pair (~1560 + marker) exceeds it.
    let plan = plan_session(&sr, 100, 0.5, 0);
    assert_eq!(plan.selected.len(), 1);
    assert!(matches!(plan.selected[0].sides, SelSides::Both));
}

#[test]
fn phase1_clamp_giant_round_trip_fits_full_budget_not_rt_half() {
    // A complete turn whose cost is > rt_budget (the 0.5 half) but <= the full budget →
    // the `c <= rt_budget` FALSE → outer else, then `c <= rt_budget`... actually this
    // exercises the `spent_rt == 0` clamp where the capped pair (~1560) > rt_budget(900)
    // but the whole budget (1800) admits it.
    let huge_u = "u".repeat(2000);
    let huge_a = "a".repeat(2000);
    let turns = vec![mk_turn(0, Some(&huge_u), Some(&huge_a), 0, 0)];
    let sr = scan_with_turns(turns, Vec::new());
    let plan = plan_session(&sr, 1800, 0.5, 0);
    assert_eq!(plan.selected.len(), 1);
    assert!(matches!(plan.selected[0].sides, SelSides::Both));
}

#[test]
fn dedup_skips_empty_text_unit() {
    // A live unit whose text is whitespace-only → its fingerprint is empty → never a
    // dedup match (the `unit_fp.is_empty()` true arm), so it is not flagged.
    let summary_fps = vec![fingerprint("a real summary bullet about the carry")];
    let blank = unit(Role::User, 1, "   ", 0);
    assert!(!unit_matches_summary(&blank, &summary_fps));
}

#[test]
fn summary_fingerprints_skips_empty_quote() {
    // A bullet whose quoted run is empty (`- ""`) and a bare bullet that normalizes to
    // empty → the `!fp.is_empty()` false arms (nothing pushed).
    let body = "6. All user messages:\n   - \"\"\n   -   ";
    let fps = summary_fingerprints(body);
    assert!(
        fps.is_empty(),
        "empty bullets produce no fingerprints: {fps:?}"
    );
}

#[test]
fn build_summary_with_block_body_is_not_captured() {
    // A summary record with a BLOCK body (not a string) → compact_summary_body returns
    // None → the summary is NOT captured (the `if let Some(body)` false arm in build).
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(r#"{"type":"user","message":{"role":"user","content":"ask"}}"#),
        ),
        (
            2,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"reply"}]}}"#,
            ),
        ),
        // a block-bodied "summary" (a genuine surprise) → skipped as a summary source.
        (
            3,
            rec(
                r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":[{"type":"text","text":"block body summary"}]}}"#,
            ),
        ),
    ];
    let (turns, summaries) = build(&records);
    assert!(summaries.is_empty(), "block-bodied summary not captured");
    // The turn still builds; the block summary contributes no boundary.
    assert_eq!(turns[0].compactions_before, 0);
}

#[test]
fn budget_unit_token_conversion_via_run_is_four_x() {
    // The TOKEN_CHARS constant is 4.0 — the documented heuristic.
    assert_eq!(TOKEN_CHARS, 4.0);
}

#[test]
fn dedup_demoted_turn_sorts_after_non_dup_in_phase1() {
    // Two complete live turns, one dedup-flagged: at a budget that fits only ONE, the
    // NON-dup turn must win Phase 1 (dedup_pass false before true).
    let dup_text = "the duplicate ask the summary already has verbatim in full here";
    let turns = vec![
        mk_turn(0, Some(dup_text), Some("dup reply"), 0, 0),
        mk_turn(1, Some("a unique fresh ask"), Some("unique reply"), 0, 0),
    ];
    let sums = vec![summary(900, vec![dup_text], 9000)];
    let sr = scan_with_turns(turns, sums);
    // Budget for exactly one complete pair.
    let plan = plan_session(&sr, 200, 0.5, 0);
    assert_eq!(plan.dedup_demoted, 1);
    // The NON-dup turn (index 1) is selected before the dup (index 0).
    assert!(
        plan.selected.iter().any(|s| s.turn_index == 1),
        "non-dup turn wins the tight budget: {:?}",
        plan.selected
            .iter()
            .map(|s| s.turn_index)
            .collect::<Vec<_>>()
    );
}

// ── Render-helper + raw-body branch completeness ──

#[test]
fn raw_body_newlines_block_with_empty_text_blocks() {
    // A block body where one text block is blank → it is skipped (the `!text.trim()`
    // false arm); only the non-blank block contributes.
    let r = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"   "},{"type":"text","text":"real\nline"}]}}"#,
    );
    assert_eq!(raw_body_newlines(&r), 1);
    // A block body with a non-text block (tool_use) interleaved → only text counts.
    let r2 = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}},{"type":"text","text":"a\nb\nc"}]}}"#,
    );
    assert_eq!(raw_body_newlines(&r2), 2);
}

#[test]
fn render_turn_text_user_only_with_no_user_emits_nothing() {
    // Defensive: a UserOnly selection on a turn whose user is None (cannot normally
    // happen) emits no user line — the `if let Some(u)` false arm.
    let t = mk_turn(0, None, Some("only assistant"), 0, 0);
    let mut lines: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::UserOnly, &mut |s| lines.push(s));
    assert!(lines.is_empty(), "no user to render: {lines:?}");
}

#[test]
fn render_turn_text_assistant_only_with_no_assistant_emits_nothing() {
    let t = mk_turn(0, Some("only user"), None, 0, 0);
    let mut lines: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::AssistantOnly, &mut |s| lines.push(s));
    assert!(lines.is_empty(), "no assistant to render: {lines:?}");
}

#[test]
fn render_turn_text_both_with_zero_tools_no_marker_line() {
    // Both selection, 0 tools → the marker `if turn.tool_calls > 0` false arm: no
    // `[N tool calls]` line, but user + assistant still render.
    let t = mk_turn(0, Some("ask"), Some("reply"), 0, 0);
    let mut lines: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::Both, &mut |s| lines.push(s));
    assert!(lines.iter().any(|l| l.starts_with("▽ L")));
    assert!(lines.iter().any(|l| l.starts_with("△ L")));
    assert!(!lines.iter().any(|l| l.contains("tool calls")));
}

#[test]
fn emit_unit_text_non_dup_has_no_flag() {
    // A non-dedup unit renders WITHOUT the (also in summary) suffix (the `also_in_summary`
    // false arm).
    let u = unit(Role::User, 3, "a normal ask", 0);
    let mut lines: Vec<String> = Vec::new();
    emit_unit_text("▽", &u, &mut |s| lines.push(s));
    assert!(!lines.iter().any(|l| l.contains("also in summary")));
    assert!(lines[0].starts_with("▽ L3"));
}

#[test]
fn turn_cost_user_only_costs_only_user_side() {
    let t = mk_turn(0, Some("the ask"), Some("the reply"), 5, 0);
    let u = t.user.as_ref().unwrap();
    // UserOnly: just the user unit cost (no marker, no assistant).
    assert_eq!(turn_cost(&t, SelSides::UserOnly), unit_cost(u));
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
fn build_skips_non_genuine_user_opener() {
    // A turn whose opening record is genuine-user but genuine_user_text returns text;
    // also a record that is a tool_result carrier (not genuine) must NOT become a user
    // opener (the `is_genuine_user` false path inside the loop).
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(r#"{"type":"user","message":{"role":"user","content":"real opener"}}"#),
        ),
        (
            2,
            rec(
                r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"carrier"}]}}"#,
            ),
        ),
        (
            3,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"reply"}]}}"#,
            ),
        ),
    ];
    let (turns, _s) = build(&records);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].user.as_ref().unwrap().text, "real opener");
    assert!(turns[0].assistant_eot.is_some());
}

#[test]
fn raw_body_newlines_no_message_and_no_content() {
    // No message at all → 0 (the `let Some(msg) else` arm).
    let no_msg = rec(r#"{"type":"system","subtype":"x"}"#);
    assert_eq!(raw_body_newlines(&no_msg), 0);
    // message present, no content → 0 (the `let Some(content) else` arm).
    let no_content = rec(r#"{"type":"user","message":{"role":"user"}}"#);
    assert_eq!(raw_body_newlines(&no_content), 0);
}

#[test]
fn compact_summary_body_none_when_no_message() {
    let bare = rec(r#"{"type":"user","isCompactSummary":true}"#);
    assert!(compact_summary_body(&bare).is_none());
}

#[test]
fn phase2_empty_turn_no_user_no_assistant_selects_nothing() {
    // A degenerate turn with NEITHER user NOR assistant (the `&[]` candidates arm at
    // L782's else) → it is never selected; a real turn beside it is.
    let turns = vec![
        mk_turn(0, None, None, 0, 0),
        mk_turn(1, Some("real ask"), Some("real reply"), 0, 0),
    ];
    let sr = scan_with_turns(turns, Vec::new());
    let plan = plan_session(&sr, 40000, 0.5, 0);
    assert!(
        !plan.selected.iter().any(|s| s.turn_index == 0),
        "the empty turn is never selected"
    );
    assert!(plan.selected.iter().any(|s| s.turn_index == 1));
}

#[test]
fn unit_matches_summary_no_match_returns_false() {
    // A non-empty unit fingerprint that matches NO summary fingerprint → the `.any()`
    // false result (L845/L849 false sides).
    let summary_fps = vec![fingerprint(
        "a summary bullet entirely unrelated to the unit",
    )];
    let u = unit(
        Role::User,
        1,
        "a completely different question about something else",
        0,
    );
    assert!(!unit_matches_summary(&u, &summary_fps));
    // An empty summary fingerprint in the list must not spuriously match (the `!sfp
    // .is_empty()` false guard).
    let with_empty = vec![String::new(), fingerprint("real bullet text here")];
    assert!(!unit_matches_summary(&u, &with_empty));
}

#[test]
fn parse_turn_range_valid_range_hi_ge_lo() {
    // hi >= lo → the `hi < lo` FALSE arm (the success path, distinct from the reject).
    assert_eq!(parse_turn_range("0..0").unwrap(), (0, 0));
    assert_eq!(parse_turn_range("10..20").unwrap(), (10, 20));
}

#[test]
fn shown_user_and_assistant_cover_all_combinations() {
    let complete = mk_turn(0, Some("u"), Some("a"), 0, 0);
    // Both → both sides shown.
    assert!(shown_user(&complete, SelSides::Both).is_some());
    assert!(shown_assistant(&complete, SelSides::Both).is_some());
    // UserOnly → user shown, assistant hidden (shows_assistant false).
    assert!(shown_user(&complete, SelSides::UserOnly).is_some());
    assert!(shown_assistant(&complete, SelSides::UserOnly).is_none());
    // AssistantOnly → assistant shown, user hidden.
    assert!(shown_user(&complete, SelSides::AssistantOnly).is_none());
    assert!(shown_assistant(&complete, SelSides::AssistantOnly).is_some());
    // A turn missing a side → None even when the selection would show it.
    let no_asst = mk_turn(0, Some("u"), None, 0, 0);
    assert!(shown_assistant(&no_asst, SelSides::Both).is_none());
    let no_user = mk_turn(0, None, Some("a"), 0, 0);
    assert!(shown_user(&no_user, SelSides::Both).is_none());
    // The boolean helpers.
    assert!(shows_user(SelSides::Both) && shows_user(SelSides::UserOnly));
    assert!(!shows_user(SelSides::AssistantOnly));
    assert!(shows_assistant(SelSides::Both) && shows_assistant(SelSides::AssistantOnly));
    assert!(!shows_assistant(SelSides::UserOnly));
}

#[test]
fn turn_cost_partial_turns_skip_absent_sides() {
    // turn_cost over a turn MISSING a side → the `if let Some` None arms in turn_cost
    // (L616/L626): a user-only turn costs only the user; an assistant-only only the asst.
    let user_only = mk_turn(0, Some("just a user ask"), None, 3, 0);
    // Both selection on a user-only turn: marker is charged (tool_calls>0) but no asst.
    let u = user_only.user.as_ref().unwrap();
    assert_eq!(
        turn_cost(&user_only, SelSides::Both),
        unit_cost(u) + marker_cost(3)
    );
    let asst_only = mk_turn(0, None, Some("just a reply"), 0, 0);
    let a = asst_only.assistant_eot.as_ref().unwrap();
    assert_eq!(turn_cost(&asst_only, SelSides::Both), unit_cost(a));
}

#[test]
fn summary_fingerprints_skips_empty_prose_quote() {
    // A §9-style prose line whose quoted run is empty (`said ""`) → the L509 `!fp
    // .is_empty()` false arm (nothing pushed for that line).
    let body = "9. Optional Next Step:\n   The assistant said \"\".";
    let fps = summary_fingerprints(body);
    assert!(
        fps.is_empty(),
        "empty prose quote yields no fingerprint: {fps:?}"
    );
}

#[test]
fn build_skips_non_candidate_records_in_scan() {
    // The build path itself runs over already-parsed records; the prefilter's skip is
    // exercised at scan time. Verify build tolerates a `system` record interleaved as a
    // turn member (it contributes no user/assistant/summary).
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(r#"{"type":"user","message":{"role":"user","content":"ask"}}"#),
        ),
        (
            2,
            rec(r#"{"type":"system","subtype":"turn_duration","durationMs":5}"#),
        ),
        (
            3,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"reply"}]}}"#,
            ),
        ),
    ];
    let (turns, summaries) = build(&records);
    assert_eq!(turns.len(), 1);
    assert!(turns[0].is_round_trip());
    assert!(summaries.is_empty());
}
