//! The RENDER SEAM's dispatch table, pinned per category and swept over every leaf.
//!
//! What this guards is ownership drift: a family that starts rendering a leaf it did not
//! before, a leaf that reaches two families, or a new leaf whose render nobody claims.
//! All three are silent at compile time and all three produce wrong text on one surface
//! and not another, which is the worst shape a render bug has.

use super::*;

use crate::commands::search::record_text::{render_families, RenderFamily};

/// Per CATEGORY, the families the seam offers a record to, in order - the whole table.
///
/// Written out by hand from the taxonomy map, never read back off the code: the point of
/// a drift test is that the two agree, and a table derived from the thing it checks
/// agrees with itself. `&[]` means the engine's own generic arms render every leaf of
/// that category.
const EXPECTED: &[(&str, &[&str])] = &[
    // The conversation itself: a message, a thought's record level, a tool call, a peer
    // message. The engine has rendered these since before the taxonomy existed.
    ("user", &[]),
    ("agent.tool", &[]),
    ("agent.communication", &[]),
    // A thinking LEAF is decided on a content block, so the record-level arm is empty by
    // construction; the family's real render is `thinking::block_body`, which the block
    // loop calls.
    ("agent.thinking", &["thinking"]),
    // `agent.message.structured` is an attachment payload the model produced; `.plain` is
    // an assistant text block the engine renders, and the family refuses it on the type
    // compare.
    ("agent.message", &["attachments"]),
    // The attachment families.
    ("harness.context", &["attachments"]),
    ("harness.hook", &["attachments", "injections"]),
    // The `type:"system"` event kinds, and the scheduler's own record beside the three
    // prompts it injected.
    ("harness.system", &["system"]),
    ("harness.schedule", &["system", "injections"]),
    // The isMeta injections. Their renders are empty today - an injection's text is its
    // own `message.content`, which the engine already joins - and the routing exists so
    // the family can give a template a head without touching the shared chain.
    ("harness.command", &["injections"]),
    ("harness.enforce", &["injections"]),
    ("harness.notification", &["injections"]),
    // The matchable-text law's own family.
    ("harness.bookkeeping", &["bookkeeping"]),
    // The engine's own, and each for its own reason: a compaction record renders its
    // content plus the metadata excerpt, an interrupt marker and a resume placeholder are
    // their own text, and the three turn closers keep the promoted renderer.
    ("harness.compaction", &[]),
    ("harness.interrupt", &[]),
    ("harness.resume", &[]),
    ("harness.turn", &[]),
];

/// The table is TOTAL over the taxonomy's categories, in both directions.
///
/// A category the table omits would leave its leaves' ownership unstated, and a table row
/// naming a category the taxonomy dropped would be a rule about nothing.
#[test]
fn the_render_table_covers_every_category_and_no_others() {
    let listed: Vec<&str> = EXPECTED.iter().map(|(c, _)| *c).collect();
    let mut sorted = listed.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), listed.len(), "a category is listed twice");
    let mut actual: Vec<&str> = Class::CATEGORIES.to_vec();
    actual.sort_unstable();
    assert_eq!(sorted, actual, "the table and the taxonomy disagree");
}

/// Every LEAF routes to exactly what its category's row says, and to each family at most
/// once.
///
/// The per-leaf sweep rather than a per-category one because the dispatch is free to stop
/// keying on the category one day; what the seam promises is about leaves, so that is
/// what is walked.
#[test]
fn every_leaf_routes_to_the_families_its_category_names() {
    for &c in Class::ALL {
        let row = EXPECTED
            .iter()
            .find(|(cat, _)| *cat == c.category())
            .unwrap_or_else(|| panic!("{} has no table row", c.path()));
        let got: Vec<&str> = render_families(c).iter().map(|f| f.name()).collect();
        assert_eq!(got, row.1, "{} routes elsewhere", c.path());
        let mut once = got.clone();
        once.sort_unstable();
        once.dedup();
        assert_eq!(
            once.len(),
            got.len(),
            "{} is offered to one family twice",
            c.path()
        );
    }
}

/// The EXPOSED-KEYS family never renders, and the table is where that is stated.
///
/// Its facts are keys a record exposes - which agent definition ran, which API request a
/// block came from - and none of those is what a line SAYS. So it has no `render` at all,
/// and a leaf routed to it would be a routing bug rather than a missing function.
#[test]
fn the_exposed_keys_family_owns_no_render() {
    let named: Vec<&str> = EXPECTED
        .iter()
        .flat_map(|(_, fams)| *fams)
        .copied()
        .collect();
    assert!(
        !named.contains(&"keys"),
        "the keys family has no render; nothing may route to it"
    );
    // And the five that DO render are exactly the five the seam knows.
    let five = [
        RenderFamily::Bookkeeping,
        RenderFamily::Attachments,
        RenderFamily::Injections,
        RenderFamily::System,
        RenderFamily::Thinking,
    ];
    for f in five {
        assert!(
            named.contains(&f.name()),
            "{} renders nothing the table names",
            f.name()
        );
    }
    let mut distinct: Vec<&str> = named.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), five.len(), "an unknown family is routed to");
}

/// The TWO renders that stay empty answer `None` for a record of their own family, which
/// is the difference between "this family has nothing to render" and "this family has not
/// been wired up".
///
/// A thinking leaf is decided on a content BLOCK, so a thinking unit never reaches the
/// record-level seam at all - `block_body` is the family's real render. An isMeta
/// injection's text is its own `message.content`, which the engine already joins. Both
/// are stated at their functions and both are pinned here, because an empty render that
/// nobody calls looks exactly like one somebody forgot.
#[test]
fn the_two_empty_renders_answer_nothing_for_their_own_records() {
    use crate::commands::search::record_text::{injections, thinking};
    let ctx = ClassifyCtx::top_level();
    let think = rec(
        r#"{"type":"assistant","uuid":"t1","message":{"role":"assistant","content":[{"type":"thinking","thinking":"weighing it up","signature":"c2ln"}]}}"#,
    );
    assert_eq!(
        thinking::render(&think, None, Class::AgentThinkingSummary, &ctx),
        None
    );
    let nudge = rec(
        r#"{"type":"user","uuid":"i1","isMeta":true,"message":{"role":"user","content":"[structured-output-enforce] call the tool"}}"#,
    );
    assert_eq!(
        injections::render(&nudge, None, Class::EnforceStructuredOutput, &ctx),
        None
    );
}

/// A thinking BLOCK's body is the family's, and the family answers only for the two block
/// kinds it owns: anything else is `None`, and the block loop emits nothing rather than a
/// fabricated body.
///
/// `thinking_block_leaf` already refuses everything but those two before the loop reaches
/// the arm, so the `None` cannot be produced in production today. It is pinned anyway,
/// because what it decides is what happens to a block shape a LATER build adds - and
/// "skipped" and "emitted with somebody else's text" are very different answers.
#[test]
fn a_block_this_family_does_not_own_has_no_body() {
    use crate::commands::search::record_text::thinking;
    assert_eq!(
        thinking::block_body(&Block::Text {
            text: "an assistant message".to_string()
        })
        .as_deref(),
        None
    );
    assert_eq!(
        thinking::block_body(&Block::Thinking {
            thinking: "weighing it up".to_string(),
            signature: None,
        })
        .as_deref(),
        Some("weighing it up"),
        "and it does own a thinking block"
    );
}

/// ONE leaf is rendered by a family that DELEGATES to the engine's promoted renderer, and
/// it is the only one: `harness.bookkeeping.file-history` builds its head from fields
/// `Record` models rather than from the raw line, so the family composes the promoted
/// head over the verbatim body instead of writing a second head.
///
/// Pinned because it is the one place where "the family owns the render" and "the engine
/// produced the bytes" are both true, and a reader who finds it by accident would take it
/// for the drift this file exists to catch.
#[test]
fn the_one_delegating_leaf_is_named() {
    let line = r#"{"type":"file-history-snapshot","messageId":"m1","snapshot":{"timestamp":"2026-06-07T05:00:00.000Z","trackedFileBackups":{"/Users/dev/a.txt":{"version":2}}}}"#;
    let r = rec(line);
    let ctx = ClassifyCtx::top_level();
    let head = promoted_record_text(&r).expect("the promoted renderer builds the head");
    let family = crate::commands::search::record_text::family_render(
        &r,
        Some(line),
        Class::BookkeepingFileHistory,
        &ctx,
    )
    .expect("the family renders the leaf");
    assert!(
        family.starts_with(&head),
        "the family composes the promoted head over the verbatim body"
    );
    assert!(
        family.ends_with(line),
        "and the body is the line's own bytes"
    );
}
