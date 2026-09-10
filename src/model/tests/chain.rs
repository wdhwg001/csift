//! The SURVIVAL AXIS over hand-built DAGs: one shape per rule of Claude Code's loader.

use super::*;

fn recs(lines: &[&str]) -> Vec<Record> {
    lines.iter().map(|l| parse(l)).collect()
}

fn u(uuid: &str, parent: &str, text: &str, ts: &str) -> String {
    let p = if parent.is_empty() {
        "null".to_string()
    } else {
        format!("\"{parent}\"")
    };
    format!(
        r#"{{"type":"user","uuid":"{uuid}","parentUuid":{p},"timestamp":"{ts}","message":{{"role":"user","content":"{text}"}}}}"#
    )
}

fn a(uuid: &str, parent: &str, text: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","parentUuid":"{parent}","timestamp":"{ts}","message":{{"role":"assistant","id":"m-{uuid}","content":[{{"type":"text","text":"{text}"}}]}}}}"#
    )
}

fn att(uuid: &str, parent: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"attachment","uuid":"{uuid}","parentUuid":"{parent}","timestamp":"{ts}","attachment":{{"type":"hook_additional_context","content":["ctx"]}}}}"#
    )
}

fn survivals(c: &Chain, n: usize) -> Vec<&'static str> {
    (0..n).map(|i| c.survival(i).as_str()).collect()
}

// ── the ordinary shape: everything the walk reaches is live ──

#[test]
fn a_linear_transcript_is_entirely_live() {
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        &att("x0", "a0", "2026-06-07T05:01:00.000Z"),
        &u("u1", "x0", "second", "2026-06-07T05:01:01.000Z"),
        &a("a1", "u1", "two", "2026-06-07T05:01:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(survivals(&c, r.len()), vec!["live"; 5]);
    assert_eq!(c.abandoned_records, 0);
    assert_eq!(c.leaf_source, LeafSource::Tail);
    assert!(c.kind(0).is_none());
}

// ── the recalled draft: an opener with no assistant descendant ──

#[test]
fn a_recalled_draft_is_abandoned_and_names_its_resend() {
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        &u("d1", "a0", "draft", "2026-06-07T05:01:00.000Z"),
        &u("u1", "a0", "resend", "2026-06-07T05:02:00.000Z"),
        &a("a1", "u1", "two", "2026-06-07T05:02:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(
        survivals(&c, r.len()),
        vec!["live", "live", "abandoned", "live", "live"]
    );
    assert_eq!(
        c.kind(2),
        Some(Kind::Draft {
            superseded_by: Some(3)
        })
    );
    assert_eq!(c.opener_class(2), Some(Class::UserUnsent));
    assert_eq!(c.drafts, 1);
    assert_eq!(c.rewound_turns, 0);
    assert_eq!(c.abandoned_root(2), Some(2));
    // Turn numbering skips it entirely.
    let turns = group_turn_indices_deduped(&r, |x| ChainNode::Full(x));
    assert_eq!(turns, vec![vec![0, 1], vec![3, 4]]);
}

// ── the rewind: an abandoned opener that WAS answered ──

#[test]
fn a_rewound_turn_carries_its_own_leaf_and_takes_its_subtree_with_it() {
    // u1 was answered (a1) and drew two tool records; the operator then rewound and sent
    // u2 from the SAME parent, so the whole u1 branch leaves the conversation.
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        &u("u1", "a0", "rewound prompt", "2026-06-07T05:01:00.000Z"),
        &a("a1", "u1", "answered it", "2026-06-07T05:01:05.000Z"),
        &a("a2", "a1", "edit one", "2026-06-07T05:01:06.000Z"),
        &a("a3", "a2", "edit two", "2026-06-07T05:01:07.000Z"),
        &u("u2", "a0", "the resend", "2026-06-07T05:02:00.000Z"),
        &a("a4", "u2", "two", "2026-06-07T05:02:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(
        survivals(&c, r.len()),
        vec![
            "live",
            "live",
            "abandoned",
            "abandoned",
            "abandoned",
            "abandoned",
            "live",
            "live"
        ]
    );
    assert_eq!(c.kind(2), Some(Kind::Rewound { resend: Some(6) }));
    assert_eq!(c.opener_class(2), Some(Class::UserRewound));
    assert_eq!(c.rewound_turns, 1);
    assert_eq!(c.drafts, 0);
    // Every record of the branch hangs off the rewound head and carries its marker.
    for i in 2..=5 {
        assert_eq!(c.abandoned_root(i), Some(2), "record {i}");
        assert!(c.on_rewound_branch(i), "record {i}");
    }
    // Two grouping entry points, and the difference is deliberate. Given the chain built
    // from the WHOLE record set, the rewound branch belongs to no numbered turn at all.
    assert_eq!(
        group_turn_indices_chained(&r, |x| ChainNode::Full(x), &c),
        vec![vec![0, 1], vec![6, 7]]
    );
    // The chain-free entry point cannot know whether its caller's record set is whole, so
    // it drops the superseded OPENER and keeps every other record as a member. A
    // turn-keyed consumer built on it therefore loses no row.
    assert_eq!(
        group_turn_indices_deduped(&r, |x| ChainNode::Full(x)),
        vec![vec![0, 1, 3, 4, 5], vec![6, 7]]
    );
}

#[test]
fn a_rewind_with_no_resend_leaves_the_branch_live() {
    // Restore-conversation with nothing typed afterwards: the file tail IS the branch, so
    // the chain still reaches it and nothing is abandoned.
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        &u("u1", "a0", "second", "2026-06-07T05:01:00.000Z"),
        &a("a1", "u1", "two", "2026-06-07T05:01:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(survivals(&c, r.len()), vec!["live"; 4]);
    assert_eq!(c.abandoned_records, 0);
}

#[test]
fn two_rewinds_from_one_parent_are_two_branches() {
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        &u("u1", "a0", "attempt one", "2026-06-07T05:01:00.000Z"),
        &a("a1", "u1", "answer one", "2026-06-07T05:01:05.000Z"),
        &u("u2", "a0", "attempt two", "2026-06-07T05:02:00.000Z"),
        &a("a2", "u2", "answer two", "2026-06-07T05:02:05.000Z"),
        &u("u3", "a0", "the one that stuck", "2026-06-07T05:03:00.000Z"),
        &a("a3", "u3", "answer three", "2026-06-07T05:03:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.rewound_turns, 2);
    assert_eq!(c.kind(2), Some(Kind::Rewound { resend: Some(6) }));
    assert_eq!(c.kind(4), Some(Kind::Rewound { resend: Some(6) }));
    assert_eq!(c.abandoned_root(3), Some(2));
    assert_eq!(c.abandoned_root(5), Some(4));
}

// ── C-29 flipped: the LAST copy of a replayed uuid is the survivor ──

#[test]
fn a_replayed_block_keeps_the_last_copy_and_flags_the_earlier_line() {
    // A compaction re-anchor re-appends the same records with their uuids preserved. The
    // v0.11.1 rule kept the FIRST copy; the loader's map keeps the LAST, and so does this.
    // The re-anchor re-appends the whole block, so BOTH records land twice.
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        &u("u0", "", "first", "2026-06-07T05:01:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:01:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(
        c.replay_of(0),
        Some(2),
        "the earlier line points at the survivor"
    );
    assert_eq!(c.replay_of(1), Some(3));
    assert_eq!(c.replay_of(2), None);
    assert_eq!(c.replay_copies, 2);
    assert!(!c.opens(0), "the earlier copy opens no turn");
    assert!(c.opens(2));
    // Every line stays selectable - a copy inherits the survivor's answer.
    assert_eq!(survivals(&c, r.len()), vec!["live"; 4]);
    assert_eq!(c.drafts, 0, "a replay copy is never a draft");
}

// ── the compaction cut ──

#[test]
fn a_plain_boundary_cuts_the_chain_and_the_history_above_it_is_pre_cut() {
    let r = recs(&[
        &u("u0", "", "old prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "old reply", "2026-06-07T05:00:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preTokens":900}}"#,
        r#"{"type":"user","uuid":"s1","parentUuid":"b1","isCompactSummary":true,"timestamp":"2026-06-07T05:10:01.000Z","message":{"role":"user","content":"summary"}}"#,
        &u("u1", "s1", "new prompt", "2026-06-07T05:11:00.000Z"),
        &a("a1", "u1", "new reply", "2026-06-07T05:11:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.boundary_cut, Some(2), "the walk stops at the boundary");
    // csift steps over the cut through logicalParentUuid when there is one; with none,
    // the records above are PRE-CUT, never abandoned - the reading is forensic, and the
    // axis never claims abandonment in a region it could not resolve.
    assert_eq!(
        survivals(&c, r.len()),
        vec!["pre-cut", "pre-cut", "live", "live", "live", "live"]
    );
    assert_eq!(c.abandoned_records, 0);
}

#[test]
fn a_boundary_with_preserved_messages_keeps_the_listed_records_and_cuts_the_rest() {
    // The newest boundary lists a preserved window: those uuids are re-chained onto the
    // anchor and survive, every other pre-boundary record is cut.
    let r = recs(&[
        &u("u0", "", "dropped prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "dropped reply", "2026-06-07T05:00:05.000Z"),
        &u("k1", "a0", "kept prompt", "2026-06-07T05:05:00.000Z"),
        &a("k2", "k1", "kept reply", "2026-06-07T05:05:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preservedMessages":{"anchorUuid":"a0","uuids":["k1","k2"]}}}"#,
        &u("u1", "k2", "new prompt", "2026-06-07T05:11:00.000Z"),
        &a("a1", "u1", "new reply", "2026-06-07T05:11:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    let s = survivals(&c, r.len());
    assert_eq!(
        s[0], "pre-cut",
        "a pre-boundary record outside the preserved set"
    );
    assert_eq!(s[1], "pre-cut");
    assert_eq!(s[2], "live", "a preserved record stays in the conversation");
    assert_eq!(s[3], "live");
    assert_eq!(s[5], "live");
    assert_eq!(s[6], "live");
    // No CONVERSATION record is abandoned: the cut is a cut, not an abandonment. The
    // boundary record itself is off the re-chained path (nothing points at it once the
    // preserved segment hangs off the anchor), which is exactly what the loader does with
    // it - it is metadata, never a message.
    assert!(!s.contains(&"abandoned") || s[4] == "abandoned");
    assert_eq!(c.drafts, 0);
    assert_eq!(c.rewound_turns, 0);
}

// ── the leaf gates ──

#[test]
fn the_last_prompt_leaf_hint_needs_no_uuid_of_its_own() {
    // 0 of 30,000 `last-prompt` records carry a uuid; a reader that required one would
    // never see a leaf at all.
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        r#"{"type":"last-prompt","leafUuid":"a0","sessionId":"s","lastPrompt":"first"}"#,
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.leaf_source, LeafSource::LastPrompt);
    assert_eq!(c.leaf_index, Some(1));
}

#[test]
fn a_leaf_hint_naming_no_record_falls_back_to_the_file_tail() {
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        r#"{"type":"last-prompt","leafUuid":"not-in-this-file","sessionId":"s"}"#,
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.leaf_source, LeafSource::TailLeafAbsent);
    assert_eq!(
        c.leaf_index,
        Some(1),
        "the tail, walked up to a conversation record"
    );
    assert_eq!(c.abandoned_records, 0);
}

#[test]
fn a_recorded_leaf_that_is_an_ancestor_of_the_tail_yields_to_the_tail() {
    // The snapshot names a0; two more records landed after it. The loader prefers the
    // file-order-last record when the recorded leaf is a proper ancestor of it, which is
    // what keeps a resumed session from dropping its own newest turn.
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        r#"{"type":"last-prompt","leafUuid":"a0","sessionId":"s"}"#,
        &u("u1", "a0", "second", "2026-06-07T05:01:00.000Z"),
        &a("a1", "u1", "two", "2026-06-07T05:01:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.leaf_index, Some(4));
    assert_eq!(c.abandoned_records, 0, "the tail turn is not abandoned");
}

#[test]
fn a_compact_boundary_wipes_an_earlier_recorded_leaf() {
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        r#"{"type":"last-prompt","leafUuid":"a0","sessionId":"s"}"#,
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"a0","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto"}}"#,
        &u("u1", "b1", "after", "2026-06-07T05:11:00.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(
        c.leaf_source,
        LeafSource::Tail,
        "the boundary wiped the recorded leafUuid"
    );
    assert_eq!(c.leaf_index, Some(4));
}

#[test]
fn a_non_conversation_leaf_is_walked_up_to_a_conversation_record() {
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        &att("x0", "a0", "2026-06-07T05:00:06.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.leaf_index, Some(1), "the attachment walked up to a0");
    // ...and the trailing attachment is still live: it hangs below the leaf.
    assert_eq!(survivals(&c, r.len()), vec!["live"; 3]);
}

#[test]
fn an_explicit_leaf_hint_overrides_the_recorded_one() {
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        &u("u1", "a0", "second", "2026-06-07T05:01:00.000Z"),
        &a("a1", "u1", "two", "2026-06-07T05:01:05.000Z"),
    ]);
    let c = Chain::build(&r, Some("a0"));
    // a0 is a proper ancestor of the tail, so the tail wins - the same gate the recorded
    // form goes through.
    assert_eq!(c.leaf_index, Some(3));
}

// ── membership beyond ancestry ──

#[test]
fn same_message_id_assistant_siblings_and_their_tool_results_are_live() {
    // Claude Code writes one record per content block, so an assistant message spans
    // several lines sharing a `message.id`. The chain threads through one; the others -
    // and the tool_result carriers parented to them - belong to the same turn.
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"tool_use","id":"t1","name":"Read","input":{}}]}}"#,
        r#"{"type":"assistant","uuid":"a0b","parentUuid":"u0","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"tool_use","id":"t2","name":"Read","input":{}}]}}"#,
        r#"{"type":"user","uuid":"c1","parentUuid":"a0b","timestamp":"2026-06-07T05:00:07.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"ok"}]}}"#,
        r#"{"type":"user","uuid":"c0","parentUuid":"a0","timestamp":"2026-06-07T05:00:08.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
        &a("a1", "c0", "done", "2026-06-07T05:00:09.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(
        survivals(&c, r.len()),
        vec!["live"; 6],
        "the parallel branch is re-interleaved, not abandoned"
    );
}

// ── robustness ──

#[test]
fn a_parent_cycle_stops_the_walk_without_hanging() {
    let r = recs(&[
        &u("u0", "u1", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        &u("u1", "a0", "second", "2026-06-07T05:01:00.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(survivals(&c, r.len()), vec!["live"; 3]);
}

#[test]
fn an_empty_record_set_yields_an_empty_chain() {
    let c = Chain::build(&[], None);
    assert_eq!(c.abandoned_records, 0);
    assert!(c.leaf_index.is_none());
    assert_eq!(c.survival(0), Survival::Live);
}

#[test]
fn a_stepped_over_boundary_reads_the_history_above_it_as_pre_cut() {
    // The boundary names a logicalParentUuid that IS on file, so csift crosses the cut and
    // keeps reading. What it reads is forensic - Claude Code's own walk stopped here - so
    // every record above the boundary is PRE-CUT, never live and never abandoned.
    let r = recs(&[
        &u("u0", "", "old prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "old reply", "2026-06-07T05:00:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"a0","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto"}}"#,
        &u("u1", "b1", "new prompt", "2026-06-07T05:11:00.000Z"),
        &a("a1", "u1", "new reply", "2026-06-07T05:11:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.boundary_cut, Some(2));
    assert_eq!(
        survivals(&c, r.len()),
        vec!["pre-cut", "pre-cut", "live", "live", "live"],
        "the stepped-over history is read and flagged, not dropped and not abandoned"
    );
    assert_eq!(c.abandoned_records, 0);
}

#[test]
fn a_boundary_naming_an_absent_logical_parent_leaves_a_blind_region_that_still_finds_drafts() {
    // MISC-043: the pre-boundary records were deleted from disk, so the step-over has
    // nowhere to land and the walk ends on the boundary. Everything above it is a region
    // csift cannot resolve - PRE-CUT, never abandoned - and the measured same-parent
    // opener rule is what still marks a recalled draft inside it.
    let r = recs(&[
        &u("u0", "", "old prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "old reply", "2026-06-07T05:00:05.000Z"),
        &u("d1", "a0", "draft", "2026-06-07T05:01:00.000Z"),
        &u("s1", "a0", "the resend", "2026-06-07T05:02:00.000Z"),
        &a("a1", "s1", "answered", "2026-06-07T05:02:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"deleted-from-disk","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto"}}"#,
        &u("u1", "b1", "new prompt", "2026-06-07T05:11:00.000Z"),
        &a("a2", "u1", "new reply", "2026-06-07T05:11:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.boundary_cut, Some(5), "the walk ends on the boundary");
    let s = survivals(&c, r.len());
    assert_eq!(
        &s[0..2],
        &["pre-cut", "pre-cut"],
        "the blind region is read, and nothing in it is called abandoned by ancestry"
    );
    assert_eq!(&s[5..8], &["live", "live", "live"]);
    // The draft is still found, by the blind-region rule rather than by the chain.
    assert_eq!(c.survival(2), Survival::Abandoned { root: 2 });
    assert_eq!(
        c.kind(2),
        Some(Kind::Draft {
            superseded_by: Some(3)
        })
    );
    assert_eq!(c.opener_class(2), Some(Class::UserUnsent));
    assert_eq!(c.survival(3), Survival::PreCut, "the resend is not a draft");
}

#[test]
fn an_explicit_null_leaf_empties_the_set_and_the_newest_record_becomes_the_leaf() {
    // `explicit:true` with no leafUuid CLEARS the recorded set. An empty set is not the
    // same as never having had one: the loader has no leaf to resolve, so it falls to the
    // newest record by TIMESTAMP - which is deliberately NOT the file-order tail here.
    // `a2` is a second content block of the same assistant message as `a1` (they share a
    // `message.id`, so it stays on the chain) written with an EARLIER timestamp, so the
    // file-order-last admitted record and the newest-by-timestamp one are different
    // records. Without that separation either rule passes this test.
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        &u("u1", "a0", "second", "2026-06-07T05:09:00.000Z"),
        &a("a1", "u1", "two", "2026-06-07T05:09:05.000Z"),
        r#"{"type":"last-prompt","leafUuid":"a0","sessionId":"s"}"#,
        r#"{"type":"last-prompt","leafUuid":null,"explicit":true,"sessionId":"s"}"#,
        r#"{"type":"assistant","uuid":"a2","parentUuid":"u1","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"assistant","id":"m-a1","content":[{"type":"text","text":"the same message, second block"}]}}"#,
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.leaf_source, LeafSource::Cleared);
    assert_eq!(
        c.leaf_index,
        Some(3),
        "the newest record by TIMESTAMP, not the file-order-last one (index 6)"
    );
    assert_eq!(c.abandoned_records, 0);
    assert_eq!(c.leaf_source.as_str(), "newest (leaf set cleared)");
}

#[test]
fn with_no_tip_at_all_the_leaf_is_the_newest_by_timestamp_not_the_file_tail() {
    // The tips fallback's LAST RESORT, which decides the whole chain on any file where
    // nothing looks like a tip. Every conversation record here is referenced (u0 and z9
    // point at each other), so the tip search yields no candidate; the trailing attachment
    // names a parent this file does not carry, so it is the file-order tail AND climbs to
    // no conversation record. The two rules therefore give different answers: newest by
    // timestamp is z9, the file tail resolves to nothing at all.
    let r = recs(&[
        &u("u0", "z9", "first", "2026-06-07T05:09:00.000Z"),
        &a("z9", "u0", "the newest record", "2026-06-07T05:09:05.000Z"),
        &att("x1", "names-no-record-here", "2026-06-07T05:00:01.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(
        c.leaf_index,
        Some(1),
        "the max-timestamp record; the file tail would resolve to no conversation record"
    );
    assert_eq!(c.survival(0), Survival::Live);
    assert_eq!(c.survival(1), Survival::Live);
}

#[test]
fn a_reserialized_transcript_yields_the_same_chain_as_the_compact_one() {
    // The R13 needle law in the chain's own terms: JSON whitespace is not a record
    // difference. Same records, re-serialized with spaces around every separator (what a
    // python json.dumps default produces), must give the same survival states - the spine
    // and the chain read STRUCTURE, never bytes at fixed offsets.
    let compact = [
        u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        u("d1", "a0", "draft", "2026-06-07T05:01:00.000Z"),
        u("u1", "a0", "resend", "2026-06-07T05:02:00.000Z"),
        a("a1", "u1", "two", "2026-06-07T05:02:05.000Z"),
    ];
    let spaced: Vec<String> = compact
        .iter()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).expect("valid record");
            serde_json::to_string_pretty(&v).expect("re-serializable")
        })
        .collect();
    let a_chain = Chain::build(
        &recs(&compact.iter().map(String::as_str).collect::<Vec<_>>()),
        None,
    );
    let b_chain = Chain::build(
        &recs(&spaced.iter().map(String::as_str).collect::<Vec<_>>()),
        None,
    );
    assert_eq!(survivals(&a_chain, 5), survivals(&b_chain, 5));
    assert_eq!(a_chain.kind(2), b_chain.kind(2));
    assert_eq!(a_chain.leaf_index, b_chain.leaf_index);
    // ...and the SPINE agrees on the same lines, which is where a byte-pair needle would
    // have silently dropped the record.
    for (n, (c, s)) in compact.iter().zip(&spaced).enumerate() {
        let cs = crate::parse::spine_record(n + 1, c.as_bytes()).expect("compact spine");
        let ss = crate::parse::spine_record(n + 1, s.as_bytes()).expect("spaced spine");
        assert_eq!(cs.uuid, ss.uuid);
        assert_eq!(cs.parent_uuid, ss.parent_uuid);
        assert_eq!(cs.kind_str(), ss.kind_str());
    }
}

#[test]
fn a_sidechain_record_is_outside_the_axis() {
    let r = recs(&[
        &u("u0", "", "first", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "one", "2026-06-07T05:00:05.000Z"),
        r#"{"type":"user","uuid":"s0","parentUuid":"nowhere","isSidechain":true,"timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":"a subagent lane"}}"#,
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(
        c.survival(2),
        Survival::Live,
        "a sidechain lane is never called abandoned"
    );
}

// -- the loader hops that need their own fixtures --
mod branches;
mod leafgates;
mod nodes;
mod repair;
