//! The ATTACHMENT family: the nine `harness.context` leaves, `harness.hook.{context,
//! outcome}` and `agent.message.structured` - the body the model actually received
//! (the record's own `rendered` conversion where it carries one) and the `attached_to`
//! edge every attachment names through its `parentUuid` or its `toolUseID`.
//!
//! A STUB at the skeleton amendment: the dispatch is wired here, the family's own
//! semantics are not. The body below is what the family lane replaces.

use super::*;

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
