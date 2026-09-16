//! The `harness.bookkeeping` family: the thirteen leaves of the loader's non-transcript
//! line set - their key=value HEADS and the provenance edges a bookkeeping line names
//! (`leafUuid`, `messageId`, `parentSessionId`, `continuedInSessionId`).
//!
//! A STUB at the skeleton amendment: the dispatch, the matchable-text law and the
//! whole-file gate's re-render are wired here, the family's own semantics are not. The
//! body below is what the family lane replaces.

use super::*;

/// The `harness.bookkeeping` family's per-unit facts and markers.
///
/// Called for EVERY emitted unit through [`crate::commands::search::family_facts`], so
/// the first thing a filled body does is refuse a unit that is not its own: the leaf on
/// `section`, or the record's own line type. Returning two empty vectors allocates
/// nothing, which is what every unit of every other family pays here.
pub(crate) fn facts(
    _rec: &Record,
    _raw: Option<&str>,
    _section: &Section<'_>,
    _ctx: &ClassifyCtx,
) -> (Vec<HitFact>, Vec<Marker>) {
    (Vec::new(), Vec::new())
}
