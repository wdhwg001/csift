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
    let c = Chain::build(&r, None);
    assert_eq!(
        survivals(&c, r.len()),
        vec!["live", "live", "live"],
        "the axis says nothing about a merged marker, and nothing about it moves the rest"
    );
    assert_eq!(c.leaf_index, Some(1), "the leaf is the real tail record");
    assert_eq!(c.abandoned_records, 0);
}

#[test]
fn a_boundary_the_walk_cannot_climb_crosses_the_cut_through_its_logical_parent() {
    // The step-over is what separates a region csift RESOLVED from one it could not: with
    // it the walk reaches the head, the floor drops to the top of the file, and a branch
    // left behind up there is `abandoned` - a rewound turn, named as one. Without it the
    // walk ends on the boundary, everything above is below the floor, and the same branch
    // reads `pre-cut`, because csift never calls a record abandoned where it could not see.
    //
    // On the SHAPE: every `compact_boundary` Claude Code writes carries `parentUuid: null`,
    // and the walk breaks on a null parent before this arm is reached (the plain-boundary
    // test in the parent file pins that path). This arm is the guard for a boundary the
    // walk reaches with a parent it cannot resolve, and it is where the crossing lives.
    let r = recs(&[
        &u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "charting", "2026-06-07T05:00:05.000Z"),
        &u("u2", "u0", "survey the shoal", "2026-06-07T05:00:10.000Z"),
        &a("a2", "u2", "surveying", "2026-06-07T05:00:15.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":"deleted-from-disk","logicalParentUuid":"a0","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preTokens":900}}"#,
        &u("u1", "b1", "dredge the channel", "2026-06-07T05:11:00.000Z"),
        &a("a1", "u1", "dredging", "2026-06-07T05:11:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.boundary_cut, Some(4), "the crossing still names the cut");
    assert_eq!(
        survivals(&c, r.len()),
        vec![
            "pre-cut",
            "pre-cut",
            "abandoned",
            "abandoned",
            "live",
            "live",
            "live"
        ],
        "the crossed history is read and flagged; the branch off it is resolved as abandoned"
    );
    assert_eq!(c.abandoned_records, 2);
    assert_eq!(c.rewound_turns, 1, "the abandoned opener drew a reply");
    assert!(
        matches!(c.kind(2), Some(Kind::Rewound { .. })),
        "an answered abandoned opener above the cut is a rewind, not a draft"
    );
}
