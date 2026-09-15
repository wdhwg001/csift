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
