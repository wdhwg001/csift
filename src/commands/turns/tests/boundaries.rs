//! Compaction-boundary crossing counts and banner emission.

use super::*;

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
