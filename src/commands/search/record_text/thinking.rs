//! The THINKING family: the four `agent.thinking.*` leaves plus the two shapes that are
//! markers rather than leaves - the client-minted cancel SALVAGE block and the
//! `[Thinking removed]` text the strip path can push - and the per-block facts that
//! separate a block the API signed from one the client made.
//!
//! A STUB at the skeleton amendment: the dispatch is wired here, the family's own
//! semantics are not. The body below is what the family lane replaces.

use std::borrow::Cow;

use super::*;

/// What an `agent.thinking` RECORD says - the family's half of the record-level render
/// seam.
///
/// EMPTY, and structurally so: every thinking leaf is decided on a content BLOCK, so a
/// thinking unit never renders at the record level at all. The family's real render is
/// [`block_body`], which the block loop calls. This arm exists so the seam has one shape
/// for all five rendering families rather than five minus one.
pub(crate) fn render(
    _rec: &Record,
    _raw: Option<&str>,
    _class: Class,
    _ctx: &ClassifyCtx,
) -> Option<String> {
    None
}

/// Render stand-in for a `redacted_thinking` block (GOLD §2 / oracle B3): the block carries
/// only an opaque/encrypted `data` payload (no readable text), so it surfaces this placeholder
/// while still classifying `agent.thinking` - so `-t agent.thinking` finds it without dumping
/// the opaque blob.
pub(crate) const REDACTED_THINKING_PLACEHOLDER: &str = "[redacted thinking]";

/// What ONE thinking BLOCK says - the family's half of the per-block render seam.
///
/// The block loop calls this instead of deciding the body itself, so what a thinking
/// unit's text IS becomes this family's to change. `None` means a block shape this
/// family does not own, and the block loop emits nothing for it rather than a fabricated
/// body; today it cannot be reached, because `thinking_block_leaf` already refused
/// everything but these two before the loop gets here.
///
/// A `Cow` because one arm borrows the block's own text and the other borrows a
/// constant: the body of a redacted block is opaque ciphertext with no readable text at
/// all, so it surfaces a placeholder rather than the blob, while every other thinking
/// block surfaces its own text, empty included.
pub(crate) fn block_body(block: &Block) -> Option<Cow<'_, str>> {
    match block {
        Block::Thinking { thinking, .. } => Some(Cow::Borrowed(thinking.as_str())),
        Block::RedactedThinking { .. } => Some(Cow::Borrowed(REDACTED_THINKING_PLACEHOLDER)),
        _ => None,
    }
}

/// The thinking family's own facts.
///
/// UNINHABITED at the skeleton amendment - the family lane adds its variants HERE and
/// nowhere else: one enum per family is what keeps `types/facts.rs` out of every
/// family's way, and what stops two families minting one idea twice. A PER-BLOCK fact
/// (a signature length, say) is a variant here like any other: the block it belongs to
/// arrives on [`Section::block`].
///
/// An EDGE to another record is never a variant here. Edges are SHARED on [`HitFact`]
/// itself, because several families carry them and a reader has to read every one the
/// same way; a family builds one through the shared helper instead of describing the
/// shape again.
#[derive(Debug, Clone)]
pub(crate) enum Fact {}

impl Fact {
    /// The label-zone tag this fact adds, if any.
    pub(crate) fn label_zone(&self) -> Option<String> {
        match *self {}
    }

    /// The JSON key(s) and value(s) this fact adds to its hit object.
    pub(crate) fn json(&self) -> Vec<(&'static str, serde_json::Value)> {
        match *self {}
    }
}

/// The thinking family's per-unit facts and markers.
///
/// Called for EVERY emitted unit through [`crate::commands::search::family_facts`], so
/// the first thing a filled body does is refuse a unit that is not its own. This is the
/// family that needs `section.block`: a signature belongs to the BLOCK, and a record
/// carrying two thinking blocks emits two units whose facts differ.
pub(crate) fn facts(
    _rec: &Record,
    _raw: Option<&str>,
    _section: &Section<'_>,
    _ctx: &ClassifyCtx,
) -> (Vec<HitFact>, Vec<Marker>) {
    (Vec::new(), Vec::new())
}
