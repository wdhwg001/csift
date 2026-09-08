//! Which record the chain starts from, and what a compaction boundary or a
//! re-appended block does to it.

use super::*;

#[test]
fn a_replayed_block_marks_the_earlier_line_and_keeps_both_searchable() {
    // A compaction re-anchor re-appends a block with its uuids preserved. The loader's map
    // holds the LAST line, so the earlier one is the copy - the opposite of the v0.11.1
    // rule, which kept the first.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"promptId":"p1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u0","parentUuid":null,"promptId":"p2","timestamp":"2026-06-07T05:10:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:10:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["search", "lagoon", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("[replay copy of L3]"),
        "the earlier line names its survivor:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("2 replay copy line(s)"),
        "the footer discloses them:\n{}",
        out.stdout
    );
    let j = h.run(&["search", "lagoon", &at(SESS), "--format", "json"]);
    let hits: Vec<serde_json::Value> = rows(&j.stdout)
        .into_iter()
        .filter(|v| v["kind"] == "exchange")
        .flat_map(|v| v["hits"].as_array().cloned().unwrap_or_default())
        .collect();
    let early = hits
        .iter()
        .find(|h| h["line"] == 1)
        .expect("the earlier line");
    assert_eq!(early["replay_copy_of"], 3, "{}", j.stdout);
    assert_eq!(
        early["label"], "user.message",
        "a copy is not a draft: {}",
        j.stdout
    );
    assert_eq!(summary(&j.stdout)["superseded_drafts"], 0, "{}", j.stdout);
}

#[test]
fn a_boundary_cut_is_disclosed_and_the_history_above_it_stays_selectable() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preTokens":900}}"#, "\n",
            r#"{"type":"user","uuid":"s1","parentUuid":"b1","isCompactSummary":true,"timestamp":"2026-06-07T05:10:01.000Z","message":{"role":"user","content":"summary of the lagoon work"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"s1","timestamp":"2026-06-07T05:11:00.000Z","message":{"role":"user","content":"survey the southern shoal"}}"#, "\n",
        ),
    );
    let out = h.run(&["search", "", &at(SESS)]);
    assert!(
        out.stdout.contains("cut at the compaction boundary L3"),
        "the cut is stated:\n{}",
        out.stdout
    );
    // csift keeps reading above the cut: a bare role still surfaces the pre-cut prompt.
    let role = h.run(&["search", "", &at(SESS), "-t", "user"]);
    assert!(
        role.stdout.contains("chart the lagoon") && role.stdout.contains("southern shoal"),
        "a pre-cut record is flagged, never hidden:\n{}",
        role.stdout
    );
    let j = h.run(&["search", "", &at(SESS), "-t", "user.*", "--format", "json"]);
    let hits: Vec<serde_json::Value> = rows(&j.stdout)
        .into_iter()
        .filter(|v| v["kind"] == "exchange")
        .flat_map(|v| v["hits"].as_array().cloned().unwrap_or_default())
        .collect();
    let pre = hits
        .iter()
        .find(|h| h["line"] == 1)
        .expect("the pre-cut prompt");
    assert_eq!(pre["survival"], "pre-cut", "{}", j.stdout);
    assert_eq!(summary(&j.stdout)["boundary_cut_line"], 3, "{}", j.stdout);
    assert_eq!(summary(&j.stdout)["abandoned_records"], 0, "{}", j.stdout);
}

#[test]
fn a_preserved_messages_boundary_keeps_the_listed_records() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"user","uuid":"k1","parentUuid":"a0","timestamp":"2026-06-07T05:05:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"k2","parentUuid":"k1","timestamp":"2026-06-07T05:05:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging"}]}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preservedMessages":{"anchorUuid":"a0","uuids":["k1","k2"]}}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"k2","timestamp":"2026-06-07T05:11:00.000Z","message":{"role":"user","content":"survey the southern shoal"}}"#, "\n",
        ),
    );
    let j = h.run(&["search", "", &at(SESS), "-t", "user.*", "--format", "json"]);
    let hits: Vec<serde_json::Value> = rows(&j.stdout)
        .into_iter()
        .filter(|v| v["kind"] == "exchange")
        .flat_map(|v| v["hits"].as_array().cloned().unwrap_or_default())
        .collect();
    let cut = hits
        .iter()
        .find(|h| h["line"] == 1)
        .expect("the cut prompt");
    assert_eq!(
        cut["survival"], "pre-cut",
        "outside the preserved set: {}",
        j.stdout
    );
    let kept = hits
        .iter()
        .find(|h| h["line"] == 3)
        .expect("the preserved prompt");
    assert_eq!(
        kept["survival"], "live",
        "a preserved record survives: {}",
        j.stdout
    );
    assert_eq!(summary(&j.stdout)["superseded_drafts"], 0, "{}", j.stdout);
}

#[test]
fn a_last_prompt_leaf_naming_no_record_falls_back_to_the_tail_and_says_so() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"user","uuid":"d1","parentUuid":"a0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the channel"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging"}]}}"#, "\n",
            r#"{"type":"last-prompt","leafUuid":"not-in-this-file","sessionId":"s","lastPrompt":"dredge the northern channel"}"#, "\n",
        ),
    );
    let j = h.run(&["search", "", &at(SESS), "--format", "json"]);
    let s = summary(&j.stdout);
    assert_eq!(
        s["leaf_source"], "tail (last-prompt leaf absent)",
        "{}",
        j.stdout
    );
    // The draft is still found: the fallback leaf reaches the same conversation.
    assert_eq!(s["superseded_drafts"], 1, "{}", j.stdout);
    let out = h.run(&["search", "", &at(SESS)]);
    assert!(
        out.stdout
            .contains("chain leaf: tail (last-prompt leaf absent)"),
        "the miss is stated, never silent:\n{}",
        out.stdout
    );
}

#[test]
fn a_last_prompt_leaf_decides_which_branch_is_the_conversation() {
    // Naming the REWOUND branch's tail as the leaf makes that branch the conversation and
    // the later resend the abandoned one. The leaf is what decides, so csift names it.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"a0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"survey the southern shoal"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"surveying"}]}}"#, "\n",
            r#"{"type":"last-prompt","leafUuid":"a1","sessionId":"s","lastPrompt":"dredge the northern channel"}"#, "\n",
        ),
    );
    let j = h.run(&["search", "", &at(SESS), "--format", "json"]);
    assert_eq!(
        summary(&j.stdout)["leaf_source"],
        "last-prompt",
        "{}",
        j.stdout
    );
    let role = h.run(&["search", "", &at(SESS), "-t", "user"]);
    assert!(
        role.stdout.contains("dredge the northern channel")
            && !role.stdout.contains("southern shoal"),
        "the pinned branch is the conversation:\n{}",
        role.stdout
    );
}
