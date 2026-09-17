//! What the DISCLOSURE says about a walk that met a compaction boundary: the two footer
//! lines, the dense `stats` segment, the six JSON keys, and the scope fold.

use super::*;

#[test]
fn budget_zero_says_nothing_about_crossing_and_every_other_budget_does() {
    // The silence rule: under `--cross-compactions 0` csift crossed nothing on purpose, so
    // the footer and the JSON keys are both absent and that run is byte-identical to the
    // reading csift gave before the budget existed.
    let r = five_boundaries();
    let off = Chain::build(&r, None, CrossingBudget::NONE).crossing(Some(22), None);
    assert!(!off.any(), "no footer line");
    assert!(!off.projects(), "no JSON keys");
    assert!(off.lines().is_empty());
    assert!(off.json_fields().is_empty());
    assert!(off.compact().is_none());

    let on = Chain::build(&r, None, CrossingBudget::Limited(3))
        .crossing(Some(7), Some("2026-06-07T06:02:00.000Z".to_string()));
    assert!(on.any());
    assert!(on.projects());
    let lines = on.lines();
    assert_eq!(lines.len(), 2, "the counts, then the floor and the remedy");
    assert!(lines[0].contains("3 boundary(s) stepped over"), "{lines:?}");
    assert!(
        lines[0].contains("--cross-compactions 3, exhausted"),
        "{lines:?}"
    );
    assert!(lines[0].contains("2 above the chain floor"), "{lines:?}");
    assert!(lines[1].contains("the chain floor is L7 = "), "{lines:?}");
    assert!(
        lines[1].contains("--cross-compactions 5 (or all)"),
        "the remedy is crossed + uncrossed: {lines:?}"
    );
    let keys: Vec<&str> = on.json_fields().iter().map(|(k, _)| *k).collect();
    assert_eq!(
        keys,
        vec![
            "crossing_budget",
            "boundaries_crossed",
            "budget_exhausted",
            "boundaries_above_floor",
            "floor_line",
            "floor_cause",
        ]
    );
    assert_eq!(
        on.compact().as_deref(),
        Some("compaction boundaries crossed 3/5 (budget 3, exhausted)")
    );
}

#[test]
fn the_remedy_is_named_off_the_floors_cause_and_never_promises_a_budget_that_cannot_move_it() {
    // Only a floor the BUDGET made can be raised. A boundary naming a record no line of the
    // file carries is a floor at every budget, and a walk that ended at a head never reached
    // the boundaries above it - promising `--cross-compactions N` in either case sends a
    // reader to re-run a scan that cannot move.
    let r = five_boundaries();
    let capped = Chain::build(&r, None, CrossingBudget::Limited(3)).crossing(Some(7), None);
    assert!(
        capped.lines()[1].contains("to try them: --cross-compactions 5 (or all)"),
        "{:?}",
        capped.lines()
    );

    let dangling = recs(&[
        &u("u0", "", "old prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "old reply", "2026-06-07T05:00:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"deleted-from-disk","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto"}}"#,
        &u("u1", "b1", "new prompt", "2026-06-07T05:11:00.000Z"),
        &a("a1", "u1", "new reply", "2026-06-07T05:11:05.000Z"),
    ]);
    let stuck = Chain::build(&dangling, None, CrossingBudget::Unbounded).crossing(Some(3), None);
    let lines = stuck.lines();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[1].contains("raising the budget reaches none of them"),
        "{lines:?}"
    );
    assert!(
        !lines[1].contains("--cross-compactions"),
        "no flag value is offered where none would help: {lines:?}"
    );
}

#[test]
fn a_budget_that_stopped_at_a_shut_door_offers_no_value_because_a_bigger_one_lands_here_too() {
    // The cause alone is not enough to promise anything. The walk can run out of budget AT a
    // boundary whose own logicalParentUuid names a record no line of the file carries: the
    // cause is `budget-exhausted`, which is honest about what stopped the walk, and a bigger
    // budget still lands on this same record. Named off the cause alone, the line sent a
    // reader to re-run the scan at a value that could not move the floor.
    let r = two_crossed_then_a_dangling_floor();

    // Budget 2 steps over b3 and b2 and then runs out AT b1, whose door is shut.
    let shut = Chain::build(&r, None, CrossingBudget::Limited(2));
    assert_eq!(shut.floor_cause, FloorCause::BudgetExhausted);
    assert_eq!(shut.floor, 2, "b1 is the floor");
    assert!(!shut.floor_crossable, "b1 names a record that is gone");
    let lines = shut.crossing(Some(3), None).lines();
    assert!(
        !lines[1].contains("--cross-compactions"),
        "no value is named where a bigger budget lands here too: {lines:?}"
    );
    assert!(
        lines[1].contains("raising the budget lands on this same record"),
        "{lines:?}"
    );
    // And the promise it refused to make would have been false: one more unit of budget
    // walks to the very same record, only calling the stop by its other name.
    let more = Chain::build(&r, None, CrossingBudget::Limited(3));
    assert_eq!(more.floor, shut.floor, "the same record");
    assert_eq!(more.floor_cause, FloorCause::UnresolvableBoundary);

    // One unit less and the stop is at b2, whose logicalParentUuid IS on a line here - so
    // the value is named, and it does move the floor.
    let open = Chain::build(&r, None, CrossingBudget::Limited(1));
    assert_eq!(open.floor_cause, FloorCause::BudgetExhausted);
    assert!(open.floor_crossable, "b2 names a1, which is on a line here");
    let lines = open.crossing(Some(7), None).lines();
    assert!(
        lines[1].contains("to try them: --cross-compactions 3 (or all)"),
        "{lines:?}"
    );
    assert_eq!(
        Chain::build(&r, None, CrossingBudget::Limited(2)).floor,
        2,
        "and the named value does move it"
    );
}

#[test]
fn a_report_with_nothing_above_the_floor_prints_the_counts_alone() {
    // The second line is the FLOOR's address and the remedy; with nothing left uncrossed
    // there is no remedy to name, so it is not printed at all.
    let r = rewound_above_a_boundary();
    let rep = Chain::build(&r, None, CrossingBudget::Limited(1)).crossing(Some(1), None);
    let lines = rep.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("1 boundary(s) stepped over"), "{lines:?}");
    assert!(!lines[0].contains("exhausted"), "{lines:?}");
}

#[test]
fn folding_a_second_transcript_drops_the_floor_and_keeps_the_counts() {
    // A scope has as many floors as it has files, so the floor's LINE and its CAUSE are
    // single-transcript facts and go the moment a second transcript contributes - the rule
    // `boundary_cut_line` already follows. The COUNTS add, and the remedy the footer names
    // is then a budget that reaches every transcript in scope rather than a minimal one.
    let r = five_boundaries();
    let one = Chain::build(&r, None, CrossingBudget::Limited(3))
        .crossing(Some(7), Some("2026-06-07T06:02:00.000Z".to_string()));
    // The accumulator carries the RUN's budget - a fold never reads one off a member, so a
    // transcript with nothing to say cannot turn the scope's disclosure off.
    let mut total = CrossingReport::new(CrossingBudget::Limited(3));
    total.add(&one, true);
    assert_eq!(total.floor_line, Some(7));
    assert_eq!(total.cause, Some(FloorCause::BudgetExhausted));
    total.add(&one, false);
    assert_eq!(total.boundaries_crossed, 6, "the counts add");
    assert_eq!(total.boundaries_above_floor, 4);
    assert!(total.budget_exhausted);
    assert_eq!(total.floor_line, None, "no line names two files");
    assert_eq!(total.cause, None, "and neither does one reason");
    let lines = total.lines();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[1].starts_with("(the chain floor; records above it read pre-cut"),
        "with no address and no cause the line names only the remedy: {lines:?}"
    );
    assert!(
        lines[1].contains("--cross-compactions 10 (or all)"),
        "{lines:?}"
    );
}

#[test]
fn the_budget_grammar_round_trips_and_names_both_forms_on_a_bad_token() {
    assert_eq!(
        CrossingBudget::parse("0"),
        Ok(CrossingBudget::Limited(0)),
        "0 is the loader's own reading, not an error"
    );
    assert_eq!(CrossingBudget::parse("7"), Ok(CrossingBudget::Limited(7)));
    assert_eq!(CrossingBudget::parse("all"), Ok(CrossingBudget::Unbounded));
    assert_eq!(CrossingBudget::parse("ALL"), Ok(CrossingBudget::Unbounded));
    let err = CrossingBudget::parse("-1").expect_err("a negative count is not a budget");
    assert!(err.contains("or `all`"), "{err}");
    assert!(err.contains("-1"), "the rejected token is named: {err}");
    assert_eq!(CrossingBudget::Limited(3).as_text(), "3");
    assert_eq!(CrossingBudget::Unbounded.as_text(), "all");
    assert_eq!(CrossingBudget::Unbounded.json(), serde_json::json!("all"));
    assert_eq!(CrossingBudget::Limited(3).json(), serde_json::json!(3));
    assert!(CrossingBudget::NONE.is_off());
    assert!(!CrossingBudget::Unbounded.is_off());
    assert!(CrossingBudget::Unbounded.allows(9_999));
    assert!(!CrossingBudget::Limited(2).allows(2));
    assert!(CrossingBudget::Limited(2).allows(1));
}

#[test]
fn the_floor_cause_spellings_are_the_wire_and_the_prose() {
    for (c, wire) in [
        (FloorCause::Head, "head"),
        (FloorCause::NoLeaf, "no-leaf"),
        (FloorCause::BudgetExhausted, "budget-exhausted"),
        (FloorCause::UnresolvableBoundary, "unresolvable-boundary"),
        (FloorCause::DanglingParent, "dangling-parent"),
        (FloorCause::Cycle, "cycle"),
    ] {
        assert_eq!(c.as_str(), wire);
        assert!(!c.reason().is_empty(), "{wire} has a printed reason");
    }
    assert_eq!(FloorCause::default(), FloorCause::Head);
}

#[test]
fn every_floor_cause_prints_its_own_words() {
    // The cause is the whole basis of the remedy, so each one's phrase is pinned by value
    // rather than by "is not empty": a reader who is told the wrong reason draws the wrong
    // conclusion about whether the flag would help.
    assert_eq!(FloorCause::Head.reason(), "the conversation head");
    assert_eq!(
        FloorCause::NoLeaf.reason(),
        "no leaf resolved, so nothing is on the chain"
    );
    assert_eq!(
        FloorCause::BudgetExhausted.reason(),
        "a compaction boundary the budget would not cross"
    );
    assert_eq!(
        FloorCause::UnresolvableBoundary.reason(),
        "a compaction boundary whose logicalParentUuid names no record this file carries"
    );
    assert_eq!(
        FloorCause::DanglingParent.reason(),
        "a parentUuid naming no record in this file"
    );
    assert_eq!(FloorCause::Cycle.reason(), "a cycle in the parent links");
}

#[test]
fn the_report_speaks_for_each_of_its_three_reasons_alone() {
    // `CrossingReport::any` is an OR over three independent facts and each is reason enough:
    // read as an AND, a walk that crossed a boundary would disclose nothing unless it had
    // ALSO exhausted its budget and ALSO stopped at a dangling one.
    let on = CrossingBudget::Limited(3);
    let crossed = CrossingReport {
        budget: on,
        boundaries_crossed: 1,
        ..CrossingReport::default()
    };
    assert!(crossed.any(), "a boundary stepped over");
    let exhausted = CrossingReport {
        budget: on,
        budget_exhausted: true,
        ..CrossingReport::default()
    };
    assert!(exhausted.any(), "a budget that ran out");
    let stuck = CrossingReport {
        budget: on,
        cause: Some(FloorCause::UnresolvableBoundary),
        ..CrossingReport::default()
    };
    assert!(stuck.any(), "a floor no budget can move");
    let quiet = CrossingReport {
        budget: on,
        cause: Some(FloorCause::Head),
        ..CrossingReport::default()
    };
    assert!(!quiet.any(), "a walk that met no boundary says nothing");
}

#[test]
fn a_floor_with_no_timestamp_still_prints_its_line() {
    // The floor's INSTANT is the surface's to supply and a spine row can carry none, so the
    // address degrades to the line alone rather than to nothing: a reader who cannot be told
    // when can still be told where.
    let rep = CrossingReport {
        budget: CrossingBudget::Limited(1),
        boundaries_crossed: 1,
        budget_exhausted: true,
        boundaries_above_floor: 1,
        cause: Some(FloorCause::BudgetExhausted),
        floor_crossable: true,
        floor_line: Some(88),
        floor_ts: None,
    };
    let lines = rep.lines();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[1].starts_with("(the chain floor is L88 ("),
        "{lines:?}"
    );
    assert!(
        lines[1].contains("--cross-compactions 2 (or all)"),
        "{lines:?}"
    );
    // ...and with neither, the line names the cause and the remedy and no address at all.
    let bare = CrossingReport {
        floor_line: None,
        ..rep
    };
    assert!(
        bare.lines()[1].starts_with("(the chain floor ("),
        "{:?}",
        bare.lines()
    );
}
