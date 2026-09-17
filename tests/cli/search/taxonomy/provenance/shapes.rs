//! Provenance answers that need a transcript built around ONE structural question.
//!
//! The shared 73-leaf fixture carries a line per leaf and one chain through them, which is
//! what makes it good for a vocabulary sweep and useless for these: each case here turns on
//! the SHAPE of a transcript - what sits above the opener, where a result lands relative to
//! its call, which map an id resolves through - and a fixture that carried all of them
//! would be a second fixture living inside the first.

use crate::harness::*;

use super::{LEAD_ENC, LEAD_SESS};

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

/// The same two carriers again, with `message.content` as a BLOCK ARRAY.
///
/// `message.content` is a bare string on most pulses and a block array on the rest, and the
/// LEAF that dispatches to the reader is decided from the joined text of the blocks. So a
/// reader of the string form alone classifies a block-content pulse as a notification and
/// then reports that it names no tag - a fabricated negative, and the one shape where the
/// honest empty is a lie. Measured over the file set at `Record::pulse_tool_use_id`: 211
/// pulses carry block content, 3 of them carrying a `<tool-use-id>`.
///
/// This is the case that fails if the reader is narrowed back to the string arm: both L3 and
/// L4 would print the empty reason instead of the address.
#[test]
fn a_pulse_in_block_content_names_its_call_from_either_carrier() {
    let pulse = "<task-notification><task-id>b2468013</task-id><tool-use-id>toolu_01b</tool-use-id><summary>background-command task b2468013 completed</summary></task-notification>";
    let h = Home::new();
    h.write(
        &format!("{LEAD_ENC}/{LEAD_SESS}.jsonl"),
        &format!(
            concat!(
                r#"{{"type":"user","uuid":"b0","parentUuid":null,"timestamp":"2026-06-09T04:00:00.000Z","message":{{"role":"user","content":"zzblocks start the build"}}}}"#,
                "\n",
                r#"{{"type":"assistant","uuid":"b1","parentUuid":"b0","timestamp":"2026-06-09T04:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"toolu_01b","name":"Bash","input":{{"command":"make"}}}}]}}}}"#,
                "\n",
                r#"{{"type":"user","uuid":"b2","parentUuid":"b1","timestamp":"2026-06-09T04:05:00.000Z","message":{{"role":"user","content":[{{"type":"text","text":"{pulse}"}}]}}}}"#,
                "\n",
                r#"{{"type":"user","uuid":"b3","isMeta":true,"parentUuid":"b2","timestamp":"2026-06-09T04:06:00.000Z","message":{{"role":"user","content":[{{"type":"text","text":"{pulse}"}}]}}}}"#,
                "\n",
            ),
            pulse = pulse,
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
    for (line, carrier) in [(3, "the ordinary carrier"), (4, "the isMeta frame")] {
        let edge = reports(line);
        assert_eq!(edge["basis"], "key", "{carrier} on L{line}: {edge}");
        assert_eq!(
            edge["target"]["line"].as_u64(),
            Some(2),
            "{carrier} on L{line} must resolve to the call, not report an empty: {edge}"
        );
        assert!(
            edge["reason"].is_null(),
            "{carrier} on L{line} states no reason when it has an address: {edge}"
        );
    }

    let text = h.run(&["search", "", &format!("@{LEAD_SESS}"), "--max-count", "0"]);
    assert!(text.success, "{}", text.stderr);
    assert_eq!(
        text.stdout.matches("[for: reports L2]").count(),
        2,
        "both block-content carriers render the address:\n{}",
        text.stdout
    );
    assert!(
        !text.stdout.contains("the pulse names no tool-use-id"),
        "no pulse here is tagless, so no empty reason belongs in the output:\n{}",
        text.stdout
    );
}
