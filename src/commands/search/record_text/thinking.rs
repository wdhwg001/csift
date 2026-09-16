//! The THINKING family: the four `agent.thinking.*` leaves plus the two shapes that are
//! markers rather than leaves - the client-minted cancel SALVAGE block and the
//! `[Thinking removed]` text the strip path can push - and the per-block facts that
//! separate a block the API signed from one the client made.
//!
//! A STUB at the skeleton amendment: the dispatch is wired here, the family's own
//! semantics are not. The body below is what the family lane replaces.

use super::*;

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
