//! The thinking family: four block shapes, four leaves, and the two that carry no
//! text the model could read.

use super::*;
use crate::model::classify::thinking;

#[test]
fn a_summary_block_is_the_ordinary_leaf() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"weigh the two shapes","signature":"s"}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentThinkingSummary]
    );
    assert!(Class::AgentThinkingSummary.llm_visible());
}

#[test]
fn an_empty_thinking_text_is_the_omitted_leaf() {
    // The API returned no summary text: the block still carries its signature, which
    // is echoed back, but there is nothing for the model to read.
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"","signature":"AbCdEf=="}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentThinkingOmitted]
    );
    assert!(!Class::AgentThinkingOmitted.llm_visible());
    // Whitespace-only text is the same shape - a block of spaces is not a summary.
    let ws = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"   \n ","signature":"s"}]}}"#,
    );
    assert_eq!(
        ws.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentThinkingOmitted]
    );
    // A block with text and NO signature is still a summary: the signature decides the
    // narration tag, never whether there is a summary at all.
    let unsigned = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"plan"}]}}"#,
    );
    assert_eq!(
        unsigned.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentThinkingSummary]
    );
}

#[test]
fn a_redacted_block_is_its_own_leaf() {
    // The documented shape: an opaque base64 `data` payload and no text field at all.
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"redacted_thinking","data":"EncryptedOpaqueBlob=="}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentThinkingRedacted]
    );
    assert!(!Class::AgentThinkingRedacted.llm_visible());
}

#[test]
fn the_family_table_is_block_shaped_only() {
    // Every non-thinking block yields no leaf from this table - the dispatch's other
    // arms own them, and a table that answered here would claim a text block is
    // reasoning.
    for b in [
        Block::Text {
            text: "hello".to_string(),
        },
        Block::ToolUse {
            id: None,
            name: None,
            input: None,
        },
        Block::ToolResult {
            tool_use_id: None,
            content: None,
            is_error: None,
        },
        Block::Image { source: None },
    ] {
        assert_eq!(thinking::classify(&b), None, "{b:?}");
    }
    // All four thinking leaves sit in one category, so `-t agent.thinking` reaches the
    // two that carry text and the glob form reaches all four.
    let family: Vec<&str> = Class::ALL
        .iter()
        .filter(|c| c.category() == "agent.thinking")
        .map(|c| c.path())
        .collect();
    assert_eq!(
        family,
        vec![
            "agent.thinking.summary",
            "agent.thinking.narration",
            "agent.thinking.omitted",
            "agent.thinking.redacted",
        ]
    );
}

#[test]
fn the_salvage_discriminator_is_the_model_sentinel_beside_an_unsigned_block() {
    // The one client-minted thinking shape, in the shape that reaches disk: the
    // synthetic-model sentinel plus a thinking block with text and an EMPTY signature.
    // The flag its constructor sets (`isVirtual`) is stripped on the way to the file, so
    // the test may not read it - and `isApiErrorMessage` is FALSE on this shape, so that
    // is not the discriminator either.
    let salvage = parse(
        r#"{"type":"assistant","isApiErrorMessage":false,"message":{"role":"assistant","model":"<synthetic>","id":"9f1e2d3c-4b5a-4c6d-8e7f-0a1b2c3d4e5f","stop_reason":"stop_sequence","content":[{"type":"thinking","thinking":"the partial reasoning","signature":""}]}}"#,
    );
    let block = &salvage.blocks().expect("blocks")[0];
    assert!(thinking::is_cancel_salvage(&salvage, block));
    // It keeps the ordinary summary leaf: the block IS a summary slot in the message.
    assert_eq!(
        salvage.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentThinkingSummary]
    );
}

#[test]
fn a_signed_or_field_less_block_is_never_salvage_whatever_its_model() {
    // Four near misses, one per conjunct. A signed block on the sentinel model (the API
    // answered); a block with NO signature field at all, which is a different shape from
    // one carrying the empty string and is exactly the distinction the discriminator
    // rests on; an empty-signature block on a real model id (no such producer, and the
    // sentinel is what the factory writes); an empty-signature block with no text (the
    // constructor is gated on non-empty text, so that shape is not its output).
    for line in [
        r#"{"type":"assistant","message":{"role":"assistant","model":"<synthetic>","content":[{"type":"thinking","thinking":"reasoning","signature":"AbCd"}]}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","model":"<synthetic>","content":[{"type":"thinking","thinking":"reasoning"}]}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","model":"claude-opus-5","content":[{"type":"thinking","thinking":"reasoning","signature":""}]}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","model":"<synthetic>","content":[{"type":"thinking","thinking":"","signature":""}]}}"#,
    ] {
        let r = parse(line);
        let b = &r.blocks().expect("blocks")[0];
        assert!(!thinking::is_cancel_salvage(&r, b), "{line}");
    }
    // The resume placeholder shares the model sentinel and is NOT a thinking block, so
    // the block arm refuses it before the record is ever consulted.
    let placeholder = parse(
        r#"{"type":"assistant","message":{"role":"assistant","model":"<synthetic>","content":[{"type":"text","text":"No response requested."}]}}"#,
    );
    let b = &placeholder.blocks().expect("blocks")[0];
    assert!(!thinking::is_cancel_salvage(&placeholder, b));
    // A USER record carrying the same block shape is not an assistant turn.
    let user = parse(
        r#"{"type":"user","message":{"role":"user","model":"<synthetic>","content":[{"type":"thinking","thinking":"reasoning","signature":""}]}}"#,
    );
    let b = &user.blocks().expect("blocks")[0];
    assert!(!thinking::is_cancel_salvage(&user, b));
}

#[test]
fn the_strip_stand_in_text_is_the_harness_literal() {
    // The constant the marker keys on is the whole text the strip path pushes, brackets
    // included - pinned here so a rewrite that drops a bracket is caught at the source
    // rather than in a consumer's containment check.
    assert_eq!(thinking::THINKING_REMOVED_TEXT, "[Thinking removed]");
}
