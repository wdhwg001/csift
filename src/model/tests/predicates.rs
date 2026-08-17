//! The is-genuine-user gate and its exclusions: carriers, meta, interrupts, wrappers, synthetic markers.

use super::*;

#[test]
fn genuine_user_string_content() {
    let r = parse(r#"{"type":"user","message":{"role":"user","content":"hello"}}"#);
    assert!(r.is_genuine_user());
}

#[test]
fn tool_result_carrier_is_not_genuine_user() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok"}]}}"#,
    );
    assert!(!r.is_genuine_user());
}

#[test]
fn compaction_summary_is_not_genuine_user() {
    let r = parse(
        r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"summary..."}}"#,
    );
    assert!(!r.is_genuine_user());
}

#[test]
fn is_meta_user_is_not_genuine_user() {
    // §4.2 TRAP: looks human, is system-injected — must be excluded.
    let r = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off."}}"#,
    );
    assert!(!r.is_genuine_user());
    assert!(r.genuine_user_text().is_none());
}

#[test]
fn is_meta_local_command_caveat_excluded() {
    // The real-data shape: <local-command-caveat> on an isMeta user record.
    let r = parse(
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>Caveat: ...</local-command-caveat>"}}"#,
    );
    assert!(!r.is_genuine_user());
}

// ── Branch-completeness: the negative / fallback arms ──

#[test]
fn is_genuine_user_false_for_non_user_type() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
    );
    assert!(!r.is_genuine_user());
}

#[test]
fn is_genuine_user_false_when_message_absent() {
    // A `type:"user"` record with NO `message` object at all (the `let Some(msg)
    // else` arm).
    let r = parse(r#"{"type":"user","uuid":"x"}"#);
    assert!(!r.is_genuine_user());
}

#[test]
fn is_genuine_user_false_when_role_not_user() {
    // role mismatch inside an otherwise user-typed record.
    let r = parse(r#"{"type":"user","message":{"role":"assistant","content":"hi"}}"#);
    assert!(!r.is_genuine_user());
}

#[test]
fn is_genuine_user_false_when_content_absent() {
    // message present, role user, but NO content (the `None => false` arm).
    let r = parse(r#"{"type":"user","message":{"role":"user"}}"#);
    assert!(!r.is_genuine_user());
    assert!(r.genuine_user_text().is_none());
}

#[test]
fn is_genuine_user_false_for_blocks_without_text() {
    // Block content that has NO text block (only an image) → not genuine.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{}}]}}"#,
    );
    assert!(!r.is_genuine_user());
}

// ── §4.2.1 interrupts: synthesized markers are NOT genuine-user (spurious-boundary
//    removal). Real shape: a text-block user record whose text is EXACTLY the marker
//    (116 + 21 occurrences in the corpus, none isMeta, none carrying extra prose). ──

#[test]
fn interrupt_marker_plain_is_not_genuine_user() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#,
    );
    assert!(
        !r.is_genuine_user(),
        "interrupt marker must not open a turn"
    );
    assert!(!r.opens_turn());
}

#[test]
fn interrupt_marker_for_tool_use_is_not_genuine_user() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user for tool use]"}]}}"#,
    );
    assert!(!r.is_genuine_user());
    assert!(!r.opens_turn());
}

#[test]
fn interrupt_marker_as_string_content_is_not_genuine_user() {
    // The same marker can arrive as bare-string content too — still excluded.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"[Request interrupted by user]"}}"#,
    );
    assert!(!r.is_genuine_user());
}

#[test]
fn text_that_merely_contains_interrupt_phrase_is_still_genuine() {
    // Exact-match only: a real message that QUOTES the phrase must stay genuine
    // (codepoint-safe `==`, never a substring/slice).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"why did i see [Request interrupted by user] earlier?"}}"#,
    );
    assert!(
        r.is_genuine_user(),
        "a message merely containing the phrase is still a human turn"
    );
}

// ── §4.2.2 / §4.2.3: <local-command-stdout> output + <command-name> slash wrapper
//    are machine-templated, NOT genuine-user (string content, non-isMeta). ──

#[test]
fn local_command_stdout_is_not_genuine_user() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<local-command-stdout>✓ Updated oh-my-claudecode.</local-command-stdout>"}}"#,
    );
    assert!(!r.is_genuine_user());
    assert!(!r.opens_turn());
}

#[test]
fn command_name_wrapper_is_not_genuine_user() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args></command-args>"}}"#,
    );
    assert!(
        !r.is_genuine_user(),
        "slash-command wrapper must not open a turn"
    );
    assert!(!r.opens_turn());
    // Empty args → no recoverable prose.
    assert!(r.slash_command_args().is_none());
}

#[test]
fn command_name_wrapper_with_args_recovers_prose() {
    // Real shape: `/compact Just shipped spec-batch-14 …` — the typed prose lives in
    // <command-args>; it is recovered (and is the reconstructed user text), but the
    // wrapper itself is still not a standalone genuine-user record.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-message>compact</command-message>\n<command-args> Just shipped spec-batch-14, summarize</command-args>"}}"#,
    );
    assert!(!r.is_genuine_user());
    assert_eq!(
        r.slash_command_args().as_deref(),
        Some("Just shipped spec-batch-14, summarize")
    );
    // v0.5: rendered as `/name args` — the prose keeps its command context, and the
    // wrapper XML never masquerades as the body.
    assert_eq!(
        r.reconstructed_user_text(None).as_deref(),
        Some("/compact Just shipped spec-batch-14, summarize")
    );
}

#[test]
fn command_message_first_wrapper_detected_and_recovered() {
    // The NEWER CC tag order (`<command-message>` FIRST — both orders coexist in real
    // corpora). Detection anchored on `<command-name>` alone used to misclassify this
    // as GENUINE user prose (raw XML as `user.message`, and it opened a turn).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<command-message>csift</command-message>\n<command-name>/csift</command-name>\n<command-args>what changed in v5?</command-args>"}}"#,
    );
    assert!(!r.is_genuine_user(), "wrapper is never the human");
    assert_eq!(r.slash_command_name().as_deref(), Some("/csift"));
    assert_eq!(
        r.slash_command_args().as_deref(),
        Some("what changed in v5?")
    );
    assert_eq!(
        r.reconstructed_user_text(None).as_deref(),
        Some("/csift what changed in v5?")
    );
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserMessage, Class::CommandInvocation]
    );
    // A no-args NEW-order wrapper is pure machinery: never genuine, never user.message.
    let bare = parse(
        r#"{"type":"user","message":{"role":"user","content":"<command-message>compact</command-message>\n<command-name>/compact</command-name>\n<command-args></command-args>"}}"#,
    );
    assert!(!bare.is_genuine_user());
    assert_eq!(
        bare.classify(&ClassifyCtx::top_level()),
        vec![Class::CommandInvocation]
    );
}

#[test]
fn command_name_wrapper_with_multibyte_args_is_codepoint_safe() {
    // A multi-byte args body must be recovered whole (codepoint-safe slice on the ASCII
    // tags only) — the live panic class.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":"<command-name>/compact</command-name>\n<command-args>🤖 just shipped the batch, summarize 🎉</command-args>"}}"#,
    );
    assert_eq!(
        r.slash_command_args().as_deref(),
        Some("🤖 just shipped the batch, summarize 🎉")
    );
}

#[test]
fn opens_turn_matches_genuine_user() {
    // A plain genuine user opens a turn; a plain tool_result carrier does not.
    let genuine = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
    assert!(genuine.opens_turn());
    let carrier = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok"}]}}"#,
    );
    assert!(!carrier.opens_turn());
}

#[test]
fn is_synthetic_user_marker_matches_each_form() {
    assert!(is_synthetic_user_marker("[Request interrupted by user]"));
    assert!(is_synthetic_user_marker(
        "[Request interrupted by user for tool use]"
    ));
    assert!(is_synthetic_user_marker("<local-command-stdout>anything"));
    assert!(is_synthetic_user_marker("<command-name>/x</command-name>"));
    assert!(!is_synthetic_user_marker("a normal human message"));
    // Exact-match for interrupts: a longer string merely starting with the marker is
    // NOT excluded as an interrupt.
    assert!(!is_synthetic_user_marker(
        "[Request interrupted by user] and then I said more"
    ));
}
