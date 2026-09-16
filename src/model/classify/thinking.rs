//! The `agent.thinking` family: one arm per thinking-block shape.
//!
//! Four leaves, and the discriminators are STRUCTURAL - the block's own type, the text
//! it carries, and the tag hidden in its base64 `signature` (decoded by
//! `model/narration.rs`). Nothing here reads a neighbouring block or a timestamp: a
//! thinking block says what it is on its own.

use super::*;

/// The exact text of the TEXT block Claude Code's strip path pushes when removing every
/// thinking block from a message would have left it empty.
///
/// It is the whole point of the strip path that it REMOVES rather than blanks: a thinking
/// block is filtered out of the content array, and this stand-in is pushed only when
/// nothing readable survived. The block is an ordinary `text` block, so it classifies
/// `agent.message.plain` - the marker keyed on this constant is what stops those three
/// words reading as the assistant speaking.
pub(crate) const THINKING_REMOVED_TEXT: &str = "[Thinking removed]";

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

/// The ONE client-minted thinking shape: the block Claude Code salvages from a stream the
/// person cancelled.
///
/// Every thinking block the API returns is SIGNED, and no client path blanks a signature
/// it received. The one constructor that writes an empty one is the stream guard's
/// force-end, which snapshots the partial streamed text into a thinking block through the
/// synthetic assistant factory - so the carrying record wears that factory's model
/// sentinel ([`SYNTHETIC_MODEL`]) rather than a model id.
///
/// The test is the CONJUNCTION of the two facts that reach disk: the record's model
/// sentinel, and the block's PRESENT-BUT-EMPTY signature beside non-empty text. An ABSENT
/// signature field is a DIFFERENT shape and answers false - that distinction is the whole
/// discriminator, and conflating the two would sweep in every unsigned block an older
/// transcript carries.
///
/// It deliberately reads neither the `isVirtual` flag the constructor sets, which the
/// persist path strips before the line is written, nor `isApiErrorMessage`, which that
/// factory leaves FALSE. Returns false for every ordinary thinking block, for a
/// `redacted_thinking` block (opaque, and always echoable) and for the resume placeholder,
/// which shares the model sentinel but carries a text block.
#[must_use]
pub(crate) fn is_cancel_salvage(rec: &Record, block: &Block) -> bool {
    let Block::Thinking {
        thinking,
        signature,
    } = block
    else {
        return false;
    };
    if signature.as_deref() != Some("") || thinking.trim().is_empty() {
        return false;
    }
    rec.is_type("assistant")
        && rec.message.as_ref().and_then(Message::model_id) == Some(SYNTHETIC_MODEL)
}
