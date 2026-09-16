//! The INJECTION family: the isMeta `type:"user"` templates the harness writes into a
//! turn - `harness.enforce.*`, `harness.command.caveat`, `harness.hook.feedback`,
//! `harness.context.{image,skill,reminder}` and the relayed human message - their markers
//! and the records each one points at.
//!
//! A STUB at the skeleton amendment: the dispatch is wired here, the family's own
//! semantics are not. The body below is what the family lane replaces.

use super::*;

/// What an isMeta INJECTION says - the family's half of the render seam.
///
/// Called by [`super::family_render`] for every unit whose leaf sits in one of this
/// family's categories, and BEHIND the two families it shares a category with
/// (`harness.hook`, `harness.schedule`), so an attachment or a scheduler record answers
/// first and this sees only what they refused. `None` falls through to the engine's
/// generic arms.
///
/// EMPTY at the render seam, and not because the move was skipped: an isMeta injection is
/// a `type:"user"` record whose text is its own `message.content`, so today it renders
/// through the same generic arm every conversation record does. There is no per-family
/// arm in the shared chain to move. The family lane fills this when it gives a template
/// a head of its own.
///
/// The one thing that is NOT empty is the test probe below, and it belongs to this family
/// on purpose: an isMeta injection is the shape with no leaf of its own, so this render is
/// reachable only through the caller's CLASS, and a seam nothing reaches through a class
/// is a seam the whole-file gate cannot see. KEEP the two lines when filling the body -
/// they are what holds the gate to re-rendering every view.
pub(crate) fn render(
    rec: &Record,
    raw: Option<&str>,
    class: Class,
    ctx: &ClassifyCtx,
) -> Option<String> {
    #[cfg(test)]
    if let Some(text) = super::probe::render(rec, class) {
        return Some(text);
    }
    let _ = (rec, raw, class, ctx);
    None
}

/// The isMeta-injection family's own facts.
///
/// UNINHABITED at the skeleton amendment - the family lane adds its variants HERE and
/// nowhere else: one enum per family is what keeps `types/facts.rs` out of every
/// family's way, and what stops two families minting one idea twice.
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
