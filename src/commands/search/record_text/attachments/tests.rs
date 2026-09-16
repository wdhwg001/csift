//! Unit tests for the attachment family: the body rule and the label-zone markers.
//!
//! Every fixture line is built from the SHAPE the inventory names for its payload type -
//! a hand-patterned uuid, a synthetic path - never a corpus line, and every expectation is
//! derived from the documented rule rather than copied from a run.

use super::*;

/// Parse one raw jsonl line the way a scan would.
fn rec(line: &str) -> Record {
    serde_json::from_str(line).expect("valid record")
}

/// A `ClassifyCtx` that resolves nothing: the honest answer for a caller with no row
/// space, which is what a unit test is.
fn ctx() -> ClassifyCtx<'static> {
    ClassifyCtx::top_level()
}

/// The unit's own leaf, for a section that carries one.
fn section(class: Class) -> Section<'static> {
    Section::record(Some(class))
}

/// Source 1 of the body rule: the converter's own output leads, whatever the payload holds.
#[test]
fn a_persisted_conversion_is_the_body_and_names_itself() {
    let line = r#"{"type":"attachment","uuid":"11111111-2222-4333-8444-555555555501","parentUuid":"11111111-2222-4333-8444-555555555500","attachment":{"type":"total_tokens_reminder","tokens":42},"rendered":[{"content":"<system-reminder>budget line</system-reminder>"}]}"#;
    let (body, source) = attachment_body(&rec(line)).expect("an attachment renders");
    assert_eq!(body, "<system-reminder>budget line</system-reminder>");
    assert_eq!(source, BODY_RENDERED);
}

/// A conversion of several messages joins them, in file order, one per line.
#[test]
fn a_multi_message_conversion_joins_with_newlines() {
    let line = r#"{"type":"attachment","attachment":{"type":"file"},"rendered":[{"content":"first"},{"content":"second"}]}"#;
    let (body, source) = attachment_body(&rec(line)).expect("an attachment renders");
    assert_eq!(body, "first\nsecond");
    assert_eq!(source, BODY_RENDERED);
}

/// Source 2: a payload field, for the two types whose JSON would bury it.
#[test]
fn a_hook_run_without_a_conversion_renders_its_stdout() {
    let line = r#"{"type":"attachment","attachment":{"type":"hook_success","hookEvent":"PreToolUse","hookName":"zzp-gate","exitCode":0,"durationMs":12,"command":"/synthetic/hooks/gate.sh","stdout":"gate ok\n","stderr":""}}"#;
    let (body, source) = attachment_body(&rec(line)).expect("an attachment renders");
    assert_eq!(body, "gate ok");
    assert_eq!(source, BODY_PAYLOAD);
}

/// An EMPTY stdout is not a body: the unit falls through to the payload JSON rather than
/// rendering an empty string, and still names the run in its label zone.
#[test]
fn an_empty_stdout_falls_through_to_the_payload() {
    let line = r#"{"type":"attachment","attachment":{"type":"hook_success","hookEvent":"Stop","hookName":"zzp-gate","exitCode":0,"durationMs":3,"command":"/synthetic/hooks/gate.sh","stdout":"   ","stderr":""}}"#;
    let (body, source) = attachment_body(&rec(line)).expect("an attachment renders");
    assert!(body.starts_with('{'), "the payload JSON, got {body}");
    assert_eq!(source, BODY_PAYLOAD);
}

/// The injected-context body is the joined `content` strings when no conversion exists -
/// the form csift rendered for the leaf before the field did.
#[test]
fn injected_context_without_a_conversion_joins_its_content() {
    let line = r#"{"type":"attachment","attachment":{"type":"hook_additional_context","hookEvent":"SessionStart","hookName":"zzp-seed","toolUseID":"hook-11111111","content":["one","two"]}}"#;
    let (body, source) = attachment_body(&rec(line)).expect("an attachment renders");
    assert_eq!(body, "one\ntwo");
    assert_eq!(source, BODY_PAYLOAD);
}

/// Source 3: every other payload renders its own JSON, verbatim.
#[test]
fn any_other_payload_renders_its_verbatim_json() {
    let line = r#"{"type":"attachment","attachment":{"type":"auto_mode","mode":"acceptEdits"}}"#;
    let (body, source) = attachment_body(&rec(line)).expect("an attachment renders");
    assert_eq!(body, r#"{"type":"auto_mode","mode":"acceptEdits"}"#);
    assert_eq!(source, BODY_PAYLOAD);
}

/// The rule answers for attachment lines only.
#[test]
fn a_non_attachment_record_has_no_attachment_body() {
    let line = r#"{"type":"user","message":{"role":"user","content":"hello"}}"#;
    assert!(attachment_body(&rec(line)).is_none());
}

/// The second conversion is NOT the body: csift cannot read off the record which turn it
/// was replayed into, so it reports the difference and leaves the body alone.
#[test]
fn a_differing_human_turn_conversion_is_reported_not_chosen() {
    let line = r#"{"type":"attachment","uuid":"11111111-2222-4333-8444-555555555511","attachment":{"type":"queued_command","prompt":"go"},"rendered":[{"content":"ordinary turn copy"}],"renderedInHumanTurn":[{"content":"human turn copy"}]}"#;
    let r = rec(line);
    let (body, source) = attachment_body(&r).expect("an attachment renders");
    assert_eq!(body, "ordinary turn copy");
    assert_eq!(source, BODY_RENDERED);
    let (facts, _) = facts(&r, Some(line), &section(Class::ContextTask), &ctx());
    assert!(
        facts
            .iter()
            .any(|f| matches!(f, HitFact::Attachments(Fact::HumanTurnRender))),
        "the differing second copy is reported: {facts:?}"
    );
}

/// An IDENTICAL second copy says nothing: there is no difference for a reader to weigh.
#[test]
fn an_identical_human_turn_conversion_adds_no_fact() {
    let line = r#"{"type":"attachment","attachment":{"type":"queued_command","prompt":"go"},"rendered":[{"content":"same"}],"renderedInHumanTurn":[{"content":"same"}]}"#;
    let (facts, _) = facts(&rec(line), Some(line), &section(Class::ContextTask), &ctx());
    assert!(!facts
        .iter()
        .any(|f| matches!(f, HitFact::Attachments(Fact::HumanTurnRender))));
}

/// The body source rides JSON as well as the label zone, so a machine consumer knows which
/// of the two copies it is reading.
#[test]
fn the_body_source_is_a_fact_not_only_a_marker() {
    let line = r#"{"type":"attachment","attachment":{"type":"auto_mode"}}"#;
    let (facts, _) = facts(&rec(line), Some(line), &section(Class::ContextMode), &ctx());
    let f = facts
        .iter()
        .find(|f| matches!(f, HitFact::Attachments(Fact::Body(_))))
        .expect("a body source");
    assert_eq!(
        f.json(),
        vec![("body_source", serde_json::json!(BODY_PAYLOAD))]
    );
    assert_eq!(f.label_zone().as_deref(), Some("[payload]"));
}

/// The four leaf markers, each naming the field a reader cannot otherwise see.
#[test]
fn each_leaf_names_itself_in_the_label_zone() {
    let cases: [(&str, Class, &str); 5] = [
        (
            r#"{"type":"attachment","attachment":{"type":"hook_additional_context","hookEvent":"SessionStart","hookName":"zzp-seed","content":["x"]}}"#,
            Class::HookContext,
            "[hook SessionStart zzp-seed]",
        ),
        (
            r#"{"type":"attachment","attachment":{"type":"hook_success","hookEvent":"Stop","hookName":"zzp-gate","exitCode":2,"durationMs":41,"stdout":"x"}}"#,
            Class::HookOutcome,
            "[hook Stop zzp-gate exit 2 41ms]",
        ),
        (
            r#"{"type":"attachment","attachment":{"type":"hook_cancelled"}}"#,
            Class::HookOutcome,
            "[hook hook_cancelled]",
        ),
        (
            r#"{"type":"attachment","attachment":{"type":"skill_listing"}}"#,
            Class::ContextListing,
            "[context skill_listing]",
        ),
        (
            r#"{"type":"attachment","attachment":{"type":"structured_output"}}"#,
            Class::AgentMessageStructured,
            "[structured]",
        ),
    ];
    for (line, class, want) in cases {
        let (_, markers) = facts(&rec(line), Some(line), &section(class), &ctx());
        assert_eq!(markers.first().map(Marker::render).as_deref(), Some(want));
    }
}

/// The person's own steer is a `user.message` that arrived mid-turn, and says so.
#[test]
fn the_human_steer_is_marked_mid_turn() {
    let line = r#"{"type":"attachment","attachment":{"type":"queued_command","origin":{"kind":"human"},"prompt":"stop"}}"#;
    let (_, markers) = facts(&rec(line), Some(line), &section(Class::UserMessage), &ctx());
    assert_eq!(
        markers.first().map(Marker::render).as_deref(),
        Some("[mid-turn]")
    );
}

/// A hook run with neither tail prints neither rather than a zero the record never wrote.
#[test]
fn a_hook_run_without_an_exit_code_prints_no_tail() {
    let line = r#"{"type":"attachment","attachment":{"type":"hook_success","hookEvent":"Stop","hookName":"zzp-gate","stdout":"x"}}"#;
    let (_, markers) = facts(&rec(line), Some(line), &section(Class::HookOutcome), &ctx());
    assert_eq!(
        markers.first().map(Marker::render).as_deref(),
        Some("[hook Stop zzp-gate]")
    );
}

/// A hook line with neither field says `unknown` in each empty slot rather than printing a
/// blank where a name belongs.
#[test]
fn a_hook_line_missing_both_names_says_unknown_twice() {
    let line =
        r#"{"type":"attachment","attachment":{"type":"hook_additional_context","content":["x"]}}"#;
    let (_, markers) = facts(&rec(line), Some(line), &section(Class::HookContext), &ctx());
    assert_eq!(
        markers.first().map(Marker::render).as_deref(),
        Some("[hook unknown unknown]")
    );
}

/// A view rendering its OWN text takes no body fact and no leaf marker: the pair would
/// describe a body the unit is not showing.
#[test]
fn the_channel_view_takes_no_body_fact() {
    let line = r#"{"type":"attachment","parentUuid":"11111111-2222-4333-8444-555555555500","attachment":{"type":"hook_additional_context","hookName":"csift","content":["[csift-channel v1 id=0123456789abcdef part=1/1]"]}}"#;
    let (facts, markers) = facts(&rec(line), Some(line), &section(Class::CommChannel), &ctx());
    assert!(markers.is_empty(), "no leaf marker: {markers:?}");
    assert!(!facts
        .iter()
        .any(|f| matches!(f, HitFact::Attachments(Fact::Body(_)))));
}

/// A unit that is not this family's costs one type compare and allocates nothing.
#[test]
fn a_non_attachment_unit_gets_nothing() {
    let line = r#"{"type":"user","message":{"role":"user","content":"hello"}}"#;
    let (facts, markers) = facts(&rec(line), Some(line), &section(Class::UserMessage), &ctx());
    assert!(facts.is_empty() && markers.is_empty());
}
