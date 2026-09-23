//! `message.content` and the typed content BLOCK enum it holds.
//!
//! Split from the record root at the 600-line structure gate. Nothing here changed in the
//! move: these are the same shapes the root carried, at the same bytes.

use super::*;

/// `message.content` is polymorphic: a plain string or a list of blocks.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Content {
    /// Bare-string content (older format / genuine user text / compaction summary).
    Text(String),
    /// Array of typed content blocks.
    Blocks(Vec<Block>),
}

/// A typed content block within `message.content`. `#[serde(other)]` Unknown
/// catches any future block type without a parse error.
///
/// Some block fields are deserialized for the documented model (SPEC §3.5) but not
/// read by current logic (`signature`, the `ToolUse.id`, the `ToolResult`
/// `tool_use_id`/`is_error`, `Image.source`). The round-trip reconstruction returns
/// the whole turn, so `tool_use`↔`tool_result` pairing-by-id is not needed (both sit
/// in the same emitted exchange); the fields stay for shape-completeness + future use.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        #[serde(default)]
        text: String,
    },
    Thinking {
        #[serde(default)]
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },
    /// A `redacted_thinking` block - encrypted/opaque reasoning CC emits in place of a visible
    /// `thinking` block (no readable text, only an opaque `data` payload). Classified
    /// `agent.thinking` exactly like a normal thinking block; the opaque
    /// `data` is captured for shape-completeness but never rendered.
    RedactedThinking {
        #[serde(default)]
        data: String,
    },
    ToolUse {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        input: Option<serde_json::Value>,
    },
    ToolResult {
        #[serde(default, rename = "tool_use_id")]
        tool_use_id: Option<String>,
        /// String OR array of {type:text,text}/{type:image} - keep raw.
        #[serde(default)]
        content: Option<serde_json::Value>,
        #[serde(default)]
        is_error: Option<bool>,
    },
    Image {
        #[serde(default)]
        source: Option<serde_json::Value>,
    },
    /// A `fallback` block - what the API writes in place of the text when it served the
    /// turn from a fallback model. The record is still the assistant's TURN, so it takes
    /// the plain message leaf with an empty body; the variant exists because every other
    /// unmodeled block type parses to [`Block::Unknown`], and a leaf that keys on
    /// "nothing else matched" would answer for `document` and for a whitespace-only text
    /// block as well.
    ///
    /// TWO SIDES, not one model. The block's key set is exactly {from, to, type} on 48 of
    /// 48 specimens over one projects root and 53 of 53 over another, with `from` and `to`
    /// each exactly `{model}` and a flat `model` key on none of them (claim CLS-060). It
    /// is modeled that way because `from.model` is the ONLY place the pre-fallback model
    /// survives: every other record of the same API message already names the model that
    /// answered, so a shape carrying one id would carry the wrong one.
    Fallback {
        /// The model the turn was asked of.
        #[serde(default)]
        from: Option<FallbackSide>,
        /// The model that served it.
        #[serde(default)]
        to: Option<FallbackSide>,
    },
    /// Any block type not modeled above - never a parse failure.
    #[serde(other)]
    Unknown,
}

/// One side of a [`Block::Fallback`] - the object the API writes under `from` and under
/// `to`, whose only key is the model id. Its own struct rather than a bare string because
/// that is the shape on the wire, and a model that flattened it would silently accept a
/// line the API never writes.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct FallbackSide {
    /// The model id.
    #[serde(default)]
    pub model: Option<String>,
}
