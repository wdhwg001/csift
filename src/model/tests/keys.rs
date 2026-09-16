//! The EXPOSED KEYS on the record model: what each field decodes to, which shape the
//! `error` decoder refuses, and the three accessors that assemble a value out of several
//! fields.
//!
//! Every value asserted here is derived from the documented semantics - the precedence
//! `Attribution::specific` states, the two gates `Record::api_failure` states, the subtype
//! `Record::hook_run_id` states - never copied out of a run.

use super::*;

/// An assistant record carrying every attribution key plus the two API joins.
const ATTRIBUTED: &str = r#"{"type":"assistant","uuid":"a-1","requestId":"req_1","apiBlockIndex":3,"attributionAgent":"general-purpose","attributionSkill":"csift","attributionPlugin":"omc","attributionMcpServer":"srv","attributionMcpTool":"srv__tool","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#;

#[test]
fn every_exposed_key_decodes_to_its_own_field() {
    let r = parse(ATTRIBUTED);
    assert_eq!(r.request_id.as_deref(), Some("req_1"));
    assert_eq!(r.api_block_index, Some(3));
    assert_eq!(r.attribution_agent.as_deref(), Some("general-purpose"));
    assert_eq!(r.attribution_skill.as_deref(), Some("csift"));
    assert_eq!(r.attribution_plugin.as_deref(), Some("omc"));
    assert_eq!(r.attribution_mcp_server.as_deref(), Some("srv"));
    assert_eq!(r.attribution_mcp_tool.as_deref(), Some("srv__tool"));
    // A record naming none of them carries none of them - the absence rule at key level.
    let bare = parse(
        r#"{"type":"assistant","uuid":"a-2","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
    );
    assert!(bare.attribution().is_none());
    assert_eq!(bare.request_id, None);
    assert_eq!(bare.api_block_index, None);
}

/// The precedence is a STATEMENT about which layer a reader is shown, so it is asserted
/// layer by layer rather than once on the full set: a skill outranks the plugin that owns
/// it, the plugin outranks the MCP pair, and the SERVER is the stable half of that pair.
#[test]
fn the_specific_attribution_is_the_narrowest_layer_the_record_names() {
    let full = parse(ATTRIBUTED).attribution().expect("five keys");
    assert_eq!(full.specific(), Some(("skill", "csift")));

    let no_skill = Attribution {
        skill: None,
        ..full.clone()
    };
    assert_eq!(no_skill.specific(), Some(("plugin", "omc")));

    let mcp = Attribution {
        skill: None,
        plugin: None,
        ..full.clone()
    };
    assert_eq!(mcp.specific(), Some(("mcp", "srv")));

    let tool_only = Attribution {
        mcp_server: None,
        ..mcp.clone()
    };
    assert_eq!(tool_only.specific(), Some(("mcp", "srv__tool")));

    // An AGENT alone is not specific: it is on every record of a subagent lane, so a
    // marker naming it would be the same word on every line of that transcript.
    let agent_only = Attribution {
        agent: Some("general-purpose".to_string()),
        ..Attribution::default()
    };
    assert_eq!(agent_only.specific(), None);
}

/// `error` is a STRING on the synthetic failure record and an OBJECT on a
/// `system`/`api_error` one, and that object holds the failed request's response headers.
/// The decoder is where that is decided, so a record carrying the object reads as carrying
/// no error kind at all - there is no later stage that could leak it.
#[test]
fn the_error_object_never_enters_the_record_model() {
    let string_form =
        parse(r#"{"type":"assistant","uuid":"a-3","isApiErrorMessage":true,"error":"rate_limit"}"#);
    assert_eq!(string_form.error_kind(), Some("rate_limit"));

    let object_form = parse(
        r#"{"type":"system","uuid":"s-1","subtype":"api_error","error":{"type":"overloaded_error","status":529,"headers":{"set-cookie":"SECRET"}}}"#,
    );
    assert_eq!(object_form.error_kind(), None);
    assert!(object_form.api_failure().is_none());
}

/// TWO gates, and they are different claims. The flag says the record IS a failure
/// placeholder; the second says the failure SAYS something. A record carrying the flag
/// and nothing else would otherwise produce five members reading four nulls and a
/// `false`, where the `false` pair reads as a positive "no".
#[test]
fn a_failure_is_built_only_when_the_flag_is_set_and_the_record_says_something() {
    let unflagged = parse(r#"{"type":"assistant","uuid":"a-4","error":"rate_limit"}"#);
    assert!(unflagged.api_failure().is_none(), "no flag, no failure");

    let flag_only = parse(r#"{"type":"assistant","uuid":"a-5","isApiErrorMessage":true}"#);
    assert!(flag_only.api_failure().is_none(), "flag alone says nothing");

    let kind_only = parse(
        r#"{"type":"assistant","uuid":"a-6","isApiErrorMessage":true,"error":"server_error"}"#,
    );
    assert_eq!(
        kind_only
            .api_failure()
            .and_then(|f| f.headline())
            .as_deref(),
        Some("server_error")
    );
}

/// The gate and the headline read the SAME list of parts, which is the property that
/// makes a floor word unreachable: a record naming only its quota builds a failure AND
/// renders what it named, rather than building one that renders `unknown`.
#[test]
fn a_quota_only_failure_builds_and_names_its_quota() {
    let quota_only = parse(
        r#"{"type":"assistant","uuid":"a-7","isApiErrorMessage":true,"quotaLimits":{"rateLimitType":"weekly","resetsAt":1790000001,"status":"exceeded"}}"#,
    );
    let f = quota_only.api_failure().expect("a quota says something");
    assert_eq!(f.headline().as_deref(), Some("quota weekly"));
    assert_eq!(f.error, None);
    assert_eq!(f.status, None);
    // The object is narrowed at deserialization: the other eight measured keys are gone.
    let q = f.quota.expect("a summary");
    assert_eq!(q.limit_type.as_deref(), Some("weekly"));
    assert_eq!(q.resets_at, Some(1_790_000_001));
}

/// An EMPTY quota object names neither member, so it is dropped rather than carried: an
/// empty summary widens the wire while answering nothing, which is the same rule the
/// failure's own second gate follows.
#[test]
fn an_empty_quota_object_is_dropped_rather_than_carried() {
    let r = parse(
        r#"{"type":"assistant","uuid":"a-8","isApiErrorMessage":true,"error":"rate_limit","quotaLimits":{"status":"exceeded"}}"#,
    );
    let f = r.api_failure().expect("the kind says something");
    assert_eq!(f.quota, None);
    assert_eq!(f.headline().as_deref(), Some("rate_limit"));
}

/// The headline's ORDER is what a reader scans, so it is pinned whole: kind, status,
/// quota window, then the two partial-response words.
#[test]
fn the_headline_names_every_part_the_record_gave_in_one_order() {
    let r = parse(
        r#"{"type":"assistant","uuid":"a-9","isApiErrorMessage":true,"error":"rate_limit","apiErrorStatus":429,"quotaLimits":{"rateLimitType":"five_hour"},"truncatedAfterOutput":true,"isAbortedMidStream":true}"#,
    );
    let f = r.api_failure().expect("a full failure");
    assert_eq!(
        f.headline().as_deref(),
        Some("rate_limit status 429 quota five_hour truncated after output aborted mid-stream")
    );
    assert_eq!(f.status, Some(429));
    assert!(f.truncated_after_output && f.aborted_mid_stream);
}

/// A quota with a reset instant and NO window still says something, and the headline says
/// the bare word rather than inventing a window name.
#[test]
fn a_quota_with_no_window_renders_the_bare_word() {
    let r = parse(
        r#"{"type":"assistant","uuid":"a-10","isApiErrorMessage":true,"quotaLimits":{"resetsAt":1790000002}}"#,
    );
    let f = r.api_failure().expect("a reset instant says something");
    assert_eq!(f.headline().as_deref(), Some("quota"));
}

/// `toolUseID` reads like an API tool_use id and is not one, so it is read on the ONE
/// subtype it was measured on. A later build putting a real tool_use id under the same
/// name on another shape must not be reported as a hook run.
#[test]
fn a_hook_run_id_is_read_on_its_own_subtype_and_no_other() {
    let summary = parse(
        r#"{"type":"system","uuid":"s-2","subtype":"stop_hook_summary","toolUseID":"66666666-7777-4888-8999-aaaaaaaaaaaa"}"#,
    );
    assert_eq!(
        summary.hook_run_id(),
        Some("66666666-7777-4888-8999-aaaaaaaaaaaa")
    );

    let other_subtype = parse(
        r#"{"type":"system","uuid":"s-3","subtype":"turn_duration","toolUseID":"66666666-7777-4888-8999-aaaaaaaaaaaa"}"#,
    );
    assert_eq!(other_subtype.hook_run_id(), None);

    let no_subtype = parse(r#"{"type":"system","uuid":"s-4","toolUseID":"x"}"#);
    assert_eq!(no_subtype.hook_run_id(), None);
}

/// The remaining keys, each on the record shape it was measured on.
#[test]
fn the_join_keys_and_the_refusal_text_decode_where_they_ride() {
    let ends = parse(r#"{"type":"user","uuid":"u-1","toolEndsTurn":true}"#);
    assert_eq!(ends.tool_ends_turn, Some(true));

    let refusal = parse(
        r#"{"type":"user","uuid":"u-2","toolDenialKind":"user-rejected","userFeedback":"use the staging copy"}"#,
    );
    assert_eq!(
        refusal.user_feedback.as_deref(),
        Some("use the staging copy")
    );

    let interrupted = parse(r#"{"type":"user","uuid":"u-3","interruptedMessageId":"msg_a1"}"#);
    assert_eq!(
        interrupted.interrupted_message_id.as_deref(),
        Some("msg_a1")
    );

    let fired = parse(
        r#"{"type":"user","uuid":"u-4","isMeta":true,"scheduledTaskId":"task-1","scheduledFireId":"fire-1"}"#,
    );
    assert_eq!(fired.scheduled_task_id.as_deref(), Some("task-1"));
    assert_eq!(fired.scheduled_fire_id.as_deref(), Some("fire-1"));
}
