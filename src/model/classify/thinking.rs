//! The `agent.thinking` family: one arm per thinking-block shape.
//!
//! Four leaves, and the discriminators are STRUCTURAL - the block's own type, the text
//! it carries, and the tag hidden in its base64 `signature` (decoded by
//! `model/narration.rs`). Nothing here reads a neighbouring block or a timestamp: a
//! thinking block says what it is on its own.

use super::*;

/// The leaf a thinking-family block carries, or `None` for any other block type.
///
/// - a `redacted_thinking` block is the API's safety-redacted ciphertext: opaque by
///   construction, so it can only be [`Class::AgentThinkingRedacted`];
/// - a `thinking` block with EMPTY text is the `omitted` display mode - the API
///   returned no summary text and the client cannot blank a block it received, so the
///   emptiness is upstream, not a local placeholder;
/// - a `thinking` block whose signature tag reads `narration` is the API's progress
///   update between tool calls;
/// - everything else is the ordinary reasoning SUMMARY.
#[must_use]
pub(crate) fn classify(block: &Block) -> Option<Class> {
    match block {
        Block::RedactedThinking { .. } => Some(Class::AgentThinkingRedacted),
        Block::Thinking {
            thinking,
            signature,
        } => Some(if thinking.trim().is_empty() {
            Class::AgentThinkingOmitted
        } else {
            thinking_block_class(signature.as_deref())
        }),
        _ => None,
    }
}
