//! Provenance answers that need a transcript built around ONE structural question.
//!
//! The shared 73-leaf fixture carries a line per leaf and one chain through them, which is
//! what makes it good for a vocabulary sweep and useless for these: each case here turns on
//! the SHAPE of a transcript - what sits above the opener, where a result lands relative to
//! its call, which map an id resolves through - and a fixture that carried all of them
//! would be a second fixture living inside the first.

use crate::harness::*;

/// The home each case writes its own transcript into.
const LEAD_ENC: &str = "-Users-dev-lead-project";
const LEAD_SESS: &str = "6f7e8d9c-0b1a-4c2d-8e3f-4a5b6c7d8e9f";

/// The turn's OPENER is the opener, not the turn's first record.
///
/// The shared grouper folds a synthetic pre-user LEAD into turn 0, and a real transcript
/// almost always opens with one - a SessionStart hook attachment, a
/// `<local-command-caveat>` on a `/clear`-minted file, a compaction boundary. Reading
/// position 0 as the opener named that lead as the message every unit of turn 0
/// corresponds to, and left the human message itself reading one hop BELOW a hook.
///
/// Both directions are asserted: the human message IS the opener (no tag at all), and the
/// reply names IT rather than the attachment above it.
#[test]
fn a_folded_pre_user_lead_is_not_mistaken_for_the_opener() {
    let h = Home::new();
    h.write(
        &format!("{LEAD_ENC}/{LEAD_SESS}.jsonl"),
        concat!(
            r#"{"type":"attachment","uuid":"h1","timestamp":"2026-06-08T00:59:59.000Z","attachment":{"type":"hook_success","hookName":"start","hookEvent":"SessionStart","command":"echo zzlead","exitCode":0,"stdout":"zzlead hook ran"}}"#,
            "\n",
            r#"{"type":"user","uuid":"g1","parentUuid":"h1","timestamp":"2026-06-08T01:00:00.000Z","message":{"role":"user","content":"zzlead the person's question"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"g2","parentUuid":"g1","timestamp":"2026-06-08T01:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzlead the reply"}]}}"#,
            "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "zzlead",
        &format!("@{LEAD_SESS}"),
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let roots: Vec<(u64, serde_json::Value)> = json_rows(&out.stdout, "exchange")
        .iter()
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .map(|hit| {
            (
                hit["line"].as_u64().unwrap_or_default(),
                hit["for_root"].clone(),
            )
        })
        .collect();

    let human = &roots
        .iter()
        .find(|(l, _)| *l == 2)
        .expect("the human message")
        .1;
    assert_eq!(
        human["note"], "turn opener",
        "the person's message opens the turn, whatever the grouper folded above it"
    );
    let reply = &roots.iter().find(|(l, _)| *l == 3).expect("the reply").1;
    assert_eq!(reply["line"], 2, "the reply is for the human message at L2");
    assert_eq!(reply["hops"], 1);
}

/// A unit of that folded lead names no opener at all, rather than the one BELOW it.
#[test]
fn a_unit_above_the_opener_names_no_root() {
    let h = Home::new();
    h.write(
        &format!("{LEAD_ENC}/{LEAD_SESS}.jsonl"),
        concat!(
            r#"{"type":"attachment","uuid":"h1","timestamp":"2026-06-08T00:59:59.000Z","attachment":{"type":"hook_additional_context","hookName":"start","hookEvent":"SessionStart","content":["zzlead injected at the session start"]}}"#,
            "\n",
            r#"{"type":"user","uuid":"g1","parentUuid":"h1","timestamp":"2026-06-08T01:00:00.000Z","message":{"role":"user","content":"zzlead the person's question"}}"#,
            "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "zzlead",
        &format!("@{LEAD_SESS}"),
        "-t",
        "harness.hook.context",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let root = json_rows(&out.stdout, "exchange")
        .first()
        .and_then(|r| r["hits"].as_array().and_then(|h| h.first()).cloned())
        .expect("the injected context")["for_root"]
        .clone();
    assert_eq!(root["note"], "no opener above this unit");
    assert!(
        root["line"].is_null(),
        "naming the opener below it would point the wrong way"
    );
}

/// A LATE tool result carries BOTH relations, because they name different records.
///
/// On an ordinary turn the call and the parent are the same record and only the specific
/// word renders - one relation, one edge. When a result lands in a LATER turn, parented
/// there, the two are different facts and dropping either loses one the record states.
#[test]
fn a_late_tool_result_names_both_its_call_and_where_it_sits() {
    let h = Home::new();
    h.write(
        &format!("{LEAD_ENC}/{LEAD_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"k1","timestamp":"2026-06-08T01:00:00.000Z","message":{"role":"user","content":"zzlate start the build"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"k2","parentUuid":"k1","timestamp":"2026-06-08T01:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_01k","name":"Bash","input":{"command":"zzlate make"}}]}}"#,
            "\n",
            r#"{"type":"user","uuid":"k3","parentUuid":"k2","timestamp":"2026-06-08T01:00:02.000Z","message":{"role":"user","content":"zzlate a different question"}}"#,
            "\n",
            r#"{"type":"user","uuid":"k4","parentUuid":"k3","timestamp":"2026-06-08T01:05:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01k","content":"zzlate the build finished"}]}}"#,
            "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "zzlate",
        &format!("@{LEAD_SESS}"),
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hits: Vec<serde_json::Value> = json_rows(&out.stdout, "exchange")
        .iter()
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .collect();

    let late = hits
        .iter()
        .find(|h| h["line"] == 4)
        .expect("the late result");
    let edges: Vec<(&str, u64)> = late["for"]
        .as_array()
        .expect("edges")
        .iter()
        .map(|e| {
            (
                e["kind"].as_str().unwrap_or_default(),
                e["target"]["line"].as_u64().unwrap_or_default(),
            )
        })
        .collect();
    assert!(
        edges.contains(&("result_of", 2)),
        "the call it answers: {edges:?}"
    );
    assert!(
        edges.contains(&("parent", 3)),
        "and where it sits, which is a different record: {edges:?}"
    );

    // The ORDINARY result, whose call IS its parent, carries the specific word alone.
    let ordinary = hits.iter().find(|h| h["line"] == 2).expect("the call");
    let kinds: Vec<&str> = ordinary["for"]
        .as_array()
        .expect("edges")
        .iter()
        .filter_map(|e| e["kind"].as_str())
        .collect();
    assert_eq!(kinds, ["parent"], "a call hangs off its turn's opener");
}

/// An edge naming a CALL resolves through the tool index, not only the uuid map.
///
/// A `tool_use_id` is a wire id and appears in no `uuid` field, so an edge naming one used
/// to report `not in this file` about a call two lines up. The uuid map cannot see it and
/// the tool map can; neither can answer for the other's ids, so the fallback cannot
/// mis-resolve.
#[test]
fn an_edge_naming_a_call_resolves_to_the_line_that_carries_it() {
    let h = Home::new();
    h.write(
        &format!("{LEAD_ENC}/{LEAD_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-08T01:00:00.000Z","message":{"role":"user","content":"zztu run it"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"c2","parentUuid":"c1","timestamp":"2026-06-08T01:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_01c","name":"Bash","input":{"command":"zztu make"}}]}}"#,
            "\n",
            r#"{"type":"attachment","uuid":"c3","parentUuid":"c2","timestamp":"2026-06-08T01:00:02.000Z","attachment":{"type":"hook_additional_context","hookName":"zztu","hookEvent":"PostToolUse","toolUseID":"toolu_01c","content":["zztu the hook saw the call"]}}"#,
            "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "zztu",
        &format!("@{LEAD_SESS}"),
        "-t",
        "harness.hook.context",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let edges = json_rows(&out.stdout, "exchange")
        .first()
        .and_then(|r| r["hits"].as_array().and_then(|h| h.first()).cloned())
        .expect("the injected context")["for"]
        .clone();
    let call = edges
        .as_array()
        .expect("edges")
        .iter()
        .find(|e| e["kind"] == "for_tool_use")
        .expect("the call edge")
        .clone();
    assert_eq!(call["target"]["line"], 2, "the call is at L2: {call}");
    assert!(
        call["target"]["note"].is_null(),
        "and there is nothing to explain away: {call}"
    );
}

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

/// A notification pulse names the CALL it reports on, from EITHER carrier.
///
/// The pulse arrives two ways and only the rarer one is an isMeta notification FRAME, so a
/// relation read in the family that owns the frame answered for a fraction of the pulses on
/// disk: 94 of 4,007 pulse-carrying records over one projects root (the counting rule and
/// the file set are at `Record::pulse_tool_use_id`). The engine reads the tag
/// off the record's own text instead, so both carriers give the same answer and both resolve
/// through the tool index - a `tool_use` id is a wire id and is in no `uuid` field.
///
/// A pulse that names no tag says so. About a third of them do not (1,235 of 4,007 carry a
/// `<task-id>` and no `<tool-use-id>`), and joining THOSE means reading `toolUseResult` for
/// every tool result in the file, which the section 7 laws keep off a scan.
#[test]
fn a_pulse_names_the_call_it_reports_on_from_either_carrier() {
    let pulse = "<task-notification><task-id>b1234567</task-id><tool-use-id>toolu_01n</tool-use-id><summary>background-command task b1234567 completed</summary></task-notification>";
    let bare = "<task-notification><task-id>b7654321</task-id><summary>monitor task b7654321 tick</summary></task-notification>";
    let h = Home::new();
    h.write(
        &format!("{LEAD_ENC}/{LEAD_SESS}.jsonl"),
        &format!(
            concat!(
                r#"{{"type":"user","uuid":"n0","parentUuid":null,"timestamp":"2026-06-08T04:00:00.000Z","message":{{"role":"user","content":"zzpulse start the build"}}}}"#,
                "\n",
                r#"{{"type":"assistant","uuid":"n1","parentUuid":"n0","timestamp":"2026-06-08T04:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"toolu_01n","name":"Bash","input":{{"command":"make"}}}}]}}}}"#,
                "\n",
                r#"{{"type":"user","uuid":"n2","parentUuid":"n1","timestamp":"2026-06-08T04:05:00.000Z","message":{{"role":"user","content":"{pulse}"}}}}"#,
                "\n",
                r#"{{"type":"user","uuid":"n3","isMeta":true,"parentUuid":"n2","timestamp":"2026-06-08T04:06:00.000Z","message":{{"role":"user","content":"{pulse}"}}}}"#,
                "\n",
                r#"{{"type":"user","uuid":"n4","parentUuid":"n3","timestamp":"2026-06-08T04:07:00.000Z","message":{{"role":"user","content":"{bare}"}}}}"#,
                "\n",
            ),
            pulse = pulse,
            bare = bare,
        ),
    );
    let out = h.run(&[
        "search",
        "",
        &format!("@{LEAD_SESS}"),
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
    let reports = |line: u64| -> serde_json::Value {
        hits.iter()
            .filter(|h| h["line"].as_u64() == Some(line))
            .flat_map(|h| h["for"].as_array().cloned().unwrap_or_default())
            .find(|e| e["kind"] == "reports")
            .unwrap_or_else(|| panic!("no reports edge on L{line}:\n{}", out.stdout))
    };
    // The ORDINARY carrier - the common one, which no family ever reached.
    let ordinary = reports(3);
    assert_eq!(ordinary["basis"], "key");
    assert_eq!(ordinary["target"]["line"].as_u64(), Some(2));
    // The FRAME, on the same tag and the same index.
    let frame = reports(4);
    assert_eq!(frame["basis"], "key");
    assert_eq!(frame["target"]["line"].as_u64(), Some(2));
    // A pulse with no tag states the relation and says it named nothing.
    let none = reports(5);
    assert!(none["target"].is_null(), "{none}");
    assert_eq!(none["reason"], "the pulse names no tool-use-id");

    let text = h.run(&["search", "", &format!("@{LEAD_SESS}"), "--max-count", "0"]);
    assert!(text.success, "{}", text.stderr);
    assert_eq!(
        text.stdout.matches("[for: reports L2]").count(),
        2,
        "both carriers render the address:\n{}",
        text.stdout
    );
    assert!(
        text.stdout
            .contains("[for: reports unknown (the pulse names no tool-use-id)]"),
        "the honest empty is rendered, never dropped:\n{}",
        text.stdout
    );
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
