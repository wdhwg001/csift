//! The CROSSING BUDGET: what the walk does at a `compact_boundary` under each budget, and
//! what it then says about the boundaries it did not step over.
//!
//! Every fixture here carries `compactMetadata` WITHOUT a preserved set, so the pre-walk
//! relink aborts and the only thing under test is the walk itself. A boundary carrying a
//! preserved set is the relink's own subject and is pinned in `chain_relink.rs`.

use super::*;

/// `u0 -> a0` under a boundary that names `a0` as its logical parent, with a second branch
/// (`u2 -> a2`) hanging off `u0` that the conversation was rewound past.
fn rewound_above_a_boundary() -> Vec<Record> {
    recs(&[
        &u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "charting", "2026-06-07T05:00:05.000Z"),
        &u("u2", "u0", "survey the shoal", "2026-06-07T05:00:10.000Z"),
        &a("a2", "u2", "surveying", "2026-06-07T05:00:15.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"a0","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preTokens":900}}"#,
        r#"{"type":"user","uuid":"s1","parentUuid":"b1","isCompactSummary":true,"timestamp":"2026-06-07T05:10:01.000Z","message":{"role":"user","content":"summary"}}"#,
        &u("u1", "s1", "dredge the channel", "2026-06-07T05:11:00.000Z"),
        &a("a1", "u1", "dredging", "2026-06-07T05:11:05.000Z"),
    ])
}

#[test]
fn budget_zero_leaves_the_boundary_as_the_floor_and_claims_nothing_above_it() {
    // Claude Code's own reading: the walk reads `parentUuid`, which is null here, and
    // stops. Everything above is a region csift did not resolve, so it is PRE-CUT - the
    // rewound branch up there is NOT called abandoned, because csift cannot see that far.
    let r = rewound_above_a_boundary();
    let c = Chain::build(&r, None, CrossingBudget::NONE);
    assert_eq!(c.boundary_cut, Some(4));
    assert_eq!(
        survivals(&c, r.len()),
        vec!["pre-cut", "pre-cut", "pre-cut", "pre-cut", "live", "live", "live", "live"]
    );
    assert_eq!(c.abandoned_records, 0);
    assert_eq!(c.boundaries_crossed, 0);
    assert_eq!(c.floor, 4, "the boundary itself is the floor");
    assert_eq!(c.floor_cause, FloorCause::BudgetExhausted);
    assert_eq!(c.boundaries_above_floor, 1, "the floor's own boundary");
}

#[test]
fn a_budget_of_one_steps_over_the_boundary_and_resolves_the_history_above_it() {
    // The same file, one boundary of budget: the walk follows `logicalParentUuid` to `a0`
    // and reaches the head, so the region above the cut is RESOLVED - `u0`/`a0` are the
    // conversation and the branch left behind up there is named for what it is, a turn the
    // operator rewound past. That is the whole difference the budget buys.
    let r = rewound_above_a_boundary();
    let c = Chain::build(&r, None, CrossingBudget::Limited(1));
    assert_eq!(c.boundary_cut, Some(4), "the crossing still names the cut");
    assert_eq!(
        survivals(&c, r.len()),
        vec![
            "live",
            "live",
            "abandoned",
            "abandoned",
            "live",
            "live",
            "live",
            "live"
        ]
    );
    assert_eq!(c.boundaries_crossed, 1);
    assert!(!c.budget_exhausted, "one boundary, one unit of budget");
    assert_eq!(c.floor, 0, "the walk reached the head of the file");
    assert_eq!(c.floor_cause, FloorCause::Head);
    assert_eq!(c.boundaries_above_floor, 0);
    assert_eq!(c.abandoned_records, 2);
    assert_eq!(c.rewound_turns, 1, "the abandoned opener drew a reply");
    assert!(matches!(c.kind(2), Some(Kind::Rewound { .. })));
}

#[test]
fn a_boundary_naming_no_resolvable_record_is_a_floor_under_every_budget() {
    // MISC-043: the pre-compaction records were deleted from disk, so the boundary's
    // `logicalParentUuid` names nothing the map holds. Budget is irrelevant - there is no
    // door - and the cause says so, which is how a reader learns that raising the flag
    // would change nothing.
    let r = recs(&[
        &u("u0", "", "old prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "old reply", "2026-06-07T05:00:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"deleted-from-disk","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto"}}"#,
        &u("u1", "b1", "new prompt", "2026-06-07T05:11:00.000Z"),
        &a("a1", "u1", "new reply", "2026-06-07T05:11:05.000Z"),
    ]);
    let c = Chain::build(&r, None, CrossingBudget::Unbounded);
    assert_eq!(c.boundaries_crossed, 0);
    assert!(!c.budget_exhausted, "the budget was never the obstacle");
    assert_eq!(c.floor_cause, FloorCause::UnresolvableBoundary);
    assert_eq!(c.floor, 2);
    assert_eq!(
        survivals(&c, r.len()),
        vec!["pre-cut", "pre-cut", "live", "live", "live"]
    );
}

/// Five plain boundaries stacked, each naming the assistant record just above it.
fn five_boundaries() -> Vec<Record> {
    let mut lines: Vec<String> = vec![
        u("u0", "", "prompt 0", "2026-06-07T05:00:00.000Z"),
        a("a0", "u0", "reply 0", "2026-06-07T05:00:05.000Z"),
    ];
    for k in 1..=5 {
        lines.push(format!(
            r#"{{"type":"system","subtype":"compact_boundary","uuid":"b{k}","parentUuid":null,"logicalParentUuid":"a{}","timestamp":"2026-06-07T06:0{k}:00.000Z","compactMetadata":{{"trigger":"auto"}}}}"#,
            k - 1
        ));
        lines.push(format!(
            r#"{{"type":"user","uuid":"s{k}","parentUuid":"b{k}","isCompactSummary":true,"timestamp":"2026-06-07T06:0{k}:01.000Z","message":{{"role":"user","content":"summary {k}"}}}}"#
        ));
        lines.push(u(
            &format!("u{k}"),
            &format!("s{k}"),
            "prompt",
            &format!("2026-06-07T06:0{k}:02.000Z"),
        ));
        lines.push(a(
            &format!("a{k}"),
            &format!("u{k}"),
            "reply",
            &format!("2026-06-07T06:0{k}:03.000Z"),
        ));
    }
    recs(&lines.iter().map(String::as_str).collect::<Vec<_>>())
}

#[test]
fn a_stack_of_five_boundaries_stops_after_three_under_budget_three() {
    // The counting rule the disclosure rests on: three stepped over, the fourth met with
    // no budget left (so it is the floor), and TWO boundaries remain uncrossed - the floor's
    // own and the one above it. `--cross-compactions 5` is what would reach the head, which
    // is `boundaries_crossed + boundaries_above_floor`.
    let r = five_boundaries();
    let c = Chain::build(&r, None, CrossingBudget::Limited(3));
    assert_eq!(c.boundaries_crossed, 3);
    assert!(c.budget_exhausted);
    assert_eq!(c.floor_cause, FloorCause::BudgetExhausted);
    assert_eq!(c.floor, 6, "the second boundary from the top of the file");
    assert_eq!(c.boundaries_above_floor, 2);
    let rep = c.crossing(Some(7), None);
    assert_eq!(rep.resolving_budget(), 5);
    assert_eq!(
        survivals(&c, 6),
        vec!["pre-cut"; 6],
        "everything above the floor stays unresolved"
    );
    assert!(
        (6..r.len()).all(|i| c.survival(i) == Survival::Live),
        "everything the walk reached is live"
    );
}

#[test]
fn an_unbounded_budget_crosses_every_boundary_in_the_file() {
    let r = five_boundaries();
    let c = Chain::build(&r, None, CrossingBudget::Unbounded);
    assert_eq!(c.boundaries_crossed, 5);
    assert!(!c.budget_exhausted);
    assert_eq!(c.floor, 0);
    assert_eq!(c.floor_cause, FloorCause::Head);
    assert_eq!(c.boundaries_above_floor, 0);
    assert!(
        (0..r.len()).all(|i| c.survival(i) == Survival::Live),
        "nothing is left unresolved"
    );
}

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
    let mut total = CrossingReport::default();
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
fn a_draft_above_a_crossed_boundary_is_named_by_the_chain_itself() {
    // Above a crossed boundary the ordinary rules apply, so a recalled prompt up there is
    // a DRAFT with its resend named - the chain's own answer, not the blind-region rule
    // that has to stand in when the walk cannot reach.
    let r = recs(&[
        &u("u0", "", "old prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "old reply", "2026-06-07T05:00:05.000Z"),
        &u("d1", "a0", "draft", "2026-06-07T05:01:00.000Z"),
        &u("s0", "a0", "the resend", "2026-06-07T05:02:00.000Z"),
        &a("a1", "s0", "answered", "2026-06-07T05:02:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"a1","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto"}}"#,
        r#"{"type":"user","uuid":"sm","parentUuid":"b1","isCompactSummary":true,"timestamp":"2026-06-07T05:10:01.000Z","message":{"role":"user","content":"summary"}}"#,
        &u("u1", "sm", "new prompt", "2026-06-07T05:11:00.000Z"),
        &a("a2", "u1", "new reply", "2026-06-07T05:11:05.000Z"),
    ]);
    let c = Chain::build(&r, None, CrossingBudget::Limited(1));
    assert_eq!(c.boundaries_crossed, 1);
    assert_eq!(c.survival(2), Survival::Abandoned { root: 2 });
    assert_eq!(
        c.kind(2),
        Some(Kind::Draft {
            superseded_by: Some(3)
        })
    );
    assert_eq!(c.opener_class(2), Some(Class::UserUnsent));
    assert_eq!(c.survival(3), Survival::Live, "the resend is the chain");
    assert_eq!(c.drafts, 1);
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
fn a_file_with_no_leaf_at_all_reports_every_boundary_as_uncrossed() {
    // An empty record set has no leaf, so nothing is on the chain and the floor sits past
    // the end. Counting boundaries "at or above the floor" then covers the whole file,
    // which is the honest answer: none of them was crossed.
    let c = Chain::build(&[], None, CrossingBudget::Limited(3));
    assert_eq!(c.floor_cause, FloorCause::NoLeaf);
    assert_eq!(c.boundaries_crossed, 0);
    assert_eq!(
        c.boundaries_above_floor, 0,
        "no boundaries in an empty file"
    );
}

#[test]
fn a_dangling_parent_that_is_not_a_boundary_names_its_own_cause() {
    // The walk can also end on an ordinary record whose parent names nothing here (a
    // truncated file). It is a floor like any other, but raising the budget cannot move it
    // and the cause says which one it is.
    let r = recs(&[
        &u(
            "u0",
            "gone-from-this-file",
            "first",
            "2026-06-07T05:00:00.000Z",
        ),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
    ]);
    let c = Chain::build(&r, None, CrossingBudget::Unbounded);
    assert_eq!(c.floor_cause, FloorCause::DanglingParent);
    assert_eq!(c.boundaries_crossed, 0);
    assert!(
        !c.crossing(None, None).any(),
        "no boundary was involved, so there is nothing to disclose"
    );
}
