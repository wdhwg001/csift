//! The EXPOSED KEYS end to end: the label zone a `search` hit renders, the JSON an
//! ADDRESSED `show` record row carries, and the one shape whose contents reach no path
//! but `--raw`.
//!
//! Every fixture line is built from the shapes the record model documents, never copied
//! from a transcript.

use crate::harness::*;

const ENC: &str = "-Users-dev-keys-project";
const SESS: &str = "7c6b5a49-3827-4615-9403-f2e1d0c9b8a7";

/// A transcript with one line per exposed-key shape, in a fixed order the line numbers
/// below address.
fn keys_home() -> Home {
    let lines: Vec<String> = vec![
        // L1 the human opener
        r#"{"type":"user","uuid":"u-1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzkeys open the survey"}}"#.to_string(),
        // L2 every attribution key plus the two API joins
        r#"{"type":"assistant","uuid":"a-1","parentUuid":"u-1","timestamp":"2026-06-07T05:00:01.000Z","requestId":"req_abc123","apiBlockIndex":2,"attributionAgent":"general-purpose","attributionSkill":"csift","attributionPlugin":"omc","attributionMcpServer":"srv","attributionMcpTool":"srv__probe","message":{"role":"assistant","id":"msg_a1","model":"claude","content":[{"type":"text","text":"zzkeys attributed reply"}]}}"#.to_string(),
        // L3 the full API-failure set
        r#"{"type":"assistant","uuid":"a-2","parentUuid":"a-1","timestamp":"2026-06-07T05:00:02.000Z","isApiErrorMessage":true,"error":"rate_limit","apiErrorStatus":429,"quotaLimits":{"rateLimitType":"five_hour","resetsAt":1790000000,"status":"exceeded"},"truncatedAfterOutput":true,"isAbortedMidStream":true,"message":{"role":"assistant","id":"msg_a2","model":"<synthetic>","content":[{"type":"text","text":"zzkeys api failure"}]}}"#.to_string(),
        // L4 the flag and a QUOTA and nothing else - the record that used to render a
        // headline naming nothing
        r#"{"type":"assistant","uuid":"a-3","parentUuid":"a-2","timestamp":"2026-06-07T05:00:03.000Z","isApiErrorMessage":true,"quotaLimits":{"rateLimitType":"weekly","resetsAt":1790000001},"message":{"role":"assistant","id":"msg_a3","model":"<synthetic>","content":[{"type":"text","text":"zzkeys quota only"}]}}"#.to_string(),
        // L5 a turn-ending tool result
        r#"{"type":"user","uuid":"u-2","parentUuid":"a-3","timestamp":"2026-06-07T05:00:04.000Z","toolEndsTurn":true,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu-1","content":"zzkeys structured output"}]}}"#.to_string(),
        // L6 a refusal carrying the words a person typed
        r#"{"type":"user","uuid":"u-3","parentUuid":"u-2","timestamp":"2026-06-07T05:00:05.000Z","toolDenialKind":"user-rejected","userFeedback":"do not touch that file, use the staging copy instead and tell me first","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu-2","content":"zzkeys refused","is_error":true}]}}"#.to_string(),
        // L7 an interrupted turn
        r#"{"type":"user","uuid":"u-4","parentUuid":"u-3","timestamp":"2026-06-07T05:00:06.000Z","interruptedMessageId":"msg_a1","message":{"role":"user","content":"zzkeys interrupted turn"}}"#.to_string(),
        // L8 a Stop-hook summary naming its run
        r#"{"type":"system","uuid":"s-1","parentUuid":"u-4","timestamp":"2026-06-07T05:00:07.000Z","subtype":"stop_hook_summary","content":"zzkeys stop hooks ran","toolUseID":"66666666-7777-4888-8999-aaaaaaaaaaaa"}"#.to_string(),
        // L9 a Stop-hook summary naming none
        r#"{"type":"system","uuid":"s-2","parentUuid":"s-1","timestamp":"2026-06-07T05:00:08.000Z","subtype":"stop_hook_summary","content":"zzkeys stop hooks unnamed"}"#.to_string(),
        // L10 a fired scheduled prompt
        r#"{"type":"user","uuid":"u-5","parentUuid":"s-2","timestamp":"2026-06-07T05:00:09.000Z","isMeta":true,"scheduledTaskId":"task-1","scheduledFireId":"fire-1","message":{"role":"user","content":"zzkeys scheduled prompt"}}"#.to_string(),
        // L11 an api_error record: the ONE shape whose `error` is an object, headers and all
        r#"{"type":"system","uuid":"s-3","parentUuid":"u-5","timestamp":"2026-06-07T05:00:10.000Z","subtype":"api_error","content":"zzkeys api error line","error":{"type":"overloaded_error","status":529,"requestID":"req_x","headers":{"set-cookie":"ZZCOOKIEVALUE","cf-ray":"zzray"}}}"#.to_string(),
    ];
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &(lines.join("\n") + "\n"));
    h
}

/// One `show --line N --format json` record row, as a parsed object.
fn row(h: &Home, line: &str) -> serde_json::Value {
    let out = h.run(&["show", &at(SESS), "--line", line, "--format", "json"]);
    assert!(out.success, "show --line {line}: {}", out.stderr);
    let rec = out
        .stdout
        .lines()
        .find(|l| l.contains("\"kind\":\"record\""))
        .unwrap_or_else(|| panic!("no record row for line {line}:\n{}", out.stdout));
    serde_json::from_str(rec).expect("a record row is one JSON object")
}

/// The four label-zone tags this family renders, on the units that earn them.
#[test]
fn the_exposed_keys_render_their_tags_in_the_label_zone() {
    let h = keys_home();
    let out = h.run(&[
        "search",
        "zzkeys",
        &at(SESS),
        "-t",
        "agent.*",
        "-t",
        "user.*",
    ]);
    assert!(out.success, "{}", out.stderr);
    // The SPECIFIC attribution only: a skill outranks its plugin and the MCP pair, and
    // the agent name is on the wire rather than in the zone.
    assert!(
        out.stdout.contains("[via skill csift]"),
        "the specific attribution:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("[via plugin") && !out.stdout.contains("general-purpose"),
        "only the narrowest layer renders:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(
            "[api failure rate_limit status 429 quota five_hour truncated after output \
             aborted mid-stream]"
        ),
        "the failure names every half it has:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("[ends turn]"),
        "a turn-ending result:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("[feedback: do not touch that file"),
        "the refusal's own words:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("[for: interrupted msg_a1 (not in this file)]"),
        "the cancelled message is named with the reason no line answers it:\n{}",
        out.stdout
    );
}

/// The record that names ONLY its quota. It builds a failure (a quota says something) and
/// the tag names the quota - never a floor word, because the gate and the headline read
/// one list of parts.
#[test]
fn a_quota_only_failure_names_its_quota_and_no_floor_word() {
    let h = keys_home();
    let out = h.run(&["search", "zzkeys quota only", &at(SESS), "-t", "agent.*"]);
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout.contains("[api failure quota weekly]"),
        "the quota is what the record said:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("api failure unknown"),
        "no floor word is reachable:\n{}",
        out.stdout
    );
    let r = row(&h, "4");
    let q = &r["api_failure"]["quota"];
    assert_eq!(q["limit_type"], "weekly");
    assert_eq!(q["resets_at"], 1_790_000_001u64);
    assert_eq!(r["api_failure"]["error"], serde_json::Value::Null);
}

/// An ADDRESSED record carries exactly what the scanned one does - the stamping site runs
/// at both construction points, so a key is never a `search`-only fact.
#[test]
fn an_addressed_record_row_carries_every_key_its_line_names() {
    let h = keys_home();

    let attributed = row(&h, "2");
    assert_eq!(attributed["request_id"], "req_abc123");
    assert_eq!(attributed["api_block_index"], 2);
    assert_eq!(attributed["attribution"]["agent"], "general-purpose");
    assert_eq!(attributed["attribution"]["skill"], "csift");
    assert_eq!(attributed["attribution"]["mcp_tool"], "srv__probe");

    let failure = row(&h, "3");
    assert_eq!(failure["api_failure"]["error"], "rate_limit");
    assert_eq!(failure["api_failure"]["status"], 429);
    assert_eq!(failure["api_failure"]["truncated_after_output"], true);
    assert_eq!(failure["api_failure"]["aborted_mid_stream"], true);

    assert_eq!(row(&h, "5")["tool_ends_turn"], true);
    assert_eq!(
        row(&h, "6")["user_feedback"],
        "do not touch that file, use the staging copy instead and tell me first"
    );
    assert_eq!(row(&h, "7")["interrupted_message_id"], "msg_a1");
    assert_eq!(
        row(&h, "8")["hook_run_id"],
        "66666666-7777-4888-8999-aaaaaaaaaaaa"
    );
    let fired = row(&h, "10");
    assert_eq!(fired["scheduled_task_id"], "task-1");
    assert_eq!(fired["scheduled_fire_id"], "fire-1");
}

/// The `for` edge on the wire: one array, one entry from THIS family, the basis named, and
/// the honest empty where the leaf owes an edge the record cannot supply.
///
/// The array also carries the ENGINE's own `parent` edge, which every unit has and which
/// this family does not build - so the family's own entry is selected by its relation word
/// rather than by position, and the count is of the family's edges rather than of the array.
#[test]
fn the_for_edge_rides_one_key_with_its_basis_and_its_honest_empty() {
    let h = keys_home();

    let named = row(&h, "8");
    let all = named["for"].as_array().expect("an array");
    let edges: Vec<&serde_json::Value> = all
        .iter()
        .filter(|e| e["kind"] != "parent" && e["kind"] != "result_of")
        .collect();
    assert_eq!(edges.len(), 1, "at most one family edge per unit: {all:?}");
    assert_eq!(edges[0]["kind"], "hook_run");
    assert_eq!(edges[0]["basis"], "key");
    assert_eq!(
        edges[0]["target"]["uuid"],
        "66666666-7777-4888-8999-aaaaaaaaaaaa"
    );
    assert_eq!(edges[0]["target"]["line"], serde_json::Value::Null);
    assert_eq!(edges[0]["target"]["note"], "not in this file");

    let unnamed = row(&h, "9");
    let all = unnamed["for"].as_array().expect("an array");
    let edges: Vec<&serde_json::Value> = all
        .iter()
        .filter(|e| e["kind"] != "parent" && e["kind"] != "result_of")
        .collect();
    assert_eq!(edges[0]["target"], serde_json::Value::Null);
    assert_eq!(
        edges[0]["reason"],
        "no toolUseID on this stop_hook_summary record"
    );
    assert_eq!(
        unnamed["hook_run_id"],
        serde_json::Value::Null,
        "a record that names no run carries no id key"
    );
}

/// The `error` OBJECT of an `api_error` record holds the failed request's response
/// headers. It reaches no rendered or JSON path; `--raw` is the documented way to the
/// bytes and it prints the line as it is.
#[test]
fn a_header_value_reaches_no_path_but_raw() {
    let h = keys_home();

    for argv in [
        vec![
            "search",
            "zzkeys",
            &at(SESS),
            "-t",
            "harness.system.api-error",
        ],
        vec!["show", &at(SESS), "--line", "11"],
        vec!["show", &at(SESS), "--line", "11", "--format", "json"],
    ] {
        let out = h.run(&argv);
        assert!(out.success, "{argv:?}: {}", out.stderr);
        assert!(
            !out.stdout.contains("ZZCOOKIEVALUE") && !out.stdout.contains("zzray"),
            "{argv:?} must carry no header VALUE:\n{}",
            out.stdout
        );
    }

    let raw = h.run(&["show", &at(SESS), "--line", "11", "--raw"]);
    assert!(raw.success, "{}", raw.stderr);
    assert!(
        raw.stdout.contains("ZZCOOKIEVALUE"),
        "--raw is the escape hatch and prints the line:\n{}",
        raw.stdout
    );

    // And the api_error record produces no failure fact at all: its `error` is an object,
    // which the record model's decoder refuses.
    let r = row(&h, "11");
    assert_eq!(r["api_failure"], serde_json::Value::Null);
}

/// A record naming none of the keys carries none of them, and the absence is at KEY level:
/// the fact objects are absent rather than present-and-empty.
#[test]
fn a_record_that_names_no_key_carries_no_key() {
    let h = keys_home();
    let opener = row(&h, "1");
    for key in [
        "attribution",
        "request_id",
        "api_block_index",
        "tool_ends_turn",
        "user_feedback",
        "api_failure",
        "interrupted_message_id",
        "hook_run_id",
        "scheduled_task_id",
        "scheduled_fire_id",
        "for",
    ] {
        assert_eq!(
            opener[key],
            serde_json::Value::Null,
            "{key} is absent, not empty: {opener}"
        );
    }
}

/// A key of an UNEXPECTED JSON type costs that key, never the record.
///
/// Every exposed key is a narrow type - a `u32`, a `bool`, an object, a string - and a
/// strictly decoded field turns one wrong type into a parse error for the WHOLE line: the
/// record is booked malformed and its TEXT disappears from every surface, which is the
/// opposite of the trade csift makes everywhere else (AGENTS section 4: extend the model
/// tolerantly, never tighten it into a crash). So each key decodes leniently and an
/// unexpected type reads exactly as an absent key does.
///
/// The four shapes below are one per decoder - a number key given a string, a boolean key
/// given a word, an object key given a string, a string key given a number - and each line
/// also carries a WELL-typed sibling key, because the rule is per key rather than per
/// record: the odd one reads absent and the good one still decodes.
#[test]
fn an_unexpected_type_costs_its_own_key_and_never_the_record() {
    let lines = [
        r#"{"type":"assistant","uuid":"a-1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","apiBlockIndex":"3","requestId":"req_ok1","message":{"role":"assistant","id":"m-1","model":"claude","content":[{"type":"text","text":"zzodd block index"}]}}"#,
        r#"{"type":"user","uuid":"u-1","parentUuid":"a-1","timestamp":"2026-06-07T05:00:01.000Z","toolEndsTurn":"yes","message":{"role":"user","content":"zzodd ends turn"}}"#,
        r#"{"type":"assistant","uuid":"a-2","parentUuid":"u-1","timestamp":"2026-06-07T05:00:02.000Z","isApiErrorMessage":true,"quotaLimits":"weekly","error":"rate_limit","message":{"role":"assistant","id":"m-2","model":"<synthetic>","content":[{"type":"text","text":"zzodd quota"}]}}"#,
        r#"{"type":"assistant","uuid":"a-3","parentUuid":"a-2","timestamp":"2026-06-07T05:00:03.000Z","attributionSkill":42,"attributionPlugin":"omc","message":{"role":"assistant","id":"m-3","model":"claude","content":[{"type":"text","text":"zzodd attribution"}]}}"#,
    ];
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &(lines.join("\n") + "\n"));

    // Every line is emitted and NOTHING is booked malformed.
    let out = h.run(&["search", "zzodd", &at(SESS), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let summary: serde_json::Value = serde_json::from_str(
        out.stdout
            .lines()
            .find(|l| l.contains("\"kind\":\"summary\""))
            .expect("a summary line"),
    )
    .expect("the summary is one object");
    assert_eq!(summary["skipped_lines"], 0, "{}", out.stdout);
    // All four records land in the ONE turn the opener starts, so the exchange count is 1
    // and the four are its hits.
    assert_eq!(summary["matched"], 1, "{}", out.stdout);
    let hits: Vec<serde_json::Value> = serde_json::from_str::<serde_json::Value>(
        out.stdout
            .lines()
            .find(|l| l.contains("\"kind\":\"exchange\""))
            .expect("an exchange line"),
    )
    .expect("the exchange is one object")["hits"]
        .as_array()
        .expect("hits is an array")
        .clone();
    assert_eq!(hits.len(), 4, "every line emitted: {}", out.stdout);
    for text in [
        "zzodd block index",
        "zzodd ends turn",
        "zzodd quota",
        "zzodd attribution",
    ] {
        assert!(
            out.stdout.contains(text),
            "the text on the line survives its odd key: {text}\n{}",
            out.stdout
        );
    }

    // The odd key reads ABSENT; the well-typed sibling on the same record decodes.
    let r1 = row(&h, "1");
    assert!(
        r1["api_block_index"].is_null(),
        "a string is not an index: {r1}"
    );
    assert_eq!(r1["request_id"], "req_ok1", "{r1}");

    let r2 = row(&h, "2");
    assert!(
        r2["tool_ends_turn"].is_null(),
        "a word is not a boolean: {r2}"
    );

    // The failure still builds from the parts the record DID give, and the object-typed
    // key that was a string contributes nothing rather than failing the line.
    let r3 = row(&h, "3");
    assert!(
        r3["api_failure"]["quota"].is_null(),
        "a string is not a quota: {r3}"
    );
    assert_eq!(r3["api_failure"]["error"], "rate_limit", "{r3}");

    let r4 = row(&h, "4");
    assert!(
        r4["attribution"]["skill"].is_null(),
        "a number is not a name: {r4}"
    );
    assert_eq!(r4["attribution"]["plugin"], "omc", "{r4}");
    // The label zone names the layer the record DID give, so the odd key costs the tag
    // its own name and nothing else.
    let text = h.run(&["search", "zzodd attribution", &at(SESS)]);
    assert!(
        text.stdout.contains("[via plugin omc]"),
        "the surviving layer still renders:\n{}",
        text.stdout
    );
}
