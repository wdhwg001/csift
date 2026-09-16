//! The `harness.bookkeeping` family: the thirteen leaves of the loader's non-transcript
//! line set - their key=value HEADS and the provenance edges a bookkeeping line names
//! (`leafUuid`, `messageId`, `parentSessionId`, `continuedInSessionId`).
//!
//! A STUB at the skeleton amendment: the dispatch, the matchable-text law and the
//! whole-file gate's re-render are wired here, the family's own semantics are not. The
//! body below is what the family lane replaces.

use super::*;

/// What a `harness.bookkeeping` LINE says - the family's half of the render seam.
///
/// Called by [`super::family_render`] for every unit whose leaf sits in this family's
/// category, BEFORE the engine's generic arms; `None` falls through to them. A STUB at
/// the render seam: the composition this family owns still lives in `record_text.rs`
/// and moves here in its own commit.
pub(crate) fn render(
    _rec: &Record,
    _raw: Option<&str>,
    _class: Class,
    _ctx: &ClassifyCtx,
) -> Option<String> {
    None
}

/// The `harness.bookkeeping` family's own facts.
///
/// UNINHABITED at the skeleton amendment apart from the pin below - the family lane adds
/// its variants HERE and nowhere else: one enum per family is what keeps
/// `types/facts.rs` out of every family's way, and what stops two families minting one
/// idea twice.
///
/// An EDGE to another record is never a variant here. Edges are SHARED on [`HitFact`]
/// itself, because several families carry them and a reader has to read every one the
/// same way; a family builds one through the shared helper instead of describing the
/// shape again.
#[derive(Debug, Clone)]
pub(crate) enum Fact {
    /// The DELEGATION PIN. Each of [`HitFact`]'s six family arms forwards `label_zone`
    /// and `json` to its family's own enum, and a forward nothing ever takes is a
    /// forward that can be deleted with every test still passing - which is exactly the
    /// state six uninhabited enums leave the crate in. This variant is the one inhabited
    /// value in the whole set, so a unit test can watch a family fact cross both
    /// projections. The family lane adds its real variants BESIDE it and leaves it
    /// alone.
    #[cfg(test)]
    Probe(&'static str),
}

impl Fact {
    /// The label-zone tag this fact adds, if any.
    pub(crate) fn label_zone(&self) -> Option<String> {
        match *self {
            #[cfg(test)]
            Fact::Probe(head) => Some(head.to_string()),
        }
    }

    /// The JSON key(s) and value(s) this fact adds to its hit object.
    pub(crate) fn json(&self) -> Vec<(&'static str, serde_json::Value)> {
        match *self {
            #[cfg(test)]
            Fact::Probe(head) => vec![("bookkeeping_probe", serde_json::json!(head))],
        }
    }
}

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
