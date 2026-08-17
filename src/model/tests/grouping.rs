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

// ── §6.4.1 esc-cancel / edit-resend DRAFT SUPPRESSION ──
// Real shape (verified on ~/.claude/projects): the user submits, ESC-cancels or edits,
// and resends; CC appends EACH draft as its own genuine `type:"user"` record sharing the
// SAME `parentUuid`. Only the last in file order reached the model. `superseded_draft_indices`
// marks the earlier siblings; `group_turn_indices_deduped` drops them so they never become
// phantom turns. None of these patterns are reachable through bool fixtures — they need the
// real uuid/parentUuid tree, so they parse genuine record JSON.

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

    let sup = superseded_draft_indices(&records, |r| r);
    assert_eq!(
        sup.len(),
        2,
        "the two earlier same-parent drafts are superseded"
    );
    assert!(
        sup.contains(&2) && sup.contains(&3),
        "drafts d1,d2 superseded; u1 (last in file order) survives"
    );
    assert!(!sup.contains(&4));

    let turns = group_turn_indices_deduped(&records, |r| r);
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
        group_turn_indices_deduped(&records, |r| r),
        vec![vec![0, 3]],
        "3 identical drafts → 1 turn (opener idx3)"
    );
}

#[test]
fn superseded_drafts_distinct_parents_not_merged() {
    // Two identical-content user records with DIFFERENT parents are two real turns —
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
    assert!(superseded_draft_indices(&records, |r| r).is_empty());
    assert_eq!(
        group_turn_indices_deduped(&records, |r| r),
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
    assert!(superseded_draft_indices(&records, |r| r).is_empty());
    assert_eq!(
        group_turn_indices_deduped(&records, |r| r),
        vec![vec![0], vec![1]]
    );
}

#[test]
fn deduped_grouping_matches_plain_when_no_drafts() {
    // With no same-parent draft siblings, deduped grouping is identical to the plain
    // delimiter — a regression guard on the shared core.
    let records: Vec<Record> = [
            r#"{"type":"user","uuid":"u0","parentUuid":"r","message":{"role":"user","content":"q1"}}"#,
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","message":{"role":"assistant","content":[{"type":"text","text":"r1"}]}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","message":{"role":"user","content":"q2"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
    let plain = group_turn_indices(&records, |r| r.opens_turn());
    let deduped = group_turn_indices_deduped(&records, |r| r);
    assert_eq!(plain, deduped);
    assert_eq!(deduped, vec![vec![0, 1], vec![2]]);
}
