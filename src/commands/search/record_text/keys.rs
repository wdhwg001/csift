//! The EXPOSED KEYS family: facts Claude Code stamps on a record about the call behind
//! it - which agent definition, skill, plugin and MCP tool it ran under, which API
//! request and block it came from, what a person typed refusing a tool, why a call
//! failed. They belong to no single leaf, which is why they are a family of their own.
//!
//! A STUB at the skeleton amendment: the dispatch is wired here, the family's own
//! semantics are not. The body below is what the family lane replaces.

use super::*;

/// The exposed-keys family's per-unit facts and markers.
///
/// Called for EVERY emitted unit through [`crate::commands::search::family_facts`].
/// Unlike the other five this family is keyed on FIELDS rather than on a leaf, so a
/// filled body reads the record's own keys and returns nothing when it carries none -
/// which is the common case, and the reason an absent key means "this record does not
/// say" rather than "no".
pub(crate) fn facts(
    _rec: &Record,
    _raw: Option<&str>,
    _section: &Section<'_>,
    _ctx: &ClassifyCtx,
) -> (Vec<HitFact>, Vec<Marker>) {
    (Vec::new(), Vec::new())
}
