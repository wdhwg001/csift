//! The EXPOSED KEYS family's own `facts()`: which values reach a hit, which of them
//! render a label-zone tag, and the ONE `for` edge a unit carries.
//!
//! Read through [`HitFact::label_zone`] and [`HitFact::json`] rather than off the enum,
//! because those two are the whole surface a fact has.

use super::*;
use crate::commands::search::record_text::keys;

/// The facts this record's record-level unit produces, as `(label zone, json keys)`.
fn projections(line: &str, class: Option<Class>) -> (Vec<String>, Vec<String>) {
    let r = rec(line);
    let ctx = ClassifyCtx::top_level();
    let (facts, markers) = keys::facts(&r, None, &Section::record(class), &ctx);
    assert!(
        markers.is_empty(),
        "this family renders every tag through a FACT, so a marker here would be a second \
         vocabulary for one idea: {markers:?}"
    );
    let zones: Vec<String> = facts.iter().filter_map(HitFact::label_zone).collect();
    let mut json: Vec<String> = Vec::new();
    for f in &facts {
        for (k, v) in f.json() {
            json.push(format!("{k}={v}"));
        }
    }
    (zones, json)
}

const ATTRIBUTED: &str = r#"{"type":"assistant","uuid":"a-1","requestId":"req_1","apiBlockIndex":3,"attributionAgent":"general-purpose","attributionSkill":"csift","attributionPlugin":"omc","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#;

/// A record naming NO exposed key produces no fact at all - which is the common case and
/// the reason a missing key reads as "this record does not say" rather than as "no".
#[test]
fn a_record_that_names_no_key_produces_no_fact() {
    let (zones, json) = projections(
        r#"{"type":"assistant","uuid":"a-0","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
        Some(Class::AgentMessagePlain),
    );
    assert!(zones.is_empty() && json.is_empty(), "{zones:?} {json:?}");
}

/// Five of the eleven facts render a tag and six ride JSON alone. The split is the family's
/// statement about what is worth a reader's line, so it is pinned at both ends: the ids
/// are on the wire and NOT in the label zone.
#[test]
fn a_join_key_rides_json_alone_while_the_specific_attribution_renders() {
    let (zones, json) = projections(ATTRIBUTED, Some(Class::AgentMessagePlain));
    assert_eq!(zones, vec!["[via skill csift]".to_string()]);
    assert!(
        json.iter().any(|j| j.starts_with("request_id=")),
        "the request id rides JSON: {json:?}"
    );
    assert!(
        json.iter().any(|j| j == "api_block_index=3"),
        "the block index rides JSON: {json:?}"
    );
    assert!(
        !zones.iter().any(|z| z.contains("req_1")),
        "a join key is not a word to scan: {zones:?}"
    );
}

/// The attribution object names EVERY member, with null for one the record lacks, so a
/// consumer reads presence from the KEY and never from an interior.
#[test]
fn the_attribution_object_names_every_member_with_null_for_an_absent_one() {
    let (_, json) = projections(ATTRIBUTED, Some(Class::AgentMessagePlain));
    let attribution = json
        .iter()
        .find(|j| j.starts_with("attribution="))
        .expect("an attribution fact");
    for member in ["agent", "skill", "plugin", "mcp_server", "mcp_tool"] {
        assert!(
            attribution.contains(&format!("\"{member}\"")),
            "{member} named: {attribution}"
        );
    }
    assert!(
        attribution.contains("\"mcp_server\":null") && attribution.contains("\"mcp_tool\":null"),
        "an absent member is explicitly null: {attribution}"
    );
}

/// A refusal's typed words are CUT in the label zone and whole on the wire, because the
/// label zone is one line beside a leaf path and a person refusing can type a paragraph.
#[test]
fn the_refusal_text_is_cut_in_the_label_zone_and_whole_on_the_wire() {
    let long = "do not touch that file, use the staging copy instead and tell me first";
    let line = format!(
        r#"{{"type":"user","uuid":"u-1","toolDenialKind":"user-rejected","userFeedback":"{long}"}}"#
    );
    let (zones, json) = projections(&line, Some(Class::AgentToolResult));
    let zone = zones.first().expect("a feedback tag");
    assert!(zone.starts_with("[feedback: "), "{zone}");
    assert!(
        zone.contains("(+10 chars)"),
        "the cut is explicit and counts the remainder: {zone}"
    );
    assert!(
        json.iter().any(|j| j.contains(long)),
        "the whole text is on the wire: {json:?}"
    );
}

/// `[ends turn]` is a tag because it says something about THIS record a reader acts on.
#[test]
fn a_turn_ending_tool_result_says_so_in_the_label_zone() {
    let (zones, json) = projections(
        r#"{"type":"user","uuid":"u-2","toolEndsTurn":true}"#,
        Some(Class::AgentToolResult),
    );
    assert_eq!(zones, vec!["[ends turn]".to_string()]);
    assert_eq!(json, vec!["tool_ends_turn=true".to_string()]);
}

/// The api-failure tag names every half the record gave and no half it did not - and a
/// record that named only its quota renders THAT, never a floor word.
#[test]
fn the_api_failure_tag_names_what_the_record_said() {
    let (full, _) = projections(
        r#"{"type":"assistant","uuid":"a-2","isApiErrorMessage":true,"error":"rate_limit","apiErrorStatus":429,"quotaLimits":{"rateLimitType":"five_hour","resetsAt":1790000000},"truncatedAfterOutput":true,"isAbortedMidStream":true}"#,
        Some(Class::AgentMessagePlain),
    );
    assert_eq!(
        full,
        vec![
            "[api failure rate_limit status 429 quota five_hour truncated after output \
             aborted mid-stream]"
                .to_string()
        ]
    );

    let (quota_only, json) = projections(
        r#"{"type":"assistant","uuid":"a-3","isApiErrorMessage":true,"quotaLimits":{"rateLimitType":"weekly","resetsAt":1790000001}}"#,
        Some(Class::AgentMessagePlain),
    );
    assert_eq!(quota_only, vec!["[api failure quota weekly]".to_string()]);
    let obj = json.first().expect("an api_failure object");
    assert!(
        obj.contains("\"limit_type\":\"weekly\"") && obj.contains("\"resets_at\":1790000001"),
        "the quota is summarised to its two members: {obj}"
    );
    assert!(
        obj.contains("\"error\":null") && obj.contains("\"status\":null"),
        "the object names every member: {obj}"
    );
}

/// A `system`/`api_error` record carries an `error` OBJECT holding the failed request's
/// response headers. This family refuses the object at the record model, so the unit
/// produces no failure fact and nothing it emits can carry a header name.
#[test]
fn an_api_error_record_produces_no_failure_fact_and_no_header() {
    let (zones, json) = projections(
        r#"{"type":"system","uuid":"s-1","subtype":"api_error","content":"boom","error":{"type":"overloaded_error","status":529,"headers":{"set-cookie":"SECRETCOOKIE"}}}"#,
        Some(Class::SystemApiError),
    );
    assert!(zones.is_empty(), "{zones:?}");
    assert!(json.is_empty(), "{json:?}");
}

/// The ONE `for` edge, and the three things it has to say. Neither id this family carries
/// is a record uuid, so on any transcript the edge takes the shared builder's middle
/// answer - the id, with the note that no line here carries it.
#[test]
fn the_interrupted_edge_names_the_id_no_line_here_carries() {
    let (zones, json) = projections(
        r#"{"type":"user","uuid":"u-3","interruptedMessageId":"msg_a1","message":{"role":"user","content":"x"}}"#,
        Some(Class::UserMessage),
    );
    assert!(
        zones.contains(&"[for: interrupted msg_a1 (not in this file)]".to_string()),
        "{zones:?}"
    );
    let edge = json
        .iter()
        .find(|j| j.starts_with("for="))
        .expect("an edge");
    assert!(
        edge.contains("\"kind\":\"interrupted\"") && edge.contains("\"basis\":\"key\""),
        "{edge}"
    );
    assert!(edge.contains("\"note\":\"not in this file\""), "{edge}");
    assert!(
        json.iter()
            .any(|j| j == "interrupted_message_id=\"msg_a1\""),
        "the id also rides its own key: {json:?}"
    );
}

/// A Stop-hook summary that names no run renders the edge's HONEST EMPTY rather than one
/// fewer field: the leaf owes the edge, so a shape that cannot supply it is worth saying.
#[test]
fn a_stop_hook_summary_with_no_run_id_renders_the_edges_honest_empty() {
    let (zones, json) = projections(
        r#"{"type":"system","uuid":"s-2","subtype":"stop_hook_summary","content":"hooks ran"}"#,
        Some(Class::TurnStopHooks),
    );
    assert_eq!(
        zones,
        vec!["[for: hook run unknown (no toolUseID on this stop_hook_summary record)]".to_string()]
    );
    let edge = json
        .iter()
        .find(|j| j.starts_with("for="))
        .expect("an edge");
    assert!(edge.contains("\"target\":null"), "{edge}");

    // With the id, the same leaf renders the resolvable form and carries the key.
    let (with_id, json) = projections(
        r#"{"type":"system","uuid":"s-3","subtype":"stop_hook_summary","content":"hooks ran","toolUseID":"66666666-7777-4888-8999-aaaaaaaaaaaa"}"#,
        Some(Class::TurnStopHooks),
    );
    assert_eq!(
        with_id,
        vec!["[for: hook run 66666666 (not in this file)]".to_string()]
    );
    assert!(
        json.iter()
            .any(|j| j == "hook_run_id=\"66666666-7777-4888-8999-aaaaaaaaaaaa\""),
        "{json:?}"
    );
}

/// AT MOST ONE edge per unit. A unit with two answers to "which record is this FOR" has
/// none, so the precedence is fixed rather than left to which key happens to be there.
#[test]
fn a_unit_carries_at_most_one_for_edge() {
    let both = r#"{"type":"user","uuid":"u-4","interruptedMessageId":"msg_a1","subtype":"stop_hook_summary","toolUseID":"66666666-7777-4888-8999-aaaaaaaaaaaa"}"#;
    let (zones, json) = projections(both, Some(Class::TurnStopHooks));
    assert_eq!(
        zones.iter().filter(|z| z.starts_with("[for:")).count(),
        1,
        "{zones:?}"
    );
    assert_eq!(json.iter().filter(|j| j.starts_with("for=")).count(), 1);
    assert!(
        zones.contains(&"[for: interrupted msg_a1 (not in this file)]".to_string()),
        "the cancelled message outranks the hook run: {zones:?}"
    );
}

/// The scheduler's two ids ride JSON alone: the label zone already names the fired
/// prompt's instant through the engine's own field, and these are joins.
#[test]
fn the_scheduler_ids_ride_json_alone() {
    let (zones, json) = projections(
        r#"{"type":"user","uuid":"u-5","isMeta":true,"scheduledTaskId":"task-1","scheduledFireId":"fire-1"}"#,
        Some(Class::ScheduleFire),
    );
    assert!(zones.is_empty(), "{zones:?}");
    assert!(
        json.contains(&"scheduled_task_id=\"task-1\"".to_string()),
        "{json:?}"
    );
    assert!(
        json.contains(&"scheduled_fire_id=\"fire-1\"".to_string()),
        "{json:?}"
    );
}

/// The PDF page RANGE a Read with a `pages` parameter returned - the range half of the
/// same derivation `image` prints per page, so the two surfaces cannot drift. The tag
/// carries the document's BASENAME because it sits beside a leaf path on one line; the
/// whole path rides the JSON key.
#[test]
fn a_pdf_page_result_carries_its_range_and_names_the_document() {
    let (zones, json) = projections(
        r#"{"type":"user","uuid":"u-6","toolUseResult":{"type":"parts","firstPage":5,"file":{"count":3,"filePath":"/Users/dev/docs/report.pdf","originalSize":223744,"outputDir":"/Users/dev/tmp/pdf-1"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[
            {"type":"text","text":"PDF pages extracted: 3 page(s) from /Users/dev/docs/report.pdf (218.5KB)"},
            {"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"aGk="}},
            {"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"aGVsbG8="}},
            {"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"eWVz"}}]}]}}"#,
        Some(Class::AgentToolResult),
    );
    assert_eq!(zones, vec!["[pdf pages 5-7 of report.pdf]".to_string()]);
    assert!(
        json.contains(
            &"pdf_pages={\"count\":3,\"first\":5,\"last\":7,\"path\":\"/Users/dev/docs/report.pdf\"}"
                .to_string()
        ),
        "{json:?}"
    );
}

/// A tool result that is not a page read carries no such fact - the gate is the record's
/// own result object, so a screenshot and a carrier with no offset both stay silent
/// rather than reporting a range that starts at page 1.
#[test]
fn a_tool_result_without_the_offset_carries_no_range() {
    for line in [
        // A screenshot: an image result, no parts and no firstPage.
        r#"{"type":"user","uuid":"u-7","toolUseResult":{"type":"image","file":{"filePath":"/Users/dev/shot.png"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[
            {"type":"text","text":"shot"},
            {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGk="}}]}]}}"#,
        // A parts result from a build before the key existed.
        r#"{"type":"user","uuid":"u-8","toolUseResult":{"type":"parts","file":{"count":1,"filePath":"/Users/dev/docs/report.pdf"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[
            {"type":"text","text":"PDF pages extracted: 1 page(s) from /Users/dev/docs/report.pdf (1KB)"},
            {"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"aGk="}}]}]}}"#,
    ] {
        let (zones, json) = projections(line, Some(Class::AgentToolResult));
        assert!(zones.is_empty(), "{zones:?}");
        assert!(
            !json.iter().any(|j| j.starts_with("pdf_pages=")),
            "{json:?}"
        );
    }
}
