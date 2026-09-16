//! A test-only seventh arm of the family dispatch.
//!
//! Every real family ships as a stub returning two empty vectors, and a stub that
//! returns nothing cannot show that the site was reached at all - so the one thing the
//! amendment has to pin (the dispatch runs, and it runs at BOTH hit-construction sites)
//! would go untested. This arm answers on a fixture sentinel instead, and the unit tests
//! read it back from a scanned unit and from an addressed one.
//!
//! It is `#[cfg(test)]`, so it is not in the binary: the e2e suite runs the built
//! `csift` and can never see it, and no production path can reach it.

use super::*;

/// The `sessionId` a fixture sets to ask for a probe answer. Not a uuid and not a bare
/// agent id, so no resolvable target shape collides with it.
pub(crate) const SENTINEL_SESSION: &str = "family-facts-probe";

/// The marker head the probe pushes on every unit of a sentinel record - what a test
/// looks for in the label zone.
pub(crate) const SENTINEL_MARKER: &str = "family probe";

/// Pushed only when the unit came from a content BLOCK, so a test can tell that the
/// per-unit half of [`Section`] arrives and is not a per-record stand-in.
pub(crate) const SENTINEL_BLOCK_MARKER: &str = "family probe block";

/// Pushed only when the unit carries NO leaf - the unmodeled addressed unit.
pub(crate) const SENTINEL_LEAFLESS_MARKER: &str = "family probe leafless";

/// The FACT the probe pushes on every unit of a sentinel record, beside the markers.
///
/// A marker alone cannot pin the dispatch. `Hit.markers` and `Hit.facts` are separate
/// fields filled by separate lines - the two `extend`s inside `family_facts`' `merge`, and
/// the `.chain(family)` that folds the family facts in behind the shape fact at the
/// addressed site - so a suite that reads only markers watches one of those lines and
/// leaves the other free to be deleted. Both deletions survived the whole unit suite
/// before this fact existed.
pub(crate) const SENTINEL_FACT: &str = "family probe fact";

/// The probe's per-unit facts and markers: one fact and one marker on every unit of a
/// record carrying the sentinel session id, plus one marker per `Section` field so both
/// halves are observable, and nothing at all on any other record.
pub(crate) fn facts(
    rec: &Record,
    _raw: Option<&str>,
    section: &Section<'_>,
    _ctx: &ClassifyCtx,
) -> (Vec<HitFact>, Vec<Marker>) {
    if rec.session_id.as_deref() != Some(SENTINEL_SESSION) {
        return (Vec::new(), Vec::new());
    }
    let mut markers = vec![Marker::plain(SENTINEL_MARKER)];
    if section.block.is_some() {
        markers.push(Marker::plain(SENTINEL_BLOCK_MARKER));
    }
    if section.class.is_none() {
        markers.push(Marker::plain(SENTINEL_LEAFLESS_MARKER));
    }
    (vec![HitFact::Probe(SENTINEL_FACT)], markers)
}
