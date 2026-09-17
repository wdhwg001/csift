//! What the ENGINE answers for every unit, rather than what a family states about one.
//!
//! A family edge exists because a record carries a key naming a target, so its presence IS
//! the fact and the root's set pin is the right shape for it. These are on a different
//! footing: `parent` is on every record that has one, `result_of` on every unit that
//! answers a call, and the root on every unit there is - so what is worth pinning about
//! them is which units they are on, which basis they carry and when they are ABSENT.

use crate::harness::*;

use super::{edges, Edge, ENGINE_KINDS, REACH_TARGET, SURVIVAL_KINDS};

/// The SURVIVAL relations the reachability fixture carries, and the leaves they sit on.
///
/// The fixture holds a rewound branch and a recalled draft, so three of the four appear here
/// without the file being built for them - which is the point of asserting them in this
/// suite as well as in the survival one: the vocabulary's cross-family view has to show them
/// under the leaves a reader will meet them on. `replays` needs two lines carrying one uuid,
/// which this fixture does not have and must not gain: it carries one line per LEAF, and a
/// second copy of a line would double a leaf's census.
#[test]
fn the_survival_relations_sit_on_the_leaves_that_carry_them() {
    let h = reachability_home();
    let got = edges(&h);
    let have: Vec<(&str, &str, &str, &str)> = got
        .iter()
        .filter(|e| SURVIVAL_KINDS.contains(&e.kind.as_str()))
        .map(Edge::tuple)
        .collect();
    assert_eq!(
        have,
        [
            ("agent.message.plain", "branches", "position", "line"),
            ("user.rewound", "continues", "position", "line"),
            ("user.unsent", "supersedes", "position", "line"),
        ]
    );
}

/// The ENGINE's own edges, on the terms that make them worth pinning.
///
/// `parent` is on a unit whose record names a `parentUuid` and NOT on a unit whose turn
/// opener is that same record under a family's more specific word - one relation, one
/// edge, which is the rule that keeps a reader from learning two words for the link
/// between an injection and the turn it was written under.
///
/// `result_of` is on a unit that ANSWERS a call, and its basis is `key`, because the API
/// itself wrote the `tool_use_id` that joins the two. Three leaves qualify: the plain
/// `agent.tool.result`, and the two user-facing duals that ride one - an answered
/// AskUserQuestion and a refused tool call are the person's message AND a result.
#[test]
fn the_engine_edges_are_on_the_units_that_have_them() {
    let h = reachability_home();
    let got = edges(&h);
    let engine: Vec<&Edge> = got
        .iter()
        .filter(|e| ENGINE_KINDS.contains(&e.kind.as_str()))
        .collect();

    for e in &engine {
        assert_eq!(
            e.basis,
            if e.kind == "result_of" {
                "key"
            } else {
                "position"
            },
            "{e:?}: the engine's basis is a statement about how it found the target"
        );
    }

    let result_of: Vec<&str> = engine
        .iter()
        .filter(|e| e.kind == "result_of")
        .map(|e| e.leaf.as_str())
        .collect();
    assert_eq!(
        result_of,
        ["agent.tool.result", "user.answer", "user.rejection"],
        "result_of belongs to a unit that answers a call: the plain tool result, and the \
         two user-facing duals that ride one"
    );

    // ONE RELATION, ONE EDGE: a leaf whose family already names the parent link carries
    // the family's word and not the generic one. These four are the shapes that do.
    for leaf in [
        "harness.hook.feedback",
        "harness.enforce.retry",
        "harness.schedule.fire",
        "harness.system.command",
    ] {
        assert!(
            !engine.iter().any(|e| e.leaf == leaf && e.kind == "parent"),
            "{leaf} carries both its family's word for the parent link and the engine's"
        );
        assert!(
            got.iter().any(|e| e.leaf == leaf && e.basis == "position"),
            "{leaf} lost the family edge whose TARGET stands the engine's parent edge down"
        );
    }
}

/// The two user-facing duals point at the CALL they answer, by line.
///
/// An answered AskUserQuestion and a refused tool call are the person's message and a tool
/// result at once, and the person's half is the richer view - so the unit renders under
/// `user.answer` / `user.rejection` and the tool-result view is deduped away. The API's own
/// `tool_use_id` is on the carrier either way, which is what makes `result_of` answerable
/// here: the answer names the ask, the rejection names the call it refused, and the reader
/// gets the LINE rather than a wire id.
///
/// `parent` stands down on both, under the one-relation-one-edge rule: on these shapes the
/// call is also the record above, so the two edges would resolve to the same line.
#[test]
fn the_user_facing_duals_name_the_call_they_answer() {
    let h = reachability_home();
    let mut argv: Vec<&str> = vec![
        "search",
        "",
        REACH_TARGET,
        "--format",
        "json",
        "--max-count",
        "0",
    ];
    argv.extend_from_slice(&EVERY_LEAF);
    let out = h.run(&argv);
    assert!(out.success, "scan failed: {}", out.stderr);

    let hits: Vec<serde_json::Value> = json_rows(&out.stdout, "exchange")
        .iter()
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .collect();
    // The line each call was MADE on, read off the tool_use unit that carries the id.
    let call_line = |id: &str| -> u64 {
        hits.iter()
            .find(|h| h["label"] == "agent.tool.use" && h["tool_use_id"] == id)
            .and_then(|h| h["line"].as_u64())
            .unwrap_or_else(|| panic!("no tool_use unit for {id}"))
    };
    for (leaf, id) in [
        ("user.answer", "toolu_auq"),
        ("user.rejection", "toolu_plan"),
    ] {
        let hit = hits
            .iter()
            .find(|h| h["label"] == leaf)
            .unwrap_or_else(|| panic!("no {leaf} unit"));
        assert_eq!(
            hit["tool_use_id"], id,
            "{leaf} carries the id of the call it answers"
        );
        let edges = hit["for"].as_array().cloned().unwrap_or_default();
        let result_of = edges
            .iter()
            .find(|e| e["kind"] == "result_of")
            .unwrap_or_else(|| panic!("{leaf} carries no result_of edge: {hit}"));
        assert_eq!(result_of["basis"], "key");
        assert_eq!(
            result_of["target"]["line"].as_u64(),
            Some(call_line(id)),
            "{leaf} points at the line the call was made on"
        );
        assert!(
            !edges.iter().any(|e| e["kind"] == "parent"),
            "{leaf}: parent stands down where it would name the same line"
        );
    }
}

/// EVERY emitted unit carries a root, and its FOUR answers are four shapes.
///
/// The root is the primitive's own question (which message does this thing correspond to),
/// so it is the one provenance field with no "only on records that state it" clause. A
/// consumer that had to tell "this unit opens its turn" from "csift did not look" out of a
/// missing key could not, which is why the object is always there.
///
/// The fourth shape is the one a real transcript meets first: the turn grouper folds a
/// synthetic pre-user LEAD into turn 0 (a SessionStart hook attachment, a
/// `<local-command-caveat>`, a compaction boundary - this fixture's head is the last of
/// those), so those units sit ABOVE the opener and there is nothing above THEM to name.
#[test]
fn every_unit_carries_a_root_and_names_which_answer_it_is() {
    let h = reachability_home();
    let mut argv: Vec<&str> = vec![
        "search",
        "",
        REACH_TARGET,
        "--format",
        "json",
        "--max-count",
        "0",
    ];
    argv.extend_from_slice(&EVERY_LEAF);
    let out = h.run(&argv);
    assert!(out.success, "scan failed: {}", out.stderr);

    let (mut openers, mut chained, mut placed, mut lead) = (0, 0, 0, 0);
    for row in json_rows(&out.stdout, "exchange") {
        for hit in row["hits"].as_array().cloned().unwrap_or_default() {
            let r = &hit["for_root"];
            assert!(r.is_object(), "every hit carries a root object: {hit}");
            // Keyed on the NOTE, because the note is what NAMES the answer; `line` can be
            // null on a chained root too (an opener merged from the elicitation sidecar has
            // no physical line), and a test that read the answer off the null-ness of a
            // field would call that shape a fifth one.
            match r["note"].as_str() {
                Some("turn opener") => {
                    assert!(r["line"].is_null() && r["hops"].is_null(), "{r}");
                    openers += 1;
                }
                None => {
                    assert!(
                        r["hops"].as_u64().is_some(),
                        "a reached root names its distance: {r}"
                    );
                    chained += 1;
                }
                Some("not reached by the parent chain") => {
                    assert!(r["hops"].is_null(), "{r}");
                    placed += 1;
                }
                Some("no opener above this unit") => {
                    assert!(r["line"].is_null() && r["hops"].is_null(), "{r}");
                    lead += 1;
                }
                other => panic!("a fifth root shape: {other:?} in {r}"),
            }
        }
    }
    assert!(openers > 0, "no unit opened its own turn");
    assert!(
        lead > 0,
        "the fixture opens with a compaction boundary, so turn 0 carries a folded lead"
    );
    assert!(chained > 0, "no unit reached its opener through the chain");
    assert!(
        placed > 0,
        "no unit was placed in a turn the chain could not carry it to - the fixture's \
         harness lines hang off records the prefilter drops, so some must be"
    );
}
