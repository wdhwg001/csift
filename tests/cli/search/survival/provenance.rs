//! The four SURVIVAL relations as `for` edges, end to end.
//!
//! The survival axis already renders every one of these answers for a person to read -
//! `[replay copy of L2]`, `[abandoned]`, `[rewound]`, the draft-distance line. What it had
//! no shape for was a consumer FOLLOWING them: a machine holding a hit had to parse an
//! English marker to learn which line replaced a recalled draft. The edges are that shape,
//! and they are stated under words the closed vocabulary declares.
//!
//! They ride JSON alone, so every test here reads the wire form - and the one that reads
//! the TEXT surface reads it to prove nothing moved there.

use super::*;

/// L1+L2 carry one uuid (L1 is the replay copy), L3 answers it, L4 is a prompt nothing
/// answered, L5 one that WAS answered at L6 and then rewound past, and L7 is the surviving
/// opener that replaced both - all three prompts hang off L3.
fn branch_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting the lagoon"}]}}"#, "\n",
            r#"{"type":"user","uuid":"d1","parentUuid":"a0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern chanel"}}"#, "\n",
            r#"{"type":"user","uuid":"s1","parentUuid":"a0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"s1","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging the northern channel"}]}}"#, "\n",
            r#"{"type":"user","uuid":"s2","parentUuid":"a0","timestamp":"2026-06-07T05:03:00.000Z","message":{"role":"user","content":"survey the southern shoal instead"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"s2","timestamp":"2026-06-07T05:03:05.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"surveying the southern shoal"}]}}"#, "\n",
        ),
    );
    h
}

/// The four relations this file is about. Filtering to them is deliberate: a record also
/// carries the engine's `parent` edge, which is a different fact and is pinned elsewhere,
/// and a test that asserted the whole list would fail the day an unrelated relation landed.
const SURVIVAL_KINDS: [&str; 4] = ["supersedes", "continues", "replays", "branches"];

/// The SURVIVAL `for[]` entries of the hit on `line`, as `(kind, basis, target line)`.
fn edges_at(out: &str, line: u64) -> Vec<(String, String, Option<u64>)> {
    rows(out)
        .iter()
        .filter(|r| r["kind"] == "exchange")
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .filter(|h| h["line"].as_u64() == Some(line))
        .flat_map(|h| h["for"].as_array().cloned().unwrap_or_default())
        .map(|e| {
            (
                e["kind"].as_str().unwrap_or_default().to_string(),
                e["basis"].as_str().unwrap_or("null").to_string(),
                e["target"]["line"].as_u64(),
            )
        })
        .filter(|(k, _, _)| SURVIVAL_KINDS.contains(&k.as_str()))
        .collect()
}

/// Every relation word on the hit at `line`, survival or not - for the assertions about
/// what did NOT move.
fn kinds_at(out: &str, line: u64) -> Vec<String> {
    rows(out)
        .iter()
        .filter(|r| r["kind"] == "exchange")
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .filter(|h| h["line"].as_u64() == Some(line))
        .flat_map(|h| h["for"].as_array().cloned().unwrap_or_default())
        .map(|e| e["kind"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// The whole scan's edges, JSON, with every leaf reachable.
fn scan(h: &Home) -> String {
    let out = h.run(&[
        "search",
        "",
        &at(SESS),
        "--format",
        "json",
        "--max-count",
        "0",
    ]);
    assert!(out.success, "scan failed: {}", out.stderr);
    out.stdout
}

/// A recalled DRAFT names the message that replaced it, and a rewound prompt names the one
/// the conversation continued from instead.
///
/// Two relations rather than one, because the two facts are not the same: `supersedes` says
/// nothing ever answered this prompt, `continues` says something did and the conversation
/// was rewound past it anyway. Both resolve to the SAME surviving opener here, which is
/// what makes the distinction worth a word each - the target cannot carry it.
#[test]
fn a_draft_and_a_rewound_prompt_name_the_opener_that_replaced_them() {
    let out = scan(&branch_home());
    assert_eq!(
        edges_at(&out, 4),
        [("supersedes".to_string(), "position".to_string(), Some(7))],
        "the recalled draft on L4:\n{out}"
    );
    assert_eq!(
        edges_at(&out, 5),
        [("continues".to_string(), "position".to_string(), Some(7))],
        "the rewound prompt on L5:\n{out}"
    );
    // The edge is ADDED beside what the unit already carried: the engine's `parent` is
    // untouched, because the two name different records and are different facts.
    assert!(
        kinds_at(&out, 4).contains(&"parent".to_string()),
        "the parent edge stood down for a survival relation:\n{out}"
    );
}

/// A record on an abandoned branch names the HEAD it hangs off, and the head names none.
///
/// The branch head is the address a reader needs: an `[abandoned]` marker says the record
/// is off the conversation and nothing more, while `branches` says which prompt it was off
/// the conversation UNDER. On the head itself the edge would point at the record the reader
/// is already looking at.
#[test]
fn a_record_on_an_abandoned_branch_names_its_head() {
    let out = scan(&branch_home());
    assert_eq!(
        edges_at(&out, 6),
        [("branches".to_string(), "position".to_string(), Some(5))],
        "the reply on the rewound branch:\n{out}"
    );
    assert!(
        !edges_at(&out, 5).iter().any(|(k, _, _)| k == "branches"),
        "the head carries no edge to itself:\n{out}"
    );
    // A record on the SURVIVING conversation carries none of the four - it has a parent
    // like any record, and nothing the survival axis has to say.
    assert!(
        edges_at(&out, 8).is_empty(),
        "a live record states no survival relation:\n{out}"
    );
    assert_eq!(kinds_at(&out, 8), ["parent"], "{out}");
}

/// A replay COPY names the surviving line, on the key the loader itself joins on.
///
/// The two lines carry one uuid, so the join is the record's own id rather than a reading
/// of where it sits - which is why this one relation of the four is `key`. The existing
/// `replay_copy_of` field stays exactly what it was; the edge is the same answer in the
/// shape a consumer walks.
#[test]
fn a_replay_copy_names_its_survivor_on_the_shared_uuid() {
    let out = scan(&branch_home());
    assert_eq!(
        edges_at(&out, 1),
        [("replays".to_string(), "key".to_string(), Some(2))],
        "the earlier copy on L1:\n{out}"
    );
    let copy = rows(&out)
        .iter()
        .filter(|r| r["kind"] == "exchange")
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .find(|h| h["line"].as_u64() == Some(1))
        .unwrap_or_else(|| panic!("no hit on L1:\n{out}"));
    assert_eq!(
        copy["replay_copy_of"].as_u64(),
        Some(2),
        "the field the edge projects stays what it was: {copy}"
    );
}

/// The four ride JSON alone, and the TEXT surface is byte-unchanged by them.
///
/// Each restates a marker the same unit already prints, so a tag per hit would cost a
/// reader more than it tells them - the rule `parent` and `result_of` already follow. The
/// consequence is that a default text scan BUILDS none of them, which is also why the
/// census below has to be checked separately: it reads the edges while printing text.
#[test]
fn the_survival_relations_render_no_tag_and_the_census_still_sees_them() {
    let h = branch_home();
    let text = h.run(&["search", "", &at(SESS), "--max-count", "0"]);
    assert!(text.success, "{}", text.stderr);
    for kind in ["supersedes", "continues", "replays", "branches"] {
        assert!(
            !text.stdout.contains(&format!("[for: {kind}")),
            "{kind} rendered a label-zone tag:\n{}",
            text.stdout
        );
    }
    // The markers they restate are still there, on the text surface, unchanged.
    assert!(
        text.stdout.contains("[replay copy of L2]"),
        "{}",
        text.stdout
    );
    assert!(text.stdout.contains("[rewound]"), "{}", text.stdout);

    let census = h.run(&["search", "", &at(SESS), "--count-by", "for-kind"]);
    assert!(census.success, "{}", census.stderr);
    let keys: Vec<&str> = census
        .stdout
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .collect();
    for kind in ["supersedes", "continues", "replays", "branches"] {
        assert!(
            keys.contains(&kind),
            "{kind} is missing from the text census: {keys:?}\n{}",
            census.stdout
        );
    }
}
