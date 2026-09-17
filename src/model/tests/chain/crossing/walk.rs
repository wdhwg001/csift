//! What the WALK does at a compaction boundary under each budget: what it steps over,
//! where it stops, and what the records above that floor then read as.

use super::*;

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
    assert_eq!(
        c.boundary_cut, None,
        "the boundary was CROSSED, so it is counted and not named as the cut"
    );
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

#[test]
fn a_shut_boundary_door_is_named_by_which_of_three_facts_shut_it() {
    // Both floors are boundaries the walk cannot climb, and a reader is told which kind.
    // Here the boundary's `logicalParentUuid` names `a0`, which IS on a line of this file -
    // the pre-boundary relink cut took it out of the uuid map, because the compaction
    // PRESERVED a later record and everything before the preserved set is dropped. Calling
    // that "names no record this file carries" sends a reader looking for a line that is
    // right there; calling it relink-removed tells them the record is on disk and off the
    // map, which is the fact they need.
    let r = recs(&[
        &u("u0", "", "old prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "old reply", "2026-06-07T05:00:05.000Z"),
        &u("u1", "a0", "kept prompt", "2026-06-07T05:00:10.000Z"),
        &a("a1", "u1", "kept reply", "2026-06-07T05:00:15.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"a0","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preservedMessages":{"anchorUuid":"u1","uuids":["u1","a1"],"allUuids":["u1","a1"]}}}"#,
        r#"{"type":"user","uuid":"s1","parentUuid":"b1","isCompactSummary":true,"timestamp":"2026-06-07T05:10:01.000Z","message":{"role":"user","content":"summary"}}"#,
        &u("u2", "s1", "new prompt", "2026-06-07T05:11:00.000Z"),
        &a("a2", "u2", "new reply", "2026-06-07T05:11:05.000Z"),
    ]);
    let c = Chain::build(&r, None, CrossingBudget::Unbounded);
    assert_eq!(c.floor_cause, FloorCause::RelinkRemoved);
    assert_eq!(c.boundaries_crossed, 0, "the door is shut either way");
    assert!(!c.budget_exhausted, "the budget was never the obstacle");
    assert!(
        c.crossing(Some(5), None).lines()[1].contains("removed from the uuid map"),
        "{:?}",
        c.crossing(Some(5), None).lines()
    );

    // THIRD shape, and the commonest of the three on real data: the boundary's logical
    // parent is a record the walk has ALREADY reached, so its door leads back into the
    // conversation rather than above the cut. Nothing is gone and nothing was removed - the
    // link simply points downstream - and a reader told "names no record this file carries"
    // about a line they can open learns the wrong thing.
    let loops = recs(&[
        &u("u0", "", "old prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "old reply", "2026-06-07T05:00:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"a1","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto"}}"#,
        r#"{"type":"user","uuid":"s1","parentUuid":"b1","isCompactSummary":true,"timestamp":"2026-06-07T05:10:01.000Z","message":{"role":"user","content":"summary"}}"#,
        &u("u1", "s1", "new prompt", "2026-06-07T05:11:00.000Z"),
        &a("a1", "u1", "new reply", "2026-06-07T05:11:05.000Z"),
    ]);
    let c = Chain::build(&loops, None, CrossingBudget::Unbounded);
    assert_eq!(c.floor_cause, FloorCause::BoundaryLoop);
    assert_eq!(c.boundaries_crossed, 0);
    assert!(
        c.crossing(Some(3), None).lines()[1].contains("the walk had already reached"),
        "{:?}",
        c.crossing(Some(3), None).lines()
    );

    // The same shape with the logical parent naming a uuid on no line keeps the other name.
    let gone = recs(&[
        &u("u0", "", "old prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "old reply", "2026-06-07T05:00:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"deleted-from-disk","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto"}}"#,
        &u("u1", "b1", "new prompt", "2026-06-07T05:11:00.000Z"),
        &a("a1", "u1", "new reply", "2026-06-07T05:11:05.000Z"),
    ]);
    assert_eq!(
        Chain::build(&gone, None, CrossingBudget::Unbounded).floor_cause,
        FloorCause::UnresolvableBoundary
    );
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

#[test]
fn the_named_cut_is_the_boundary_the_walk_stopped_at_never_one_it_stepped_over() {
    // `boundary_cut` is what the footer calls "the compaction boundary the surviving
    // conversation is cut at", and it kept that meaning through the crossing: a boundary the
    // walk got PAST is not a cut it stopped at. Recorded before the budget test, it named the
    // NEWEST boundary instead, so a crossing printed "cut at L11" two lines above "the chain
    // floor is L3" - two different records offered as the same fact.
    let r = two_crossed_then_a_dangling_floor();
    let c = Chain::build(&r, None, CrossingBudget::Unbounded);
    assert_eq!(c.boundaries_crossed, 2, "b3 and b2 are stepped over");
    assert_eq!(c.floor, 2, "b1 is the floor");
    assert_eq!(c.floor_cause, FloorCause::UnresolvableBoundary);
    assert_eq!(
        c.boundary_cut,
        Some(c.floor),
        "the named cut IS the floor, not the newest boundary (index 10)"
    );

    // At budget 0 nothing is crossed and the newest boundary is both the stop and the cut -
    // the reading csift gave before the budget existed.
    let off = Chain::build(&r, None, CrossingBudget::NONE);
    assert_eq!(off.boundaries_crossed, 0);
    assert_eq!(off.floor, 10);
    assert_eq!(off.boundary_cut, Some(10));

    // A walk that ends at the HEAD names no cut at all.
    let head = Chain::build(
        &rewound_above_a_boundary(),
        None,
        CrossingBudget::Limited(1),
    );
    assert_eq!(head.floor_cause, FloorCause::Head);
    assert_eq!(
        head.boundary_cut, None,
        "a crossed boundary is counted, never named as the cut"
    );
}
