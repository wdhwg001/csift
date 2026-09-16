//! The SYSTEM family: the five `harness.system.*` event kinds, the scheduler's
//! `harness.schedule.event` record and the three `harness.turn.*` closers - the fields
//! that make each event useful, and the edges it names (`refusedUserMessageUuid`, the
//! recap chain, the hook-run id).
//!
//! A STUB at the skeleton amendment: the dispatch is wired here, the family's own
//! semantics are not. The body below is what the family lane replaces.
//!
//! ONE law is not the family's to choose. A `system`/`api_error` record's `error` object
//! carries the whole HTTP response header set, so this family renders the FIELDS it
//! names and never the line - which is why the shared matchable-text law
//! ([`verbatim_body`]) stops at the bookkeeping leaves and does not reach here.

use super::*;

/// The system family's per-unit facts and markers.
///
/// Called for EVERY emitted unit through [`crate::commands::search::family_facts`], so
/// the first thing a filled body does is refuse a unit that is not its own (a record
/// whose own `type` is not `system` answers on one compare). A fabricated head this
/// family puts in the MATCHABLE text must register its raw marker in
/// `matcher::synth_marker_finders`, or the whole-file gate can prune a file whose only
/// hit lies in that head.
pub(crate) fn facts(
    _rec: &Record,
    _raw: Option<&str>,
    _section: &Section<'_>,
    _ctx: &ClassifyCtx,
) -> (Vec<HitFact>, Vec<Marker>) {
    (Vec::new(), Vec::new())
}
