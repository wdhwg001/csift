//! The INJECTION family: the isMeta `type:"user"` templates the harness writes into a
//! turn - `harness.enforce.*`, `harness.command.caveat`, `harness.hook.feedback`,
//! `harness.context.{image,skill,reminder}` and the relayed human message - their markers
//! and the records each one points at.
//!
//! A STUB at the skeleton amendment: the dispatch is wired here, the family's own
//! semantics are not. The body below is what the family lane replaces.

use super::*;

/// The injection family's per-unit facts and markers.
///
/// Called for EVERY emitted unit through [`crate::commands::search::family_facts`], so
/// the first thing a filled body does is refuse a unit that is not its own. An edge this
/// family resolves to another record of the same file goes through
/// [`ClassifyCtx::line_of_uuid`], which is backed by the chain's own uuid map - never a
/// second index.
pub(crate) fn facts(
    _rec: &Record,
    _raw: Option<&str>,
    _section: &Section<'_>,
    _ctx: &ClassifyCtx,
) -> (Vec<HitFact>, Vec<Marker>) {
    (Vec::new(), Vec::new())
}
