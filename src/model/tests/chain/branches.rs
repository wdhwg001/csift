//! Naming what replaced an abandoned opener, and the child index the Draft/Rewound
//! discriminator reads.

use super::*;

/// An AskUserQuestion ANSWER: a `tool_result` carrier with a structured `answers` map. It
/// is a turn opener (the operator's answer IS their message) and it is not on the walk, so
/// it is the one shape that puts a SECOND surviving opener under one parent.
fn auq_answer(uuid: &str, parent: &str, tool_use_id: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","parentUuid":"{parent}","timestamp":"{ts}","toolUseResult":{{"answers":{{"Pick one":"A"}}}},"message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{tool_use_id}","content":"(answer recorded)"}}]}}}}"#
    )
}

/// An assistant record carrying two `tool_use` blocks, so both carriers below it are real.
fn two_calls(uuid: &str, parent: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","parentUuid":"{parent}","timestamp":"{ts}","message":{{"role":"assistant","id":"m1","content":[{{"type":"tool_use","id":"t1","name":"AskUserQuestion","input":{{}}}},{{"type":"tool_use","id":"t2","name":"Read","input":{{}}}}]}}}}"#
    )
}

#[test]
fn the_survivor_of_a_draft_is_an_opener_and_not_merely_the_newest_record_sharing_its_parent() {
    // `superseded_by` answers "what did the operator send instead", so only a TURN OPENER
    // can be it. Two records share `a0` as their parent here: the answered question at L4,
    // which opens a turn, and the plain tool_result at L5, which does not. The later one is
    // not the answer, and neither is the chain's own child of `a0` - which IS that plain
    // carrier, so the shortcut through it has to check that it opens a turn before taking it.
    let r = recs(&[
        &u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        &two_calls("a0", "u0", "2026-06-07T05:00:05.000Z"),
        &u(
            "d1",
            "a0",
            "sound the reef margn",
            "2026-06-07T05:01:00.000Z",
        ),
        &auq_answer("q1", "a0", "t1", "2026-06-07T05:01:30.000Z"),
        r#"{"type":"user","uuid":"c0","parentUuid":"a0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"soundings"}]}}"#,
        &a("a2", "c0", "sounding", "2026-06-07T05:02:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    let kind = c.kind(2).expect("the recalled prompt has a kind");
    assert!(
        matches!(kind, Kind::Draft { .. }),
        "nothing ever answered it: {kind:?}"
    );
    assert_eq!(
        kind.survivor(),
        Some(3),
        "the answered question, not the plain carrier written after it"
    );
}

#[test]
fn the_chains_own_child_wins_over_the_last_opener_sharing_the_parent() {
    // When the chain's own child of the shared parent DOES open a turn it is the answer:
    // it is the branch the conversation actually took. A second surviving opener sits under
    // `a0` here (an answered question the message-id rescue brought back), and it is later
    // in the file - so falling through to "the last opener under this parent" would name the
    // record the conversation did NOT continue from.
    let r = recs(&[
        &u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        &two_calls("a0", "u0", "2026-06-07T05:00:05.000Z"),
        &u(
            "d1",
            "a0",
            "sound the reef margn",
            "2026-06-07T05:01:00.000Z",
        ),
        &auq_answer("q1", "a0", "t1", "2026-06-07T05:01:30.000Z"),
        &auq_answer("q2", "a0", "t2", "2026-06-07T05:02:00.000Z"),
        &a("a2", "q1", "sounding", "2026-06-07T05:02:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    let kind = c.kind(2).expect("the recalled prompt has a kind");
    assert_eq!(
        kind.survivor(),
        Some(3),
        "the branch the walk took, not the later opener under the same parent"
    );
}

#[test]
fn the_survivor_is_named_from_the_chains_own_child_only_when_that_child_opens_a_turn() {
    // The chain's own child of the shared parent is the first record the walk meets going
    // up, and here that is a tool_result carrier - part of the turn `a0` opened, not a new
    // prompt. There is no surviving opener under `a0` at all, and saying there is one would
    // point a reader at a record the operator never sent.
    let r = recs(&[
        &u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"tool_use","id":"t1","name":"Read","input":{}}]}}"#,
        &u("d1", "a0", "sound the reef", "2026-06-07T05:01:00.000Z"),
        r#"{"type":"user","uuid":"c0","parentUuid":"a0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"soundings"}]}}"#,
        &a("a2", "c0", "read them", "2026-06-07T05:02:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    let kind = c.kind(2).expect("the abandoned prompt has a kind");
    assert_eq!(
        kind.survivor(),
        None,
        "no opener replaced it, so none is named: {kind:?}"
    );
}

#[test]
fn a_replay_copy_below_an_opener_does_not_make_it_a_rewound_turn() {
    // Draft or Rewound turns on one fact: did an assistant record hang below the opener.
    // The only assistant under `d1` is the EARLIER line of a duplicated uuid, which the
    // loader's map does not hold - the survivor of that uuid hangs under the resend. Reading
    // the copy as a reply would call a recalled prompt a rewound turn.
    let r = recs(&[
        &u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "charting", "2026-06-07T05:00:05.000Z"),
        &u(
            "d1",
            "a0",
            "sound the reef margn",
            "2026-06-07T05:01:00.000Z",
        ),
        &a("dup", "d1", "a replay copy", "2026-06-07T05:01:05.000Z"),
        &u(
            "u2",
            "a0",
            "sound the reef margin",
            "2026-06-07T05:02:00.000Z",
        ),
        &a("dup", "u2", "its survivor", "2026-06-07T05:02:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    let kind = c.kind(2).expect("the recalled prompt has a kind");
    assert!(
        matches!(kind, Kind::Draft { .. }),
        "no assistant record the map still holds hangs below it: {kind:?}"
    );
    assert_eq!(c.drafts, 1);
    assert_eq!(c.rewound_turns, 0);
}

#[test]
fn every_child_of_one_parent_is_indexed_not_just_the_first() {
    // The child index is built in one bucketed pass, and the Draft/Rewound discriminator
    // reads it for every record. The reply is the FIRST of this opener's three children, so
    // a cursor that does not advance files all three into one slot, the last one written
    // wins, and the reply - the only fact that separates a rewound turn from a recalled
    // prompt - is gone.
    let r = recs(&[
        &u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "charting", "2026-06-07T05:00:05.000Z"),
        &u(
            "d1",
            "a0",
            "sound the reef margn",
            "2026-06-07T05:01:00.000Z",
        ),
        &a("r1", "d1", "an answer", "2026-06-07T05:01:02.000Z"),
        &att("x1", "d1", "2026-06-07T05:01:03.000Z"),
        &att("x2", "d1", "2026-06-07T05:01:05.000Z"),
        &u(
            "u2",
            "a0",
            "sound the reef margin",
            "2026-06-07T05:02:00.000Z",
        ),
        &a("a2", "u2", "sounding", "2026-06-07T05:02:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    let kind = c.kind(2).expect("the rewound prompt has a kind");
    assert!(
        matches!(kind, Kind::Rewound { .. }),
        "the reply is the FIRST of the opener's three children: {kind:?}"
    );
    assert_eq!(kind.survivor(), Some(6));
    assert_eq!(c.rewound_turns, 1);
    assert_eq!(c.drafts, 0);
}

#[test]
fn the_relink_re_points_the_anchors_other_children_at_the_preserved_tail() {
    // The loader hangs the preserved segment off the anchor and then moves everything ELSE
    // that referenced the anchor to the end of that segment - everything except the segment
    // head, which is now the anchor's child itself. Here the only such record is a trailing
    // attachment, and it is what keeps the file readable at all: re-pointed, it hangs off
    // the leaf and comes with it; left on the removed anchor, nothing climbs to a
    // conversation record and the whole transcript reads as unresolved.
    let r = recs(&[
        &u("u0", "", "dropped prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "dropped reply", "2026-06-07T05:00:05.000Z"),
        &u("k1", "a0", "kept prompt", "2026-06-07T05:05:00.000Z"),
        &a("k2", "k1", "kept reply", "2026-06-07T05:05:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preservedMessages":{"anchorUuid":"a0","uuids":["k1","k2"]}}}"#,
        &att("x1", "a0", "2026-06-07T05:11:00.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.leaf_index, Some(3), "the preserved tail is the leaf");
    let s = survivals(&c, r.len());
    assert_eq!(&s[0..2], ["pre-cut", "pre-cut"], "the cut still cuts");
    assert_eq!(s[2], "live");
    assert_eq!(s[3], "live");
    assert_eq!(
        s[5], "live",
        "the re-pointed attachment hangs off the leaf and comes with it"
    );
}
