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
    let k = r
        .exposed_keys()
        .expect("the record names eight of the keys");
    assert_eq!(k.request_id.as_deref(), Some("req_1"));
    assert_eq!(k.api_block_index, Some(3));
    assert_eq!(k.attribution_agent.as_deref(), Some("general-purpose"));
    assert_eq!(k.attribution_skill.as_deref(), Some("csift"));
    assert_eq!(k.attribution_plugin.as_deref(), Some("omc"));
    assert_eq!(k.attribution_mcp_server.as_deref(), Some("srv"));
    assert_eq!(k.attribution_mcp_tool.as_deref(), Some("srv__tool"));
    // A record naming none of them carries none of them - the absence rule at key level.
    let bare = parse(
        r#"{"type":"assistant","uuid":"a-2","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
    );
    assert!(bare.attribution().is_none());
    assert!(
        bare.exposed_keys().is_none(),
        "a record naming no exposed key allocates no box for them"
    );
}

/// ONE key is enough. The gate on the attribution object is ANY of the five, not all of
/// them, and a record naming a single one is the ordinary main-lane shape rather than an
/// edge case: measured over two projects roots, 3,082 and 3,178 records carry a skill,
/// plugin or MCP attribution with no `attributionAgent` beside it.
#[test]
fn any_one_attribution_key_is_enough_to_carry_the_object() {
    for (key, read) in [
        ("attributionAgent", "agent"),
        ("attributionSkill", "skill"),
        ("attributionPlugin", "plugin"),
        ("attributionMcpServer", "mcp_server"),
        ("attributionMcpTool", "mcp_tool"),
    ] {
        let line = format!(r#"{{"type":"assistant","uuid":"a-11","{key}":"only-one"}}"#);
        let a = parse(&line)
            .attribution()
            .unwrap_or_else(|| panic!("{key} alone carries the object"));
        let member = match read {
            "agent" => &a.agent,
            "skill" => &a.skill,
            "plugin" => &a.plugin,
            "mcp_server" => &a.mcp_server,
            _ => &a.mcp_tool,
        };
        assert_eq!(member.as_deref(), Some("only-one"), "{key}");
    }
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
    assert_eq!(
        ends.exposed_keys().and_then(|k| k.tool_ends_turn),
        Some(true)
    );

    let refusal = parse(
        r#"{"type":"user","uuid":"u-2","toolDenialKind":"user-rejected","userFeedback":"use the staging copy"}"#,
    );
    assert_eq!(
        refusal
            .exposed_keys()
            .and_then(|k| k.user_feedback.as_deref()),
        Some("use the staging copy")
    );

    let interrupted = parse(r#"{"type":"user","uuid":"u-3","interruptedMessageId":"msg_a1"}"#);
    assert_eq!(
        interrupted
            .exposed_keys()
            .and_then(|k| k.interrupted_message_id.as_deref()),
        Some("msg_a1")
    );

    let fired = parse(
        r#"{"type":"user","uuid":"u-4","isMeta":true,"scheduledTaskId":"task-1","scheduledFireId":"fire-1"}"#,
    );
    let k = fired
        .exposed_keys()
        .expect("the fired prompt names both ids");
    assert_eq!(k.scheduled_task_id.as_deref(), Some("task-1"));
    assert_eq!(k.scheduled_fire_id.as_deref(), Some("fire-1"));
}

/// A key of an UNEXPECTED JSON type costs that key and nothing else.
///
/// Every value below is what the DOCUMENTED rule says: the decoders accept one JSON type
/// each and answer `None` for every other, so an odd type and an absent key give the same
/// answer - this record does not say. Read directly off the field rather than through a
/// render, because the decision is the decoder's.
#[test]
fn an_unexpected_type_reads_as_an_absent_key() {
    // A number key given a string, and a string key given a number.
    let odd = parse(
        r#"{"type":"assistant","uuid":"a-20","apiBlockIndex":"3","apiErrorStatus":[429],"attributionSkill":42,"attributionPlugin":"omc","requestId":"req_ok"}"#,
    );
    let k = odd.exposed_keys().expect("the two good keys carry the box");
    assert_eq!(k.api_block_index, None, "a string is not an index");
    assert_eq!(k.api_error_status, None, "an array is not a status");
    assert_eq!(k.attribution_skill, None, "a number is not a name");
    // The well-typed siblings on the SAME record still decode: the rule is per key.
    assert_eq!(k.attribution_plugin.as_deref(), Some("omc"));
    assert_eq!(k.request_id.as_deref(), Some("req_ok"));

    // A boolean key given a word, and an object key given a string.
    let odd2 = parse(
        r#"{"type":"user","uuid":"u-20","toolEndsTurn":"yes","truncatedAfterOutput":1,"quotaLimits":"weekly","userFeedback":"say so"}"#,
    );
    let k2 = odd2.exposed_keys().expect("the feedback carries the box");
    assert_eq!(k2.tool_ends_turn, None, "a word is not a boolean");
    assert_eq!(k2.truncated_after_output, None, "a number is not a boolean");
    assert!(k2.quota_limits.is_none(), "a string is not a quota object");
    assert_eq!(k2.user_feedback.as_deref(), Some("say so"));

    // A quota object whose OWN members are oddly typed keeps the object and drops the
    // members, for the same reason one level up.
    let odd3 = parse(
        r#"{"type":"assistant","uuid":"a-21","quotaLimits":{"rateLimitType":5,"resetsAt":"soon"}}"#,
    );
    let q = odd3
        .exposed_keys()
        .and_then(|k| k.quota_limits.as_deref())
        .expect("the object itself decodes");
    assert_eq!(q.limit_type, None);
    assert_eq!(q.resets_at, None);

    // A value too large for the field's own type reads absent for the same reason: the
    // record said something this field cannot carry.
    let big = parse(r#"{"type":"assistant","uuid":"a-22","apiBlockIndex":4294967296}"#);
    assert!(
        big.exposed_keys().is_none(),
        "a u32 field cannot carry 2^32, so the record names no key at all"
    );

    // A NEGATIVE number is refused the same way - none of the three keys that use this
    // decoder (an HTTP status, a block index, an epoch second) has a meaning below zero.
    let neg = parse(r#"{"type":"assistant","uuid":"a-23","apiErrorStatus":-1,"error":"x"}"#);
    assert_eq!(
        neg.exposed_keys().and_then(|k| k.api_error_status),
        None,
        "a negative is not a status"
    );
}

/// EVERY JSON type, through every decoder, read off the field.
///
/// The decoders accept one type each and answer `None` for all the others, and that is a
/// per-TYPE statement rather than a per-key one: serde dispatches a JSON value by its own
/// shape, so each shape is its own arm and each arm is its own way to lose a record. The
/// table feeds one key per decoder every shape a JSON value can take - a plain string, a
/// string carrying an escape (a different arm: the value is owned rather than borrowed), a
/// non-negative integer, a negative one, a float, a boolean, null, an array and an object -
/// and asserts the value the documented rule gives.
#[test]
fn every_json_shape_reaches_every_decoder_and_only_its_own_type_decodes() {
    // (the JSON value as written, what each decoder makes of it). The two OBJECT decoders
    // answer alike on every shape, which is why one column carries both: each keeps an
    // object and refuses every other type.
    //                      value        str        u32      bool     the two objects
    let table: &[(&str, bool, bool, bool, bool)] = &[
        (r#""ok""#, true, false, false, false),
        (r#""a\nb""#, true, false, false, false),
        ("7", false, true, false, false),
        ("-7", false, false, false, false),
        ("1.5", false, false, false, false),
        ("true", false, false, true, false),
        ("null", false, false, false, false),
        ("[1,2]", false, false, false, false),
        (r#"{"k":1}"#, false, false, false, true),
    ];
    for (value, want_str, want_u32, want_bool, want_quota) in table {
        // The stop-details decoder sits on `message`, so it rides the same value through
        // its own key; a member it models is named so an object shape SAYS something and
        // the accessor's own gate does not swallow the row.
        let sd = parse(&format!(
            r#"{{"type":"assistant","uuid":"a-32","message":{{"role":"assistant","stop_details":{value},"content":[{{"type":"text","text":"anchor"}}]}}}}"#
        ));
        assert_eq!(
            sd.message
                .as_ref()
                .and_then(|m| m.stop_details.as_ref())
                .is_some(),
            *want_quota,
            "stop_details_only on {value}"
        );
        let r = parse(&format!(
            r#"{{"type":"assistant","uuid":"a-30","requestId":{value},"apiBlockIndex":{value},"toolEndsTurn":{value},"quotaLimits":{value},"attributionAgent":"anchor"}}"#
        ));
        let k = r
            .exposed_keys()
            .unwrap_or_else(|| panic!("the anchor key carries the box: {value}"));
        assert_eq!(k.request_id.is_some(), *want_str, "string_only on {value}");
        assert_eq!(
            k.api_block_index.is_some(),
            *want_u32,
            "u32_only on {value}"
        );
        assert_eq!(
            k.tool_ends_turn.is_some(),
            *want_bool,
            "bool_only on {value}"
        );
        assert_eq!(
            k.quota_limits.is_some(),
            *want_quota,
            "quota_only on {value}"
        );
        // Whatever the odd key did, the record itself parsed and kept its other fields.
        assert_eq!(k.attribution_agent.as_deref(), Some("anchor"), "{value}");
    }

    // The two decoded values, not just their presence: an escape survives, and the number
    // is the number rather than a coincidence of ordering.
    let esc = parse(r#"{"type":"assistant","uuid":"a-31","requestId":"a\nb","apiBlockIndex":7}"#);
    let k = esc.exposed_keys().expect("both keys");
    assert_eq!(k.request_id.as_deref(), Some("a\nb"));
    assert_eq!(k.api_block_index, Some(7));

    // The quota object's OWN members take the same table one level down.
    let q = parse(
        r#"{"type":"assistant","uuid":"a-32","quotaLimits":{"rateLimitType":"weekly","resetsAt":1790000003}}"#,
    )
    .exposed_keys()
    .and_then(|k| k.quota_limits.as_deref().cloned())
    .expect("the object decodes");
    assert_eq!(q.limit_type.as_deref(), Some("weekly"));
    assert_eq!(q.resets_at, Some(1790000003));
}

/// ONE key is enough, for each of the NINETEEN.
///
/// `Exposed` is built only for a record that names at least one key, and the gate is ANY
/// of them rather than all - so the statement is per KEY, and a record naming exactly one
/// is the ordinary shape rather than an edge case. The table walks them one at a time,
/// each alone on its record, which is the only way to see the gate answer for that key.
#[test]
fn any_one_exposed_key_alone_carries_the_box() {
    // (key, a value of the type that key's decoder accepts)
    let keys: &[(&str, &str)] = &[
        ("attributionAgent", r#""general-purpose""#),
        ("attributionSkill", r#""csift""#),
        ("attributionPlugin", r#""omc""#),
        ("attributionMcpServer", r#""srv""#),
        ("attributionMcpTool", r#""srv__tool""#),
        ("requestId", r#""req_1""#),
        ("apiBlockIndex", "3"),
        ("toolEndsTurn", "true"),
        ("userFeedback", r#""use the staging copy""#),
        ("error", r#""rate_limit""#),
        ("apiErrorStatus", "429"),
        ("quotaLimits", r#"{"rateLimitType":"weekly"}"#),
        ("truncatedAfterOutput", "true"),
        ("isAbortedMidStream", "true"),
        ("interruptedMessageId", r#""msg_a1""#),
        ("scheduledTaskId", r#""task-1""#),
        ("scheduledFireId", r#""fire-1""#),
        ("toolUseID", r#""66666666-7777-4888-8999-aaaaaaaaaaaa""#),
        ("turnCompanion", "true"),
    ];
    assert_eq!(
        keys.len(),
        19,
        "the table is the nineteen exposed keys; a new key adds a row here"
    );
    for (key, value) in keys {
        let line = format!(r#"{{"type":"assistant","uuid":"a-40","{key}":{value}}}"#);
        assert!(
            parse(&line).exposed_keys().is_some(),
            "{key} alone carries the box"
        );
    }
    // And a record naming NONE of them allocates nothing, which is the other half of the
    // same gate.
    assert!(
        parse(r#"{"type":"assistant","uuid":"a-41","promptId":"p-1"}"#)
            .exposed_keys()
            .is_none(),
        "a record naming no exposed key carries no box"
    );
}

/// `message.stop_details` - the API's own account of a stop it made. The gate is the
/// OBJECT, and an object saying nothing is the same answer as an absent key.
#[test]
fn stop_details_reads_every_member_and_refuses_an_object_that_says_nothing() {
    let refused = parse(
        r#"{"type":"assistant","uuid":"a-50","message":{"role":"assistant","stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber","explanation":"withheld","fallback_has_prefill_claim":null},"content":[{"type":"thinking","thinking":"weighing it","signature":"c2ln"}]}}"#,
    );
    let s = refused.stop_details().expect("the record names the object");
    assert_eq!(s.r#type.as_deref(), Some("refusal"));
    assert_eq!(s.category.as_deref(), Some("cyber"));
    assert_eq!(s.explanation.as_deref(), Some("withheld"));
    assert_eq!(
        s.fallback_has_prefill_claim, None,
        "a null member is the record not saying, not a false"
    );
    // A turn that ended normally names no such object at all.
    let ended = parse(
        r#"{"type":"assistant","uuid":"a-51","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"done"}]}}"#,
    );
    assert!(ended.stop_details().is_none());
    // An object whose every member is null says only that the key was written, which the
    // absent key already says, so the accessor refuses it.
    let hollow = parse(
        r#"{"type":"assistant","uuid":"a-52","message":{"role":"assistant","stop_reason":"refusal","stop_details":{"type":null,"category":null,"explanation":null,"fallback_has_prefill_claim":null},"content":[{"type":"text","text":"x"}]}}"#,
    );
    assert!(hollow.stop_details().is_none());
}

/// The same tolerance rule the exposed keys follow, applied to this object: a member of
/// an unexpected JSON type reads as absent and costs its own member, never the record;
/// a whole object of an unexpected type reads as absent and costs only the object.
#[test]
fn a_stop_details_of_the_wrong_shape_reads_as_absent_and_never_fails_the_record() {
    let odd = parse(
        r#"{"type":"assistant","uuid":"a-53","message":{"role":"assistant","stop_details":{"type":7,"category":"cyber","explanation":["a","b"],"fallback_has_prefill_claim":"yes"},"content":[{"type":"text","text":"still searchable"}]}}"#,
    );
    let s = odd.stop_details().expect("one member still reads");
    assert_eq!(s.r#type, None, "a number is not the kind this field names");
    assert_eq!(s.category.as_deref(), Some("cyber"));
    assert_eq!(s.explanation, None);
    assert_eq!(
        s.fallback_has_prefill_claim, None,
        "a string is not coerced to a boolean"
    );
    // A whole object of the wrong type is absent, and the record's own text survives.
    let scalar = parse(
        r#"{"type":"assistant","uuid":"a-54","message":{"role":"assistant","stop_details":"refusal","content":[{"type":"text","text":"still searchable"}]}}"#,
    );
    assert!(scalar.stop_details().is_none());
    assert_eq!(
        scalar.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentMessagePlain]
    );
}

/// ONE member is enough, for each of the FOUR. The gate on the stop-details object is ANY
/// member named, not all of them, and a record naming a single one is the ordinary shape
/// rather than an edge case: the API sends three member sets and the smallest of them is
/// three members, so each of the four has to carry the object on its own.
#[test]
fn any_single_stop_details_member_carries_the_object() {
    let members = [
        (r#""type":"refusal""#, "type"),
        (r#""category":"cyber""#, "category"),
        (r#""explanation":"withheld""#, "explanation"),
        (
            r#""fallback_has_prefill_claim":true"#,
            "fallback_has_prefill_claim",
        ),
    ];
    for (member, name) in members {
        let line = format!(
            r#"{{"type":"assistant","uuid":"a-55","message":{{"role":"assistant","stop_details":{{{member}}},"content":[{{"type":"text","text":"x"}}]}}}}"#
        );
        assert!(
            parse(&line).stop_details().is_some(),
            "{name} alone carries the object"
        );
    }
    // And naming NONE of them does not, which is the other half of the same gate.
    let empty = parse(
        r#"{"type":"assistant","uuid":"a-56","message":{"role":"assistant","stop_details":{},"content":[{"type":"text","text":"x"}]}}"#,
    );
    assert!(empty.stop_details().is_none());
}
