//! Record accessors and tolerant parsing: text extraction, blocks, persisted-output paths.

use super::*;

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
fn agent_text_none_for_non_assistant() {
    let r = parse(r#"{"type":"system","subtype":"away_summary"}"#);
    assert!(r.agent_text().is_none());
}

#[test]
fn agent_text_handles_bare_string_assistant_content() {
    // CC normally sends assistant content as a block array; a bare-string body is
    // a surprise we surface rather than drop (the Content::Text fallback arm).
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":"  surprise bare string  "}}"#,
    );
    assert_eq!(r.agent_text().as_deref(), Some("surprise bare string"));
}

#[test]
fn agent_text_none_for_empty_bare_string() {
    // A bare-string assistant body that is all whitespace normalizes to empty → None.
    let r = parse(r#"{"type":"assistant","message":{"role":"assistant","content":"   "}}"#);
    assert!(r.agent_text().is_none());
}

#[test]
fn agent_text_none_when_no_content() {
    let r = parse(r#"{"type":"assistant","message":{"role":"assistant"}}"#);
    assert!(r.agent_text().is_none());
}

#[test]
fn agent_text_skips_blank_text_blocks() {
    // A text block that is all whitespace is skipped; only the real one counts.
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"   "},{"type":"text","text":"real"}]}}"#,
    );
    assert_eq!(r.agent_text().as_deref(), Some("real"));
}

#[test]
fn flatten_blocks_ignores_non_text_blocks() {
    // genuine_user_text over blocks where a non-text block is interleaved: the
    // tool_use/image are filtered out, only text survives. (Exercised via a
    // genuine-user record whose blocks mix text with an image.)
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"keep me"},{"type":"image","source":{}}]}}"#,
    );
    assert_eq!(r.genuine_user_text().as_deref(), Some("keep me"));
}

#[test]
fn tool_result_content_text_object_fallback() {
    // A non-string/non-array content (e.g. an object) renders compactly so a
    // regex can still match structured payloads (the `other => to_string` arm).
    let obj = serde_json::json!({"k": "v", "n": 1});
    let out = tool_result_content_text(&obj);
    assert!(out.contains("\"k\""), "compact object render: {out}");
    // A bare number/bool/null also goes through the same arm.
    assert_eq!(tool_result_content_text(&serde_json::json!(42)), "42");
    assert_eq!(tool_result_content_text(&serde_json::json!(null)), "null");
}

#[test]
fn scrape_persisted_path_empty_after_marker_is_none() {
    // The marker is present but the path is blank → None (the `path.is_empty()`
    // true arm).
    assert!(scrape_persisted_path("Full output saved to:   \nnext").is_none());
    // Marker with the path running to EOF (no trailing newline) → the `unwrap_or`
    // line_end == rest.len() branch.
    assert_eq!(
        scrape_persisted_path("Full output saved to: /a/b.txt"),
        Some("/a/b.txt".to_string())
    );
}

#[test]
fn normalize_line_collapses_and_trims() {
    // Leading/trailing/internal whitespace runs collapse to single spaces; the
    // trailing-space pop loop runs.
    assert_eq!(normalize_line("  a\t\tb \n c   "), "a b c");
    assert_eq!(normalize_line(""), "");
    assert_eq!(normalize_line("   "), "");
}

#[test]
fn bash_command_some_and_none() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"rm -rf /tmp/x","description":"clean"}}]}}"#,
    );
    assert_eq!(r.bash_command(), Some("rm -rf /tmp/x"));
    // A Bash tool_use with NO command → None.
    let no_cmd = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"description":"x"}}]}}"#,
    );
    assert!(no_cmd.bash_command().is_none());
    // A non-Bash tool_use → None.
    let other = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/p/x"}}]}}"#,
    );
    assert!(other.bash_command().is_none());
    // A record with no blocks → None.
    let no_blocks = parse(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
    assert!(no_blocks.bash_command().is_none());
}
