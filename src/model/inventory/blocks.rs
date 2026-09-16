//! Axis 4: the `type` of a content block inside `message.content[]` - 8 values.
//!
//! Blocks ride user and assistant lines, which a flagless scan already keeps, so the
//! gate here is always the default one. ONE of the eight carries no leaf: `document` is a
//! shape csift has never modeled, so a record whose only block is one renders as an
//! unmodeled unit under an address and as nothing at all on a scan.

use super::*;

/// One row per block `type`. The `text` and `thinking` rows name the DOMINANT leaf; a
/// user-side `text` block routes to the record-text axis instead, and a thinking block
/// splits further on its signature tag and its own emptiness.
pub(crate) const ROWS: &[Entry] = &[
    e(
        Shape::Block("text"),
        Some(Class::AgentMessagePlain),
        Gate::Default,
        true,
        Human::Always,
    ),
    e(
        Shape::Block("thinking"),
        Some(Class::AgentThinkingSummary),
        Gate::Default,
        true,
        Human::Verbose,
    ),
    e(
        Shape::Block("redacted_thinking"),
        Some(Class::AgentThinkingRedacted),
        Gate::Default,
        false,
        Human::Verbose,
    ),
    e(
        Shape::Block("tool_use"),
        Some(Class::AgentToolUse),
        Gate::Default,
        true,
        Human::Always,
    ),
    e(
        Shape::Block("tool_result"),
        Some(Class::AgentToolResult),
        Gate::Default,
        true,
        Human::Always,
    ),
    e(
        // The leaf on this row is the image-ONLY `type:"user"` record (see `disk`), and
        // every one of those in the corpus carries `isMeta`, which the transcript render
        // gate refuses (claim CLS-036) - so the person never saw the record csift labels
        // here. An image beside TEXT is a different row: it rides the person's own
        // message, which they did see.
        Shape::Block("image"),
        Some(Class::ContextImage),
        Gate::Default,
        true,
        Human::Never,
    ),
    e(
        Shape::Block("document"),
        None,
        Gate::Address,
        false,
        Human::Unstated,
    ),
    e(
        // The API served the turn from a fallback model and wrote this block in place of
        // the text. The turn still reached the conversation, so the record carries the
        // plain message leaf with an EMPTY body; the marker naming the fallback is a
        // family lane's, and the human column stays unstated until that render exists.
        Shape::Block("fallback"),
        Some(Class::AgentMessagePlain),
        Gate::Default,
        true,
        Human::Unstated,
    ),
];
