//! The two SEAMS a taxonomy family builds against, and nothing else.
//!
//! Both exist so a family never edits this module's root: [`family_render`] is where the
//! family that owns a line's leaf gets to decide what that line SAYS, and [`for_edge`] is
//! the one builder every family's provenance edge goes through. They sit together because
//! they are the same promise from two sides - the render a family owns, and the fact it
//! may attach - and because the root was at the structure cap with both inside it.

use super::*;

/// THE RENDER SEAM: the family that owns a line's leaf decides what that line SAYS.
///
/// A family owns one classify file and one render file. Before this, its renders lived
/// in the shared chain instead - `record_raw_text`'s arms and `promoted.rs`'s per-leaf
/// match - so a family lane changing a body had to edit a file five other lanes were
/// editing at the same time. This dispatch is what moved those decisions into the
/// family files: `Some` wins, `None` falls through to the engine's own generic arms
/// below, and a family that has nothing to say about a record answers `None` on one
/// compare.
///
/// Which families are offered a record, and in which order, is [`render_families`]. Two
/// categories name two families, and there FIRST-SOME-WINS is a tie-break, never a
/// fallback chain: a record belongs to one family, and a SECOND family answering behind
/// the first means two families both believe they own it. The winner is arbitrary, the
/// loser's text is discarded in silence, and which one that is depends on a table order
/// nobody reading the output can see - so it is a bug, and this says so.
///
/// Release keeps the short-circuit: the first answer wins and the families behind it are
/// never asked. A DEBUG build asks them all and trips `debug_assert!` on a shadowed
/// answer, which is what puts the rule under the whole test suite rather than under the
/// two specimens a sweep can hold. The seam test sweeps the shared categories besides.
pub(crate) fn family_render(
    rec: &Record,
    raw: Option<&str>,
    class: Class,
    ctx: &ClassifyCtx,
) -> Option<String> {
    let families = render_families(class);
    let mut answer: Option<(RenderFamily, String)> = None;
    for &f in families {
        if answer.is_some() && !cfg!(debug_assertions) {
            break;
        }
        let Some(text) = render_one(f, rec, raw, class, ctx) else {
            continue;
        };
        match &answer {
            None => answer = Some((f, text)),
            Some((first, _)) => debug_assert!(
                false,
                "{}: {f:?} answers behind {first:?} - a shadowed render is two families \
                 claiming one record, not a fallback",
                class.path()
            ),
        }
    }
    answer.map(|(_, text)| text)
}

/// One family's render, by variant. Split out of [`family_render`] so the dispatch arm
/// list is read once and the shadowing rule above reads as the rule rather than as part
/// of a `match`.
fn render_one(
    family: RenderFamily,
    rec: &Record,
    raw: Option<&str>,
    class: Class,
    ctx: &ClassifyCtx,
) -> Option<String> {
    match family {
        RenderFamily::Bookkeeping => bookkeeping::render(rec, raw, class, ctx),
        RenderFamily::Attachments => attachments::render(rec, raw, class, ctx),
        RenderFamily::Injections => injections::render(rec, raw, class, ctx),
        RenderFamily::System => system::render(rec, raw, class, ctx),
        RenderFamily::Thinking => thinking::render(rec, raw, class, ctx),
    }
}

/// The five families that have a record-level render. `keys` is deliberately absent: its
/// facts are keys a record exposes, never what a line SAYS, so it has nothing to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderFamily {
    Bookkeeping,
    Attachments,
    Injections,
    System,
    Thinking,
}

impl RenderFamily {
    /// The family's own word, for the drift table a person reads. `cfg(test)` because
    /// that table is the only reader a word has: the dispatch matches on the variant,
    /// which costs nothing, and a string the binary never compares would be a name with
    /// no use rather than a name waiting for one.
    #[cfg(test)]
    pub(crate) fn name(self) -> &'static str {
        match self {
            RenderFamily::Bookkeeping => "bookkeeping",
            RenderFamily::Attachments => "attachments",
            RenderFamily::Injections => "injections",
            RenderFamily::System => "system",
            RenderFamily::Thinking => "thinking",
        }
    }
}

/// THE DISPATCH TABLE of the render seam, as DATA: which families are offered a leaf's
/// record, in order. EMPTY means the engine's own generic arms render that leaf.
///
/// A table rather than a `match` inside [`family_render`] because it is a CONTRACT six
/// lanes work against, and a contract a test can only re-derive is a contract nothing
/// checks. The drift test walks every leaf through this and pins the answer, so a family
/// that starts rendering a leaf it did not before has to say so.
///
/// The key is the leaf's CATEGORY, and TWO categories are shared by two families -
/// `harness.hook` (`.context` and `.outcome` are attachment payloads, `.feedback` is an
/// isMeta injection) and `harness.schedule` (`.event` is the scheduler's own
/// `type:"system"` record, the other three are the prompts it injected). There the ORDER
/// is the fact: the payload family goes first, because an attachment and a scheduler
/// record are what they are whichever leaf a caller asked for, while an injection is
/// decided by its text.
pub(crate) fn render_families(class: Class) -> &'static [RenderFamily] {
    match class.category() {
        "harness.bookkeeping" => &[RenderFamily::Bookkeeping],
        "agent.message" | "harness.context" => &[RenderFamily::Attachments],
        "harness.hook" => &[RenderFamily::Attachments, RenderFamily::Injections],
        "harness.system" => &[RenderFamily::System],
        "harness.schedule" => &[RenderFamily::System, RenderFamily::Injections],
        "agent.thinking" => &[RenderFamily::Thinking],
        "harness.command" | "harness.enforce" | "harness.notification" => {
            &[RenderFamily::Injections]
        }
        _ => &[],
    }
}

/// Every leaf whose CATEGORY names at least one family - the classes a caller's view can
/// carry into [`family_render`], and nothing else.
///
/// The reader is the §7f WHOLE-FILE GATE, the one path in csift allowed to prove a
/// negative. Its stage-2 re-render has to see everything emission would render, and a
/// record with no leaf of its OWN (every isMeta injection, every `<task-notification>`)
/// renders only through the caller's class - so a gate that re-renders the line alone
/// cannot reach that text at all, and prunes the file it is in.
///
/// It enumerates the classes a record COULD carry rather than the ones
/// [`Record::classify`] answers for it, and the difference is load-bearing. Several leaves
/// are decided by a per-FILE index the gate cannot have: [`Class::ScheduleFire`] is the
/// parent-link join `ClassifyCtx::schedule_fires`, and that index is built from the very
/// records the gate exists to avoid parsing. Classifying under the gate's neutral context
/// therefore returns a strict SUBSET of the emission's views, and a subset is exactly how
/// a file gets pruned. A superset can only cost work.
///
/// The work is bounded by the same order that makes the seam behaviour-identical: a line
/// WITH a leaf of its own ([`render_leaf`]) renders one way for every view, so the gate
/// walks this list only for the lines that have no leaf.
pub(crate) fn rendering_classes() -> &'static [Class] {
    static CLASSES: std::sync::OnceLock<Vec<Class>> = std::sync::OnceLock::new();
    CLASSES.get_or_init(|| {
        Class::ALL
            .iter()
            .copied()
            .filter(|c| !render_families(*c).is_empty())
            .collect()
    })
}

/// The leaf the RENDER SEAM dispatches on for a LINE, independent of which view a caller
/// is emitting.
///
/// The line's own leaf comes first and the caller's class is only the fallback, and that
/// order is what makes the seam behaviour-identical: `record_raw_text` answers "what does
/// this line say", which is a property of the line, so a `hook_additional_context`
/// attachment renders its joined content whether the caller asked for
/// `harness.hook.context` or for the `agent.communication.channel` view of the same
/// record. A NON-TRANSCRIPT line's leaf is decided by its own top-level type (or a
/// `type:"system"` subtype), which is [`Record::promoted_class`]; an ATTACHMENT's by its
/// payload type. Everything else - the isMeta injections above all - has no leaf of its
/// own to read, and there the caller's class is the only answer there is.
///
/// `None` from both leaves the engine's generic arms to answer, which is what every
/// conversation record gets.
pub(crate) fn render_leaf(rec: &Record) -> Option<Class> {
    if rec.is_type("attachment") {
        // A payload type csift models NO leaf for reaches no family, and falls to the
        // engine's verbatim-payload arm - the honest body for a shape csift has not
        // caught up with, and the one an address renders.
        return rec
            .attachment_type()
            .and_then(|t| crate::model::attachment_leaf(&t));
    }
    rec.promoted_class()
}

/// Build the ONE `for` EDGE fact from a uuid a record names - the only place the
/// uuid-to-line lookup happens, so every family's edge resolves and renders alike.
///
/// THREE answers, and the difference between them is the whole point of the helper:
/// - the record names a uuid this transcript carries -> [`HitFact::For`] with the LINE,
///   which is an address a reader can fetch;
/// - the record names a uuid no line here carries -> still [`HitFact::For`], with the id
///   and the note `not in this file`, because the id is real and worth printing (a
///   compaction deleted the line, another session holds it, or it is a wire id that was
///   never a record at all);
/// - the record names NOTHING -> [`HitFact::ForUnknown`] with the reason, never an
///   absent fact: a missing edge reads as "there is no such relation", which is a
///   different claim from "this record does not say".
///
/// `ctx.line_of_uuid` is backed by the chain's own uuid map, so the lookup costs nothing
/// and answers the way the loader would. A caller with no row space (a unit test, a
/// file-oriented surface) gets `None` from it, and the edge then renders its id rather
/// than a guessed line.
///
/// A DESCENT edge - one record to many - is not this helper's shape and never will be:
/// the map answers uuid to line in one direction only. A family that owns one builds
/// [`HitFact::ForUnknown`] with that as the reason.
// The ordinary "no caller yet" allow (AGENTS section 4): the amendment lands the helper
// so the four edge-carrying families all build their edges one way, and the first of
// them to land takes the allow away.
#[allow(dead_code)]
pub(crate) fn for_edge(
    kind: &'static str,
    basis: Basis,
    uuid: Option<&str>,
    ctx: &ClassifyCtx,
) -> HitFact {
    let Some(uuid) = uuid else {
        return HitFact::ForUnknown {
            kind,
            reason: "the record carries no such key".to_string(),
        };
    };
    let line = ctx.line_of_uuid(uuid);
    HitFact::For {
        kind,
        basis,
        target: ForTarget {
            line,
            uuid: Some(uuid.to_string()),
            session: None,
            note: line.is_none().then(|| "not in this file".to_string()),
        },
    }
}
