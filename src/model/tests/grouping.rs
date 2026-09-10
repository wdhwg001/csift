//! Turn grouping via opens_turn and superseded-draft dedup.

use super::*;

// ── group_turn_indices (the shared §6.4 turn delimiter) ──

#[test]
fn group_turn_indices_basic_two_turns() {
    // bools = is_genuine_user; [user, member, member, user, member].
    let flags = [true, false, false, true, false];
    let turns = group_turn_indices(&flags, |b| *b);
    assert_eq!(turns, vec![vec![0, 1, 2], vec![3, 4]]);
}

#[test]
fn group_turn_indices_synthetic_lead_folds_into_first_turn() {
    // Leading non-genuine records (a synthetic lead) fold into turn 0 when a real
    // user follows, so indices stay 0-based on genuine users.
    let flags = [false, false, true, false, true];
    let turns = group_turn_indices(&flags, |b| *b);
    // The two lead members (0,1) join the first real turn (opening at idx 2).
    assert_eq!(turns, vec![vec![0, 1, 2, 3], vec![4]]);
}

#[test]
fn group_turn_indices_only_synthetic_lead_no_genuine_user() {
    // No genuine user ever → a single standalone turn 0 holding the orphans (the
    // `turns.len() > 1` false guard means no fold).
    let flags = [false, false, false];
    let turns = group_turn_indices(&flags, |b| *b);
    assert_eq!(turns, vec![vec![0, 1, 2]]);
}

#[test]
fn group_turn_indices_empty_is_empty() {
    let flags: [bool; 0] = [];
    assert!(group_turn_indices(&flags, |b| *b).is_empty());
}

#[test]
fn group_turn_indices_first_record_genuine_no_fold() {
    // The first record IS genuine → no synthetic lead → no fold.
    let flags = [true, false, true];
    let turns = group_turn_indices(&flags, |b| *b);
    assert_eq!(turns, vec![vec![0, 1], vec![2]]);
}

#[test]
fn opens_turn_grouping_splits_on_auq_answer_and_skips_interrupt() {
    // A realistic record sequence over the PRODUCTION predicate (`opens_turn`):
    //   0 genuine user      → turn 0 opener
    //   1 assistant AUQ tool_use (member of turn 0)
    //   2 AUQ answer carrier → turn 1 opener (the behavior change)
    //   3 assistant reply   (member of turn 1)
    //   4 interrupt marker  → NOT a boundary (member of turn 1)
    //   5 genuine user      → turn 2 opener
    let records: Vec<Record> = [
            r#"{"type":"user","message":{"role":"user","content":"pick one"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
            r#"{"type":"user","toolUseResult":{"answers":{"which?":"the bold one"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"the bold one\"."}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":"next question"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
    let turns = group_turn_indices(&records, |r| r.opens_turn());
    assert_eq!(
        turns,
        vec![vec![0, 1], vec![2, 3, 4], vec![5]],
        "AUQ answer opens turn 1; interrupt is a member, not a boundary"
    );
}

// ── §6.4.1 under the SURVIVAL AXIS ──
// v0.12.0: the three opener heuristics are gone; `group_turn_indices_deduped` now asks
// Claude Code's own conversation chain which records the surviving conversation still
// reaches, and drops the rest. These cases keep their old shapes and their old ANSWERS -
// the chain reduces to the same result on a same-parent resend - and the per-record
// verdicts they used to assert through `superseded_draft_indices` / `collapse_openers`
// now live in `chain.rs`, which pins the whole DAG matrix. None of these patterns are
// reachable through bool fixtures: they need the real uuid/parentUuid tree.

#[test]
fn superseded_drafts_collapse_same_parent_edit_resend() {
    // u0 (parent root) → assistant a0 → THREE drafts of one turn under parent a0:
    // "draft v1" → edited "draft v2" → "draft v2, with a tail" (the one that continued) → assistant a1.
    // Only the last sibling survives; the two earlier ones are superseded drafts.
    let records: Vec<Record> = [
            r#"{"type":"user","uuid":"u0","parentUuid":"root","message":{"role":"user","content":"start"}}"#,
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"user","uuid":"d1","parentUuid":"a0","message":{"role":"user","content":"draft v1"}}"#,
            r#"{"type":"user","uuid":"d2","parentUuid":"a0","message":{"role":"user","content":"draft v2"}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","message":{"role":"user","content":"draft v2, with a tail"}}"#,
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","message":{"role":"assistant","content":[{"type":"text","text":"noted"}]}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();

    let chain = Chain::build(&records, None);
    assert_eq!(
        chain.drafts, 2,
        "the two earlier same-parent drafts are abandoned"
    );
    assert_eq!(
        chain.opener_class(2),
        Some(Class::UserUnsent),
        "d1 is a recalled draft"
    );
    assert_eq!(chain.opener_class(3), Some(Class::UserUnsent));
    assert!(chain.opener_class(4).is_none(), "u1 survives");
    assert_eq!(
        chain.superseding(2),
        Some(4),
        "both map to the final survivor"
    );
    assert_eq!(chain.superseding(3), Some(4));

    let turns = group_turn_indices_deduped(&records, |r| ChainNode::Full(r));
    assert_eq!(
        turns,
        vec![vec![0, 1], vec![4, 5]],
        "two real turns; abandoned drafts vanish entirely (neither boundary nor member)"
    );
}

#[test]
fn superseded_drafts_exact_duplicate_collapses_to_one() {
    // The same message appears 3× verbatim under one parent (ESC-cancel re-submits) →
    // exactly one turn, not three.
    let records: Vec<Record> = [
            r#"{"type":"assistant","uuid":"a0","parentUuid":"root","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
            r#"{"type":"user","uuid":"d1","parentUuid":"a0","message":{"role":"user","content":"the same resent line"}}"#,
            r#"{"type":"user","uuid":"d2","parentUuid":"a0","message":{"role":"user","content":"the same resent line"}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","message":{"role":"user","content":"the same resent line"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
    // Leading assistant (idx0) is a synthetic lead that folds into the first real turn.
    assert_eq!(
        group_turn_indices_deduped(&records, |r| ChainNode::Full(r)),
        vec![vec![0, 3]],
        "3 identical drafts → 1 turn (opener idx3)"
    );
}

#[test]
fn superseded_drafts_distinct_parents_not_merged() {
    // Two identical-content user records with DIFFERENT parents are two real turns -
    // distinct turns legitimately share content but never a parentUuid (each is parented
    // to the assistant message that preceded it).
    let records: Vec<Record> = [
            r#"{"type":"user","uuid":"u0","parentUuid":"a0","message":{"role":"user","content":"continue"}}"#,
            r#"{"type":"assistant","uuid":"x","parentUuid":"u0","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"x","message":{"role":"user","content":"continue"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
    assert_eq!(Chain::build(&records, None).drafts, 0);
    assert_eq!(
        group_turn_indices_deduped(&records, |r| ChainNode::Full(r)),
        vec![vec![0, 1], vec![2]]
    );
}

#[test]
fn superseded_drafts_null_parent_never_grouped() {
    // No parentUuid → never grouped (grouping on "no parent" would merge unrelated
    // first-message records). In real data a genuine user always carries a parent.
    let records: Vec<Record> = [
        r#"{"type":"user","uuid":"u0","message":{"role":"user","content":"a"}}"#,
        r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"b"}}"#,
    ]
    .iter()
    .map(|l| parse(l))
    .collect();
    assert_eq!(Chain::build(&records, None).drafts, 0);
    assert_eq!(
        group_turn_indices_deduped(&records, |r| ChainNode::Full(r)),
        vec![vec![0], vec![1]]
    );
}

// ── the compaction RE-ANCHOR replay ──
// A compaction re-anchor re-appends a contiguous block of records with their uuids
// PRESERVED (the copies differ in `promptId` alone). The copy is the same logical record,
// so it must not mark the original as an abandoned draft and must not open a second turn.

#[test]
fn a_replayed_same_uuid_opener_is_not_a_draft() {
    // u1 is re-appended after a compaction boundary with the same uuid/parentUuid and a
    // fresh promptId. Before the guard, the copy looked like a later sibling and made the
    // ORIGINAL - a message that was sent and answered - a user.unsent draft.
    let records: Vec<Record> = [
            r#"{"type":"assistant","uuid":"a0","parentUuid":"root","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","promptId":"p1","message":{"role":"user","content":"chart the reef"}}"#,
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","message":{"role":"assistant","content":[{"type":"text","text":"charted"}]}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","promptId":"p2","message":{"role":"user","content":"chart the reef"}}"#,
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","message":{"role":"assistant","content":[{"type":"text","text":"charted"}]}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
    // v0.12.0 flips the DIRECTION and keeps the effect: the loader's map holds the LAST
    // line carrying a uuid, so index 3 is the survivor and index 1 is the earlier copy.
    let c = Chain::build(&records, None);
    assert_eq!(c.drafts, 0, "a replayed copy supersedes nothing");
    assert_eq!(c.replay_of(1), Some(3), "index 1 is the earlier copy");
    assert!(!c.opens(1), "the earlier copy never opens a turn");
    assert!(c.opens(3));
    // ONE turn for the message, and the earlier copy stays a member rather than vanishing.
    assert_eq!(
        group_turn_indices_deduped(&records, |r| ChainNode::Full(r)),
        vec![vec![0, 1, 2, 3, 4]],
        "one turn; the earlier copy folds in as a member"
    );
}

#[test]
fn a_genuine_edit_resend_is_still_a_draft_when_the_uuids_differ() {
    // The guard keys on uuid IDENTITY, so an ordinary esc-edit-resend (distinct uuids
    // under one parent) is unaffected.
    let records: Vec<Record> = [
            r#"{"type":"assistant","uuid":"a0","parentUuid":"root","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
            r#"{"type":"user","uuid":"d1","parentUuid":"a0","message":{"role":"user","content":"chart the reef"}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","message":{"role":"user","content":"chart the reef and the harbor"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
    let c = Chain::build(&records, None);
    assert_eq!(c.superseding(1), Some(2), "d1 -> its survivor u1");
    assert_eq!(c.opener_class(1), Some(Class::UserUnsent));
    assert_eq!(c.replay_copies, 0);
}

#[test]
fn a_replayed_pair_still_marks_a_later_real_resend() {
    // The two corrections coexist: u1 is replayed (no draft), and a genuine edit-resend
    // under the SAME parent afterwards still marks its own earlier sibling.
    let records: Vec<Record> = [
            r#"{"type":"assistant","uuid":"a0","parentUuid":"root","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","promptId":"p1","message":{"role":"user","content":"first"}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","promptId":"p2","message":{"role":"user","content":"first"}}"#,
            r#"{"type":"user","uuid":"d2","parentUuid":"a0","message":{"role":"user","content":"second draft"}}"#,
            r#"{"type":"user","uuid":"u2","parentUuid":"a0","message":{"role":"user","content":"second sent"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
    let c = Chain::build(&records, None);
    assert_eq!(c.replay_of(1), Some(2), "index 1 is the earlier copy of u1");
    // The surviving u1 line (idx2) is itself replaced by the real later sibling u2, and
    // d2 (idx3) is too. The earlier copy is neither a draft nor a survivor.
    assert_eq!(c.superseding(3), Some(4), "d2 -> u2");
    assert!(
        c.opener_class(1).is_none(),
        "the earlier replay copy is never a draft"
    );
}

#[test]
fn deduped_grouping_matches_plain_when_no_drafts() {
    // With no same-parent draft siblings, deduped grouping is identical to the plain
    // delimiter - a regression guard on the shared core.
    let records: Vec<Record> = [
            r#"{"type":"user","uuid":"u0","parentUuid":"r","message":{"role":"user","content":"q1"}}"#,
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","message":{"role":"assistant","content":[{"type":"text","text":"r1"}]}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","message":{"role":"user","content":"q2"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
    let plain = group_turn_indices(&records, |r| r.opens_turn());
    let deduped = group_turn_indices_deduped(&records, |r| ChainNode::Full(r));
    assert_eq!(plain, deduped);
    assert_eq!(deduped, vec![vec![0, 1], vec![2]]);
}
