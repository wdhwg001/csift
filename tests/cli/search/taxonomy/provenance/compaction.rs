//! The COMPACTION EVENT's provenance, over transcripts built around its two halves.
//!
//! One event is two records - a `compact_boundary` and the summary that replaced the region
//! it cut - and every relation here is about the join between them: `pairs` both ways,
//! `precedes` the predecessor the boundary re-links to, `preserves` the head and the tail of
//! what it kept, and the honest empty a summary prints when this file holds no boundary for
//! it. Each needs a transcript built around ONE of those shapes, which is why they are not
//! on the shared 73-leaf fixture; they sit apart from `shapes.rs` because they are four
//! views of one subject rather than four unrelated structural questions.

use crate::harness::*;

use super::{LEAD_ENC, LEAD_SESS};

/// A compaction boundary carrying the WHOLE `compactMetadata` states three relations, and
/// each of them resolves to a line.
///
/// The shared fixture's boundary carries only the four original scalars, which is the older
/// shape and the one that falls back: its `pairs` is position and its `precedes` is the
/// honest empty. This transcript is built around the current shape instead - the boundary
/// names the summary in `anchorUuid`, the predecessor in `logicalParentUuid`, and both ends
/// of the segment it kept - so the three relations are asserted where the KEYS exist.
///
/// `preserves` is TWO edges rather than one naming a range: an edge names one neighbour,
/// and head and tail are both addresses a reader fetches.
#[test]
fn a_whole_compaction_boundary_names_its_summary_its_predecessor_and_what_it_kept() {
    let h = Home::new();
    h.write(
        &format!("{LEAD_ENC}/{LEAD_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"c1","parentUuid":null,"timestamp":"2026-06-08T02:00:00.000Z","message":{"role":"user","content":"zzcompact chart the lagoon"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"c2","parentUuid":"c1","timestamp":"2026-06-08T02:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzcompact charting"}]}}"#,
            "\n",
            r#"{"type":"user","uuid":"c3","parentUuid":"c2","timestamp":"2026-06-08T02:01:00.000Z","message":{"role":"user","content":"zzcompact dredge the channel"}}"#,
            "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"c4","parentUuid":null,"logicalParentUuid":"c3","timestamp":"2026-06-08T02:02:00.000Z","compactMetadata":{"trigger":"auto","preTokens":90000,"postTokens":9000,"durationMs":1200,"preservedSegment":{"headUuid":"c1","anchorUuid":"c5","tailUuid":"c3"}}}"#,
            "\n",
            r#"{"type":"user","uuid":"c5","parentUuid":"c4","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"timestamp":"2026-06-08T02:02:01.000Z","message":{"role":"user","content":"zzcompact the summary of the work so far"}}"#,
            "\n",
        ),
    );
    // An EMPTY pattern: a boundary's rendered text is its own `[compact trigger=...]`
    // head, so the words the fixture's records carry are not in it and a pattern scan
    // reaches the summary alone.
    let out = h.run(&[
        "search",
        "",
        &format!("@{LEAD_SESS}"),
        "-t",
        "harness.compaction.*",
        "--format",
        "json",
        "--max-count",
        "0",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hits: Vec<serde_json::Value> = json_rows(&out.stdout, "exchange")
        .iter()
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .collect();
    let edges_of = |line: u64| -> Vec<(String, String, Option<u64>)> {
        hits.iter()
            .filter(|h| h["line"].as_u64() == Some(line))
            .flat_map(|h| h["for"].as_array().cloned().unwrap_or_default())
            .map(|e| {
                (
                    e["kind"].as_str().unwrap_or_default().to_string(),
                    e["basis"].as_str().unwrap_or("null").to_string(),
                    e["target"]["line"].as_u64(),
                )
            })
            .collect()
    };
    assert_eq!(
        edges_of(4),
        [
            ("pairs".to_string(), "key".to_string(), Some(5)),
            ("precedes".to_string(), "key".to_string(), Some(3)),
            ("preserves".to_string(), "key".to_string(), Some(1)),
            ("preserves".to_string(), "key".to_string(), Some(3)),
        ],
        "the boundary at L4:\n{}",
        out.stdout
    );
    // The summary names the boundary BACK, and on position: no key of a summary names one.
    assert_eq!(
        edges_of(5),
        [("pairs".to_string(), "position".to_string(), Some(4))],
        "the summary at L5:\n{}",
        out.stdout
    );
    // The three words RENDER - they are what a record states, not a reading of the file's
    // shape, so they carry a label-zone tag like every other family edge.
    let text = h.run(&[
        "search",
        "",
        &format!("@{LEAD_SESS}"),
        "-t",
        "harness.compaction.*",
        "--max-count",
        "0",
    ]);
    assert!(text.success, "{}", text.stderr);
    for tag in [
        "[for: pairs L5]",
        "[for: precedes L3]",
        "[for: preserves L1]",
        "[for: preserves L3]",
        "[for: pairs L4]",
    ] {
        assert!(
            text.stdout.contains(tag),
            "{tag} is missing from the text surface:\n{}",
            text.stdout
        );
    }
}

/// A boundary whose `anchorUuid` is its OWN uuid pairs by POSITION, never with itself.
///
/// Claude Code's metadata builder takes the anchor as an argument and its two callers pass
/// different things: the compaction path passes the last summary message's uuid, the
/// `/rewind` summarize path passes the BOUNDARY's own uuid whenever the direction is not
/// `up_to`. Read as a key, that second shape would make the boundary its own other half -
/// an edge naming the record the reader is already looking at. It falls back to the summary
/// that follows it, exactly as a boundary carrying no segment at all does.
#[test]
fn a_boundary_anchored_at_itself_pairs_with_the_summary_instead() {
    let h = Home::new();
    h.write(
        &format!("{LEAD_ENC}/{LEAD_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"w1","parentUuid":null,"timestamp":"2026-06-08T03:00:00.000Z","message":{"role":"user","content":"zzself chart the lagoon"}}"#,
            "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"w2","parentUuid":null,"timestamp":"2026-06-08T03:01:00.000Z","compactMetadata":{"trigger":"manual","preTokens":900,"messagesSummarized":4,"preservedSegment":{"headUuid":"w1","anchorUuid":"w2","tailUuid":"w1"}}}"#,
            "\n",
            r#"{"type":"user","uuid":"w3","parentUuid":"w2","isCompactSummary":true,"summarizeMetadata":{"direction":"from"},"timestamp":"2026-06-08T03:01:01.000Z","message":{"role":"user","content":"zzself the summary"}}"#,
            "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "",
        &format!("@{LEAD_SESS}"),
        "-t",
        "harness.compaction.*",
        "--format",
        "json",
        "--max-count",
        "0",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let boundary = json_rows(&out.stdout, "exchange")
        .iter()
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .find(|h| h["line"].as_u64() == Some(2))
        .unwrap_or_else(|| panic!("the boundary at L2:\n{}", out.stdout));
    let pairs = boundary["for"]
        .as_array()
        .expect("edges")
        .iter()
        .find(|e| e["kind"] == "pairs")
        .unwrap_or_else(|| panic!("no pairs edge: {boundary}"));
    assert_eq!(pairs["basis"], "position", "{boundary}");
    assert_eq!(pairs["target"]["line"].as_u64(), Some(3), "{boundary}");
    // The segment it kept is still read off the keys, which the self-anchor does not touch.
    let preserves: Vec<Option<u64>> = boundary["for"]
        .as_array()
        .expect("edges")
        .iter()
        .filter(|e| e["kind"] == "preserves")
        .map(|e| e["target"]["line"].as_u64())
        .collect();
    assert_eq!(preserves, [Some(1), Some(1)], "{boundary}");
}

/// `pairs` is SYMMETRIC even where the summary's gesture is one csift does not model.
///
/// The mode and the pairing are different questions and they come apart on exactly this
/// shape: `summarizeMetadata.direction` reads a word csift has no `SummarizeMode` for, so the
/// pair has no mode - and the two records are still each other's other half. Reading the
/// mode's absence as "not a summary" left the boundary naming the summary off its own
/// `anchorUuid` key while the summary printed an honest empty about a record one line below
/// it: one relation, two answers, one file.
#[test]
fn the_compaction_pair_is_symmetric_even_with_an_unmodeled_direction() {
    let h = Home::new();
    h.write(
        &format!("{LEAD_ENC}/{LEAD_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"y1","parentUuid":null,"timestamp":"2026-06-08T05:00:00.000Z","message":{"role":"user","content":"zzside chart the lagoon"}}"#,
            "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"y2","parentUuid":null,"timestamp":"2026-06-08T05:01:00.000Z","compactMetadata":{"trigger":"manual","preTokens":900,"preservedSegment":{"headUuid":"y1","anchorUuid":"y3","tailUuid":"y1"}}}"#,
            "\n",
            r#"{"type":"user","uuid":"y3","parentUuid":"y2","isCompactSummary":true,"summarizeMetadata":{"direction":"sideways"},"timestamp":"2026-06-08T05:01:01.000Z","message":{"role":"user","content":"zzside the summary"}}"#,
            "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "",
        &format!("@{LEAD_SESS}"),
        "-t",
        "harness.compaction.*",
        "--format",
        "json",
        "--max-count",
        "0",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hits: Vec<serde_json::Value> = json_rows(&out.stdout, "exchange")
        .iter()
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .collect();
    let pairs = |line: u64| -> serde_json::Value {
        hits.iter()
            .filter(|h| h["line"].as_u64() == Some(line))
            .flat_map(|h| h["for"].as_array().cloned().unwrap_or_default())
            .find(|e| e["kind"] == "pairs")
            .unwrap_or_else(|| panic!("no pairs edge on L{line}:\n{}", out.stdout))
    };
    assert_eq!(
        pairs(2)["target"]["line"].as_u64(),
        Some(3),
        "{}",
        out.stdout
    );
    assert_eq!(
        pairs(3)["target"]["line"].as_u64(),
        Some(2),
        "{}",
        out.stdout
    );
    // The MODE is still absent - an unmodeled gesture is never folded into a known one.
    let summary = hits
        .iter()
        .find(|h| h["line"].as_u64() == Some(3))
        .unwrap_or_else(|| panic!("no summary hit:\n{}", out.stdout));
    assert!(summary["mode"].is_null(), "{summary}");
}

/// A summary with no boundary above it says the TRUE reason it names nothing.
///
/// No compaction summary carries a key naming its boundary, so "the record carries no such
/// key" would be true of every summary ever written and would tell a reader nothing about
/// the one in front of them. The fact is that this FILE holds no boundary before it - a
/// clone head, whose boundary lives in the session it was copied from.
#[test]
fn an_unpaired_summary_names_the_reason_the_file_gives() {
    let h = Home::new();
    h.write(
        &format!("{LEAD_ENC}/{LEAD_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"z1","parentUuid":null,"isCompactSummary":true,"isVisibleInTranscriptOnly":true,"timestamp":"2026-06-08T06:00:00.000Z","message":{"role":"user","content":"zzclone the summary this file opens with"}}"#,
            "\n",
            r#"{"type":"user","uuid":"z2","parentUuid":"z1","timestamp":"2026-06-08T06:01:00.000Z","message":{"role":"user","content":"zzclone carry on"}}"#,
            "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "",
        &format!("@{LEAD_SESS}"),
        "-t",
        "harness.compaction.*",
        "--format",
        "json",
        "--max-count",
        "0",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let edge = json_rows(&out.stdout, "exchange")
        .iter()
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .flat_map(|h| h["for"].as_array().cloned().unwrap_or_default())
        .find(|e| e["kind"] == "pairs")
        .unwrap_or_else(|| panic!("no pairs edge:\n{}", out.stdout));
    assert!(edge["target"].is_null(), "{edge}");
    assert_eq!(
        edge["reason"], "no compaction boundary precedes this summary in this file",
        "{edge}"
    );
}
