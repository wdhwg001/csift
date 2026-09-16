//! The provenance an attachment unit carries: the two shared `for` edges, the family's
//! own `produced_by` fact, their honest empties, and their JSON shape.

use super::*;

#[test]
fn every_attachment_unit_says_which_record_it_is_for() {
    // Two `for` edges and a producer on every unit, each read from a key the line carries.
    // The parent RESOLVES to a line, which is an address a reader can fetch; the payload's
    // tool-call id is a wire id no line of this transcript carries, so the edge keeps the
    // id WHOLE and says where it is not - a synthetic hook id is not a uuid, and the
    // eight-character handle would leave `hook-bb0`, which names nothing.
    let h = attachment_home();
    let out = h.run(&["search", "zzatt", &at_sess(), "--attachments"]);
    assert!(out.success, "stderr {}", out.stderr);
    let ctx = line_with(&out.stdout, "harness.hook.context");
    assert!(
        ctx.contains("[for: attached_to L1]"),
        "the injected context hangs from the person's message, resolved to its line:\n{ctx}"
    );
    assert!(
        ctx.contains("[for: for_tool_use hook-bb000000 (not in this file)]"),
        "and answers for a tool call no line here carries:\n{ctx}"
    );
    let run = line_with(&out.stdout, "zzatt stdout reef ok");
    assert!(
        run.contains("[produced_by zzatt-check --reef]"),
        "a hook run names the command it executed, whole:\n{run}"
    );
}

#[test]
fn an_edge_with_no_key_on_the_line_renders_its_reason() {
    // Honest empties: a payload type that records no producer says so and names the type,
    // rather than leaving a reader to guess whether csift looked.
    let h = attachment_home();
    let out = h.run(&["search", "zzatt skills", &at_sess(), "--attachments"]);
    assert!(out.success, "stderr {}", out.stderr);
    let listing = line_with(&out.stdout, "harness.context.listing");
    assert!(
        listing.contains("[for: for_tool_use unknown ("),
        "a payload with no tool-call id says so:\n{listing}"
    );
    assert!(
        listing.contains("[produced_by unknown (no producer key; type=skill_listing)]"),
        "and the producer fact names the type it read:\n{listing}"
    );
}

#[test]
fn the_edges_ride_json_under_the_shared_for_key() {
    // Both edges accumulate into ONE `for` array - the shared merge rule - so a consumer
    // reads every provenance edge of every family the same way, and the producer rides its
    // own key beside them because it points at a command rather than at a record.
    let h = attachment_home();
    let out = h.run(&[
        "search",
        "zzatt the reef brief",
        &at_sess(),
        "--attachments",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr {}", out.stderr);
    let hit: serde_json::Value = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["hits"][0]["label"] == "harness.hook.context")
        .map(|v| v["hits"][0].clone())
        .unwrap_or_else(|| panic!("no hook.context hit in:\n{}", out.stdout));
    let edges = hit["for"].as_array().expect("the shared for array");
    assert_eq!(edges.len(), 2, "both edges accumulate: {edges:?}");
    let parent = edges
        .iter()
        .find(|e| e["kind"] == "attached_to")
        .expect("the parent edge");
    assert_eq!(
        parent["target"]["uuid"], "aa000000-0000-4000-8000-000000000001",
        "the WHOLE uuid, which `show --uuid` takes directly"
    );
    assert_eq!(parent["target"]["line"], 1);
    assert_eq!(parent["basis"], "key");
    let tool = edges
        .iter()
        .find(|e| e["kind"] == "for_tool_use")
        .expect("the tool edge");
    assert_eq!(tool["target"]["uuid"], "hook-bb000000");
    assert_eq!(
        tool["target"]["line"],
        serde_json::Value::Null,
        "a tool-call id is not a record uuid, so no line is fabricated"
    );
    assert_eq!(
        hit["produced_by"]["command"],
        serde_json::Value::Null,
        "injected context records no command"
    );
    assert_eq!(
        hit["produced_by"]["hook_name"], "reef-brief",
        "and the fact names what the line DOES carry"
    );
}

#[test]
fn an_addressed_unit_carries_the_same_edges_a_scanned_one_does() {
    // The same keys must not answer two ways depending on how the record was reached.
    let h = attachment_home();
    let out = h.run(&["show", &at_sess(), "--line", "3", "--format", "json"]);
    assert!(out.success, "stderr {}", out.stderr);
    let row: serde_json::Value = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["label"] == "harness.hook.outcome")
        .unwrap_or_else(|| panic!("no addressed row in:\n{}", out.stdout));
    assert_eq!(row["produced_by"]["command"], "zzatt-check --reef");
    assert_eq!(
        row["produced_by"]["hook_name"], "reef-gate",
        "the hook's name rides the command branch too"
    );
    let edges = row["for"].as_array().expect("the shared for array");
    assert_eq!(
        edges
            .iter()
            .find(|e| e["kind"] == "for_tool_use")
            .expect("the tool edge")["target"]["uuid"],
        "toolu_01zzreef"
    );
    assert_eq!(
        edges
            .iter()
            .find(|e| e["kind"] == "attached_to")
            .expect("the parent edge")["target"]["line"],
        2
    );
}
