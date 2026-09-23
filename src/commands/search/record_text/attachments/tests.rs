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

// -- the three RECORD facts: two `for` edges and the producer --

/// The edge of one kind, or a panic naming what was there instead.
fn edge_of<'a>(facts: &'a [HitFact], kind: &str) -> &'a HitFact {
    facts
        .iter()
        .find(|f| match f {
            HitFact::For { kind: k, .. } | HitFact::ForUnknown { kind: k, .. } => {
                k.as_str() == kind
            }
            _ => false,
        })
        .unwrap_or_else(|| panic!("no {kind} edge among {facts:?}"))
}

/// The `for` array a hit's JSON carries, folded by the one merge rule so several edges of
/// the same kind accumulate rather than overwrite.
fn for_json(facts: &[HitFact]) -> serde_json::Value {
    let mut obj = serde_json::json!({});
    crate::commands::search::attach_facts(&mut obj, facts);
    obj["for"].clone()
}

/// One edge object out of that array, by kind.
fn edge_row(facts: &[HitFact], kind: &str) -> serde_json::Value {
    for_json(facts)
        .as_array()
        .expect("an array of edges")
        .iter()
        .find(|e| e["kind"] == kind)
        .unwrap_or_else(|| panic!("no {kind} row"))
        .clone()
}

/// A resolver that answers for ONE uuid, so the resolved and the unresolved arms are both
/// reachable from a unit test.
struct OneLine;

impl crate::model::LineOfUuid for OneLine {
    fn line_of_uuid(&self, uuid: &str) -> Option<usize> {
        (uuid == "11111111-2222-4333-8444-555555555500").then_some(7)
    }
}

/// Every attachment unit carries all three record facts, so a reader never has to guess
/// whether csift looked.
#[test]
fn all_three_record_facts_ride_every_attachment_unit() {
    let line = r#"{"type":"attachment","attachment":{"type":"auto_mode"}}"#;
    let (facts, _) = facts(&rec(line), Some(line), &section(Class::ContextMode), &ctx());
    for kind in ["attached_to", "for_tool_use"] {
        assert!(
            matches!(edge_of(&facts, kind), HitFact::ForUnknown { .. }),
            "{kind} is the honest empty on a line carrying none of its keys"
        );
    }
    assert!(facts
        .iter()
        .any(|f| matches!(f, HitFact::Attachments(Fact::ProducedBy { .. }))));
}

/// The honest empty names its reason in the label zone AND in JSON, never an absent key.
#[test]
fn an_absent_key_renders_its_reason() {
    let line = r#"{"type":"attachment","attachment":{"type":"auto_mode"}}"#;
    let (facts, _) = facts(&rec(line), Some(line), &section(Class::ContextMode), &ctx());
    assert_eq!(
        edge_of(&facts, "attached_to").label_zone().as_deref(),
        Some("[for: attached_to unknown (the record carries no such key)]")
    );
    let parent = edge_row(&facts, "attached_to");
    assert_eq!(parent["target"], serde_json::Value::Null);
    assert_eq!(parent["basis"], serde_json::Value::Null);
    assert!(
        parent["reason"].is_string(),
        "the reason rides JSON: {parent}"
    );
}

/// A parent this transcript carries renders as an ADDRESS - the line, not the id - and the
/// JSON carries the whole uuid beside it.
#[test]
fn a_resolved_parent_carries_its_line() {
    let line = r#"{"type":"attachment","parentUuid":"11111111-2222-4333-8444-555555555500","attachment":{"type":"auto_mode"}}"#;
    let lines = OneLine;
    let mut ctx = ClassifyCtx::top_level();
    ctx.lines = Some(&lines);
    let (facts, _) = facts(&rec(line), Some(line), &section(Class::ContextMode), &ctx);
    assert_eq!(
        edge_of(&facts, "attached_to").label_zone().as_deref(),
        Some("[for: attached_to L7]")
    );
    let parent = edge_row(&facts, "attached_to");
    assert_eq!(parent["basis"], "key");
    assert_eq!(parent["target"]["line"], 7);
    assert_eq!(
        parent["target"]["uuid"],
        "11111111-2222-4333-8444-555555555500"
    );
}

/// The inner tool-call id answers `for_tool_use` and keeps its id: it is a WIRE id, so no
/// line of this transcript carries it and the edge says so rather than inventing one.
#[test]
fn an_inner_tool_use_id_keeps_its_id_with_no_line() {
    let line = r#"{"type":"attachment","attachment":{"type":"read_truncation_notice","toolUseID":"toolu_zzq1"}}"#;
    let lines = OneLine;
    let mut ctx = ClassifyCtx::top_level();
    ctx.lines = Some(&lines);
    let (facts, _) = facts(&rec(line), Some(line), &section(Class::ContextFile), &ctx);
    let tool = edge_row(&facts, "for_tool_use");
    assert_eq!(tool["target"]["uuid"], "toolu_zzq1");
    assert_eq!(tool["target"]["line"], serde_json::Value::Null);
    assert_eq!(tool["target"]["note"], "not in this file");
}

/// The id is read from the PAYLOAD and never from the top level, which is where no line
/// carries it.
#[test]
fn a_top_level_tool_use_id_is_not_the_payloads() {
    let line =
        r#"{"type":"attachment","toolUseID":"toolu_zztop","attachment":{"type":"auto_mode"}}"#;
    let (facts, _) = facts(&rec(line), Some(line), &section(Class::ContextMode), &ctx());
    assert!(
        matches!(edge_of(&facts, "for_tool_use"), HitFact::ForUnknown { .. }),
        "the payload carries no id, so the edge is the honest empty"
    );
}

/// A hook run's producer is the command it executed, with the hook's name beside it.
#[test]
fn a_hook_run_names_its_command_and_its_hook() {
    let line = r#"{"type":"attachment","attachment":{"type":"hook_success","hookName":"zzp-gate","command":"/synthetic/hooks/gate.sh","stdout":"x"}}"#;
    let (facts, _) = facts(&rec(line), Some(line), &section(Class::HookOutcome), &ctx());
    let f = facts
        .iter()
        .find(|f| matches!(f, HitFact::Attachments(Fact::ProducedBy { .. })))
        .expect("a producer fact");
    assert_eq!(
        f.label_zone().as_deref(),
        Some("[produced_by /synthetic/hooks/gate.sh]"),
        "a command is rendered whole - cutting it to an id handle would make it unusable"
    );
    assert_eq!(
        f.json(),
        vec![(
            "produced_by",
            serde_json::json!({
                "command": "/synthetic/hooks/gate.sh",
                "hook_name": "zzp-gate",
                "payload_type": "hook_success",
            })
        )]
    );
}

/// Injected context records the hook's name and no command, so the fact is the honest
/// empty that still names what the line carries.
#[test]
fn injected_context_has_no_command_and_says_so() {
    let line = r#"{"type":"attachment","attachment":{"type":"hook_additional_context","hookName":"zzp-seed","content":["x"]}}"#;
    let (facts, _) = facts(&rec(line), Some(line), &section(Class::HookContext), &ctx());
    let f = facts
        .iter()
        .find(|f| matches!(f, HitFact::Attachments(Fact::ProducedBy { .. })))
        .expect("a producer fact");
    assert_eq!(
        f.label_zone().as_deref(),
        Some("[produced_by unknown (no command on this line; hookName=zzp-seed)]")
    );
}

/// A payload type that is no hook's output says THAT, and names the type, so the reason is
/// checkable against the line.
#[test]
fn a_payload_with_no_producer_key_names_its_type() {
    let line = r#"{"type":"attachment","attachment":{"type":"skill_listing"}}"#;
    let (facts, _) = facts(
        &rec(line),
        Some(line),
        &section(Class::ContextListing),
        &ctx(),
    );
    let f = facts
        .iter()
        .find(|f| matches!(f, HitFact::Attachments(Fact::ProducedBy { .. })))
        .expect("a producer fact");
    assert_eq!(
        f.label_zone().as_deref(),
        Some("[produced_by unknown (no producer key; type=skill_listing)]")
    );
}

/// An addressed attachment csift models no leaf for gets the SAME record facts a labelled
/// one does: the keys are on the line either way, and two answers for one line would be the
/// defect the 1:1 law exists to close.
#[test]
fn an_unmodeled_attachment_still_carries_its_record_facts() {
    let line = r#"{"type":"attachment","parentUuid":"11111111-2222-4333-8444-555555555500","attachment":{"type":"some_future_type","toolUseID":"toolu_zzq1"}}"#;
    let (facts, markers) = facts(&rec(line), Some(line), &Section::record(None), &ctx());
    assert!(markers.is_empty(), "no leaf to name: {markers:?}");
    assert_eq!(
        edge_row(&facts, "for_tool_use")["target"]["uuid"],
        "toolu_zzq1"
    );
    assert!(facts
        .iter()
        .any(|f| matches!(f, HitFact::Attachments(Fact::Body(_)))));
}

/// A conversion message whose `content` is a BLOCK ARRAY joins its text blocks - the
/// second of the two shapes the writer persists.
#[test]
fn a_block_array_conversion_joins_its_text_blocks() {
    let line = r#"{"type":"attachment","attachment":{"type":"file"},"rendered":[{"content":[{"type":"text","text":"first"},{"type":"image"},{"type":"text","text":"second"}]}]}"#;
    let (body, source) = attachment_body(&rec(line)).expect("an attachment renders");
    assert_eq!(body, "first\nsecond", "the image block contributes no text");
    assert_eq!(source, BODY_RENDERED);
}

/// A conversion carrying NO text at all is no body: the unit falls through to the payload
/// rather than rendering an empty string.
#[test]
fn a_textless_conversion_falls_through_to_the_payload() {
    for line in [
        r#"{"type":"attachment","attachment":{"type":"file","note":"x"},"rendered":[{"content":42}]}"#,
        r#"{"type":"attachment","attachment":{"type":"file","note":"x"},"rendered":[{"content":[{"type":"image"}]}]}"#,
    ] {
        let (body, source) = attachment_body(&rec(line)).expect("an attachment renders");
        assert!(body.starts_with('{'), "the payload JSON, got {body}");
        assert_eq!(source, BODY_PAYLOAD);
    }
}

/// A leaf this family does not own takes no marker, even on an attachment line: one line
/// can carry a view whose name is another family's to give.
#[test]
fn a_leaf_outside_the_family_takes_no_marker() {
    let line = r#"{"type":"attachment","attachment":{"type":"hook_success","hookName":"zzp-gate","stdout":"x"}}"#;
    let (_, markers) = facts(
        &rec(line),
        Some(line),
        &section(Class::HookFeedback),
        &ctx(),
    );
    assert!(
        markers.is_empty(),
        "no marker for a foreign leaf: {markers:?}"
    );
}

/// The person's mid-turn steer takes the family's ordinary body rule, like every other
/// view of an attachment line: the conversion the harness persisted, which is what the
/// model received.
///
/// It is NOT an exception, and the emission path is where one would come from: resolving a
/// `user.message` unit as the reconstructed opener text, then the verbatim payload, and
/// only then the seam would show the payload JSON here while an address on the same line
/// showed the conversion, and the fact would have to say `[payload]` about a body it is not
/// describing. `record_text_emission` falls through to the seam, so there is one body and
/// one word for it.
#[test]
fn the_mid_turn_steer_reports_the_conversion_the_model_received() {
    let line = r#"{"type":"attachment","attachment":{"type":"queued_command","origin":{"kind":"human"},"prompt":"stop"},"rendered":[{"content":"the converted copy"}]}"#;
    let r = rec(line);
    assert_eq!(
        attachment_body(&r).map(|(_, s)| s),
        Some(BODY_RENDERED),
        "the record does carry a conversion"
    );
    let (facts, _) = facts(&r, Some(line), &section(Class::UserMessage), &ctx());
    let f = facts
        .iter()
        .find(|f| matches!(f, HitFact::Attachments(Fact::Body(_))))
        .expect("a body source");
    assert_eq!(f.label_zone().as_deref(), Some("[rendered]"));
}

/// A differing second conversion is reported wherever the body IS the ordinary one, and
/// the steer's body is that now - so the steer reports it too. The pairing rule is
/// unchanged and still reads the body source rather than the leaf: `[human-turn render
/// differs]` describes a difference from the text the unit shows.
#[test]
fn the_steer_reports_a_differing_second_conversion_like_any_other_view() {
    let line = r#"{"type":"attachment","attachment":{"type":"queued_command","origin":{"kind":"human"},"prompt":"stop"},"rendered":[{"content":"ordinary"}],"renderedInHumanTurn":[{"content":"human turn"}]}"#;
    let (both, _) = facts(&rec(line), Some(line), &section(Class::UserMessage), &ctx());
    assert!(
        both.iter()
            .any(|f| matches!(f, HitFact::Attachments(Fact::HumanTurnRender))),
        "the body is the ordinary conversion, so the second copy is a real difference: {both:?}"
    );
    // And a record whose body is the PAYLOAD still reports none: the rule keys on the
    // body source, not on the leaf, so nothing about it moved.
    let payload_only = r#"{"type":"attachment","attachment":{"type":"queued_command","origin":{"kind":"human"},"prompt":"stop"},"renderedInHumanTurn":[{"content":"human turn"}]}"#;
    let (payload_facts, _) = facts(
        &rec(payload_only),
        Some(payload_only),
        &section(Class::UserMessage),
        &ctx(),
    );
    assert!(
        !payload_facts
            .iter()
            .any(|f| matches!(f, HitFact::Attachments(Fact::HumanTurnRender))),
        "no difference is reported beside a payload body: {payload_facts:?}"
    );
}
