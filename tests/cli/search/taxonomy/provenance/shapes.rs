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
