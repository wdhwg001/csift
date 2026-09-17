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
use crate::model::{Block, Class, EdgeKind};

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

/// HOW a `for` edge was resolved - the honesty column of the edge.
///
/// An edge read off a KEY the record carries is a fact Claude Code wrote down. One read
/// off POSITION (the parent link, the record physically above) is csift's own reading of
/// the file's shape. An INFERRED one is neither: a join csift computed from two other
/// facts. A reader that cannot tell them apart cannot weigh them, so the basis rides
/// every resolved edge and is never defaulted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Basis {
    /// A key on the record names the target.
    Key,
    /// The target is where the record sits - a parent link, the line above.
    Position,
    /// csift joined it from other facts.
    // The only one of the three no family carries yet: every edge on the tree today is
    // read off a key or off the parent link. It stays because the honesty column is the
    // WHOLE closed set - a family that computes a join has a word for it waiting rather
    // than a reason to call its join a key - and the allow is per-VARIANT so the day one
    // is built the enum needs no edit.
    #[allow(dead_code)]
    Inferred,
}

impl Basis {
    /// The wire word, which is also what the closed set is named by.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Basis::Key => "key",
            Basis::Position => "position",
            Basis::Inferred => "inferred",
        }
    }
}

/// WHAT a resolved `for` edge points at.
///
/// Every field is optional because each one can genuinely be absent: a target in another
/// transcript has no line HERE, a target named by position has no uuid, a target inside
/// this file needs no session. The `note` is what an edge says about itself when the
/// line is missing (`not in this file`), so a reader is never left with a bare id and no
/// reason for it.
#[derive(Debug, Clone, Default)]
pub(crate) struct ForTarget {
    /// The 1-based jsonl line of the target, when it is in THIS transcript.
    pub(crate) line: Option<usize>,
    /// The target record's uuid, when the edge names one.
    pub(crate) uuid: Option<String>,
    /// The transcript the target lives in, when it is not this one.
    pub(crate) session: Option<String>,
    /// Why the line is absent, when it is.
    pub(crate) note: Option<String>,
}

impl ForTarget {
    /// The label-zone form: an ADDRESS when the target is in this file, else the id with
    /// whatever the edge can say about where it went.
    fn label_zone(&self, kind: &str) -> String {
        if let Some(line) = self.line {
            return format!("[for: {kind} L{line}]");
        }
        // A target that is a WHOLE TRANSCRIPT and not a record in one: the session IS the
        // answer, so it renders alone. Without this arm such an edge reads
        // `[for: <kind> @<session> unknown]`, which says the edge failed to name a record
        // when there was never a record to name - and `@<session>` is a target a reader
        // can feed straight back to csift.
        if let (None, Some(session)) = (self.uuid.as_deref(), self.session.as_deref()) {
            return match self.note.as_deref() {
                Some(note) => format!("[for: {kind} @{session} ({note})]"),
                None => format!("[for: {kind} @{session}]"),
            };
        }
        // No line: name the id, and prefix the SESSION when the target is in another
        // transcript - a bare handle is not addressable from the text surface, and the
        // JSON carries the session either way.
        //
        // THE CUT IS FOR A UUID AND FOR NOTHING ELSE. Eight characters is the handle the
        // compaction excerpt already prints, and it works there because a uuid's first
        // eight hex digits identify it: the other 28 are the same shape. A WIRE id is not
        // that. `hook-bb000000` cuts to `hook-bb0`, which is the word `hook` and three
        // digits; `toolu_01zzreef` cuts to `toolu_01`, which is the word `toolu` and the
        // version prefix every one of them carries. Both cuts throw away the whole
        // distinguishing tail and keep only the part that is constant across the
        // population, so the handle names nothing - which is worse than long.
        let id = self.uuid.as_deref().map_or_else(
            || "unknown".to_string(),
            |u| {
                if crate::path::is_uuid(u) {
                    u.chars().take(8).collect::<String>()
                } else {
                    u.to_string()
                }
            },
        );
        let id = match self.session.as_deref() {
            Some(s) => format!("@{s} {id}"),
            None => id,
        };
        match self.note.as_deref() {
            Some(note) => format!("[for: {kind} {id} ({note})]"),
            None => format!("[for: {kind} {id}]"),
        }
    }

    /// The wire form - every field, explicitly null where absent.
    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "line": self.line,
            "uuid": self.uuid,
            "session": self.session,
            "note": self.note,
        })
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
    /// A RESOLVED edge from this record to another: which relation (`kind`), how it was
    /// resolved ([`Basis`]) and what it points at ([`ForTarget`]). Built through
    /// [`record_text::for_edge`], the one place the uuid lookup happens.
    ///
    /// SHARED rather than per-family because four of the six families carry edges, and
    /// four separate renders of one idea is what a reader would then have to learn.
    For {
        /// The relation this edge names, from the closed provenance vocabulary.
        kind: EdgeKind,
        /// How the edge was resolved.
        basis: Basis,
        /// What it points at.
        target: ForTarget,
    },
    /// The HONEST EMPTY of the same edge: the record names no target, or the target is a
    /// DESCENT (one record to many), which is the direction no uuid map answers and a
    /// shape this edge could not carry anyway - it names ONE neighbour. Rendered, never
    /// omitted - a silently absent edge reads as "there is no such relation", which is a
    /// different claim from "this record does not say".
    ForUnknown {
        /// The relation the edge would have carried.
        kind: EdgeKind,
        /// Why there is no target.
        reason: String,
    },
    // THE SIX FAMILY ARMS, one per taxonomy family. Each carries an enum that lives in
    // that family's own `record_text/<family>.rs`, so a family adds variants to its own
    // file and edits neither this enum nor its parent.
    /// The `harness.bookkeeping` family's own facts.
    Bookkeeping(record_text::bookkeeping::Fact),
    /// The attachment family's own facts.
    Attachments(record_text::attachments::Fact),
    /// The isMeta-injection family's own facts.
    Injections(record_text::injections::Fact),
    /// The `type:"system"` family's own facts.
    System(record_text::system::Fact),
    /// The thinking family's own facts.
    Thinking(record_text::thinking::Fact),
    /// The exposed-keys family's own facts.
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
            // A relation the engine reads off the file's own shape rides JSON alone
            // ([`EdgeKind::rendered_in_label`]); a relation a record STATES renders its tag.
            HitFact::For { kind, target, .. } => kind
                .rendered_in_label()
                .then(|| target.label_zone(kind.as_str())),
            HitFact::ForUnknown { kind, reason } => {
                Some(format!("[for: {} unknown ({reason})]", kind.as_str()))
            }
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

    /// The RELATION this fact names, when it is a provenance edge - resolved or not,
    /// because both say the relation exists. `None` for every other kind of fact.
    ///
    /// The reader is the `--count-by for-kind` census, which is why this is a method on the
    /// enum rather than a match at the census: a family adding an edge reaches the axis with
    /// no edit there, exactly as it reaches the label zone and JSON with none.
    #[must_use]
    pub(crate) fn edge_kind(&self) -> Option<EdgeKind> {
        match self {
            HitFact::For { kind, .. } | HitFact::ForUnknown { kind, .. } => Some(*kind),
            _ => None,
        }
    }

    /// The JSON key(s) and value(s) this fact adds to its hit object. A list rather than
    /// one pair because a family fact can be several keys, and because the `for` edges
    /// all project the SAME key and accumulate into one array ([`attach_facts`]).
    #[must_use]
    pub(crate) fn json(&self) -> Vec<(&'static str, serde_json::Value)> {
        match self {
            HitFact::Unmodeled(shape) => vec![("unmodeled", serde_json::json!(shape))],
            // ONE key for every edge, carrying a ONE-element array, so several edges on
            // one unit accumulate through [`attach_facts`] instead of a key per relation
            // (which would make `for` unreadable without knowing every family's words).
            // `reason` is null on a resolved edge and the target is null on an unknown
            // one, so the objects have one key set either way.
            HitFact::For {
                kind,
                basis,
                target,
            } => vec![(
                "for",
                serde_json::json!([{
                    "kind": kind.as_str(),
                    "basis": basis.as_str(),
                    "target": target.json(),
                    "reason": serde_json::Value::Null,
                }]),
            )],
            HitFact::ForUnknown { kind, reason } => vec![(
                "for",
                serde_json::json!([{
                    "kind": kind.as_str(),
                    "basis": serde_json::Value::Null,
                    "target": serde_json::Value::Null,
                    "reason": reason,
                }]),
            )],
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
