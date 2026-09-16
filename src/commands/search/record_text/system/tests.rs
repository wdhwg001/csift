//! The system family's FACTS: which keys each event kind exposes, and what a record that
//! is not this family's carries (nothing).

use super::*;

fn rec(line: &str) -> Record {
    serde_json::from_str(line).expect("fixture parses")
}

/// The family's facts as (key, compact JSON) pairs, in emission order.
fn keys_of(rec: &Record, raw: Option<&str>) -> Vec<(&'static str, String)> {
    let ctx = ClassifyCtx::top_level();
    let (facts, markers) = facts(rec, raw, &Section::record(None), &ctx);
    assert!(markers.is_empty(), "this family renders, it does not mark");
    facts
        .iter()
        .flat_map(|f| {
            f.json()
                .into_iter()
                .map(|(k, v)| (k, v.to_string()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The exposed keys are the LINE's, and `retried` is the subtype's. The error fact
/// carries the two descriptive fields only, and the edge rides beside them.
#[test]
fn refusal_and_api_error_facts_expose_their_keys() {
    let line = r#"{"type":"system","subtype":"model_refusal_fallback","uuid":"r1","originalModel":"model-a","fallbackModel":"model-b","apiRefusalCategory":"cyber","refusedUserMessageUuid":"00000000-1111-4222-8333-444444444444","retractedMessageUuids":["wire-1"]}"#;
    let seen = keys_of(&rec(line), Some(line));
    assert_eq!(
        seen[..4].to_vec(),
        vec![
            ("retried", "true".to_string()),
            ("original_model", "\"model-a\"".to_string()),
            ("fallback_model", "\"model-b\"".to_string()),
            ("category", "\"cyber\"".to_string()),
        ]
    );
    assert_eq!(seen[4].0, "for", "the refusal edge rides beside the keys");
    assert!(
        !seen.iter().any(|(_, v)| v.contains("wire-1")),
        "retractedMessageUuids is documented, never exposed: it resolves to no line"
    );

    let no_fb = r#"{"type":"system","subtype":"model_refusal_no_fallback","uuid":"r2","originalModel":"model-a"}"#;
    let seen = keys_of(&rec(no_fb), Some(no_fb));
    assert_eq!(seen[0], ("retried", "false".to_string()));
    assert!(
        !seen.iter().any(|(k, _)| *k == "fallback_model"),
        "the key the line does not carry is absent, not null"
    );

    let err = r#"{"type":"system","subtype":"api_error","uuid":"e1","error":{"type":"overloaded_error","message":"Overloaded","status":529,"headers":{"set-cookie":"sess=zz"},"requestID":"req-zz"},"retryAttempt":3,"maxRetries":20,"retryInMs":1522.5}"#;
    assert_eq!(
        keys_of(&rec(err), Some(err)),
        vec![
            ("status", "529".to_string()),
            ("retry_attempt", "3".to_string()),
            ("max_retries", "20".to_string()),
            ("retry_in_ms", "1522.5".to_string()),
            (
                "error",
                r#"{"message":"Overloaded","type":"overloaded_error"}"#.to_string()
            ),
        ]
    );
}

/// The fire record's own keys: the scheduler's task id, the cron expression, and the
/// armed prompt CUT to the preview budget. `scheduled_at` is NOT here - the instant is an
/// engine field with one writer, and a family fact beside it would be a second.
#[test]
fn schedule_event_facts_expose_the_cron_arm_with_a_cut_prompt() {
    let long = "z".repeat(260);
    let cron = format!(
        r#"{{"type":"system","subtype":"scheduled_task_fire","uuid":"s1","content":"Running scheduled task (Jun 7 5:28am)","taskId":"task-zz","cron":"0 5 * * *","prompt":"{long}"}}"#
    );
    let seen = keys_of(&rec(&cron), Some(&cron));
    let names: Vec<&str> = seen.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        names,
        vec!["task_id", "cron", "prompt", "for"],
        "the event exposes its task id, its schedule, its cut prompt and its edge"
    );
    let prompt: String = serde_json::from_str(&seen[2].1).expect("a JSON string");
    // The budget is the LITERAL 200 on this side and `PROMPT_MAX` on the other, so a
    // change to the constant fails here rather than agreeing with itself.
    assert_eq!(
        prompt,
        crate::text::truncate_excerpt(&long, 200),
        "cut at the documented 200 characters, with the dropped count stated"
    );
    assert!(
        prompt.starts_with(&"z".repeat(200)) && prompt.contains("+60 chars"),
        "and that cut really is 200 characters plus the elision marker: {prompt}"
    );

    let wakeup = r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"s2","content":"Claude resuming /loop wakeup (Jun 7 6:40am)"}"#;
    let names: Vec<&str> = keys_of(&rec(wakeup), Some(wakeup))
        .iter()
        .map(|(k, _)| *k)
        .collect();
    assert_eq!(names, vec!["for"], "the wakeup arm carries no key at all");
}

/// No source line, no keys - and no fabricated nulls either. The `retried` fact is the
/// exception by construction: it reads the subtype, which IS on the record.
#[test]
fn facts_without_a_source_line_carry_only_what_the_record_holds() {
    let line = r#"{"type":"system","subtype":"model_refusal_fallback","uuid":"r1","originalModel":"model-a"}"#;
    let names: Vec<&str> = keys_of(&rec(line), None).iter().map(|(k, _)| *k).collect();
    assert_eq!(names, vec!["retried", "for"]);
    let err = r#"{"type":"system","subtype":"api_error","uuid":"e1","retryAttempt":3}"#;
    assert!(keys_of(&rec(err), None).is_empty());
    assert!(keys_of(&rec(r#"{"type":"user","uuid":"u1"}"#), None).is_empty());
    // A line the caller HOLDS but cannot read as an object carries no keys either.
    assert!(keys_of(&rec(err), Some("not json at all")).is_empty());
}

/// A system subtype this family models no leaf for carries no facts: the unmodeled unit
/// names the shape, and inventing keys for it would be the opposite.
#[test]
fn an_unmodeled_system_subtype_carries_no_family_facts() {
    let line = r#"{"type":"system","subtype":"some_new_subtype","uuid":"n1","status":7}"#;
    assert!(keys_of(&rec(line), Some(line)).is_empty());
    // And a subtype whose leaf belongs to another category is another family's record.
    let boundary = r#"{"type":"system","subtype":"compact_boundary","uuid":"b1"}"#;
    assert!(keys_of(&rec(boundary), Some(boundary)).is_empty());
}

/// The two field readers report what the line says and never re-type it: an empty or
/// non-string value is absent, and a numeric slot holding a string prints that string
/// rather than a JSON fragment.
#[test]
fn the_field_readers_report_the_lines_own_shape() {
    let m = match serde_json::from_str(r#"{"a":"v","b":"","c":7,"d":"9"}"#) {
        Ok(serde_json::Value::Object(m)) => m,
        _ => panic!("fixture parses"),
    };
    assert_eq!(str_field(&m, "a").as_deref(), Some("v"));
    assert_eq!(str_field(&m, "b"), None, "an empty string is absent");
    assert_eq!(str_field(&m, "c"), None, "a number is not a string field");
    assert_eq!(str_field(&m, "missing"), None);
    assert_eq!(num_text(&serde_json::json!(7)), "7");
    assert_eq!(num_text(&serde_json::json!(1522.5)), "1522.5");
    assert_eq!(num_text(&serde_json::json!("529")), "529");
    assert_eq!(num_text(&serde_json::json!(null)), "null");
}

/// No family fact renders a label-zone tag: every value is already in the head or body,
/// and a tag would print the same field twice on one line.
#[test]
fn the_familys_facts_add_no_label_zone_tag() {
    for f in [
        Fact::Refusal {
            retried: true,
            original_model: Some("model-a".to_string()),
            fallback_model: None,
            category: None,
        },
        Fact::ApiError {
            status: Some(serde_json::json!(529)),
            retry_attempt: None,
            max_retries: None,
            retry_in_ms: None,
            error: None,
        },
        Fact::ScheduleArm {
            task_id: Some("task-zz".to_string()),
            cron: None,
            prompt: None,
        },
    ] {
        assert_eq!(f.label_zone(), None, "{f:?}");
    }
}
