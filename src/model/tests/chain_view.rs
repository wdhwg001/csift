//! [`ChainView`] over a NON-EMPTY spine: the merge, the back-map, and the group shape.
//!
//! The surfaces that use the view keep a DIFFERENT slice of the transcript from the one
//! the chain needs, so every answer has to survive a translation between two index spaces.
//! These tests hold the fixture's two sides apart deliberately - `records` is what a
//! surface's prefilter kept, `spine` is the structural row of everything it dropped - and
//! then assert in the SURFACE's numbering, which is the only numbering its callers have.

use super::*;

/// The two sides a prefilter splits a transcript into: what it KEPT, and the structural
/// rows of what it dropped.
type Split = (Vec<(usize, Record)>, Vec<(usize, Record)>);

/// The rewind shape, split the way a prefilter splits it. Physical lines:
/// L1 opener · L2 its reply · L3 a hook attachment · L4 the REWOUND opener · L5 its reply ·
/// L6 the resend (same parent as L4) · L7 its reply.
/// The surface keeps only the three USER records; everything else is spine.
fn split_fixture() -> Split {
    let records = vec![
        (
            1,
            parse(
                r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#,
            ),
        ),
        (
            4,
            parse(
                r#"{"type":"user","uuid":"u1","parentUuid":"x0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#,
            ),
        ),
        (
            6,
            parse(
                r#"{"type":"user","uuid":"u2","parentUuid":"x0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"survey the southern shoal"}}"#,
            ),
        ),
    ];
    let spine = vec![
        (
            2,
            parse(
                r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z"}"#,
            ),
        ),
        (
            3,
            parse(
                r#"{"type":"attachment","uuid":"x0","parentUuid":"a0","timestamp":"2026-06-07T05:00:50.000Z"}"#,
            ),
        ),
        (
            5,
            parse(
                r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z"}"#,
            ),
        ),
        (
            7,
            parse(
                r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:02:05.000Z"}"#,
            ),
        ),
    ];
    (records, spine)
}

#[test]
fn the_spine_is_merged_by_line_number_and_answers_land_in_surface_indices() {
    let (records, spine) = split_fixture();
    let view = ChainView::build(&records, &spine);
    // The chain only resolves at all because the spine put L2/L3 between L1 and L4 - the
    // resend's parent is the ATTACHMENT, which the surface never kept.
    assert_eq!(view.survival(0), Survival::Live, "the first opener");
    assert!(
        matches!(view.survival(1), Survival::Abandoned { .. }),
        "the rewound opener: {:?}",
        view.survival(1)
    );
    assert_eq!(view.survival(2), Survival::Live, "the resend");
    // Answers are addressed by the SURFACE's index, and the abandoned stamp names a
    // physical LINE (4), never an index - the caller has no index space but its own.
    assert_eq!(view.stamp(0), TurnStamp::Live(0));
    assert_eq!(view.stamp(1), TurnStamp::Abandoned { root_line: 4 });
    assert_eq!(view.stamp(2), TurnStamp::Live(1));
    assert_eq!(view.abandoned_openers(), &[4]);
}

#[test]
fn an_abandoned_record_still_carries_the_live_turn_it_follows() {
    let (records, spine) = split_fixture();
    let view = ChainView::build(&records, &spine);
    // The ordering key exists so a `--turn` window on a DISK surface can admit the row: the
    // rewound branch physically follows live turn 0.
    assert_eq!(view.order_turn(0), 0);
    assert_eq!(view.order_turn(1), 0, "the abandoned opener follows turn 0");
    assert_eq!(view.order_turn(2), 1);
}

#[test]
fn the_groups_are_the_live_turns_then_one_per_abandoned_branch() {
    let (records, spine) = split_fixture();
    let view = ChainView::build(&records, &spine);
    assert_eq!(view.turn_count(), 2, "two live turns survive the rewind");
    // Turn 0's members in SURFACE space are just its opener: L2 and L3 are spine rows and
    // belong to no surface index at all.
    assert_eq!(view.turns(), &[vec![0], vec![2]]);
    let groups = view.groups();
    assert_eq!(groups.len(), 3, "two live turns + one abandoned branch");
    assert_eq!(groups[0].0, TurnStamp::Live(0));
    assert_eq!(groups[1].0, TurnStamp::Live(1));
    assert_eq!(groups[2].0, TurnStamp::Abandoned { root_line: 4 });
    assert_eq!(groups[2].1, &[1], "the branch's only surface member");
}

#[test]
fn a_turn_opened_by_a_real_record_keeps_that_record_in_surface_space() {
    // A group is anchored one of two ways, and only one of them can be a spine row. An
    // OPENER anchor is always a surface record: a spine row carries no `message`, so
    // `opens_turn` is false for it and no numbered turn is ever anchored on one. The other
    // anchor is the pre-first-user LEAD seed, which any record can take - see the next two
    // tests for the shapes where that one survives as an empty group.
    let (records, spine) = split_fixture();
    let view = ChainView::build(&records, &spine);
    for (i, group) in view.turns().iter().enumerate() {
        assert!(
            !group.is_empty(),
            "live turn {i} lost its opener in the back-map"
        );
    }
    // And the spine rows themselves reach no surface index: `survival` past the surface's
    // own length reads Live rather than indexing into the merged array.
    assert_eq!(view.survival(records.len()), Survival::Live);
    assert_eq!(view.stamp(records.len()), TurnStamp::Live(0));
}

#[test]
fn an_all_spine_lead_group_is_kept_empty_rather_than_renumbered() {
    // The pre-first-user lead seeds a group from whatever record comes first, spine row
    // included. It is folded into the first real turn when one follows; when none does,
    // it survives with NO surface member. The view KEEPS that group on purpose - dropping
    // it would slide every later turn number down by one and break the whole point of the
    // view, which is that a `verbatim` turn 7 and a `search` t7 are the same turn. The
    // consumers are written for it: `turns/build.rs` skips an empty group (nothing to
    // replay), `files`/`recover` iterate members, and `image` never reads `turns()`.
    let (_, spine) = split_fixture();
    let view = ChainView::build(&[], &spine);
    assert_eq!(
        view.turns(),
        &[Vec::<usize>::new()],
        "one group, anchored on a spine row, with no surface member"
    );
    assert_eq!(view.turn_count(), 1);
}

#[test]
fn a_surface_holding_only_an_abandoned_opener_still_numbers_the_live_turns() {
    // The grouper SKIPS an abandoned record entirely (`Survival::selectable` is false), so
    // a surface whose only kept record is the rewound opener contributes nothing to any
    // group - yet the live turns still exist and must still be numbered, because the
    // numbering is a property of the transcript, not of this surface's prefilter.
    let (records, spine) = split_fixture();
    let only_abandoned = vec![records[1].clone()];
    let mut full_spine = spine;
    full_spine.push(records[0].clone());
    full_spine.push(records[2].clone());
    full_spine.sort_by_key(|(line, _)| *line);
    let view = ChainView::build(&only_abandoned, &full_spine);
    assert!(
        matches!(view.survival(0), Survival::Abandoned { .. }),
        "the one kept record is still off the chain: {:?}",
        view.survival(0)
    );
    assert_eq!(view.stamp(0), TurnStamp::Abandoned { root_line: 4 });
    assert!(
        view.turns().iter().all(Vec::is_empty),
        "no live turn has a surface member here: {:?}",
        view.turns()
    );
    assert!(
        view.turn_count() >= 1,
        "the live turns are still counted: {:?}",
        view.turns()
    );
}

#[test]
fn an_empty_spine_is_the_degenerate_case_and_still_answers() {
    // A surface whose prefilter kept everything hands in no spine at all; the view must
    // behave exactly like the plain chain then.
    let (records, _) = split_fixture();
    let view = ChainView::build(&records, &[]);
    assert_eq!(view.turn_count(), view.turns().len());
    // With L2/L3 absent the resend's parent uuid resolves to nothing, so the chain cannot
    // see the fork - which is precisely why the spine exists.
    assert!(view.abandoned_openers().len() <= 1);
}
