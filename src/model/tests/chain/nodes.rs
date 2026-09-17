//! The two chain rules keyed on what a node ANSWERS rather than on where it sits: which
//! lines the loader's uuid map admits at all, and the forensic step over a compaction cut.

use super::*;

#[test]
fn a_merged_elicitation_marker_is_outside_the_chain_and_takes_no_one_with_it() {
    // A pending elicitation is the record Claude Code has NOT written yet: csift merges it
    // from its own sidecar, it has no place in any on-disk DAG, and it carries no physical
    // line. Admitting it would make it the file's newest admitted record, so the leaf would
    // land on it, the walk would stop dead at its absent parent, and the whole real
    // conversation below would read `pre-cut` - a live question filed as history.
    let r = recs(&[
        &u("u0", "", "start the work", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "working", "2026-06-07T05:00:05.000Z"),
        r#"{"type":"assistant","uuid":"e-toolu_1","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"toolu_1","name":"AskUserQuestion","input":{"questions":[{"question":"deploy now?"}]}}]},"csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"toolu_1"}"#,
    ]);
    let c = Chain::build(&r, None, CrossingBudget::NONE);
    assert_eq!(
        survivals(&c, r.len()),
        vec!["live", "live", "live"],
        "the axis says nothing about a merged marker, and nothing about it moves the rest"
    );
    assert_eq!(c.leaf_index, Some(1), "the leaf is the real tail record");
    assert_eq!(c.abandoned_records, 0);
}

#[test]
fn a_boundary_reached_with_an_unresolvable_parent_takes_the_same_crossing_door() {
    // There are TWO ways the walk arrives at a boundary with nowhere to climb: the null
    // `parentUuid` every Claude Code boundary carries (the ordinary path, pinned in
    // `crossing.rs`), and a boundary whose own parent is SET and resolves to no record.
    // Both reach the one crossing rule, so both consult the budget and both follow
    // `logicalParentUuid` - there is no second arm to drift.
    let r = recs(&[
        &u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "charting", "2026-06-07T05:00:05.000Z"),
        &u("u2", "u0", "survey the shoal", "2026-06-07T05:00:10.000Z"),
        &a("a2", "u2", "surveying", "2026-06-07T05:00:15.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":"deleted-from-disk","logicalParentUuid":"a0","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preTokens":900}}"#,
        &u("u1", "b1", "dredge the channel", "2026-06-07T05:11:00.000Z"),
        &a("a1", "u1", "dredging", "2026-06-07T05:11:05.000Z"),
    ]);
    let off = Chain::build(&r, None, CrossingBudget::NONE);
    assert_eq!(off.boundary_cut, Some(4), "the cut is named either way");
    assert_eq!(off.boundaries_crossed, 0);
    assert_eq!(
        survivals(&off, r.len()),
        vec!["pre-cut", "pre-cut", "pre-cut", "pre-cut", "live", "live", "live"],
        "with no budget the boundary is the floor and nothing above it is resolved"
    );
    assert_eq!(off.abandoned_records, 0);

    let on = Chain::build(&r, None, CrossingBudget::Limited(1));
    assert_eq!(on.boundaries_crossed, 1);
    assert_eq!(
        survivals(&on, r.len()),
        vec![
            "live",
            "live",
            "abandoned",
            "abandoned",
            "live",
            "live",
            "live"
        ],
        "the crossed history is resolved; the branch off it is abandoned"
    );
    assert_eq!(on.abandoned_records, 2);
    assert_eq!(on.rewound_turns, 1, "the abandoned opener drew a reply");
    assert!(
        matches!(on.kind(2), Some(Kind::Rewound { .. })),
        "an answered abandoned opener above the cut is a rewind, not a draft"
    );
}
