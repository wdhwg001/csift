//! C-33, across every surface that renders a compaction: the boundary excerpt's survivor
//! fields, the `/rewind` summarize MODE on both the boundary and its summary, and the
//! legacy four-scalar boundary that must still render byte-for-byte what it always did.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "00000000-0000-4000-8000-0000000c3300";
const LEGACY: &str = "00000000-0000-4000-8000-0000000c33ff";

/// A transcript shaped like a live `/rewind` -> "Summarize up to here": the boundary carries
/// the full survivor metadata, and the summary that follows it carries `summarizeMetadata`
/// INSTEAD of `isVisibleInTranscriptOnly`.
fn summarize_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef pass"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"assistant","content":[{"type":"text","text":"charting now"}]}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","parentUuid":null,"logicalParentUuid":"a1","timestamp":"2026-06-07T05:10:00.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"manual","preTokens":41041,"postTokens":8121,"durationMs":44736,"messagesSummarized":66,"cumulativeDroppedTokens":32920,"preservedSegment":{"headUuid":"aaaa1111-2222-4333-8444-555566667777","anchorUuid":"bbbb2222-3333-4444-8555-666677778888","tailUuid":"cccc3333-4444-4555-8666-777788889999"},"preservedMessages":{"anchorUuid":"bbbb2222-3333-4444-8555-666677778888","uuids":["aaaa1111-2222-4333-8444-555566667777","cccc3333-4444-4555-8666-777788889999"],"allUuids":["aaaa1111-2222-4333-8444-555566667777","dddd4444-5555-4666-8777-888899990000","cccc3333-4444-4555-8666-777788889999"]}}}"#, "\n",
            r#"{"type":"user","uuid":"s1","parentUuid":"cb1","timestamp":"2026-06-07T05:10:01.000Z","isCompactSummary":true,"summarizeMetadata":{"messagesSummarized":66,"direction":"up_to"},"message":{"role":"user","content":"This session is being continued. The reef pass work is summarized above."}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"s1","timestamp":"2026-06-07T05:11:00.000Z","message":{"role":"user","content":"now sound the channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:11:30.000Z","message":{"role":"assistant","content":[{"type":"text","text":"sounding the channel"}]}}"#, "\n",
        ),
    );
    // A second transcript with the OLD four-scalar boundary and a plain compaction summary.
    h.write(
        &format!("{ENC}/{LEGACY}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"lu1","parentUuid":null,"timestamp":"2026-06-07T04:00:00.000Z","message":{"role":"user","content":"survey the old harbor"}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"lcb1","parentUuid":null,"logicalParentUuid":"lu1","timestamp":"2026-06-07T04:10:00.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"auto","preTokens":1000,"postTokens":200,"durationMs":50}}"#, "\n",
            r#"{"type":"user","uuid":"ls1","parentUuid":"lcb1","timestamp":"2026-06-07T04:10:01.000Z","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"message":{"role":"user","content":"This session is being continued. The harbor survey is summarized above."}}"#, "\n",
            r#"{"type":"user","uuid":"lu2","parentUuid":"ls1","timestamp":"2026-06-07T04:11:00.000Z","message":{"role":"user","content":"and the outer bar"}}"#, "\n",
        ),
    );
    h
}

#[test]
fn boundary_excerpt_carries_the_survivor_fields_on_search_and_show() {
    let h = summarize_home();
    let expected = "[compaction boundary: trigger=manual preTokens=41041 postTokens=8121 \
                    durationMs=44736 messagesSummarized=66 cumulativeDroppedTokens=32920 \
                    preserved=2 uuids, 3 allUuids, anchor bbbb2222 \
                    segment=aaaa1111..cccc3333]";
    let found = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "-t",
        "harness.compaction.boundary",
    ]);
    assert!(found.success, "stderr: {}", found.stderr);
    assert!(
        found.stdout.contains(expected),
        "the boundary excerpt names every survivor field:\n{}",
        found.stdout
    );
    // The one-line excerpt COUNTS the uuid lists; the full lists are JSON-only.
    assert!(
        !found.stdout.contains("dddd4444"),
        "the excerpt never dumps the uuid lists:\n{}",
        found.stdout
    );
    let shown = h.run(&["show", at(SESS).as_str(), "--line", "3"]);
    assert!(
        shown.stdout.contains(expected) && shown.stdout.contains("[logicalParent=a1]"),
        "show renders the same excerpt:\n{}",
        shown.stdout
    );
}

#[test]
fn a_legacy_boundary_renders_byte_for_byte_as_before() {
    let h = summarize_home();
    let out = h.run(&[
        "search",
        "",
        at(LEGACY).as_str(),
        "-t",
        "harness.compaction.boundary",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(
            "Conversation compacted [compaction boundary: trigger=auto preTokens=1000 \
             postTokens=200 durationMs=50] [logicalParent=lu1]"
        ),
        "a four-scalar boundary is unchanged:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("preserved=") && !out.stdout.contains("segment="),
        "absent survivor keys print nothing:\n{}",
        out.stdout
    );
    // And its summary keeps the bare leaf path (no `[summarize ...]` tag).
    let sum = h.run(&[
        "search",
        "",
        at(LEGACY).as_str(),
        "-t",
        "harness.compaction.summary",
    ]);
    assert!(
        sum.stdout.contains("harness.compaction.summary  L3"),
        "an ordinary compaction summary is untagged:\n{}",
        sum.stdout
    );
}

#[test]
fn the_summarize_mode_rides_both_records_in_text_and_json() {
    let h = summarize_home();
    let sum = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "-t",
        "harness.compaction.summary",
    ]);
    assert!(sum.success, "stderr: {}", sum.stderr);
    assert!(
        sum.stdout
            .contains("harness.compaction.summary [summarize up_to]"),
        "the summary names its direction in the label zone:\n{}",
        sum.stdout
    );
    let json = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "-t",
        "harness.compaction",
        "--format",
        "json",
    ]);
    assert!(json.success, "stderr: {}", json.stderr);
    let modes: Vec<(String, String)> = json
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "exchange")
        .flat_map(|v| {
            v["hits"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|h| {
                    (
                        h["label"].as_str().unwrap_or_default().to_string(),
                        h["mode"].as_str().unwrap_or("null").to_string(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        modes.contains(&(
            "harness.compaction.boundary".into(),
            "summarize-up-to-here".into()
        )),
        "the BOUNDARY learns its mode from the summary that follows it: {modes:?}"
    );
    assert!(
        modes.contains(&(
            "harness.compaction.summary".into(),
            "summarize-up-to-here".into()
        )),
        "the summary names its own mode: {modes:?}"
    );
    // The legacy pair reads `compact` on both records.
    let legacy = h.run(&[
        "search",
        "",
        at(LEGACY).as_str(),
        "-t",
        "harness.compaction",
        "--format",
        "json",
    ]);
    assert_eq!(
        legacy.stdout.matches(r#""mode":"compact""#).count(),
        2,
        "an ordinary compaction pairs as `compact` on both records:\n{}",
        legacy.stdout
    );
}

#[test]
fn boundary_json_keeps_the_full_uuid_lists_verbatim() {
    let h = summarize_home();
    let out = h.run(&["show", at(SESS).as_str(), "--line", "3", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let row = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["kind"] == "record")
        .expect("a record row");
    assert_eq!(row["mode"], "summarize-up-to-here");
    let meta = &row["compact_metadata"];
    assert_eq!(meta["messagesSummarized"], 66);
    assert_eq!(meta["cumulativeDroppedTokens"], 32920);
    assert_eq!(
        meta["preservedMessages"]["allUuids"]
            .as_array()
            .map(Vec::len),
        Some(3),
        "the JSON keeps the full list the excerpt only counted: {meta}"
    );
    assert_eq!(
        meta["preservedSegment"]["anchorUuid"],
        "bbbb2222-3333-4444-8555-666677778888"
    );
    // A non-compaction record carries the keys as nulls, never a fabricated mode.
    let other = h.run(&["show", at(SESS).as_str(), "--line", "1", "--format", "json"]);
    let orow = other
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["kind"] == "record")
        .expect("a record row");
    assert!(orow["mode"].is_null() && orow["compact_metadata"].is_null());
}

#[test]
fn verbatim_banners_the_mode_and_still_reconstructs_across_the_cut() {
    let h = summarize_home();
    let out = h.run(&["verbatim", at(SESS).as_str(), "--budget", "4000"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("compaction boundary · summary at L4 · summarize up_to ·"),
        "the banner names the gesture:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("chart the reef pass"),
        "a summarize is a compaction like any other: the clipped turns still come back:\n{}",
        out.stdout
    );
    let json = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--budget",
        "4000",
        "--format",
        "json",
    ]);
    assert!(
        json.stdout.contains(
            r#"{"kind":"compaction_boundary","line":4,"mode":"summarize-up-to-here","summary_chars":"#
        ),
        "the JSON boundary row carries the mode:\n{}",
        json.stdout
    );
    // The legacy transcript's banner is untouched (no mode segment at all).
    let legacy = h.run(&["verbatim", at(LEGACY).as_str(), "--budget", "4000"]);
    assert!(
        legacy
            .stdout
            .contains("compaction boundary · summary at L3 · (turns below predate it)"),
        "an ordinary compaction's banner is byte-stable:\n{}",
        legacy.stdout
    );
}

/// A `direction` csift does not model reads NULL, never `compact`: a new gesture must not
/// silently render as one of the two known ones on either record.
#[test]
fn an_unmodeled_direction_reads_a_null_mode_on_both_records() {
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-0000000c3301";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef pass"}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","parentUuid":null,"timestamp":"2026-06-07T05:10:00.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"manual","preTokens":900}}"#, "\n",
            r#"{"type":"user","uuid":"s1","parentUuid":"cb1","timestamp":"2026-06-07T05:10:01.000Z","isCompactSummary":true,"summarizeMetadata":{"messagesSummarized":3,"direction":"sideways"},"message":{"role":"user","content":"This session is being continued."}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"s1","timestamp":"2026-06-07T05:11:00.000Z","message":{"role":"user","content":"now sound the channel"}}"#, "\n",
        ),
    );
    let json = h.run(&[
        "search",
        "",
        at(sess).as_str(),
        "-t",
        "harness.compaction",
        "--format",
        "json",
    ]);
    assert!(json.success, "stderr: {}", json.stderr);
    let modes: Vec<(String, serde_json::Value)> = json
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "exchange")
        .flat_map(|v| {
            v["hits"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|h| {
                    (
                        h["label"].as_str().unwrap_or_default().to_string(),
                        h["mode"].clone(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(modes.len(), 2, "both compaction records surface: {modes:?}");
    for (label, mode) in &modes {
        assert!(
            mode.is_null(),
            "{label} must read a null mode for an unmodeled direction, got {mode}"
        );
    }
    // The verbatim word still renders, so the reader sees the gesture csift cannot name.
    let text = h.run(&[
        "search",
        "",
        at(sess).as_str(),
        "-t",
        "harness.compaction.summary",
    ]);
    assert!(
        text.stdout
            .contains("harness.compaction.summary [summarize sideways]"),
        "the tag renders the verbatim direction:\n{}",
        text.stdout
    );
}

/// The pairing is built from the WHOLE parsed record set, before any turn window or cap, so
/// a window that admits the boundary but not its summary still reports the mode. This is the
/// deliberate choice over a window-dependent index: a windowed read must not silently turn a
/// known gesture into an unknown one.
#[test]
fn the_pairing_is_window_independent() {
    let h = summarize_home();
    // `show --line 3` addresses the boundary ALONE - the summary on line 4 is not fetched.
    let one = h.run(&["show", at(SESS).as_str(), "--line", "3", "--format", "json"]);
    assert!(one.success, "stderr: {}", one.stderr);
    let row = one
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["kind"] == "record")
        .expect("a record row");
    assert_eq!(
        row["mode"], "summarize-up-to-here",
        "the boundary keeps its mode when the summary is outside the window"
    );
    // A `--turn` window on turn 0 (the pre-compaction turn, which the boundary belongs to)
    // reaches the boundary without the summary's turn.
    let win = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "-t",
        "harness.compaction.boundary",
        "--turn",
        "0",
        "--format",
        "json",
    ]);
    assert!(win.success, "stderr: {}", win.stderr);
    let modes: Vec<serde_json::Value> = win
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "exchange")
        .flat_map(|v| v["hits"].as_array().cloned().unwrap_or_default())
        .map(|h| h["mode"].clone())
        .collect();
    assert!(
        modes.iter().all(|m| m == "summarize-up-to-here"),
        "a windowed boundary keeps its mode: {modes:?}"
    );
}

/// Two boundaries in a row: the summary pairs with the SECOND only, and the first stays
/// unpaired rather than borrowing a gesture it did not cause.
#[test]
fn a_summary_after_two_boundaries_pairs_with_the_second_only() {
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-0000000c3302";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef pass"}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","parentUuid":null,"timestamp":"2026-06-07T05:10:00.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"auto","preTokens":900}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"cb2","parentUuid":null,"timestamp":"2026-06-07T05:20:00.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"manual","preTokens":800}}"#, "\n",
            r#"{"type":"user","uuid":"s1","parentUuid":"cb2","timestamp":"2026-06-07T05:20:01.000Z","isCompactSummary":true,"summarizeMetadata":{"messagesSummarized":3,"direction":"from"},"message":{"role":"user","content":"This session is being continued."}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"s1","timestamp":"2026-06-07T05:21:00.000Z","message":{"role":"user","content":"now sound the channel"}}"#, "\n",
        ),
    );
    let json = h.run(&[
        "search",
        "",
        at(sess).as_str(),
        "-t",
        "harness.compaction.boundary",
        "--format",
        "json",
    ]);
    assert!(json.success, "stderr: {}", json.stderr);
    let by_line: Vec<(u64, serde_json::Value)> = json
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "exchange")
        .flat_map(|v| v["hits"].as_array().cloned().unwrap_or_default())
        .map(|h| (h["line"].as_u64().unwrap_or(0), h["mode"].clone()))
        .collect();
    assert_eq!(by_line.len(), 2, "both boundaries surface: {by_line:?}");
    let first = by_line.iter().find(|(l, _)| *l == 2).expect("L2");
    let second = by_line.iter().find(|(l, _)| *l == 3).expect("L3");
    assert!(
        first.1.is_null(),
        "the first boundary is unpaired: {:?}",
        first.1
    );
    assert_eq!(
        second.1, "summarize-from-here",
        "the summary pairs with the boundary immediately before it"
    );
}

#[test]
fn an_unrelated_system_record_between_the_two_does_not_break_the_pairing() {
    // The pairing walks records looking for a BOUNDARY, and a boundary is a `compact_boundary`
    // - not merely a record carrying a `subtype`. Every system record has one, and the
    // harness writes plenty of them (`informational` here); treating any of them as a
    // boundary would hand the summary's mode to a note and leave the real boundary unpaired.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-0000000c3303";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef pass"}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","parentUuid":null,"timestamp":"2026-06-07T05:10:00.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"manual","preTokens":800}}"#, "\n",
            r#"{"type":"system","subtype":"informational","uuid":"sy1","parentUuid":"u1","timestamp":"2026-06-07T05:10:01.000Z","level":"info","content":"a harness note"}"#, "\n",
            r#"{"type":"user","uuid":"s1","parentUuid":"cb1","timestamp":"2026-06-07T05:10:02.000Z","isCompactSummary":true,"summarizeMetadata":{"messagesSummarized":3,"direction":"up_to"},"message":{"role":"user","content":"This session is being continued."}}"#, "\n",
        ),
    );
    // Both leaves are named explicitly so the system catch-all is actually parsed - the note
    // has to be IN the record set for it to be able to steal the pairing.
    let json = h.run(&[
        "search",
        "",
        at(sess).as_str(),
        "-t",
        "harness.compaction.boundary",
        "-t",
        "harness.meta.system",
        "--format",
        "json",
    ]);
    assert!(json.success, "stderr: {}", json.stderr);
    let hits: Vec<serde_json::Value> = json
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "exchange")
        .flat_map(|v| v["hits"].as_array().cloned().unwrap_or_default())
        .collect();
    let boundary = hits
        .iter()
        .find(|h| h["label"] == "harness.compaction.boundary")
        .expect("the boundary hit");
    assert_eq!(
        boundary["mode"], "summarize-up-to-here",
        "the boundary keeps the summary that follows it:\n{}",
        json.stdout
    );
    let note = hits
        .iter()
        .find(|h| h["label"] == "harness.meta.system")
        .expect("the informational record was parsed");
    assert!(
        note["mode"].is_null(),
        "a note is not a compaction event:\n{}",
        json.stdout
    );
}

/// The §7f whole-file gate must not prune a file whose ONLY match is the FABRICATED boundary
/// text: `messagesSummarized=66` appears nowhere in the raw line (the record carries
/// `"messagesSummarized":66`), so the literal prefilter cannot see it and only the
/// `compact_boundary` synth marker keeps the file in the scan.
#[test]
fn the_whole_file_gate_keeps_a_file_whose_only_match_is_the_fabricated_boundary_text() {
    let h = summarize_home();
    for args in [
        vec![
            "search",
            "messagesSummarized=66",
            at(SESS).as_str(),
            "-t",
            "harness.compaction.boundary",
        ],
        // A bare scan (no `-t`) also reaches the boundary and registers the marker.
        vec!["search", "messagesSummarized=66", at(SESS).as_str()],
    ] {
        let label = args.len();
        let out = h.run(&args);
        assert!(out.success, "stderr: {}", out.stderr);
        assert!(
            out.stdout.contains("messagesSummarized=66"),
            "the fabricated excerpt must survive the whole-file gate (args len {label}):\n{}",
            out.stdout
        );
        assert!(
            !out.stdout.contains("no matching exchanges"),
            "the file was pruned (args len {label}):\n{}",
            out.stdout
        );
    }
    // The same pattern against a transcript with no such boundary is a definitive absence,
    // which is what proves the gate is still doing its job.
    let none = h.run(&["search", "messagesSummarized=66", at(LEGACY).as_str()]);
    assert!(none.success && none.stdout.contains("no matching exchanges"));
}
