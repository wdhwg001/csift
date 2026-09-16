//! The ATTACHMENT family: the nine `harness.context` leaves, `harness.hook.{context,
//! outcome}` and `agent.message.structured` - the body the model actually received
//! (the record's own `rendered` conversion where it carries one) and the `attached_to`
//! edge every attachment names through its `parentUuid` or its `toolUseID`.
//!
//! A STUB at the skeleton amendment: the dispatch is wired here, the family's own
//! semantics are not. The body below is what the family lane replaces.

use super::*;

/// What an ATTACHMENT line says - the family's half of the render seam.
///
/// Called by [`super::family_render`] for every unit whose leaf sits in one of this
/// family's categories, BEFORE the engine's generic arms; `None` falls through to them.
///
/// TWO answers, both keyed on the RECORD and not on the leaf, which is why every view of
/// one attachment renders the same text:
/// - a hook's injected additionalContext renders its content strings JOINED, which is the
///   form the model received (a `<system-reminder>`-wrapped user message) rather than the
///   array it is stored as;
/// - every other payload renders its VERBATIM JSON - honestly raw, and a byte substring
///   of the source line, so the section 7d literal prefilter and the 7f whole-file gate
///   hold with no synthesized-marker registration.
///
/// `None` for every non-attachment record, on one type compare. A payload csift models no
/// leaf for never arrives here at all (the seam has no leaf to dispatch on) and the
/// engine renders its JSON itself, which is the same bytes this would have produced.
pub(crate) fn render(
    rec: &Record,
    _raw: Option<&str>,
    _class: Class,
    _ctx: &ClassifyCtx,
) -> Option<String> {
    if !rec.is_type("attachment") {
        return None;
    }
    rec.hook_additional_context_text()
        .or_else(|| rec.attachment_payload_text())
}

/// The attachment family's own facts.
///
/// UNINHABITED at the skeleton amendment - the family lane adds its variants HERE and
/// nowhere else: one enum per family is what keeps `types/facts.rs` out of every
/// family's way, and what stops two families minting one idea twice.
///
/// An EDGE to another record is never a variant here. Edges are SHARED on [`HitFact`]
/// itself, because several families carry them and a reader has to read every one the
/// same way. Several edges on ONE unit is the shape this family needs, and the shared
/// fact already answers it: each projects a one-element array under one key and
/// [`attach_facts`] accumulates them.
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

/// The attachment family's per-unit facts and markers.
///
/// Called for EVERY emitted unit through [`crate::commands::search::family_facts`], so
/// the first thing a filled body does is refuse a unit that is not its own (a
/// non-attachment record answers on one type compare). A filled body reads the payload
/// through [`Record::attachment_value`], which is memoised per record - so the parse
/// `classify` already paid is the only one.
pub(crate) fn facts(
    _rec: &Record,
    _raw: Option<&str>,
    _section: &Section<'_>,
    _ctx: &ClassifyCtx,
) -> (Vec<HitFact>, Vec<Marker>) {
    (Vec::new(), Vec::new())
}
