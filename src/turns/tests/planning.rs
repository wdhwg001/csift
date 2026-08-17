//! Budget planning: round-trip floors, phases, dedup ordering, determinism.

use super::*;

#[test]
fn is_round_trip_requires_both_sides() {
    assert!(mk_turn(0, Some("a"), Some("b"), 0, 0).is_round_trip());
    assert!(!mk_turn(0, Some("a"), None, 0, 0).is_round_trip());
    assert!(!mk_turn(0, None, Some("b"), 0, 0).is_round_trip());
}

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
    let plan = plan_session(&sr, 2000, 0.5, 0, &cfg());
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
    let plan = plan_session(&sr, 700, 0.5, 0, &cfg());
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
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
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
    // the `else if t.assistant_eot().is_some()` arm.
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
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    assert!(
        plan.selected
            .iter()
            .any(|s| s.turn_index == 0 && matches!(s.sides, SelSides::AssistantOnly)),
        "orphan assistant-only turn taken assistant-only"
    );
}

#[test]
fn count_sides_counts_each_selection_kind() {
    // Each selected turn must exist in `plan.turns` (the count walks the real agent lane —
    // a single-agent turn contributes one assistant unit). Both → +1 user +1 asst;
    // UserOnly → +1 user; AssistantOnly → +1 asst.
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
        turns: vec![
            mk_turn(0, Some("u0"), Some("a0"), 0, 0),
            mk_turn(1, Some("u1"), Some("a1"), 0, 0),
            mk_turn(2, Some("u2"), Some("a2"), 0, 0),
        ],
        spanned_boundaries: 0,
        rendered_chars: 0,
        newest_summary_line: None,
        dedup_demoted: 0,
    };
    assert_eq!(count_sides(&plan, &cfg()), (2, 2));
}

#[test]
fn empty_session_plans_to_nothing() {
    let sr = scan_with_turns(Vec::new(), Vec::new());
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    assert!(plan.selected.is_empty());
    assert_eq!(plan.spanned_boundaries, 0);
    assert_eq!(plan.rendered_chars, 0);
}

// ── More branch-completeness: clamp arms, empty fingerprints, dedup edge ──

#[test]
fn giant_round_trip_bigger_than_unit_budget_is_not_force_included_both() {
    // A single complete turn whose CAPPED cost exceeds even the whole UNIT budget must
    // NOT be force-included as a both-sides pair — doing so would overshoot the budget,
    // which is the exact bug this fix removes. Phase 2 instead takes the cheaper single
    // (user-first) side that DOES fit, preserving the ≤-budget guarantee.
    let huge_u = "u".repeat(5000);
    let huge_a = "a".repeat(5000);
    let pair = mk_turn(0, Some(&huge_u), Some(&huge_a), 9, 0);
    // Budget that fits the (capped ~600) user side alone but NOT the full ~1560 pair.
    let user_side = turn_cost(&pair, SelSides::UserOnly, &cfg());
    let both = turn_cost(&pair, SelSides::Both, &cfg());
    let sr = scan_with_turns(vec![pair], Vec::new());
    // doc-header reservation is tiny (no summaries); pick a budget between the user side
    // and the full pair, with headroom for the header block.
    let budget = user_side + 400;
    assert!(budget < both, "budget must be below the full-pair cost");
    let plan = plan_session(&sr, budget, 0.5, 0, &cfg());
    assert_eq!(
        plan.selected.len(),
        1,
        "the single turn is still represented"
    );
    assert!(
        matches!(plan.selected[0].sides, SelSides::UserOnly),
        "the cheaper user side fits where the full pair does not: {:?}",
        plan.selected[0].sides
    );
    // And the whole emitted document still respects budget by construction.
    assert!(
        plan.rendered_chars <= budget,
        "{} <= {budget}",
        plan.rendered_chars
    );
}

#[test]
fn giant_round_trip_fits_full_budget_not_rt_half_is_force_included() {
    // A complete turn whose cost is > the 50% round-trip reservation but <= the whole
    // (post-header-block) unit budget → the `spent_units == 0` force-include arm: it is
    // taken as a both-sides pair even though it blows past rt_budget, because the FULL
    // document still fits the budget. Sized off the real `turn_cost`, not a magic number.
    let huge_u = "u".repeat(2000);
    let huge_a = "a".repeat(2000);
    let pair = mk_turn(0, Some(&huge_u), Some(&huge_a), 0, 0);
    let both = turn_cost(&pair, SelSides::Both, &cfg());
    let sr = scan_with_turns(vec![pair], Vec::new());
    // Budget that, after the (tiny, no-summary) header-block reservation, admits the whole
    // pair — but whose 50% rt reservation does NOT (so the rt-half branch is skipped and
    // the force-include arm fires). A generous +1000 covers the header block reservation.
    let budget = both + 1000;
    assert!(
        (budget as f64 * 0.5) < both as f64,
        "rt_budget (~half) must be below the pair cost so the force-include arm is hit"
    );
    let plan = plan_session(&sr, budget, 0.5, 0, &cfg());
    assert_eq!(plan.selected.len(), 1);
    assert!(matches!(plan.selected[0].sides, SelSides::Both));
    assert!(
        plan.rendered_chars <= budget,
        "{} <= {budget}",
        plan.rendered_chars
    );
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
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    assert!(
        !plan.selected.iter().any(|s| s.turn_index == 0),
        "the empty turn is never selected"
    );
    assert!(plan.selected.iter().any(|s| s.turn_index == 1));
}

#[test]
fn shown_user_and_assistant_cover_all_combinations() {
    let complete = mk_turn(0, Some("u"), Some("a"), 0, 0);
    // The assistant LANE (shown_agent_lane) is non-empty exactly when the side is shown
    // AND the turn has agent messages; empty otherwise (replaces the old shown_assistant).
    let lane = |t: &TurnSlice, s: SelSides| !shown_agent_lane(t, s, &cfg()).is_empty();
    // Both → both sides shown.
    assert!(shown_user(&complete, SelSides::Both).is_some());
    assert!(lane(&complete, SelSides::Both));
    // UserOnly → user shown, assistant hidden (shows_assistant false).
    assert!(shown_user(&complete, SelSides::UserOnly).is_some());
    assert!(!lane(&complete, SelSides::UserOnly));
    // AssistantOnly → assistant shown, user hidden.
    assert!(shown_user(&complete, SelSides::AssistantOnly).is_none());
    assert!(lane(&complete, SelSides::AssistantOnly));
    // A turn missing a side → empty lane even when the selection would show it.
    let no_asst = mk_turn(0, Some("u"), None, 0, 0);
    assert!(!lane(&no_asst, SelSides::Both));
    let no_user = mk_turn(0, None, Some("a"), 0, 0);
    assert!(shown_user(&no_user, SelSides::Both).is_none());
    // The boolean helpers.
    assert!(shows_user(SelSides::Both) && shows_user(SelSides::UserOnly));
    assert!(!shows_user(SelSides::AssistantOnly));
    assert!(shows_assistant(SelSides::Both) && shows_assistant(SelSides::AssistantOnly));
    assert!(!shows_assistant(SelSides::UserOnly));
}
