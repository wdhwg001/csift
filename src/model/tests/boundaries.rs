//! AskUserQuestion answers and plan rejections as turn-opening boundaries, plus plan pointers.

use super::*;

#[test]
fn auq_answer_carrier_detected() {
    // Real shape: a user-carrier whose tool_result content is the synthesized
    // "User has answered your questions: …" string (§4.4).
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"User has answered your questions: \"Q\"=\"A\". You can now continue."}]}}"#,
    );
    assert!(r.is_auq_answer());
    // It is NOT a genuine user (it's a carrier) but IS an AUQ answer — and, per the
    // §6.4 behavior change, it DOES open a turn (the answer is the user's message).
    assert!(!r.is_genuine_user());
    assert!(r.is_auq_answer_boundary());
    assert!(r.opens_turn());
}

#[test]
fn auq_answer_alternate_phrasing_detected() {
    // The DOMINANT real-data phrasing the single hardcoded marker used to miss:
    // "Your questions have been answered: …" (verified across real sessions —
    // 16 sessions use this form vs 13 the other, 4 contain BOTH). Must be
    // recognised under the `user` category exactly like the other phrasing.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"Your questions have been answered: \"Q\"=\"A\". You can now continue with these answers in mind."}]}}"#,
    );
    assert!(r.is_auq_answer(), "alternate AUQ phrasing must be detected");
    assert!(!r.is_genuine_user());
    // The dominant phrasing is also a turn boundary (§6.4).
    assert!(r.is_auq_answer_boundary());
    assert!(r.opens_turn());
}

#[test]
fn is_auq_answer_text_recognises_both_phrasings() {
    assert!(is_auq_answer_text(
        "User has answered your questions: \"q\"=\"a\"."
    ));
    assert!(is_auq_answer_text(
        "Your questions have been answered: \"q\"=\"a\"."
    ));
    assert!(!is_auq_answer_text("a normal tool output"));
}

#[test]
fn is_auq_answer_text_is_start_anchored_not_contains() {
    // The reported bug: a tool_result (e.g. a Read of SPEC.md / a fixture) that merely
    // QUOTES the marker mid-content must NOT be taken for a synthesized AUQ answer. The
    // real machine answer LEADS with the marker; a quote does not.
    assert!(!is_auq_answer_text(
            "# SPEC.md\nThe synthesized answer string is \"User has answered your questions:\" which leads the body."
        ));
    assert!(!is_auq_answer_text(
        "see the marker \"Your questions have been answered\" documented above"
    ));
    // Leading whitespace the renderer may prepend is tolerated (still anchored).
    assert!(is_auq_answer_text(
        "  User has answered your questions: \"q\"=\"a\"."
    ));
}

#[test]
fn auq_answer_no_false_positive_on_file_quoting_marker() {
    // A Read/grep tool_result whose content QUOTES the marker mid-text (the csift
    // dev-session failure): NOT an AUQ answer, NOT a boundary, and classify yields a
    // plain `agent.tool.result` — never `user.answer` dumping the whole file.
    // NB: `r##"…"##` delimiter — the JSON `"content":"# SPEC.md` has `"#`, which would
    // close a plain `r#"…"#` raw string early.
    let r = parse(
        r##"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"# SPEC.md (10000 chars)\n...the marker is \"User has answered your questions:\" which CC emits to synthesize the answer record. Lots more file content follows here."}]}}"##,
    );
    assert!(!r.is_auq_answer());
    assert!(!r.is_auq_answer_boundary());
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::AgentToolResult]
    );
}

#[test]
fn auq_answer_genuine_marker_lead_still_classifies_as_user_answer() {
    // A genuine synthesized answer (content STARTS with the marker, NO structured
    // toolUseResult.answers) stays detected via the fallback arm: a boundary + the
    // [user.answer, agent.tool.result] dual label.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"User has answered your questions: \"Pick one\"=\"A\". You can now continue with the user's answers in mind."}]}}"#,
    );
    assert!(r.is_auq_answer());
    assert!(r.is_auq_answer_boundary());
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserAnswer, Class::AgentToolResult]
    );
}

#[test]
fn auq_answer_structured_path_independent_of_marker_text() {
    // The PRIMARY (modern) path — a non-empty structured `toolUseResult.answers` —
    // is start-anchor-independent: it classifies `user.answer` even when the carrier's
    // content does NOT lead with the marker.
    let r = parse(
        r#"{"type":"user","toolUseResult":{"answers":{"Pick one":"A"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"(answer recorded)"}]}}"#,
    );
    assert!(r.is_auq_answer_boundary());
    assert_eq!(
        r.classify(&ClassifyCtx::top_level()),
        vec![Class::UserAnswer, Class::AgentToolResult]
    );
}

#[test]
fn plain_tool_result_is_not_auq_answer() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"just a normal tool output"}]}}"#,
    );
    assert!(!r.is_auq_answer());
}

#[test]
fn is_auq_answer_false_for_non_user() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"x"}]}}"#,
    );
    assert!(!r.is_auq_answer());
}

#[test]
fn is_auq_answer_false_when_no_blocks() {
    // user record with string content → no blocks → not an AUQ answer.
    let r = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
    assert!(!r.is_auq_answer());
}

#[test]
fn is_auq_answer_skips_non_tool_result_blocks() {
    // is_auq_answer's `.any()` must return false for a block that is NOT a
    // tool_result (the match's `_ => false` arm) when no AUQ marker is present.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"plain user text, no AUQ marker"}]}}"#,
    );
    assert!(!r.is_auq_answer());
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

#[test]
fn plan_rejection_of_non_exit_plan_tool_has_no_pointer() {
    // A rejection-with-message whose tool_use_id is NOT an ExitPlanMode → still a
    // genuine-user boundary, but no plan pointer (the index does not resolve it).
    let rejection = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_EDIT","is_error":true,"content":"The user doesn't want to proceed with this tool use. To tell you how to proceed, the user said:\nuse a different file path"}]}}"#,
    );
    let index = PlanIndex::default(); // no ExitPlanMode indexed
    assert!(rejection.is_plan_rejection_boundary());
    let text = rejection.reconstructed_user_text(Some(&index)).unwrap();
    assert_eq!(text, "use a different file path");
    assert!(!text.contains("[plan:"));
}

#[test]
fn exit_plan_pointers_extracts_id_and_path() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"p1","name":"ExitPlanMode","input":{"plan":"x","planFilePath":"/plans/a.md"}}]}}"#,
    );
    assert_eq!(
        r.exit_plan_pointers(),
        vec![("p1".to_string(), "/plans/a.md".to_string())]
    );
    // A non-ExitPlanMode record yields nothing.
    let other = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Read","input":{"file_path":"/x"}}]}}"#,
    );
    assert!(other.exit_plan_pointers().is_empty());
}

#[test]
fn plan_index_skips_pointer_without_path() {
    // An ExitPlanMode with NO planFilePath is not indexed (empty path → no pointer).
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"p1","name":"ExitPlanMode","input":{"plan":"x"}}]}}"#,
    );
    let index = PlanIndex::from_records([&r]);
    assert!(index.plan_path("p1").is_none());
}
