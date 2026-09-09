//! The two passes that run BEFORE any leaf is picked - the preserved-set relink and the
//! pre-boundary cut - and the TIPS fallback the leaf choice drops into when the ordinary
//! branch has nothing to hand it.

use super::*;

fn recs(lines: &[&str]) -> Vec<Record> {
    lines.iter().map(|l| parse(l)).collect()
}

fn u(uuid: &str, parent: &str, text: &str, ts: &str) -> String {
    let p = if parent.is_empty() {
        "null".to_string()
    } else {
        format!("\"{parent}\"")
    };
    format!(
        r#"{{"type":"user","uuid":"{uuid}","parentUuid":{p},"timestamp":"{ts}","message":{{"role":"user","content":"{text}"}}}}"#
    )
}

fn a(uuid: &str, parent: &str, text: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"{uuid}","parentUuid":"{parent}","timestamp":"{ts}","message":{{"role":"assistant","id":"m-{uuid}","content":[{{"type":"text","text":"{text}"}}]}}}}"#
    )
}

fn att(uuid: &str, parent: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"attachment","uuid":"{uuid}","parentUuid":"{parent}","timestamp":"{ts}","attachment":{{"type":"hook_additional_context","content":["ctx"]}}}}"#
    )
}

fn survivals(c: &Chain, n: usize) -> Vec<&'static str> {
    (0..n).map(|i| c.survival(i).as_str()).collect()
}

#[test]
fn the_tips_fallback_takes_the_one_record_nothing_points_at() {
    // The ordinary leaf branch hands the choice over when the file tail climbs to no
    // conversation record at all - here a trailing attachment whose parent is not in this
    // file. What is left is the tip search: the ONE record nothing points at, collapsed to
    // its first conversational ancestor, kept only because that ancestor has no
    // conversational child of its own. Falling back to the newest record by timestamp
    // instead would pick the trailing attachment, which climbs to nothing, and the whole
    // transcript would read as unresolved.
    let r = recs(&[
        &u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        &att("x1", "u0", "2026-06-07T05:01:00.000Z"),
        &u("u2", "x1", "survey the shoal", "2026-06-07T05:02:00.000Z"),
        &att("x9", "not-in-this-file", "2026-06-07T05:09:00.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.leaf_index, Some(2), "the tip, not the newest record");
    let s = survivals(&c, r.len());
    assert_eq!(
        &s[0..3],
        ["live", "live", "live"],
        "the tip's whole ancestry, attachment included"
    );
    assert_eq!(
        s[3], "abandoned",
        "the trailing attachment is off the chain, above the floor"
    );
}

#[test]
fn two_tips_climbing_to_one_ancestor_are_one_candidate() {
    // Both attachments hang off the same reply, so both collapse to it. That is ONE
    // candidate, not two: counting it twice turns the outright winner into a tie, and a tie
    // is resolved by a file tail that here climbs to no conversation record at all - so the
    // whole transcript would come back unresolved over a duplicate.
    let r = recs(&[
        &u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        &a(
            "a0",
            "u0",
            "charting the lagoon",
            "2026-06-07T05:01:00.000Z",
        ),
        &att("x1", "a0", "2026-06-07T05:02:00.000Z"),
        &att("x2", "a0", "2026-06-07T05:03:00.000Z"),
        &att("x9", "not-in-this-file", "2026-06-07T05:09:00.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.leaf_index, Some(1), "the shared ancestor wins outright");
    let s = survivals(&c, r.len());
    assert_eq!(
        &s[0..4],
        ["live", "live", "live", "live"],
        "the leaf's own trailing attachments come with it"
    );
    assert_eq!(s[4], "abandoned");
}

#[test]
fn a_replay_copy_is_no_tip_of_its_own() {
    // The earlier of two lines carrying one uuid is a replay copy: the loader's map holds
    // the later one, so nothing can point at the earlier line and it looks unreferenced to
    // any rule that only asks who points where. It is still not a tip - the survivor is -
    // and admitting it would make two candidates out of one record.
    let r = recs(&[
        &u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        &a(
            "a0",
            "u0",
            "charting the lagoon",
            "2026-06-07T05:01:00.000Z",
        ),
        &u("d1", "a0", "sound the reef", "2026-06-07T05:02:00.000Z"),
        &att("x1", "a0", "2026-06-07T05:03:00.000Z"),
        &u("d1", "a0", "sound the reef", "2026-06-07T05:04:00.000Z"),
        &att("x9", "not-in-this-file", "2026-06-07T05:09:00.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(
        c.leaf_index,
        Some(4),
        "the surviving line of the duplicated uuid"
    );
    assert_eq!(c.replay_copies, 1, "the earlier line is the copy");
    let s = survivals(&c, r.len());
    assert_eq!(&s[0..2], ["live", "live"], "the ancestry the leaf reaches");
    assert_eq!(s[4], "live");
    assert_eq!(s[5], "abandoned");
}

#[test]
fn a_preserved_segment_is_walked_from_its_tail_up_to_its_head() {
    // The other half of the preserved metadata: a boundary that names a SEGMENT rather
    // than a list. The walk climbs `parentUuid` from the tail to the head and keeps what
    // it collected; a walk that stopped on its own first record would resolve nothing, and
    // an unresolvable segment aborts the whole pass - leaving the pre-compaction history
    // reading as if no compaction had happened.
    let r = recs(&[
        &u("u0", "", "dropped prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "dropped reply", "2026-06-07T05:00:05.000Z"),
        &u("k1", "a0", "kept prompt", "2026-06-07T05:05:00.000Z"),
        &a("k2", "k1", "kept reply", "2026-06-07T05:05:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preservedSegment":{"anchorUuid":"a0","headUuid":"k1","tailUuid":"k2"}}}"#,
        &u("u1", "k2", "new prompt", "2026-06-07T05:11:00.000Z"),
        &a("a1", "u1", "new reply", "2026-06-07T05:11:05.000Z"),
    ]);
    let c = Chain::build(&r, None);
    let s = survivals(&c, r.len());
    assert_eq!(s[0], "pre-cut", "the segment cut ran");
    assert_eq!(s[1], "pre-cut");
    assert_eq!(s[2], "live", "the segment head survives");
    assert_eq!(s[3], "live", "and its tail");
    assert_eq!(s[5], "live");
    assert_eq!(s[6], "live");
}

#[test]
fn the_orphan_reparent_is_conversation_records_only() {
    // The loader's fourth step re-points a surviving USER/ASSISTANT record whose parent the
    // cut removed. An attachment is not one of those, so it keeps the parent it was written
    // with - which here is a record the cut removed, leaving the file with no reachable
    // conversation at all. Re-pointing it would invent a chain: the attachment would climb
    // to the preserved tail and the transcript would read as live.
    let r = recs(&[
        &u("u0", "", "dropped prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "dropped reply", "2026-06-07T05:00:05.000Z"),
        &u("k1", "a0", "kept prompt", "2026-06-07T05:05:00.000Z"),
        &a("k2", "k1", "kept reply", "2026-06-07T05:05:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preservedMessages":{"anchorUuid":"a0","uuids":["k1","k2"]}}}"#,
        &att("x1", "u0", "2026-06-07T05:11:00.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert!(
        c.leaf_index.is_none(),
        "no record climbs to a conversation record, so there is no leaf"
    );
    assert!(
        survivals(&c, r.len()).iter().all(|v| *v == "pre-cut"),
        "csift declines to call anything abandoned in a region it could not resolve"
    );
}

#[test]
fn a_cut_record_and_a_line_the_loader_never_admits_are_both_outside_the_tip_search() {
    // Two records here are not in the loader's map at all: `u0`/`a0`, which the cut removed,
    // and the `file-history-snapshot` line, which is not one of the four types the loader
    // loads. Neither may take part in the tip search - a cut record would be a tip of its
    // own (nothing can point at it once it is out of the map), and an unloaded line would
    // make the real tip look referenced. Either way the single candidate becomes a tie or a
    // blank, and the tie breaks on a file tail that climbs to nothing.
    let r = recs(&[
        &u("u0", "", "dropped prompt", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "dropped reply", "2026-06-07T05:00:05.000Z"),
        &u("k1", "a0", "kept prompt", "2026-06-07T05:05:00.000Z"),
        &a("k2", "k1", "kept reply", "2026-06-07T05:05:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preservedMessages":{"anchorUuid":"a0","uuids":["k1","k2"]}}}"#,
        &u("u1", "k2", "new prompt", "2026-06-07T05:11:00.000Z"),
        &a("a1", "u1", "new reply", "2026-06-07T05:11:05.000Z"),
        r#"{"type":"file-history-snapshot","uuid":"fh1","parentUuid":"a1","timestamp":"2026-06-07T05:12:00.000Z","messageId":"m-1"}"#,
        &att("x9", "not-in-this-file", "2026-06-07T05:15:00.000Z"),
    ]);
    let c = Chain::build(&r, None);
    assert_eq!(c.leaf_index, Some(6), "the one tip after the cut");
    let s = survivals(&c, r.len());
    assert_eq!(&s[0..2], ["pre-cut", "pre-cut"], "the cut still cuts");
    assert_eq!(s[2], "live", "the preserved pair");
    assert_eq!(s[3], "live");
    assert_eq!(&s[5..7], ["live", "live"], "and everything after it");
}
