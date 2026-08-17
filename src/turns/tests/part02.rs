use super::*;

#[test]
fn is_human_round_trip_excludes_automation_notifications() {
    // A HUMAN round-trip qualifies for the HARD FLOOR; an automation-pulse round-trip is a
    // structural round-trip (is_round_trip true) but NOT a human one — so it is excluded
    // from the protected `--round-trip-fraction` lane (the budget-floor consumer).
    let human = mk_turn(0, Some("a"), Some("b"), 0, 0);
    assert!(human.is_round_trip() && human.is_human_round_trip());
    let mut pulse = mk_turn(
        1,
        Some("<task-notification>…</task-notification>"),
        Some("ack"),
        0,
        0,
    );
    pulse.is_automation = true;
    assert!(
        pulse.is_round_trip(),
        "an automation pulse + ack is STRUCTURALLY a round-trip"
    );
    assert!(
        !pulse.is_human_round_trip(),
        "but it is NOT a human round-trip → excluded from the floor"
    );
}

// ── Budget allocation (the load-bearing 50% floor) ──

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
    let plan = plan_session(&sr, 8000, 0.5, 0, &cfg());
    let (n_user, _n_asst) = count_sides(&plan, &cfg());
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
    let big = plan_session(&sr, 40000, 0.5, 0, &cfg());
    let small = plan_session(&sr, 4000, 0.5, 0, &cfg());
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
    let low = plan_session(&sr, 6000, 0.3, 0, &cfg());
    let high = plan_session(&sr, 6000, 0.8, 0, &cfg());
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
    let p1 = plan_session(&sr, 5000, 0.5, 0, &cfg());
    let p2 = plan_session(&sr, 5000, 0.5, 0, &cfg());
    let idx = |p: &SessionPlan| p.selected.iter().map(|s| s.turn_index).collect::<Vec<_>>();
    assert_eq!(idx(&p1), idx(&p2));
    assert_eq!(p1.rendered_chars, p2.rendered_chars);
}

// ── Multi-compaction spanning + dedup demotion ──

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
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
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
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
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
    let capped = plan_session(&sr, 40000, 0.5, 1, &cfg());
    // Only turns with compactions_before <= 1 survive.
    for s in &capped.selected {
        assert!(s.turn_index >= 2, "turn {} beyond cap leaked", s.turn_index);
    }
    assert!(capped.spanned_boundaries <= 1);
}

// ── compact_summary_body / raw_body_newlines via real Record parse ──

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
    let (turns, summaries) = build(&records, &[]);
    assert_eq!(turns.len(), 2);
    // turn 0: round-trip, 2 tool calls, before the (one) summary → compactions_before 1.
    assert!(turns[0].is_round_trip());
    assert_eq!(turns[0].tool_calls, 2);
    assert_eq!(turns[0].compactions_before, 1);
    assert_eq!(turns[0].user.as_ref().unwrap().line_no, 1);
    assert_eq!(turns[0].assistant_eot().unwrap().line_no, 3);
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
    let (turns, _s) = build(&records, &[]);
    assert_eq!(turns.len(), 1);
    assert!(turns[0].user.is_some());
    assert!(
        turns[0].assistant_eot().is_none(),
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
