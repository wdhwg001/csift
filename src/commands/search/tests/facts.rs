//! The two EXTENSION POINTS as a consumer reads them: a family's own `Fact` reaching
//! both projections through [`HitFact`], and the one rule that folds facts onto a JSON
//! object.

use super::*;

use crate::commands::search::record_text::bookkeeping;

/// A family fact reaches BOTH projections through its `HitFact` arm.
///
/// This is the pin the six family arms rest on. Each arm forwards `label_zone` and
/// `json` to the family's own enum, and with every family uninhabited nothing takes
/// either forward - so both could be replaced by `None` and an empty list and the whole
/// suite would still pass. The bookkeeping family carries one `cfg(test)` variant for
/// exactly this, and the assertion is built from that variant's own constants rather
/// than from whatever the code returned.
#[test]
fn a_family_fact_reaches_both_projections_through_its_arm() {
    let fact = HitFact::Bookkeeping(bookkeeping::Fact::Probe("probe head"));
    assert_eq!(fact.label_zone().as_deref(), Some("probe head"));
    assert_eq!(
        fact.json(),
        vec![("bookkeeping_probe", serde_json::json!("probe head"))]
    );
}

/// And a family that has no variants yet projects NOTHING - which is what every other
/// arm does today, and why the pin above has to exist at all.
#[test]
fn an_uninhabited_family_contributes_no_projection() {
    // There is no value of an uninhabited `Fact` to build, so the statement is about
    // the hit: a unit of a record no family owns carries an empty fact list, and the
    // two projectors therefore add nothing to it.
    let mut obj = serde_json::json!({ "label": "user.message" });
    attach_facts(&mut obj, &[]);
    assert_eq!(obj, serde_json::json!({ "label": "user.message" }));
}

/// The MERGE rule, scalar side: a fact's key is written onto the object, and a second
/// fact writing the same key REPLACES it.
///
/// Replacement is the right answer for a scalar - two facts claiming one scalar key is a
/// family bug, and keeping the first would hide it behind stale data rather than showing
/// the newer answer. The ARRAY side of the same rule accumulates instead, which is what
/// lets one unit carry several facts of one kind.
#[test]
fn a_scalar_fact_key_is_written_and_a_second_replaces_it() {
    let mut obj = serde_json::json!({ "label": "harness.bookkeeping.mode" });
    attach_facts(
        &mut obj,
        &[
            HitFact::Bookkeeping(bookkeeping::Fact::Probe("first")),
            HitFact::Bookkeeping(bookkeeping::Fact::Probe("second")),
        ],
    );
    assert_eq!(obj["bookkeeping_probe"], serde_json::json!("second"));
    // The fixed keys the projector wrote before the facts are untouched: a fact
    // appends, it never rebuilds the row.
    assert_eq!(obj["label"], serde_json::json!("harness.bookkeeping.mode"));
}

// ── the ONE `for` edge ──
//
// Four families carry edges, and every one of them goes through `for_edge`, so these
// four cases are the whole contract: what a RESOLVED edge renders, what an edge naming a
// record this file does not hold renders, what an absent key renders, and how two edges
// on one unit reach the wire.

/// The lookup a test uses when it wants one uuid to resolve: everything else is absent,
/// exactly as a real transcript answers for an id no line carries.
struct OneLine(&'static str, usize);

impl crate::model::LineOfUuid for OneLine {
    fn line_of_uuid(&self, uuid: &str) -> Option<usize> {
        (uuid == self.0).then_some(self.1)
    }
}

/// A ctx whose uuid map holds exactly one record.
fn ctx_with(lookup: &dyn crate::model::LineOfUuid) -> ClassifyCtx<'_> {
    let mut ctx = ClassifyCtx::top_level();
    ctx.lines = Some(lookup);
    ctx
}

/// A RESOLVED edge renders the LINE, because a line is an address `csift show --line`
/// takes and a uuid is not.
#[test]
fn a_resolved_edge_renders_the_line_it_points_at() {
    let lookup = OneLine("target-uuid", 42);
    let fact = for_edge(
        EdgeKind::Refused,
        Basis::Key,
        Some("target-uuid"),
        None,
        &ctx_with(&lookup),
    );
    assert_eq!(fact.label_zone().as_deref(), Some("[for: refused L42]"));
    assert_eq!(
        fact.json(),
        vec![(
            "for",
            serde_json::json!([{
                "kind": "refused",
                "basis": "key",
                "target": { "line": 42, "uuid": "target-uuid", "session": null, "note": null },
                "reason": null,
            }])
        )]
    );
}

/// An edge naming a record NO line of this transcript carries keeps the id and says so.
/// Dropping it would lose a real fact: a compaction deleted the line, another session
/// holds it, or the id was never a record uuid at all - and only the id distinguishes
/// those.
#[test]
fn an_unresolved_edge_keeps_the_id_and_says_the_line_is_elsewhere() {
    let lookup = OneLine("some-other-record", 7);
    let fact = for_edge(
        EdgeKind::AttachedTo,
        Basis::Position,
        Some("abcdef0123456789"),
        None,
        &ctx_with(&lookup),
    );
    assert_eq!(
        fact.label_zone().as_deref(),
        Some("[for: attached_to abcdef0123456789 (not in this file)]"),
        "a non-uuid id prints whole, and the note says why there is no line"
    );
    assert_eq!(
        fact.json(),
        vec![(
            "for",
            serde_json::json!([{
                "kind": "attached_to",
                "basis": "position",
                "target": {
                    "line": null,
                    "uuid": "abcdef0123456789",
                    "session": null,
                    "note": "not in this file",
                },
                "reason": null,
            }])
        )]
    );
}

/// An ABSENT key is the honest empty, and it is rendered rather than omitted: an edge
/// that simply disappears reads as "there is no such relation", which is a different
/// claim from "this record does not say".
#[test]
fn an_absent_key_renders_the_honest_empty_with_its_reason() {
    let fact = for_edge(
        EdgeKind::Blocked,
        Basis::Key,
        None,
        None,
        &ClassifyCtx::top_level(),
    );
    assert_eq!(
        fact.label_zone().as_deref(),
        Some("[for: blocked unknown (the record carries no such key)]")
    );
    assert_eq!(
        fact.json(),
        vec![(
            "for",
            serde_json::json!([{
                "kind": "blocked",
                "basis": null,
                "target": null,
                "reason": "the record carries no such key",
            }])
        )]
    );
}

/// TWO edges on one unit both reach the wire. This is the array arm of the merge rule,
/// and the reason the `for` key carries a list: an attachment names its parent AND the
/// tool call it belongs to, and a projector that assigned the key would ship the second
/// alone with no sign the first existed.
#[test]
fn two_edges_on_one_unit_accumulate_under_the_one_key() {
    let lookup = OneLine("parent-uuid", 3);
    let ctx = ctx_with(&lookup);
    let mut obj = serde_json::json!({ "label": "harness.hook.context" });
    attach_facts(
        &mut obj,
        &[
            for_edge(
                EdgeKind::AttachedTo,
                Basis::Position,
                Some("parent-uuid"),
                None,
                &ctx,
            ),
            for_edge(EdgeKind::ForToolUse, Basis::Key, None, None, &ctx),
        ],
    );
    let edges = obj["for"].as_array().expect("the edges ride one array");
    assert_eq!(edges.len(), 2, "neither edge overwrote the other");
    assert_eq!(edges[0]["kind"], serde_json::json!("attached_to"));
    assert_eq!(edges[0]["target"]["line"], serde_json::json!(3));
    assert_eq!(edges[1]["kind"], serde_json::json!("for_tool_use"));
    assert_eq!(
        edges[1]["reason"],
        serde_json::json!("the record carries no such key")
    );
}

/// A ctx with NO row space answers `None` for every id, so an edge built there renders
/// the id rather than a guessed line. That is what a unit test and a file-oriented
/// surface both get, and it must never become a fabricated address.
#[test]
fn an_edge_built_without_a_row_space_renders_the_id_not_a_line() {
    let fact = for_edge(
        EdgeKind::Fires,
        Basis::Inferred,
        Some("11111111-2222-4333-8444-555555555555"),
        None,
        &ClassifyCtx::top_level(),
    );
    assert_eq!(
        fact.label_zone().as_deref(),
        Some("[for: fires 11111111 (not in this file)]")
    );
}

/// A target that is a WHOLE TRANSCRIPT and no record in it: the session IS the answer, so
/// the edge is RESOLVED and renders `@<session>` alone.
///
/// `continued-in` is the shape - a background handover names the child file, and the
/// record it moved to does not exist yet when the line is written. Before this the
/// builder had no way to say it: a session with no uuid rendered
/// `[for: continued_in @<session> unknown]`, which reads as an edge that failed to name a
/// record when there was never a record to name. The `unknown` word belongs to
/// `ForUnknown`, and the two facts must not render alike.
#[test]
fn a_target_that_is_a_whole_transcript_renders_the_session_alone() {
    let fact = for_edge(
        EdgeKind::ContinuedIn,
        Basis::Key,
        None,
        Some("88888888-7777-4666-8555-444444444444"),
        &ClassifyCtx::top_level(),
    );
    assert_eq!(
        fact.label_zone().as_deref(),
        Some("[for: continued_in @88888888-7777-4666-8555-444444444444]"),
        "the session prints whole: it is the `@` target a reader feeds back to csift"
    );
    assert_eq!(
        fact.json(),
        vec![(
            "for",
            serde_json::json!([{
                "kind": "continued_in",
                "basis": "key",
                "target": {
                    "line": null,
                    "uuid": null,
                    "session": "88888888-7777-4666-8555-444444444444",
                    "note": null,
                },
                "reason": null,
            }])
        )],
        "a resolved edge, not an unknown one: `reason` stays null"
    );
    // With NEITHER a uuid nor a session the answer is still the honest empty, so the new
    // arm widened the builder rather than replacing its `None` case.
    let none = for_edge(
        EdgeKind::ContinuedIn,
        Basis::Key,
        None,
        None,
        &ClassifyCtx::top_level(),
    );
    assert_eq!(
        none.label_zone().as_deref(),
        Some("[for: continued_in unknown (the record carries no such key)]")
    );
}

/// The THREE remaining shapes of an unresolved target, each of which a family can build
/// even though `for_edge` never produces them itself.
///
/// A target named only by POSITION carries no uuid at all and renders `unknown` in the id
/// slot - which is not the same tag as `ForUnknown`'s, and must not be, because "the
/// record points somewhere csift cannot name" and "the record points nowhere" are
/// different facts. A target in ANOTHER transcript prints its session, because an
/// eight-character handle with no session is not an address a reader can use. And a
/// target with neither line nor note prints the id alone rather than an empty
/// parenthesis.
#[test]
fn an_unresolved_target_renders_what_it_has_and_no_more() {
    let zone = |t: ForTarget| {
        HitFact::For {
            kind: EdgeKind::Blocked,
            basis: Basis::Position,
            target: t,
        }
        .label_zone()
    };
    assert_eq!(
        zone(ForTarget {
            note: Some("the parent link names no record".to_string()),
            ..ForTarget::default()
        })
        .as_deref(),
        Some("[for: blocked unknown (the parent link names no record)]")
    );
    assert_eq!(
        zone(ForTarget {
            uuid: Some("abcdef0123456789".to_string()),
            session: Some("11111111-2222-4333-8444-555555555555".to_string()),
            note: Some("not in this file".to_string()),
            line: None,
        })
        .as_deref(),
        Some(
            "[for: blocked @11111111-2222-4333-8444-555555555555 abcdef0123456789 \
             (not in this file)]"
        )
    );
    assert_eq!(
        zone(ForTarget {
            uuid: Some("abcdef0123456789".to_string()),
            ..ForTarget::default()
        })
        .as_deref(),
        Some("[for: blocked abcdef0123456789]"),
        "no note, no parenthesis"
    );
}

/// The EIGHT-CHARACTER CUT is for a uuid and for nothing else.
///
/// It works on a uuid because the first eight hex digits identify one: the other 28 are
/// the same shape, and the compaction excerpt has printed that handle for versions. A
/// WIRE id is the opposite shape - its head is the constant part. `hook-bb000000` cut to
/// eight is `hook-bb0`, and `toolu_01zzreef` is `toolu_01`, which is the tool prefix
/// every one of them carries: the cut throws away the whole distinguishing tail and keeps
/// what does not distinguish. Both shapes reach this render through a real edge (the
/// injections family's `attached_to` on a notification frame's `<tool-use-id>`, and the
/// attachment family's `for_tool_use`), so the two are pinned beside a uuid rather than
/// left to whichever family happens to notice.
#[test]
fn only_a_uuid_shaped_target_is_cut_to_its_handle() {
    let zone = |uuid: &str| {
        HitFact::For {
            kind: EdgeKind::ForToolUse,
            basis: Basis::Key,
            target: ForTarget {
                uuid: Some(uuid.to_string()),
                note: Some("not in this file".to_string()),
                ..ForTarget::default()
            },
        }
        .label_zone()
    };
    assert_eq!(
        zone("11111111-2222-4333-8444-555555555555").as_deref(),
        Some("[for: for_tool_use 11111111 (not in this file)]"),
        "a uuid keeps the handle it has always printed"
    );
    assert_eq!(
        zone("hook-bb000000").as_deref(),
        Some("[for: for_tool_use hook-bb000000 (not in this file)]"),
        "a synthetic hook id prints whole: `hook-bb0` names nothing"
    );
    assert_eq!(
        zone("toolu_01zzreef").as_deref(),
        Some("[for: for_tool_use toolu_01zzreef (not in this file)]"),
        "a tool-call id prints whole: `toolu_01` is the prefix they all share"
    );
}

/// The three BASIS words are the closed set a consumer filters on, so they are pinned
/// here rather than left to whatever each family happens to spell.
#[test]
fn the_basis_words_are_the_three_the_wire_carries() {
    assert_eq!(Basis::Key.as_str(), "key");
    assert_eq!(Basis::Position.as_str(), "position");
    assert_eq!(Basis::Inferred.as_str(), "inferred");
}

/// The unmodeled SHAPE still projects what it always did - one key, one string - through
/// the widened `Vec` return. The 1:1 law's whole wire surface is that key.
#[test]
fn the_unmodeled_shape_projects_one_key_through_the_widened_return() {
    let fact = HitFact::Unmodeled("system/some_future_subtype".to_string());
    assert_eq!(fact.label_zone(), None, "the shape IS the label, not a tag");
    assert_eq!(
        fact.json(),
        vec![("unmodeled", serde_json::json!("system/some_future_subtype"))]
    );
}
