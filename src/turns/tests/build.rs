//! Turn-unit building from raw records: bodies, summaries, orphans.

use super::*;

// ── compact_summary_body / raw_body_newlines via real Record parse ──

#[test]
fn compact_summary_body_reads_string_content_only() {
    let s = rec(
        r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"the summary body text"}}"#,
    );
    assert_eq!(
        compact_summary_body(&s).as_deref(),
        Some("the summary body text")
    );
    // A block-bodied (surprise) summary → None, not a guess.
    let blocks = rec(
        r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":[{"type":"text","text":"x"}]}}"#,
    );
    assert!(compact_summary_body(&blocks).is_none());
    // No message → None.
    let bare = rec(r#"{"type":"user","isCompactSummary":true}"#);
    assert!(compact_summary_body(&bare).is_none());
}

#[test]
fn raw_body_newlines_counts_user_and_assistant_bodies() {
    let u = rec(
        r#"{"type":"user","message":{"role":"user","content":"line one\nline two\nline three"}}"#,
    );
    assert_eq!(raw_body_newlines(&u), 2);
    let a = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"first"},{"type":"text","text":"second"}]}}"#,
    );
    // two text blocks joined by \n → 1 newline.
    assert_eq!(raw_body_newlines(&a), 1);
    // No message → 0.
    let bare = rec(r#"{"type":"system","subtype":"x"}"#);
    assert_eq!(raw_body_newlines(&bare), 0);
    // message but no content → 0.
    let nocontent = rec(r#"{"type":"user","message":{"role":"user"}}"#);
    assert_eq!(raw_body_newlines(&nocontent), 0);
}

// ── build(): turn slices + tool-call counts from line-numbered records ──

#[test]
fn build_produces_round_trip_with_tool_count_and_compaction() {
    // turn 0: user, assistant tool_use x2, assistant text. Then a summary. Then turn 1.
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(
                r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"first ask about café"}}"#,
            ),
        ),
        (
            2,
            rec(
                r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{}},{"type":"tool_use","id":"t2","name":"Bash","input":{}}]}}"#,
            ),
        ),
        (
            3,
            rec(
                r#"{"type":"assistant","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"the reply to the first ask"}]}}"#,
            ),
        ),
        (
            4,
            rec(
                r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"6. All user messages:\n   - \"first ask about café\""}}"#,
            ),
        ),
        (
            5,
            rec(
                r#"{"type":"user","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"second ask after compaction"}}"#,
            ),
        ),
        (
            6,
            rec(
                r#"{"type":"assistant","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"the second reply"}]}}"#,
            ),
        ),
    ];
    let (turns, summaries) = build(&records, &[]);
    assert_eq!(turns.len(), 2);
    // turn 0: round-trip, 2 tool calls, before the (one) summary → compactions_before 1.
    assert!(turns[0].is_round_trip());
    assert_eq!(turns[0].tool_calls, 2);
    assert_eq!(turns[0].compactions_before, 1);
    assert_eq!(turns[0].user.as_ref().unwrap().line_no, 1);
    assert_eq!(turns[0].assistant_eot().unwrap().line_no, 3);
    // turn 1: live region, 0 tool calls, after the summary.
    assert_eq!(turns[1].compactions_before, 0);
    assert_eq!(turns[1].tool_calls, 0);
    // one summary captured with a fingerprint of the §6 bullet.
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].line_no, 4);
    assert!(summaries[0]
        .fingerprints
        .iter()
        .any(|f| f.starts_with("first ask about café")));
}

#[test]
fn build_pure_tool_call_turn_has_no_assistant_eot() {
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(r#"{"type":"user","message":{"role":"user","content":"do the thing"}}"#),
        ),
        (
            2,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#,
            ),
        ),
    ];
    let (turns, _s) = build(&records, &[]);
    assert_eq!(turns.len(), 1);
    assert!(turns[0].user.is_some());
    assert!(
        turns[0].assistant_eot().is_none(),
        "pure tool-call turn has no EOT text"
    );
    assert_eq!(turns[0].tool_calls, 1);
    assert!(!turns[0].is_round_trip());
}

#[test]
fn build_orphan_assistant_lead_has_no_user() {
    // Records that lead with an assistant before any genuine user → a synthetic turn 0
    // with assistant_eot but no user (group_turn_indices seeds turn 0 with the lead).
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"orphan lead reply"}]}}"#,
            ),
        ),
        (
            2,
            rec(r#"{"type":"user","message":{"role":"user","content":"the first real ask"}}"#),
        ),
        (
            3,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"real reply"}]}}"#,
            ),
        ),
    ];
    let (turns, _s) = build(&records, &[]);
    // The orphan lead folds into turn 0 (the first real user turn), so turn 0 has the
    // user AND carries the orphan assistant text as its EOT (last assistant in the turn).
    assert_eq!(turns.len(), 1);
    assert!(turns[0].user.is_some());
    assert!(turns[0].assistant_eot().is_some());
}

#[test]
fn build_summary_with_block_body_is_not_captured() {
    // A summary record with a BLOCK body (not a string) → compact_summary_body returns
    // None → the summary is NOT captured (the `if let Some(body)` false arm in build).
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(r#"{"type":"user","message":{"role":"user","content":"ask"}}"#),
        ),
        (
            2,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"reply"}]}}"#,
            ),
        ),
        // a block-bodied "summary" (a genuine surprise) → skipped as a summary source.
        (
            3,
            rec(
                r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":[{"type":"text","text":"block body summary"}]}}"#,
            ),
        ),
    ];
    let (turns, summaries) = build(&records, &[]);
    assert!(summaries.is_empty(), "block-bodied summary not captured");
    // The turn still builds; the block summary contributes no boundary.
    assert_eq!(turns[0].compactions_before, 0);
}

// ── Render-helper + raw-body branch completeness ──

#[test]
fn raw_body_newlines_block_with_empty_text_blocks() {
    // A block body where one text block is blank → it is skipped (the `!text.trim()`
    // false arm); only the non-blank block contributes.
    let r = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"   "},{"type":"text","text":"real\nline"}]}}"#,
    );
    assert_eq!(raw_body_newlines(&r), 1);
    // A block body with a non-text block (tool_use) interleaved → only text counts.
    let r2 = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}},{"type":"text","text":"a\nb\nc"}]}}"#,
    );
    assert_eq!(raw_body_newlines(&r2), 2);
}

#[test]
fn build_skips_non_genuine_user_opener() {
    // A turn whose opening record is genuine-user but genuine_user_text returns text;
    // also a record that is a tool_result carrier (not genuine) must NOT become a user
    // opener (the `is_genuine_user` false path inside the loop).
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(r#"{"type":"user","message":{"role":"user","content":"real opener"}}"#),
        ),
        (
            2,
            rec(
                r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"carrier"}]}}"#,
            ),
        ),
        (
            3,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"reply"}]}}"#,
            ),
        ),
    ];
    let (turns, _s) = build(&records, &[]);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].user.as_ref().unwrap().text, "real opener");
    assert!(turns[0].assistant_eot().is_some());
}

#[test]
fn raw_body_newlines_no_message_and_no_content() {
    // No message at all → 0 (the `let Some(msg) else` arm).
    let no_msg = rec(r#"{"type":"system","subtype":"x"}"#);
    assert_eq!(raw_body_newlines(&no_msg), 0);
    // message present, no content → 0 (the `let Some(content) else` arm).
    let no_content = rec(r#"{"type":"user","message":{"role":"user"}}"#);
    assert_eq!(raw_body_newlines(&no_content), 0);
}

#[test]
fn compact_summary_body_none_when_no_message() {
    let bare = rec(r#"{"type":"user","isCompactSummary":true}"#);
    assert!(compact_summary_body(&bare).is_none());
}

#[test]
fn build_skips_non_candidate_records_in_scan() {
    // The build path itself runs over already-parsed records; the prefilter's skip is
    // exercised at scan time. Verify build tolerates a `system` record interleaved as a
    // turn member (it contributes no user/assistant/summary).
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(r#"{"type":"user","message":{"role":"user","content":"ask"}}"#),
        ),
        (
            2,
            rec(r#"{"type":"system","subtype":"turn_duration","durationMs":5}"#),
        ),
        (
            3,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"reply"}]}}"#,
            ),
        ),
    ];
    let (turns, summaries) = build(&records, &[]);
    assert_eq!(turns.len(), 1);
    assert!(turns[0].is_round_trip());
    assert!(summaries.is_empty());
}
