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
/// The dispatch key is the leaf's CATEGORY. Two categories are shared by two families -
/// `harness.hook` (`.context` and `.outcome` are attachment payloads, `.feedback` is an
/// isMeta injection) and `harness.schedule` (`.event` is the scheduler's own
/// `type:"system"` record, the other three are the prompts it injected) - so both
/// families are offered the record in a fixed order and each refuses what is not its
/// own. A category no family owns never calls anything.
pub(crate) fn family_render(
    rec: &Record,
    raw: Option<&str>,
    class: Class,
    ctx: &ClassifyCtx,
) -> Option<String> {
    match class.category() {
        "harness.bookkeeping" => bookkeeping::render(rec, raw, class, ctx),
        "agent.message" | "harness.context" => attachments::render(rec, raw, class, ctx),
        "harness.hook" => attachments::render(rec, raw, class, ctx)
            .or_else(|| injections::render(rec, raw, class, ctx)),
        "harness.system" => system::render(rec, raw, class, ctx),
        "harness.schedule" => system::render(rec, raw, class, ctx)
            .or_else(|| injections::render(rec, raw, class, ctx)),
        "agent.thinking" => thinking::render(rec, raw, class, ctx),
        "harness.command" | "harness.enforce" | "harness.notification" => {
            injections::render(rec, raw, class, ctx)
        }
        _ => None,
    }
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
