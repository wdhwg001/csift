//! The resume repair pair: `harness.resume.prompt` + `harness.resume.placeholder`.
//!
//! Fixture shapes are the live receipt: the loader appends the prompt after the dangling
//! user record, then splices the placeholder in as its child, both with one timestamp.

use super::*;
use std::collections::HashSet;

/// The prompt as the loader writes it: `isMeta`, a single text block, the bare literal.
const PROMPT: &str = r#"{"type":"user","uuid":"p1","parentUuid":"att1","isMeta":true,"promptId":"pr-1","userType":"external","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":[{"type":"text","text":"Continue from where you left off."}]}}"#;

/// The placeholder as the splice writes it: the `<synthetic>` model, `isApiErrorMessage`
/// false, `stop_reason` a stop_sequence, the bare literal, parented on the prompt.
const PLACEHOLDER: &str = r#"{"type":"assistant","uuid":"h1","parentUuid":"p1","isApiErrorMessage":false,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"assistant","model":"<synthetic>","stop_reason":"stop_sequence","content":[{"type":"text","text":"No response requested."}]}}"#;

fn ctx_with(prompt_uuids: &HashSet<String>) -> ClassifyCtx<'_> {
    ClassifyCtx {
        resume_prompt_uuids: Some(prompt_uuids),
        ..ClassifyCtx::top_level()
    }
}

fn uuid_set(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn the_pair_classifies_under_its_own_leaves() {
    let prompt = parse(PROMPT);
    let placeholder = parse(PLACEHOLDER);
    let prompts = uuid_set(&["p1"]);
    let ctx = ctx_with(&prompts);
    assert_eq!(prompt.classify(&ctx), vec![Class::ResumePrompt]);
    // The placeholder takes the harness leaf INSTEAD of agent.message: the `<synthetic>`
    // model says Claude Code fabricated it, so the model never wrote that text.
    assert_eq!(placeholder.classify(&ctx), vec![Class::ResumePlaceholder]);
    assert!(prompt.is_resume_prompt());
    assert!(placeholder.is_resume_placeholder());
}

#[test]
fn neither_half_opens_a_turn() {
    // The prompt is isMeta, so `is_genuine_user` (and every other opener case) excludes it;
    // the placeholder is an assistant record, which opens no turn by construction. A
    // resumed session's human-turn count is therefore unchanged by the repair.
    let prompt = parse(PROMPT);
    assert!(!prompt.is_genuine_user());
    assert!(!prompt.opens_turn());
    assert!(prompt.reconstructed_user_text(None).is_none());
    let placeholder = parse(PLACEHOLDER);
    assert!(!placeholder.is_genuine_user());
    assert!(!placeholder.opens_turn());
}

#[test]
fn both_halves_are_delivered_and_take_no_override() {
    // The pair exists so the loaded conversation reaches the model well-formed, so both
    // must read as delivered. The `<synthetic>` API-error override does NOT fire on the
    // placeholder: that arm needs `isApiErrorMessage` true, and the loader mints this
    // record without it.
    let prompt = parse(PROMPT);
    let placeholder = parse(PLACEHOLDER);
    assert_eq!(prompt.delivery_override(), None);
    assert_eq!(placeholder.delivery_override(), None);
    assert!(Class::ResumePrompt.llm_visible());
    assert!(Class::ResumePlaceholder.llm_visible());
    // The neighbouring shape that DOES take the override, so the two stay distinguishable.
    let api_error = parse(
        r#"{"type":"assistant","isApiErrorMessage":true,"message":{"role":"assistant","model":"<synthetic>","content":[{"type":"text","text":"API Error: overloaded"}]}}"#,
    );
    assert_eq!(api_error.delivery_override(), Some(false));
}

#[test]
fn pairing_is_by_parent_and_never_guessed() {
    let placeholder = parse(PLACEHOLDER);
    let prompts = uuid_set(&["p1"]);
    assert_eq!(placeholder.resume_paired(&ctx_with(&prompts)), Some(true));
    // Same record, a file whose prompts do not include its parent: unpaired, not unknown.
    let others = uuid_set(&["somewhere-else"]);
    assert_eq!(placeholder.resume_paired(&ctx_with(&others)), Some(false));
    // No index at all: no verdict rather than a fabricated `false`.
    assert_eq!(placeholder.resume_paired(&ClassifyCtx::top_level()), None);
    // A non-placeholder never carries the fact.
    assert_eq!(parse(PROMPT).resume_paired(&ctx_with(&prompts)), None);
}

#[test]
fn an_unpaired_placeholder_keeps_the_leaf() {
    // The SAME splice writes a placeholder after any trailing user record, so its parent is
    // often an interrupt marker rather than a resume prompt. It is still loader text, so it
    // still takes the leaf - only the pair FACT differs.
    let after_interrupt = parse(
        r#"{"type":"assistant","uuid":"h2","parentUuid":"int1","isApiErrorMessage":false,"message":{"role":"assistant","model":"<synthetic>","content":[{"type":"text","text":"No response requested."}]}}"#,
    );
    let prompts = uuid_set(&["p1"]);
    let ctx = ctx_with(&prompts);
    assert_eq!(
        after_interrupt.classify(&ctx),
        vec![Class::ResumePlaceholder]
    );
    assert_eq!(after_interrupt.resume_paired(&ctx), Some(false));
}

#[test]
fn a_real_assistant_turn_is_never_the_placeholder() {
    // The model sentinel is the load-bearing half. A genuine reply that happens to say the
    // same sentence carries a real model id and stays agent.message.
    let real = parse(
        r#"{"type":"assistant","message":{"role":"assistant","model":"claude-opus-4-8","content":[{"type":"text","text":"No response requested."}]}}"#,
    );
    assert!(!real.is_resume_placeholder());
    assert_eq!(
        real.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentMessage]
    );
    // An API-error notice shares the sentinel model but is not the placeholder text.
    let notice = parse(
        r#"{"type":"assistant","isApiErrorMessage":true,"message":{"role":"assistant","model":"<synthetic>","content":[{"type":"text","text":"No response requested."}]}}"#,
    );
    assert!(!notice.is_resume_placeholder());
    // Multi-block content is not the single-text shape the producer's recogniser reads.
    let two_blocks = parse(
        r#"{"type":"assistant","message":{"role":"assistant","model":"<synthetic>","content":[{"type":"text","text":"No response requested."},{"type":"text","text":"and more"}]}}"#,
    );
    assert!(!two_blocks.is_resume_placeholder());
}

#[test]
fn prompt_text_that_differs_by_one_char_carries_no_label() {
    // The isMeta pseudo-turn rule: an isMeta record matching NO harness marker classifies
    // EMPTY rather than user.message, so a near-miss never becomes the human either.
    let near = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off"}}"#,
    );
    assert!(!near.is_resume_prompt());
    assert!(near.classify(&ClassifyCtx::top_level()).is_empty());
    // Leading whitespace is tolerated (the arm trims the start), a different sentence is not.
    let padded = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"  Continue from where you left off."}}"#,
    );
    assert!(padded.is_resume_prompt());
}

#[test]
fn a_human_message_that_starts_with_the_sentence_stays_the_human() {
    // The isMeta gate is the one that protects a person. Claude Code stamps isMeta at the
    // injecting call site, and its own recogniser requires it; without the same requirement a
    // real prompt that merely OPENS with the sentence would be reclassified as machinery and
    // dropped from turn numbering.
    let typed = parse(
        r#"{"type":"user","uuid":"u9","message":{"role":"user","content":"Continue from where you left off. and also fix the tests"}}"#,
    );
    assert!(!typed.is_resume_prompt());
    assert_eq!(
        typed.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage]
    );
    assert!(typed.is_genuine_user());
    assert!(typed.opens_turn());
}

#[test]
fn an_ismeta_prompt_with_a_promptsource_is_still_accepted() {
    // The documented gap: Claude Code's recogniser also requires promptSource to be absent,
    // and csift does not test that field. This pins the wider set deliberately - a record
    // csift calls a repair prompt that the producer's own recogniser would skip.
    let with_source = parse(
        r#"{"type":"user","uuid":"p2","isMeta":true,"promptSource":"system","message":{"role":"user","content":[{"type":"text","text":"Continue from where you left off."}]}}"#,
    );
    assert!(with_source.is_resume_prompt());
    assert_eq!(
        with_source.classify(&ClassifyCtx::top_level()),
        vec![Class::ResumePrompt]
    );
}

#[test]
fn a_placeholder_with_no_parent_reads_unpaired() {
    // A missing parentUuid is a real shape (the loader splices onto whatever the tail was, and
    // a torn or head record can carry none). It is a verdict, not a panic and not unknown.
    let orphan = parse(
        r#"{"type":"assistant","uuid":"h3","isApiErrorMessage":false,"message":{"role":"assistant","model":"<synthetic>","content":[{"type":"text","text":"No response requested."}]}}"#,
    );
    let prompts = uuid_set(&["p1"]);
    assert!(orphan.parent_uuid.is_none());
    assert_eq!(orphan.resume_paired(&ctx_with(&prompts)), Some(false));
}

#[test]
fn a_claude_code_set_longer_variant_is_still_the_prompt() {
    // Claude Code sets CLAUDE_CODE_RESUME_PROMPT itself on two respawn paths, and both
    // values BEGIN with the sentence and continue. csift cannot observe the receiver's
    // environment, so it matches the constant as a prefix and both variants land here.
    let restarted = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off. Note: this session was automatically restarted after its process exited unexpectedly."}}"#,
    );
    assert!(restarted.is_resume_prompt());
    assert_eq!(
        restarted.classify(&ClassifyCtx::top_level()),
        vec![Class::ResumePrompt]
    );
}

#[test]
fn a_tool_result_carrier_is_never_the_prompt() {
    // `classify_user` routes a tool_result carrier away before any marker test, so the
    // standalone predicate must exclude it too or the two would disagree about the index.
    let carrier = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":[{"type":"text","text":"Continue from where you left off."},{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
    );
    assert!(!carrier.is_resume_prompt());
    assert_eq!(
        carrier.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentToolResult]
    );
}

#[test]
fn single_text_content_mirrors_the_producers_reader() {
    // A bare string, or exactly one text block - anything else is None.
    let s = parse(r#"{"type":"user","message":{"role":"user","content":"solo"}}"#);
    assert_eq!(s.single_text_content(), Some("solo"));
    let one = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"one"}]}}"#,
    );
    assert_eq!(one.single_text_content(), Some("one"));
    let two = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}}"#,
    );
    assert_eq!(two.single_text_content(), None);
    let non_text = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"x"}]}}"#,
    );
    assert_eq!(non_text.single_text_content(), None);
}
