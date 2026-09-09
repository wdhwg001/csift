//! The recorded-leaf accumulation and the newest-by-timestamp last resort - the two ends
//! of the leaf choice that decide which branch of a forked transcript IS the conversation.

use super::*;

/// A `last-prompt` line: no timestamp of its own (the harness writes none), so it never
/// competes in the newest-by-timestamp fallback.
fn last_prompt(leaf: Option<&str>, explicit: Option<bool>) -> String {
    let mut fields = String::from(r#""type":"last-prompt""#);
    if let Some(l) = leaf {
        fields.push_str(&format!(r#","leafUuid":"{l}""#));
    }
    if let Some(e) = explicit {
        fields.push_str(&format!(r#","explicit":{e}"#));
    }
    format!("{{{fields},\"lastPrompt\":\"go\"}}")
}

/// A record with `isSidechain` written explicitly.
fn side(uuid: &str, parent: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","parentUuid":"{parent}","isSidechain":true,"timestamp":"{ts}","message":{{"role":"assistant","id":"m-{uuid}","content":[{{"type":"text","text":"aside"}}]}}}}"#
    )
}

#[test]
fn the_newest_fallback_skips_the_other_lane_and_the_replay_copies_and_keeps_the_first_of_a_tie() {
    // An explicit null leaf empties the leaf set, and an empty set drops the pick to the
    // newest record by TIMESTAMP. Three records here carry a later instant than the winner
    // and none of them may take it: one is on the sidechain lane, one is an earlier line of
    // a duplicated uuid (the later line is the survivor), and the third ties with the
    // winner - a tie is decided by the first, so the answer does not move with file order.
    let r = recs(&[
        &u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        &a("a1", "u0", "the winner", "2026-06-07T05:00:05.000Z"),
        &a(
            "a2",
            "u0",
            "ties with the winner",
            "2026-06-07T05:00:05.000Z",
        ),
        &side("sx", "u0", "2026-06-07T05:00:09.000Z"),
        &a("dup", "u0", "a replay copy", "2026-06-07T05:00:08.000Z"),
        &a("dup", "u0", "its survivor", "2026-06-07T05:00:01.000Z"),
        &last_prompt(None, Some(true)),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(
        c.leaf_source,
        LeafSource::Cleared,
        "the leaf set was emptied"
    );
    assert_eq!(
        c.leaf_index,
        Some(1),
        "the newest eligible record, and the first of the tie"
    );
}

#[test]
fn a_removed_record_never_wins_the_newest_fallback() {
    // The cut takes records out of the loader's map, and the newest fallback reads that map.
    // `a0` carries the file's latest instant (an out-of-order write) and is on the far side
    // of a compaction, so it is not a candidate for anything - taking it would start the
    // walk inside a region the loader has already dropped.
    let r = recs(&[
        &u("u0", "", "dropped prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "dropped reply", "2026-06-07T05:09:00.000Z"),
        &u("k1", "a0", "kept prompt", "2026-06-07T05:01:00.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"timestamp":"2026-06-07T05:02:00.000Z","compactMetadata":{"trigger":"auto","preservedMessages":{"anchorUuid":"a0","uuids":["k1"]}}}"#,
        &u("u1", "k1", "new prompt", "2026-06-07T05:03:00.000Z"),
        &last_prompt(None, Some(true)),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.leaf_source, LeafSource::Cleared);
    assert_eq!(
        c.leaf_index,
        Some(4),
        "the newest record still in the map, not the newest line in the file"
    );
    let s = survivals(&c, r.len());
    assert_eq!(&s[0..2], ["pre-cut", "pre-cut"]);
}

#[test]
fn one_explicit_last_prompt_pins_the_leaf_and_a_repeat_keeps_it_pinned() {
    // `explicit` is what a pinned leaf means: the loader stops replacing it with the file
    // tail. A line that SAYS explicit sets it; a later line naming the SAME leaf keeps it;
    // a later line naming a DIFFERENT leaf drops back to the ordinary recorded form.
    let base = [
        u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        a("a0", "u0", "charting", "2026-06-07T05:00:05.000Z"),
        u("u1", "a0", "sound the reef", "2026-06-07T05:01:00.000Z"),
        a("a1", "u1", "sounding", "2026-06-07T05:01:05.000Z"),
    ];
    let build = |tail: &[String]| -> Chain {
        let mut lines: Vec<&str> = base.iter().map(String::as_str).collect();
        lines.extend(tail.iter().map(String::as_str));
        let r: Vec<Record> = lines.iter().map(|l| parse(l)).collect();
        Chain::build(&r, None)
    };

    let one = build(&[last_prompt(Some("a0"), Some(true))]);
    assert_eq!(
        one.leaf_source,
        LeafSource::Explicit,
        "a single line saying explicit is enough"
    );
    assert_eq!(one.leaf_index, Some(1), "and the tail does not replace it");

    let repeated = build(&[
        last_prompt(Some("a0"), Some(true)),
        last_prompt(Some("a0"), None),
    ]);
    assert_eq!(
        repeated.leaf_source,
        LeafSource::Explicit,
        "a repeat of the same leaf keeps the pin"
    );

    let moved = build(&[
        last_prompt(Some("a0"), Some(true)),
        last_prompt(Some("u1"), None),
    ]);
    assert_eq!(
        moved.leaf_source,
        LeafSource::LastPrompt,
        "naming a different leaf drops the pin"
    );

    let never = build(&[last_prompt(Some("a0"), None), last_prompt(Some("a0"), None)]);
    assert_eq!(
        never.leaf_source,
        LeafSource::LastPrompt,
        "two ordinary lines never add up to a pin"
    );
}

#[test]
fn an_empty_leaf_uuid_clears_the_set_and_a_bare_line_leaves_it_alone() {
    // Two shapes that are NOT a recorded leaf. An empty `leafUuid` with `explicit` is the
    // cleared set, the same as a null one - reading it as a leaf named "" would resolve to
    // nothing and fall back to the file tail instead. A line with no leaf and no explicit
    // flag says nothing at all and must leave an earlier leaf standing.
    let base = [
        u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        a("a0", "u0", "charting", "2026-06-07T05:00:05.000Z"),
        u("u1", "a0", "sound the reef", "2026-06-07T05:01:00.000Z"),
        a("a1", "u1", "sounding", "2026-06-07T05:01:05.000Z"),
    ];
    let build = |tail: &[String]| -> Chain {
        let mut lines: Vec<&str> = base.iter().map(String::as_str).collect();
        lines.extend(tail.iter().map(String::as_str));
        let r: Vec<Record> = lines.iter().map(|l| parse(l)).collect();
        Chain::build(&r, None)
    };

    let emptied = build(&[last_prompt(Some(""), Some(true))]);
    assert_eq!(
        emptied.leaf_source,
        LeafSource::Cleared,
        "an empty leafUuid is not a leaf"
    );

    let untouched = build(&[last_prompt(Some("a0"), Some(true)), last_prompt(None, None)]);
    assert_eq!(
        untouched.leaf_source,
        LeafSource::Explicit,
        "a bare last-prompt line says nothing and clears nothing"
    );
    assert_eq!(untouched.leaf_index, Some(1));
}

#[test]
fn only_a_compaction_boundary_wipes_the_recorded_leaf() {
    // The loader forgets the recorded leaf when it passes a compaction boundary, and a
    // boundary is a `compact_boundary` - not merely a system record. The harness writes
    // plenty of other system subtypes between prompts, and treating one of them as a
    // boundary would silently throw away the leaf that decides the branch.
    let r = recs(&[
        &u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "charting", "2026-06-07T05:00:05.000Z"),
        &last_prompt(Some("a0"), Some(true)),
        r#"{"type":"system","subtype":"informational","uuid":"sy1","parentUuid":"a0","timestamp":"2026-06-07T05:00:50.000Z","level":"info","content":"a harness note"}"#,
        &u("u1", "a0", "sound the reef", "2026-06-07T05:01:00.000Z"),
        &a("a1", "u1", "sounding", "2026-06-07T05:01:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(
        c.leaf_source,
        LeafSource::Explicit,
        "a note is not a compaction"
    );
    assert_eq!(c.leaf_index, Some(1), "so the pinned leaf still stands");
}

#[test]
fn a_relink_anchor_skips_a_recorded_leaf_that_was_never_pinned() {
    // After a preserved-set relink the loader does not use the recorded leaf unless it was
    // PINNED - being merely recorded and resolvable is not enough. The leaf recorded here
    // sits on a branch the conversation left, so taking it would move the whole reading onto
    // that branch; the tip search names the branch the transcript actually ends on.
    let r = recs(&[
        &u("u0", "", "dropped prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "dropped reply", "2026-06-07T05:00:05.000Z"),
        &u("k1", "a0", "kept prompt", "2026-06-07T05:05:00.000Z"),
        &a("k2", "k1", "kept reply", "2026-06-07T05:05:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preservedMessages":{"anchorUuid":"a0","uuids":["k1","k2"]}}}"#,
        &u(
            "d1",
            "k2",
            "the branch left behind",
            "2026-06-07T05:10:30.000Z",
        ),
        &a("r1", "d1", "its reply", "2026-06-07T05:10:35.000Z"),
        &u("u1", "k2", "new prompt", "2026-06-07T05:11:00.000Z"),
        &a("a1", "u1", "new reply", "2026-06-07T05:11:05.000Z"),
        &last_prompt(Some("d1"), None),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(
        c.leaf_index,
        Some(8),
        "the tip the file ends on, not the recorded leaf"
    );
}
