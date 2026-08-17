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
fn unknown_top_level_fields_are_ignored() {
    // Real records carry attachment/isMeta/slug/etc. — must not break parsing.
    let r = parse(
        r#"{"type":"assistant","attachment":{"x":1},"isMeta":true,"slug":"s","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
    );
    assert!(r.is_type("assistant"));
}

#[test]
fn unknown_block_type_does_not_fail() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"future_block","data":1}]}}"#,
    );
    assert!(r.is_type("assistant"));
}

#[test]
fn metadata_record_without_timestamp_parses() {
    let r = parse(r#"{"type":"last-prompt","leafUuid":"abc","sessionId":"s"}"#);
    assert!(r.timestamp.is_none());
    assert!(r.is_type("last-prompt"));
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

#[test]
fn genuine_user_text_string_content() {
    let r = parse(r#"{"type":"user","message":{"role":"user","content":"  hello\n  world  "}}"#);
    assert_eq!(r.genuine_user_text().as_deref(), Some("hello world"));
}

#[test]
fn genuine_user_text_joins_text_blocks() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"line one"},{"type":"text","text":"line two"}]}}"#,
    );
    assert_eq!(r.genuine_user_text().as_deref(), Some("line one line two"));
}

#[test]
fn agent_text_extracts_visible_text_only() {
    // thinking + tool_use are NOT the visible message; only `text` counts.
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"},{"type":"tool_use","id":"t","name":"Bash","input":{}},{"type":"text","text":"Done — built."}]}}"#,
    );
    assert_eq!(r.agent_text().as_deref(), Some("Done — built."));
}

#[test]
fn agent_text_none_when_no_visible_text() {
    // A pure tool-call assistant turn has no end-of-turn message.
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#,
    );
    assert!(r.agent_text().is_none());
}

#[test]
fn agent_text_none_for_user_record() {
    let r = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
    assert!(r.agent_text().is_none());
}

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
fn persisted_output_path_structured_field_preferred() {
    let r = parse(
        r#"{"type":"user","toolUseResult":{"persistedOutputPath":"/tmp/x/tool-results/abc.txt","persistedOutputSize":123},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"<persisted-output>\nOutput too large (1 KB). Full output saved to: /wrong/inline/path.txt\n</persisted-output>"}]}}"#,
    );
    // Structured field wins over the inline marker.
    assert_eq!(
        r.persisted_output_path().as_deref(),
        Some("/tmp/x/tool-results/abc.txt")
    );
}

#[test]
fn persisted_output_path_inline_fallback() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"<persisted-output>\nOutput too large (200 KB). Full output saved to: /tmp/sess/tool-results/b070yh2rb.txt\n\nPreview (first 2KB):\n…\n</persisted-output>"}]}}"#,
    );
    assert_eq!(
        r.persisted_output_path().as_deref(),
        Some("/tmp/sess/tool-results/b070yh2rb.txt")
    );
}

#[test]
fn persisted_output_path_none_when_absent() {
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"a normal small tool output"}]}}"#,
    );
    assert!(r.persisted_output_path().is_none());
}

#[test]
fn scrape_persisted_path_handles_marker() {
    assert_eq!(
        scrape_persisted_path("Full output saved to: /a/b/c.txt\nmore"),
        Some("/a/b/c.txt".to_string())
    );
    assert!(scrape_persisted_path("no marker here").is_none());
}

#[test]
fn tool_result_content_text_string_and_array() {
    let s = serde_json::json!("hello world");
    assert_eq!(tool_result_content_text(&s), "hello world");
    let arr = serde_json::json!([
        {"type":"text","text":"first"},
        {"type":"image","source":{}},
        {"type":"text","text":"second"},
        {"type":"tool_reference","tool_name":"WebSearch"}
    ]);
    assert_eq!(tool_result_content_text(&arr), "first\nsecond\nWebSearch");
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

#[test]
fn blocks_none_for_string_content() {
    // `blocks()` returns None when content is a bare string (Content::Text arm).
    let r = parse(r#"{"type":"user","message":{"role":"user","content":"plain"}}"#);
    assert!(r.blocks().is_none());
}

#[test]
fn blocks_none_when_no_message() {
    let r = parse(r#"{"type":"system","subtype":"compact_boundary"}"#);
    assert!(r.blocks().is_none());
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
fn persisted_output_path_empty_structured_falls_through_to_inline() {
    // An empty structured persistedOutputPath must NOT win — the inline marker
    // is used instead (the `!p.is_empty()` false arm).
    let r = parse(
        r#"{"type":"user","toolUseResult":{"persistedOutputPath":""},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"<persisted-output>\nFull output saved to: /tmp/real/inline.txt\n</persisted-output>"}]}}"#,
    );
    assert_eq!(
        r.persisted_output_path().as_deref(),
        Some("/tmp/real/inline.txt")
    );
}

#[test]
fn persisted_output_path_none_when_no_blocks() {
    // user record with string content → blocks() is None → `?` short-circuits.
    let r =
        parse(r#"{"type":"user","message":{"role":"user","content":"no persisted pointer here"}}"#);
    assert!(r.persisted_output_path().is_none());
}

#[test]
fn persisted_output_path_structured_present_but_missing_key_falls_to_inline() {
    // `toolUseResult` exists but lacks `persistedOutputPath` (the
    // `tur.get(...)` None arm) → fall through to the inline marker scan.
    let r = parse(
        r#"{"type":"user","toolUseResult":{"somethingElse":1},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"Full output saved to: /tmp/fallback.txt\n"}]}}"#,
    );
    assert_eq!(
        r.persisted_output_path().as_deref(),
        Some("/tmp/fallback.txt")
    );
}

#[test]
fn persisted_output_path_skips_non_tool_result_blocks_in_inline_scan() {
    // The inline fallback loop must skip a non-ToolResult block (the `if let
    // Block::ToolResult` FALSE arm) and find the marker in a later tool_result.
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"not a tool result"},{"type":"tool_result","tool_use_id":"x","content":"Full output saved to: /tmp/later.txt\n"}]}}"#,
    );
    assert_eq!(r.persisted_output_path().as_deref(), Some("/tmp/later.txt"));
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

#[test]
fn agent_text_none_for_non_assistant() {
    let r = parse(r#"{"type":"system","subtype":"away_summary"}"#);
    assert!(r.agent_text().is_none());
}
