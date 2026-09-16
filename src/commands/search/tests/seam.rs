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

/// ONE SPECIMEN PER LEAF of the two SHARED categories, and the family that answers for
/// each - the table the drift rows above cannot hold, because the answer is a property of
/// the RECORD rather than of the leaf.
///
/// `harness.hook` and `harness.schedule` are the only categories two families are offered,
/// and there first-Some-wins is a TIE-BREAK, not a fallback chain. A record belongs to one
/// family; a second family answering behind the first means both believe they own it, and
/// then the winner is a table order and the loser's text vanishes with no trace on any
/// surface. So the law is AT MOST ONE, and the column records which one it is today.
///
/// `None` is the honest current state for four of the seven, not a gap: an isMeta
/// injection's text is its own `message.content`, which the engine's generic arms already
/// join, and the routing exists so the family lane can give a template a head of its own
/// without touching the shared chain. Those rows flip to `injections` when it does, and
/// this table is where that gets said.
/// The rows are in `Class::ALL` order, which is what the totality check compares against.
const SHARED_CATEGORY_SPECIMENS: &[(Class, &str, Option<&str>)] = &[
    (
        Class::ScheduleWakeup,
        "{\"type\":\"user\",\"uuid\":\"s5\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"# Autonomous loop check\\n\\nthe armed body\"}}",
        None,
    ),
    (
        Class::ScheduleEvent,
        r#"{"type":"system","uuid":"s4","subtype":"scheduled_task_fire","content":"Running scheduled task (every day at 09:00)"}"#,
        Some("system"),
    ),
    (
        // The one leaf whose CLASSIFY needs a per-file index (`ClassifyCtx::
        // schedule_fires` joins it to the fire record it is parented to). The seam takes
        // the class it is handed, so the render question is answerable here regardless.
        Class::ScheduleFire,
        r#"{"type":"user","uuid":"s7","parentUuid":"s4","isMeta":true,"message":{"role":"user","content":"the operator armed this text"}}"#,
        None,
    ),
    (
        Class::ScheduleTick,
        "{\"type\":\"user\",\"uuid\":\"s6\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"# Autonomous loop tick\\n\\nthe driver body\"}}",
        None,
    ),
    (
        Class::HookContext,
        r#"{"type":"attachment","uuid":"s1","attachment":{"type":"hook_additional_context","content":["a hook said this"]}}"#,
        Some("attachments"),
    ),
    (
        Class::HookOutcome,
        r#"{"type":"attachment","uuid":"s2","attachment":{"type":"hook_success","hookName":"Stop"}}"#,
        Some("attachments"),
    ),
    (
        Class::HookFeedback,
        "{\"type\":\"user\",\"uuid\":\"s3\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"Stop hook feedback:\\n- keep going\"}}",
        None,
    ),
];

/// The sweep: the table is total over the two shared categories, and each specimen is
/// answered by AT MOST one family - the one the table names.
#[test]
fn exactly_one_family_answers_for_every_leaf_of_a_shared_category() {
    use crate::commands::search::record_text::{render_families, RenderFamily};
    let ctx = ClassifyCtx::top_level();
    // Total, in both directions: every leaf of a two-family category has a row, and no
    // row names a leaf whose category is not shared.
    let shared: Vec<Class> = Class::ALL
        .iter()
        .copied()
        .filter(|c| render_families(*c).len() > 1)
        .collect();
    let listed: Vec<Class> = SHARED_CATEGORY_SPECIMENS.iter().map(|(c, ..)| *c).collect();
    assert_eq!(
        shared, listed,
        "a leaf of a shared category has no specimen, or a row names an unshared leaf"
    );

    for (class, line, want) in SHARED_CATEGORY_SPECIMENS {
        let r = rec(line);
        let answered: Vec<&str> = render_families(*class)
            .iter()
            .filter(|f| {
                let got = match f {
                    RenderFamily::Attachments => {
                        crate::commands::search::record_text::attachments::render(
                            &r,
                            Some(line),
                            *class,
                            &ctx,
                        )
                    }
                    RenderFamily::Injections => {
                        crate::commands::search::record_text::injections::render(
                            &r,
                            Some(line),
                            *class,
                            &ctx,
                        )
                    }
                    RenderFamily::System => crate::commands::search::record_text::system::render(
                        &r,
                        Some(line),
                        *class,
                        &ctx,
                    ),
                    other => panic!("{:?} is not offered a shared category", other),
                };
                got.is_some()
            })
            .map(|f| f.name())
            .collect();
        assert!(
            answered.len() <= 1,
            "{}: {answered:?} both answer - two families claim one record, and the seam \
             discards the loser in silence",
            class.path()
        );
        assert_eq!(
            answered.first().copied(),
            *want,
            "{} is answered by a different family than the table names",
            class.path()
        );
    }
}

/// A SECOND family answering behind the first trips the seam's own assertion, by name.
///
/// The sweep above can only walk the specimens somebody wrote down; this one manufactures
/// the shape - an attachment record the injections probe also answers for - so the rule is
/// under the whole debug test suite rather than under seven rows. In release the
/// short-circuit stands and the first answer wins.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "a shadowed render")]
fn a_second_family_answering_behind_the_first_is_a_bug() {
    use crate::commands::search::record_text::{family_render, probe};
    let line = format!(
        r#"{{"type":"attachment","uuid":"s8","sessionId":"{}","attachment":{{"type":"hook_additional_context","content":["a hook said this"]}}}}"#,
        probe::SENTINEL_RENDER_SESSION
    );
    let r = rec(&line);
    let _ = family_render(
        &r,
        Some(&line),
        Class::HookContext,
        &ClassifyCtx::top_level(),
    );
}

/// A FAMILY REFUSES A RECORD THAT IS NOT ITS OWN, and the dispatch does not do it for it.
///
/// The seam reads `render_leaf(rec).or(class)`: a record with no leaf of its own arrives
/// carrying only the CALLER's class, so "this arm is only reached for leaves in this
/// category" is true of the leaf and false of the record. A bookkeeping render that
/// answered unconditionally handed a role-bearing record its whole raw JSON line as the
/// body - the human's own message replaced by the bytes around it - on every surface that
/// can hand a class in.
///
/// Pinned on the family function AND through the shared resolution, because those are two
/// different promises: the family says nothing about a record it does not own, and the
/// engine's own arms then render the record the way they always did.
#[test]
fn a_family_refuses_a_record_that_is_not_its_own() {
    use crate::commands::search::record_text::{bookkeeping, render_leaf};
    let ctx = ClassifyCtx::top_level();
    let line =
        r#"{"type":"user","uuid":"b1","message":{"role":"user","content":"the human said this"}}"#;
    let r = rec(line);
    assert_eq!(render_leaf(&r), None, "no leaf of its own");
    assert_eq!(
        bookkeeping::render(&r, Some(line), Class::BookkeepingMode, &ctx),
        None,
        "the family owns the line TYPE, not whichever class a caller asked with"
    );
    let text = record_raw_text(&r, Some(line), Some(Class::BookkeepingMode), &ctx)
        .expect("the engine's own arms answer for a conversation record");
    assert_eq!(text, "the human said this");
}

/// THE WHOLE-FILE GATE RE-RENDERS EVERY VIEW, not the line alone.
///
/// The seam dispatches on the line's own leaf when it HAS one and on the caller's CLASS
/// when it does not, and the second half is a whole family: an isMeta injection and a
/// `<task-notification>` are `type:"user"` lines with no attachment payload type and no
/// promoted class, so `render_leaf` answers `None` for both and the injections family is
/// reachable through a class or not at all. The gate used to re-render the LINE - one
/// call, no class - which made it structurally blind to that family: a file whose only
/// hit lay in such a head was pruned and reported as a definitive absence while
/// `show --line` rendered the head perfectly well. That is section 7's silent-drop
/// landmine on the one path csift allows to prove a negative.
///
/// The probe render stands in for a head the family lane has not written yet. What it
/// proves is the reachability, which is what the gate is blind to; the text itself is
/// deliberately a fixture's.
#[test]
fn the_whole_file_gate_reaches_a_render_only_a_class_can_name() {
    use crate::commands::search::record_text::{probe, render_leaf};
    let ctx = ClassifyCtx::top_level();
    let m = build_matcher(&args(probe::SENTINEL_RENDER)).unwrap();

    // An isMeta injection: the shape with no leaf of its own.
    let feedback = format!(
        r#"{{"type":"user","uuid":"g1","sessionId":"{}","isMeta":true,"message":{{"role":"user","content":"Stop hook feedback: keep going"}}}}"#,
        probe::SENTINEL_RENDER_SESSION
    );
    let fb = rec(&feedback);
    assert_eq!(render_leaf(&fb), None, "no leaf of its own");
    assert!(fb.classify(&ctx).contains(&Class::HookFeedback));
    assert!(
        !feedback.contains(probe::SENTINEL_RENDER),
        "the head is FABRICATED: a literal hit would prove nothing about the re-render"
    );
    assert!(
        m.synth_texts_match(&fb, Some(&feedback)),
        "the gate must reach the family render, or it prunes the file the head is in"
    );

    // The WHOLE path on a line the marker registry already carries: stage 1 admits it
    // for re-rendering, stage 2 keeps the file. The notification leaves route to the
    // same family, so the same blindness applied.
    let pulse = format!(
        r#"{{"type":"user","uuid":"g2","sessionId":"{}","message":{{"role":"user","content":"<task-notification><task-id>b1a2b3c4</task-id><status>completed</status>a shell finished</task-notification>"}}}}"#,
        probe::SENTINEL_RENDER_SESSION
    );
    let pl = rec(&pulse);
    assert_eq!(render_leaf(&pl), None);
    assert!(
        m.synth_verifiable_hits(pulse.as_bytes()),
        "stage 1 admits the line for re-rendering"
    );
    assert!(
        m.synth_texts_match(&pl, Some(&pulse)),
        "stage 2 keeps the file"
    );

    // And the gate can still PRUNE, in BOTH of the two ways it has to. A record no
    // family answers for is a definitive absence, which is the property the whole gate
    // exists for; and so is a record a family DOES answer for whose text does not carry
    // the pattern - the loop has to walk past a `Some` that misses rather than treat any
    // answer as a reason to keep the file.
    let plain = r#"{"type":"user","uuid":"g3","isMeta":true,"message":{"role":"user","content":"Stop hook feedback: keep going"}}"#;
    assert!(!m.synth_texts_match(&rec(plain), Some(plain)));
    let elsewhere = build_matcher(&args("zzneverrendered")).unwrap();
    assert!(
        !elsewhere.synth_texts_match(&fb, Some(&feedback)),
        "a family answered and its text missed; the file is still provably absent"
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
