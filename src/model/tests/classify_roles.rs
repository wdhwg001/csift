//! classify(): user and agent role labels, multi-label carriers, dedup.

use super::*;

// -- classify(): single-label record shapes --

#[test]
fn classify_genuine_user_message() {
    let r = parse(r#"{"type":"user","message":{"role":"user","content":"please fix it"}}"#);
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage]
    );
}

#[test]
fn classify_assistant_text_is_message() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Done."}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentMessagePlain]
    );
}

#[test]
fn classify_assistant_thinking_only() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm","signature":"s"}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentThinkingSummary]
    );
}

#[test]
fn classify_assistant_redacted_thinking_only() {
    // A `redacted_thinking` block (opaque encrypted reasoning, no readable text) takes its
    // OWN leaf: the model echoes the ciphertext back but reads nothing from it. UNATTESTED
    // in the corpus -> exercised by this SYNTHETIC fixture built from the documented shape.
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"redacted_thinking","data":"EncryptedOpaqueBlob=="}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentThinkingRedacted]
    );
}

#[test]
fn classify_assistant_thinking_then_tool_use_multilabel() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"plan"},{"type":"tool_use","id":"t","name":"Bash","input":{"command":"ls"}}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentThinkingSummary, Class::AgentToolUse]
    );
}

#[test]
fn classify_assistant_text_and_tool_use_multilabel() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"running it"},{"type":"tool_use","id":"t","name":"Read","input":{"file_path":"/x"}}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentMessagePlain, Class::AgentToolUse]
    );
}

#[test]
fn classify_assistant_bare_string_message() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":"surprise bare string"}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentMessagePlain]
    );
}

// -- classify(): dual-label user carriers --

#[test]
fn classify_auq_answer_is_user_answer_plus_tool_result() {
    let r = parse(
        r#"{"type":"user","toolUseResult":{"questions":[{"question":"which?","options":[{"label":"A"}]}],"answers":{"which?":"go with A"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"go with A\"."}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserAnswer, Class::AgentToolResult]
    );
}

#[test]
fn classify_plan_rejection_is_user_rejection_plus_tool_result() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"p","is_error":true,"content":"The user doesn't want to proceed with this tool use. To tell you how to proceed, the user said:\nadd tests first"}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserRejection, Class::AgentToolResult]
    );
}

#[test]
fn classify_plain_tool_result_carrier_is_tool_result_only() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok, ran fine"}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentToolResult]
    );
}

#[test]
fn classify_dedups_repeated_labels() {
    // Two SendMessage blocks in one record -> each label appears ONCE.
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"a","name":"SendMessage","input":{"to":"x","message":"1"}},{"type":"tool_use","id":"b","name":"SendMessage","input":{"to":"y","message":"2"}}]}}"#,
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentToolUse, Class::CommSent]
    );
}
