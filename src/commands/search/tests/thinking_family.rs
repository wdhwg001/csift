//! The THINKING family's own `facts()`: which units it claims, which markers it pushes,
//! and what a block's body is when the block holds no readable text.

use super::*;

/// The family's markers for one BLOCK of a record, under one leaf.
fn markers(line: &str, block_index: usize, class: Option<Class>) -> Vec<String> {
    let r = rec(line);
    let blocks = r.blocks().expect("blocks").to_vec();
    let section = Section {
        class,
        block: Some(&blocks[block_index]),
    };
    let (_, markers) = thinking::facts(&r, None, &section, &ClassifyCtx::top_level());
    markers.iter().map(Marker::render).collect()
}

const OMITTED: &str = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"","signature":"AbCdEf=="}]}}"#;
const REDACTED: &str = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"redacted_thinking","data":"EncryptedOpaqueBlob=="}]}}"#;
const SUMMARY: &str = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"weigh the two shapes","signature":"AbCdEf=="}]}}"#;
const SALVAGE: &str = r#"{"type":"assistant","message":{"role":"assistant","model":"<synthetic>","stop_reason":"stop_sequence","content":[{"type":"thinking","thinking":"the partial reasoning","signature":""}]}}"#;
const REMOVED: &str = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"[Thinking removed]"}]}}"#;

#[test]
fn a_textless_leaf_says_what_it_is_in_the_label_zone() {
    // The two leaves that carry no text at all name themselves there, because the body a
    // reader would otherwise read is empty by construction.
    assert_eq!(
        markers(OMITTED, 0, Some(Class::AgentThinkingOmitted)),
        vec!["[no summary text]".to_string()]
    );
    assert_eq!(
        markers(REDACTED, 0, Some(Class::AgentThinkingRedacted)),
        vec!["[redacted thinking]".to_string()]
    );
    // A block that DOES carry text takes neither: the body already says it.
    assert!(markers(SUMMARY, 0, Some(Class::AgentThinkingSummary)).is_empty());
}

#[test]
fn the_two_client_minted_shapes_name_their_author() {
    // The salvage block keeps the ordinary summary leaf, so without the marker nothing
    // separates it from reasoning the API returned.
    assert_eq!(
        markers(SALVAGE, 0, Some(Class::AgentThinkingSummary)),
        vec!["[cancelled stream salvage]".to_string()]
    );
    // The strip path's stand-in is an ordinary assistant text block, so without the
    // marker those three words read as the assistant speaking.
    assert_eq!(
        markers(REMOVED, 0, Some(Class::AgentMessagePlain)),
        vec!["[thinking removed by the client]".to_string()]
    );
}

#[test]
fn the_stand_in_marker_tests_the_whole_text_and_the_leaf() {
    // A longer assistant message that QUOTES the phrase is the assistant speaking.
    let quoted = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"the client pushes [Thinking removed] when it strips"}]}}"#;
    assert!(markers(quoted, 0, Some(Class::AgentMessagePlain)).is_empty());
    // Surrounding whitespace is not content, so the trimmed form still matches.
    let padded = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"  [Thinking removed]\n"}]}}"#;
    assert_eq!(
        markers(padded, 0, Some(Class::AgentMessagePlain)),
        vec!["[thinking removed by the client]".to_string()]
    );
    // The same text under any OTHER leaf is not the assistant's say, so it takes no
    // marker - the leaf is half the test, not decoration.
    assert!(markers(REMOVED, 0, Some(Class::UserMessage)).is_empty());
    assert!(markers(REMOVED, 0, None).is_empty());
}

#[test]
fn a_record_level_unit_is_refused_before_any_block_is_read() {
    // `facts()` runs on EVERY emitted unit, so the gate that refuses a unit this family
    // does not own is the first thing it does. A record-level unit carries no block.
    let r = rec(SALVAGE);
    let (facts, markers) = thinking::facts(
        &r,
        None,
        &Section::record(Some(Class::AgentThinkingSummary)),
        &ClassifyCtx::top_level(),
    );
    assert!(facts.is_empty() && markers.is_empty());
}

#[test]
fn a_block_that_holds_no_readable_text_has_an_empty_body() {
    // The body rule, pinned on the function that computes it. A redacted block's
    // ciphertext is never the body, and a whitespace-only thinking block - which is
    // exactly the block the omitted leaf is assigned to - renders empty rather than a run
    // of spaces.
    let redacted = rec(REDACTED);
    assert_eq!(
        thinking::block_body(&redacted.blocks().expect("blocks")[0]).as_deref(),
        Some("")
    );
    let ws = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"  \t \n ","signature":"s"}]}}"#,
    );
    assert_eq!(
        thinking::block_body(&ws.blocks().expect("blocks")[0]).as_deref(),
        Some("")
    );
    // A block that DOES carry text renders it verbatim, leading run included.
    let kept = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"  weigh it","signature":"s"}]}}"#,
    );
    assert_eq!(
        thinking::block_body(&kept.blocks().expect("blocks")[0]).as_deref(),
        Some("  weigh it")
    );
    // A block shape the family does not own answers nothing at all, which is what keeps
    // the block loop from emitting a fabricated body for it.
    let text = rec(REMOVED);
    assert!(thinking::block_body(&text.blocks().expect("blocks")[0]).is_none());
}
