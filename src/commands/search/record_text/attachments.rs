//! The ATTACHMENT family: the nine `harness.context` leaves, `harness.hook.{context,
//! outcome}` and `agent.message.structured` - the body the model actually received
//! (the record's own `rendered` conversion where it carries one) and the `attached_to`
//! edge every attachment names through its `parentUuid` or its `toolUseID`.
//!
//! A STUB at the skeleton amendment: the dispatch is wired here, the family's own
//! semantics are not. The body below is what the family lane replaces.

use super::*;

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
