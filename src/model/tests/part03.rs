use super::*;

#[test]
fn superseded_drafts_distinct_parents_not_merged() {
    // Two identical-content user records with DIFFERENT parents are two real turns —
    // distinct turns legitimately share content but never a parentUuid (each is parented
    // to the assistant message that preceded it).
    let records: Vec<Record> = [
            r#"{"type":"user","uuid":"u0","parentUuid":"a0","message":{"role":"user","content":"continue"}}"#,
            r#"{"type":"assistant","uuid":"x","parentUuid":"u0","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"x","message":{"role":"user","content":"continue"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
    assert!(superseded_draft_indices(&records, |r| r).is_empty());
    assert_eq!(
        group_turn_indices_deduped(&records, |r| r),
        vec![vec![0, 1], vec![2]]
    );
}

#[test]
fn superseded_drafts_null_parent_never_grouped() {
    // No parentUuid → never grouped (grouping on "no parent" would merge unrelated
    // first-message records). In real data a genuine user always carries a parent.
    let records: Vec<Record> = [
        r#"{"type":"user","uuid":"u0","message":{"role":"user","content":"a"}}"#,
        r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"b"}}"#,
    ]
    .iter()
    .map(|l| parse(l))
    .collect();
    assert!(superseded_draft_indices(&records, |r| r).is_empty());
    assert_eq!(
        group_turn_indices_deduped(&records, |r| r),
        vec![vec![0], vec![1]]
    );
}

#[test]
fn deduped_grouping_matches_plain_when_no_drafts() {
    // With no same-parent draft siblings, deduped grouping is identical to the plain
    // delimiter — a regression guard on the shared core.
    let records: Vec<Record> = [
            r#"{"type":"user","uuid":"u0","parentUuid":"r","message":{"role":"user","content":"q1"}}"#,
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","message":{"role":"assistant","content":[{"type":"text","text":"r1"}]}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","message":{"role":"user","content":"q2"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
    let plain = group_turn_indices(&records, |r| r.opens_turn());
    let deduped = group_turn_indices_deduped(&records, |r| r);
    assert_eq!(plain, deduped);
    assert_eq!(deduped, vec![vec![0, 1], vec![2]]);
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

// ── §4.4 AskUserQuestion answer = a genuine-user TURN BOUNDARY (the sanctioned
//    behavior change). Real shape verified on a captured sample: a user tool_result
//    carrier with toolUseResult.answers + the synthesized marker, is_error absent. ──

#[test]
fn auq_answer_with_structured_answers_is_a_boundary() {
    let r = parse(
        r#"{"type":"user","toolUseResult":{"questions":[{"question":"which?","header":"FIX","options":[{"label":"opt A"},{"label":"opt B"}]}],"answers":{"which?":"go with opt A and also fix the prod gap"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"go with opt A and also fix the prod gap\". You can now continue."}]}}"#,
    );
    // It is still NOT a "genuine user" (it rides on a carrier) but IS a boundary.
    assert!(!r.is_genuine_user());
    assert!(r.is_auq_answer_boundary());
    assert!(r.opens_turn());
    // The reconstructed exchange carries Q + options + the answer prose.
    let unit = r.auq_exchange().expect("auq exchange");
    assert!(unit.contains("AskUserQuestion · 1 question"));
    assert!(unit.contains("which?"));
    assert!(unit.contains("FIX"));
    // Options render one-per-line as `- <label>` (description appended when present).
    assert!(unit.contains("- opt A"), "option A rendered: {unit}");
    assert!(unit.contains("- opt B"), "option B rendered: {unit}");
    assert!(unit.contains("go with opt A and also fix the prod gap"));
    // reconstructed_user_text routes to the same unit.
    assert!(r
        .reconstructed_user_text(None)
        .unwrap()
        .contains("go with opt A"));
}

#[test]
fn auq_answer_multibyte_is_codepoint_safe_boundary() {
    // A multi-byte answer prose — must reconstruct whole, no mid-codepoint slice.
    let r = parse(
        r#"{"type":"user","toolUseResult":{"questions":[{"question":"which option for step two? 🤖","header":"STEP TWO","options":[{"label":"option A (recommended)"}]}],"answers":{"which option for step two? 🤖":"🤖 option A is fine, the scope is broader than stated"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"Your questions have been answered: \"which option for step two? 🤖\"=\"🤖 option A is fine\"."}]}}"#,
    );
    assert!(r.is_auq_answer_boundary());
    let unit = r.auq_exchange().expect("multibyte auq exchange");
    assert!(unit.contains("🤖 option A is fine, the scope is broader than stated"));
    assert!(unit.contains("which option for step two? 🤖"));
    assert!(unit.contains("option A (recommended)"));
}

#[test]
fn auq_exchange_surfaces_each_option_description() {
    // Real-captured shape: every option carries a `description` (supplementary note)
    // alongside its `label`. BOTH must survive into the reconstructed unit — the
    // description was previously dropped (only labels rendered).
    let r = parse(
        r#"{"type":"user","toolUseResult":{"questions":[{"header":"EXIF tool","multiSelect":false,"options":[{"description":"standard route, ~10MB download, one-liner","label":"brew install exiftool (Recommended)"},{"description":"pure python, pip install piexif","label":"pip install piexif"}],"question":"which tool re-attaches EXIF?"}],"answers":{"which tool re-attaches EXIF?":"brew install exiftool (Recommended)"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which tool re-attaches EXIF?\"=\"brew install exiftool (Recommended)\". You can now continue."}]}}"#,
    );
    let unit = r.auq_exchange().expect("auq exchange");
    // Both labels AND both descriptions present, verbatim.
    assert!(unit.contains("brew install exiftool (Recommended)"));
    assert!(
        unit.contains("standard route, ~10MB download, one-liner"),
        "option description must survive: {unit}"
    );
    assert!(unit.contains("pip install piexif"));
    assert!(
        unit.contains("pure python, pip install piexif"),
        "second option description must survive: {unit}"
    );
}

#[test]
fn auq_exchange_surfaces_notes_when_answer_is_notes_only() {
    // Real-captured shape: the user answered by typing prose into the
    // notes field; the answer value is the literal "(notes only)" placeholder and the
    // ACTUAL message lives in `annotations[question].notes`. It must be surfaced —
    // previously the whole user message was silently dropped.
    let r = parse(
        r#"{"type":"user","toolUseResult":{"questions":[{"header":"Routing","multiSelect":false,"options":[{"description":"the inbound path","label":"Route A"},{"description":"the outbound path","label":"Route B"}],"question":"which route for the queue?"}],"answers":{"which route for the queue?":"(notes only)"},"annotations":{"which route for the queue?":{"notes":"never conflate the two — Route A is inbound only, Route B is outbound only"}}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which route for the queue?\"=\"(notes only)\". You can now continue."}]}}"#,
    );
    let unit = r.auq_exchange().expect("auq exchange");
    // The placeholder answer is shown, but the real message (the notes) is what the
    // user actually said — it MUST be present and searchable.
    assert!(
        unit.contains("never conflate the two"),
        "notes (the user's real message) must surface: {unit}"
    );
    assert!(
        unit.contains("Route B is outbound only"),
        "full notes verbatim: {unit}"
    );
    // Options + descriptions still present alongside.
    assert!(unit.contains("Route A"));
    assert!(unit.contains("the outbound path"));
}

#[test]
fn auq_answer_marker_only_fallback_is_a_boundary() {
    // Older-shape carrier: the synthesized marker is present but there is no
    // toolUseResult.answers → the marker fallback still classifies it a boundary.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"q\"=\"chosen label\". You can now continue."}]}}"#,
    );
    assert!(r.is_auq_answer_boundary());
    assert!(r.opens_turn());
    let unit = r.auq_exchange().expect("fallback exchange");
    assert!(unit.contains("AskUserQuestion"));
    assert!(unit.contains("chosen label"));
}

#[test]
fn cancelled_auq_is_not_a_boundary() {
    // A rejected/cancelled AUQ: is_error:true, no answers, the generic rejection
    // marker WITHOUT a "the user said" tail → must NOT open a turn (no user message).
    let r = parse(
        r#"{"type":"user","toolUseResult":"User rejected tool use","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed."}]}}"#,
    );
    assert!(
        !r.is_auq_answer_boundary(),
        "cancelled AUQ is not a boundary"
    );
    assert!(
        !r.is_plan_rejection_boundary(),
        "no typed message → not a rejection boundary"
    );
    assert!(!r.opens_turn());
}

#[test]
fn auq_validation_error_is_not_a_boundary() {
    // An InputValidationError AUQ result (is_error:true, <tool_use_error>…) is not an
    // answer → not a boundary.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","is_error":true,"content":"<tool_use_error>InputValidationError: AskUserQuestion failed…</tool_use_error>"}]}}"#,
    );
    assert!(!r.is_auq_answer_boundary());
    assert!(!r.opens_turn());
}

// ── §4.2.4 ExitPlanMode / tool-use rejection WITH a typed message = a genuine-user
//    boundary + a plan pointer. Real-captured shape (the typed tail is the message). ──

#[test]
fn plan_rejection_with_typed_message_is_a_boundary() {
    // The user rejects the plan and types a follow-up instruction.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_PLANREJECT01","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). To tell you how to proceed, the user said:\nplease run the smoke tests once and diff the output before calling it done."}]}}"#,
    );
    assert!(r.is_plan_rejection_boundary());
    assert!(r.opens_turn());
    let (id, msg) = r.plan_rejection_message().expect("rejection message");
    assert_eq!(id.as_deref(), Some("toolu_PLANREJECT01"));
    // The genuine message is ONLY the typed tail (whole), not the synthesized prefix.
    assert_eq!(
        msg,
        "please run the smoke tests once and diff the output before calling it done."
    );
    assert!(!msg.contains("doesn't want to proceed"));
}

#[test]
fn plan_rejection_with_english_message_is_a_boundary() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01YKnvDN43RnTQMGcw18HShW","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). To tell you how to proceed, the user said:\nRound-4 sign-off: run the e2e once more before declaring done."}]}}"#,
    );
    let (_, msg) = r.plan_rejection_message().expect("english rejection");
    assert_eq!(
        msg,
        "Round-4 sign-off: run the e2e once more before declaring done."
    );
}

#[test]
fn plan_rejection_without_message_is_not_a_boundary() {
    // The `STOP what you are doing and wait…` form carries NO typed message → no
    // boundary (36 such records in the corpus).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed."}]}}"#,
    );
    assert!(r.plan_rejection_message().is_none());
    assert!(!r.is_plan_rejection_boundary());
    assert!(!r.opens_turn());
}

#[test]
fn plan_approval_is_not_a_boundary() {
    // The approval path is the harness greenlight (no typed message, no is_error) —
    // must NOT become a turn boundary.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"User has approved your plan. You can now start coding. Your plan has been saved to: /Users/testuser/.claude/plans/elegant-scribbling-dream.md"}]}}"#,
    );
    assert!(!r.is_plan_rejection_boundary());
    assert!(!r.is_auq_answer_boundary());
    assert!(!r.opens_turn());
}

#[test]
fn plan_rejection_surfaces_plan_pointer_via_index() {
    // The ExitPlanMode tool_use carries planFilePath; the rejection resolves it via
    // the PlanIndex → the reconstructed user text carries a `[plan: …]` pointer.
    let tool_use = parse(
        r##"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_PLAN","name":"ExitPlanMode","input":{"plan":"# the plan body","planFilePath":"/Users/testuser/.claude/plans/elegant-scribbling-dream.md"}}]}}"##,
    );
    let rejection = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_PLAN","is_error":true,"content":"The user doesn't want to proceed with this tool use. To tell you how to proceed, the user said:\nadd a screenshot check first"}]}}"#,
    );
    let index = PlanIndex::from_records([&tool_use]);
    assert_eq!(
        index.plan_path("toolu_PLAN"),
        Some("/Users/testuser/.claude/plans/elegant-scribbling-dream.md")
    );
    let text = rejection
        .reconstructed_user_text(Some(&index))
        .expect("reconstructed");
    assert!(text.starts_with("add a screenshot check first"));
    assert!(
        text.contains("[plan: /Users/testuser/.claude/plans/elegant-scribbling-dream.md]"),
        "plan pointer surfaced: {text}"
    );
}
