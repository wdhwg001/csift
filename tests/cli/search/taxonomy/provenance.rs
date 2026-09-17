//! The `for` PROVENANCE edges the taxonomy renders, pinned over the one reachability
//! fixture.
//!
//! Each family lane pinned the edges IT builds, in its own suite, from its own fixture.
//! What no per-family suite can answer is what the taxonomy as a WHOLE says about
//! provenance: which relation words exist, which basis each carries, and what shape a
//! target takes. That is a cross-family contract - a reader learns ONE vocabulary, not
//! six - and it is the contract any later change to the edge machinery has to preserve.
//!
//! So this pins the EDGE SET of the fixture that already carries one line per leaf: for
//! every emitted unit, every `for[]` entry, as (leaf, kind, basis, target shape). It runs
//! with an EMPTY pattern and the full selector set, the same way the leaf census does and
//! for the same reason - four leaves render no body and a pattern scan cannot reach them.
//!
//! WHY THE SHAPE AND NOT THE VALUE: a line number moves the day the fixture gains a line
//! above the edge, and a test that pinned it would fail on an unrelated edit while saying
//! nothing about provenance. What must not move is which of the builder's four answers an
//! edge takes - a LINE in this transcript, an id it could not place, a whole transcript,
//! or the honest empty - because that is the difference between an address a reader can
//! fetch and a handle they cannot.

use crate::harness::*;

/// The full-census target: the top-level transcript AND its subagent lane.
const REACH_TARGET: &str = "@3a1b2c3d-4e5f-4061-8273-849506a7b8c9";

/// Every `(leaf, kind, basis, target shape)` the reachability fixture renders, sorted.
///
/// FOUR SHAPES, and they are the builder's own four answers: `line` = the target uuid
/// names a record of this transcript, so the edge is an address; `uuid` = the record
/// names an id no line here carries (a wire id, or a record another file holds), kept
/// because the id is real; `session` = the target is a whole transcript and no record in
/// it; `unknown` = the record names nothing, and the edge says so rather than vanishing.
const EVERY_EDGE: [(&str, &str, &str, &str); 39] = [
    ("agent.communication.channel", "attached_to", "key", "line"),
    (
        "agent.communication.channel",
        "for_tool_use",
        "null",
        "unknown",
    ),
    ("agent.message.structured", "attached_to", "key", "line"),
    (
        "agent.message.structured",
        "for_tool_use",
        "null",
        "unknown",
    ),
    (
        "harness.bookkeeping.continued",
        "continued_in",
        "key",
        "session",
    ),
    (
        "harness.bookkeeping.file-history",
        "versions",
        "key",
        "line",
    ),
    ("harness.bookkeeping.fork", "forked_from", "key", "line"),
    ("harness.bookkeeping.last-prompt", "leaf", "key", "line"),
    ("harness.command.caveat", "attached_to", "position", "line"),
    ("harness.context.file", "attached_to", "key", "line"),
    ("harness.context.file", "for_tool_use", "null", "unknown"),
    ("harness.context.ide", "attached_to", "key", "line"),
    ("harness.context.ide", "for_tool_use", "null", "unknown"),
    ("harness.context.image", "attached_to", "position", "line"),
    ("harness.context.listing", "attached_to", "key", "line"),
    ("harness.context.listing", "for_tool_use", "null", "unknown"),
    ("harness.context.mode", "attached_to", "key", "line"),
    ("harness.context.mode", "for_tool_use", "null", "unknown"),
    ("harness.context.prompt", "attached_to", "key", "line"),
    ("harness.context.prompt", "for_tool_use", "null", "unknown"),
    ("harness.context.reminder", "attached_to", "key", "line"),
    (
        "harness.context.reminder",
        "for_tool_use",
        "null",
        "unknown",
    ),
    ("harness.context.skill", "attached_to", "position", "line"),
    ("harness.context.task", "attached_to", "key", "line"),
    ("harness.context.task", "for_tool_use", "null", "unknown"),
    ("harness.enforce.brief", "redoes", "position", "line"),
    ("harness.enforce.handback", "redoes", "position", "line"),
    ("harness.enforce.retry", "redoes", "position", "line"),
    (
        "harness.enforce.structured-output",
        "redoes",
        "position",
        "line",
    ),
    ("harness.hook.context", "attached_to", "key", "line"),
    ("harness.hook.context", "for_tool_use", "null", "unknown"),
    ("harness.hook.feedback", "blocked", "position", "line"),
    ("harness.hook.outcome", "attached_to", "key", "line"),
    ("harness.hook.outcome", "for_tool_use", "null", "unknown"),
    ("harness.schedule.event", "armed_by", "null", "unknown"),
    ("harness.schedule.fire", "fires", "position", "line"),
    ("harness.system.command", "wraps", "position", "line"),
    ("harness.system.refusal", "refused", "key", "line"),
    ("harness.turn.stop-hooks", "hook run", "key", "uuid"),
];

/// One rendered edge, flattened to what a reader can check.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    leaf: String,
    kind: String,
    basis: String,
    shape: &'static str,
}

impl Edge {
    fn tuple(&self) -> (&str, &str, &str, &str) {
        (&self.leaf, &self.kind, &self.basis, self.shape)
    }
}

/// Every `for` edge the reachability fixture renders, sorted and deduplicated.
fn edges(h: &Home) -> Vec<Edge> {
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
    let mut got: Vec<Edge> = Vec::new();
    for row in json_rows(&out.stdout, "exchange") {
        for hit in row["hits"].as_array().cloned().unwrap_or_default() {
            let leaf = hit["label"].as_str().unwrap_or("<none>").to_string();
            for e in hit["for"].as_array().cloned().unwrap_or_default() {
                let t = &e["target"];
                let shape = if t.is_null() {
                    "unknown"
                } else if !t["line"].is_null() {
                    "line"
                } else if !t["uuid"].is_null() {
                    "uuid"
                } else {
                    "session"
                };
                got.push(Edge {
                    leaf: leaf.clone(),
                    kind: e["kind"].as_str().unwrap_or_default().to_string(),
                    basis: e["basis"].as_str().unwrap_or("null").to_string(),
                    shape,
                });
            }
        }
    }
    got.sort();
    got.dedup();
    got
}

/// The provenance vocabulary the taxonomy renders, exactly: which leaf carries which
/// relation word, on which basis, resolved to which of the builder's four answers.
///
/// BOTH directions matter, which is why this is an equality and not a containment. A
/// MISSING row is an edge a reader could follow and now cannot. An EXTRA row is a second
/// edge for a relation that already had one - which is the failure a single shared
/// builder exists to prevent, because two words for one relation is a vocabulary a reader
/// has to learn twice.
#[test]
fn every_for_edge_the_taxonomy_renders() {
    let h = reachability_home();
    let got = edges(&h);
    let have: Vec<(&str, &str, &str, &str)> = got.iter().map(Edge::tuple).collect();

    let missing: Vec<&(&str, &str, &str, &str)> = EVERY_EDGE
        .iter()
        .filter(|w| !have.contains(*w))
        .collect::<Vec<_>>();
    assert!(missing.is_empty(), "edges no longer rendered: {missing:?}");

    let extra: Vec<&(&str, &str, &str, &str)> =
        have.iter().filter(|g| !EVERY_EDGE.contains(g)).collect();
    assert!(extra.is_empty(), "edges outside the pinned set: {extra:?}");
}

/// A `null` basis belongs to the honest empty and to nothing else.
///
/// The basis is the edge's honesty column, so a RESOLVED edge always names how it was
/// found - `key` when the record states the target, `position` when csift read it off the
/// record's place in the file. An edge that resolved a target while declining to say how
/// would be exactly the unweighable answer the column exists to prevent, and an unknown
/// carrying a basis would claim a reading of a record that says nothing.
#[test]
fn only_an_unknown_edge_carries_a_null_basis() {
    for (leaf, kind, basis, shape) in EVERY_EDGE {
        assert_eq!(
            basis == "null",
            shape == "unknown",
            "{leaf} / {kind}: basis {basis} with target shape {shape}"
        );
    }
}
