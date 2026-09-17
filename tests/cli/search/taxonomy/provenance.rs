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

/// The two relations the ENGINE builds for every unit rather than the six families for
/// the records that state them. They are excluded from the family pin below and asserted
/// on their own terms, because they are on a different footing: a family edge exists
/// because a record carries a key, so its presence IS the fact; an engine edge is on every
/// unit that has a parent or answers a call, so what is worth pinning about it is when it
/// is ABSENT.
const ENGINE_KINDS: [&str; 2] = ["parent", "result_of"];

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
    let have: Vec<(&str, &str, &str, &str)> = got
        .iter()
        .filter(|e| !ENGINE_KINDS.contains(&e.kind.as_str()))
        .map(Edge::tuple)
        .collect();

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

/// The ENGINE's own edges, on the terms that make them worth pinning.
///
/// `parent` is on a unit whose record names a `parentUuid` and NOT on a unit whose turn
/// opener is that same record under a family's more specific word - one relation, one
/// edge, which is the rule that keeps a reader from learning two words for the link
/// between an injection and the turn it was written under.
///
/// `result_of` is on a tool RESULT and on nothing else, and its basis is `key`, because
/// the API itself wrote the `tool_use_id` that joins the two.
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
        ["agent.tool.result"],
        "result_of belongs to a tool result and to nothing else"
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
            "{leaf} lost the family edge that made the engine stand down"
        );
    }
}

/// EVERY emitted unit carries a root, and its three answers are the three shapes.
///
/// The root is the primitive's own question (which message does this thing correspond to),
/// so it is the one provenance field with no "only on records that state it" clause. A
/// consumer that had to tell "this unit opens its turn" from "csift did not look" out of a
/// missing key could not, which is why the object is always there.
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

    let (mut openers, mut chained, mut placed) = (0, 0, 0);
    for row in json_rows(&out.stdout, "exchange") {
        for hit in row["hits"].as_array().cloned().unwrap_or_default() {
            let r = &hit["for_root"];
            assert!(r.is_object(), "every hit carries a root object: {hit}");
            match (r["note"].as_str(), r["line"].as_u64(), r["hops"].as_u64()) {
                (Some("turn opener"), None, None) => openers += 1,
                (None, Some(_), Some(_)) => chained += 1,
                (Some("not reached by the parent chain"), Some(_), None) => placed += 1,
                other => panic!("a fourth root shape: {other:?} in {r}"),
            }
        }
    }
    assert!(openers > 0, "no unit opened its own turn");
    assert!(chained > 0, "no unit reached its opener through the chain");
    assert!(
        placed > 0,
        "no unit was placed in a turn the chain could not carry it to - the fixture's \
         harness lines hang off records the prefilter drops, so some must be"
    );
}

/// `--count-by for-kind` censuses the PROVENANCE vocabulary: one key per relation the
/// scope carries, counted once per record.
///
/// The axis is what the closed vocabulary buys. Over an open set of relation words a
/// census would report whatever strings the family files happened to contain, with no way
/// to tell a relation from a typo; here the keys are a list a reader can be handed.
///
/// MULTI-KEY like `label`, because one unit carries several relations at once: an
/// attachment names its parent AND the call it belongs to, so a census that took the first
/// would report a vocabulary narrower than the one in use.
#[test]
fn count_by_for_kind_censuses_the_relations_the_scope_carries() {
    let h = reachability_home();
    let mut argv: Vec<&str> = vec![
        "search",
        "",
        REACH_TARGET,
        "--count-by",
        "for-kind",
        "--format",
        "json",
    ];
    argv.extend_from_slice(&EVERY_LEAF);
    let out = h.run(&argv);
    assert!(out.success, "census failed: {}", out.stderr);
    let rows = json_rows(&out.stdout, "census");
    let got: Vec<(&str, u64)> = rows
        .iter()
        .map(|r| {
            (
                r["key"].as_str().unwrap_or_default(),
                r["records"].as_u64().unwrap_or_default(),
            )
        })
        .collect();

    for (_, n) in &got {
        assert!(*n >= 1, "a census key with no records: {got:?}");
    }
    // Every relation word the family pin says the fixture renders IS a key here, and the
    // engine's two are keys as well: the census sees exactly what the hits carry.
    for (_, kind, _, _) in EVERY_EDGE {
        assert!(
            got.iter().any(|(k, _)| *k == kind),
            "{kind} is rendered on a hit but missing from the for-kind census: {got:?}"
        );
    }
    for kind in ENGINE_KINDS {
        assert!(
            got.iter().any(|(k, _)| *k == kind),
            "the engine's {kind} edge is missing from the census: {got:?}"
        );
    }
    let sum = json_summary(&out.stdout);
    assert_eq!(sum["axis"], "for-kind");
    assert_eq!(
        sum["distinct_keys"].as_u64().unwrap_or_default() as usize,
        got.len(),
        "the summary's key count is the row count"
    );
    // A record carrying no edge at all is EXCLUDED and counted, never silently dropped -
    // the chain head has no parent to point at, so the fixture has at least one.
    assert!(
        sum["excluded_records"].as_u64().unwrap_or_default() >= 1,
        "no record was excluded, so the domain rule is untested: {sum}"
    );
}
