//! The two EXTENSION POINTS a hit carries, and the shared facts every family may use.
//!
//! Split from `types.rs` when the six taxonomy families were given fact enums of their
//! own: one enum holding every family's variants puts the file at the structure cap on
//! the first family that fills it, and then every later family collides in the same
//! place. So [`HitFact`] carries ONE arm per family and each family's variants live in
//! its own `record_text/<family>.rs` - after this, no family edits this file or its
//! parent again.
//!
//! What stays here is what is genuinely shared: [`Marker`] (the label-zone word),
//! [`Section`] (which unit is being emitted), the facts more than one family needs, and
//! [`attach_facts`], the one rule for folding facts onto a JSON object.

use crate::commands::search::record_text;
use crate::model::{Block, Class};

/// A display-only note in a hit's LABEL ZONE: a `[head]` or `[head detail]` tag after
/// the leaf path, rendered in PUSH ORDER after the specific markers `render_label`
/// already knows about.
///
/// The EXTENSION POINT for a family that needs to say one more thing about a record
/// without a second leaf and without a new `Hit` field. Display-only by construction:
/// the text is fabricated, so it is never matchable and needs no synthesized-marker
/// registration - a family that wants MATCHABLE text renders it into the body instead.
#[derive(Debug, Clone)]
pub struct Marker {
    /// The fixed word, e.g. `not delivered`.
    pub head: &'static str,
    /// The variable part, when there is one: `[head detail]`.
    pub detail: Option<String>,
}

impl Marker {
    /// A marker with no detail.
    pub(crate) fn plain(head: &'static str) -> Self {
        Marker { head, detail: None }
    }

    /// The rendered `[head]` / `[head detail]` form.
    #[must_use]
    pub(crate) fn render(&self) -> String {
        match &self.detail {
            Some(d) => format!("[{} {d}]", self.head),
            None => format!("[{}]", self.head),
        }
    }
}

/// WHICH UNIT of a record the engine is emitting, handed to the one stamping site so a
/// family can answer per unit rather than per record.
///
/// Two axes, independent by construction. `class` is the leaf the unit is going out
/// under - `None` for the addressed unit of a record csift models no leaf for. `block` is
/// the content block the unit came from, when it came from one: a record-level unit (the
/// record text, the answered-AUQ dual, the elicitation marker) carries `None`, and a
/// multi-block record hands each block in turn, because a per-block fact - the length of
/// a thinking block's signature, say - is the BLOCK's and not the record's.
// The THIRD of the three allows AGENTS section 4 names by hand - beside the tolerant
// `model::Record` fields and the `model::inventory` documentation surface - and, like
// them, a surface that exists before its readers do rather than rot. It is a fact about
// WHERE the readers live rather than about the fields. Both
// are written by the emission engine on every unit and read by the family `facts()`
// bodies in `record_text/`, which ship as stubs at the skeleton amendment and are filled
// one family at a time. Without the allow the amendment cannot land the parameter the
// families are being given, which is the whole seam.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct Section<'a> {
    /// The leaf this unit is emitted under; `None` on an unmodeled addressed unit.
    pub(crate) class: Option<Class>,
    /// The content block this unit came from, or `None` for a record-level unit.
    pub(crate) block: Option<&'a Block>,
}

impl Section<'_> {
    /// The record-level unit: a section of the record's own text, with no block behind it.
    pub(crate) fn record(class: Option<Class>) -> Self {
        Section { class, block: None }
    }
}

/// A per-record FACT a hit carries beside its label: something true of the record that
/// is not a leaf and not a marker word - a shape csift models no leaf for, a join a
/// family resolved, a key the record exposes.
///
/// Each variant projects ITSELF, which is what makes this an extension point:
/// `render_label` consults [`HitFact::label_zone`] and the JSON projectors call
/// [`attach_facts`], so a new fact reaches both surfaces with no edit at either.
///
/// The SIX family arms are fixed and are the whole extension surface: a family adds
/// variants to its own `Fact` enum, never to this one. Anything ELSE here is SHARED on
/// purpose - a fact more than one family needs, because a fact two families mint
/// separately is two renders of one idea.
#[derive(Debug, Clone)]
pub enum HitFact {
    /// The record's on-disk SHAPE, when csift models no leaf for it: the top-level
    /// `type`, plus the `subtype` or the attachment payload `type` where the shape
    /// splits on one. Produced ONLY under an explicit address (a scan never emits an
    /// unmodeled unit), and it is what the label renders in place of a leaf path.
    Unmodeled(String),
    // THE SIX FAMILY ARMS. Each carries an enum that is UNINHABITED until its family
    // lane lands, so nothing constructs these yet and the allow says exactly that
    // rather than letting a reader take them for rot. Each allow goes when its family
    // adds its first variant. They are per-VARIANT and not on the enum on purpose:
    // `Unmodeled` above is constructed on the production address path, and an allow
    // wide enough to cover the family arms would also hide the day that stopped being
    // true.
    /// The `harness.bookkeeping` family's own facts.
    #[allow(dead_code)]
    Bookkeeping(record_text::bookkeeping::Fact),
    /// The attachment family's own facts.
    #[allow(dead_code)]
    Attachments(record_text::attachments::Fact),
    /// The isMeta-injection family's own facts.
    #[allow(dead_code)]
    Injections(record_text::injections::Fact),
    /// The `type:"system"` family's own facts.
    #[allow(dead_code)]
    System(record_text::system::Fact),
    /// The thinking family's own facts.
    #[allow(dead_code)]
    Thinking(record_text::thinking::Fact),
    /// The exposed-keys family's own facts.
    #[allow(dead_code)]
    Keys(record_text::keys::Fact),
    /// The test-only sentinel `record_text::probe` pushes on a fixture record, so a unit
    /// test can observe a FACT crossing the stamping site and not only a [`Marker`]. The
    /// two travel on different fields and are merged by different lines, so a suite that
    /// reads markers alone leaves half the seam unpinned. `#[cfg(test)]`, like the arm
    /// that produces it: the built binary carries neither.
    #[cfg(test)]
    Probe(&'static str),
}

impl HitFact {
    /// The label-zone tag this fact adds, if any. `None` when the fact is carried in
    /// the label itself or in JSON alone.
    #[must_use]
    pub(crate) fn label_zone(&self) -> Option<String> {
        match self {
            // The shape IS the label for an unmodeled unit, so it adds no second tag.
            HitFact::Unmodeled(_) => None,
            HitFact::Bookkeeping(f) => f.label_zone(),
            HitFact::Attachments(f) => f.label_zone(),
            HitFact::Injections(f) => f.label_zone(),
            HitFact::System(f) => f.label_zone(),
            HitFact::Thinking(f) => f.label_zone(),
            HitFact::Keys(f) => f.label_zone(),
            #[cfg(test)]
            HitFact::Probe(head) => Some((*head).to_string()),
        }
    }

    /// The JSON key(s) and value(s) this fact adds to its hit object. A list rather than
    /// one pair because a family fact can be several keys, and because the `for` edges
    /// all project the SAME key and accumulate into one array ([`attach_facts`]).
    #[must_use]
    pub(crate) fn json(&self) -> Vec<(&'static str, serde_json::Value)> {
        match self {
            HitFact::Unmodeled(shape) => vec![("unmodeled", serde_json::json!(shape))],
            HitFact::Bookkeeping(f) => f.json(),
            HitFact::Attachments(f) => f.json(),
            HitFact::Injections(f) => f.json(),
            HitFact::System(f) => f.json(),
            HitFact::Thinking(f) => f.json(),
            HitFact::Keys(f) => f.json(),
            #[cfg(test)]
            HitFact::Probe(head) => vec![("family_probe", serde_json::json!(head))],
        }
    }
}

/// Fold every fact onto a hit's JSON object - the ONE merge rule, shared by `search`'s
/// hit rows and `show`'s record rows so the two can never disagree.
///
/// An ARRAY value APPENDS to an array already under that key; anything else replaces.
/// That is what lets one unit carry several facts of the SAME kind - each projects a
/// one-element array under the one key and the list grows, where a plain assignment
/// would leave the last one alone on the wire and silently drop the rest.
pub(crate) fn attach_facts(obj: &mut serde_json::Value, facts: &[HitFact]) {
    for f in facts {
        for (key, value) in f.json() {
            match (obj.get_mut(key), value) {
                (Some(serde_json::Value::Array(existing)), serde_json::Value::Array(add)) => {
                    existing.extend(add);
                }
                (_, value) => obj[key] = value,
            }
        }
    }
}
