//! Axis 4: the `type` of a content block inside `message.content[]` - 8 values.
//!
//! Blocks ride user and assistant lines, which a flagless scan already keeps, so the
//! gate here is always the default one. Two of the eight carry no leaf: `document` is a
//! shape csift has never modeled, and `fallback` - the block an assistant record carries
//! when the API served the turn from a fallback model - is a MARKER on the record rather
//! than a leaf of its own.

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
        Shape::Block("image"),
        Some(Class::ContextImage),
        Gate::Default,
        true,
        Human::Always,
    ),
    e(
        Shape::Block("document"),
        None,
        Gate::Address,
        false,
        Human::Unstated,
    ),
    e(
        Shape::Block("fallback"),
        None,
        Gate::Address,
        false,
        Human::Never,
    ),
];
