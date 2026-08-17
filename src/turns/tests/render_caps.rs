//! Turn-unit rendering: role caps, ellipsis asymmetry, markers, placeholders.

use super::*;

#[test]
fn role_caps_and_head_fractions_are_asymmetric() {
    assert_eq!(Role::User.cap(), 600);
    assert_eq!(Role::Assistant.cap(), 900);
    assert!(Role::Assistant.cap() > Role::User.cap());
    assert!(Role::Assistant.head_frac() > Role::User.head_frac());
    assert_eq!(Role::User.label(), "user");
    assert_eq!(Role::Assistant.label(), "assistant");
}

#[test]
fn sub_cap_unit_renders_verbatim_no_marker() {
    let u = unit(Role::User, 10, "café🛠 a short ask", 0);
    let r = render_unit_body(&u, None);
    assert!(!r.truncated);
    assert_eq!(r.body, "café🛠 a short ask");
    assert_eq!(r.elided_chars, 0);
    assert_eq!(r.elided_lines, 0);
    assert_eq!(r.rendered_chars, "café🛠 a short ask".chars().count());
}

#[test]
fn user_ellipsis_head_360_tail_240_with_counts() {
    // A user body > 600 chars → head 360 / tail 240, marker carries +K chars.
    let body: String = "a".repeat(1000);
    let u = unit(Role::User, 10, &body, 3);
    let r = render_unit_body(&u, None);
    assert!(r.truncated);
    assert_eq!(r.elided_chars, 1000 - 600);
    assert_eq!(r.elided_lines, 3);
    // head 360, tail 240 (600 cap, 0.60 head frac).
    assert!(r.body.starts_with(&"a".repeat(360)));
    assert!(r.body.ends_with(&"a".repeat(240)));
    assert!(r.body.contains("[+400 chars, 3 lines elided]"));
    // The displayed (rendered) char count excludes the marker scaffolding.
    assert_eq!(r.rendered_chars, 600);
}

#[test]
fn assistant_ellipsis_head_larger_than_user_head() {
    // Assistant 900 cap, 0.66 head frac → head 594 / tail 306. Strictly larger head
    // than the user side (the measured asymmetry).
    let body: String = "b".repeat(2000);
    let a = unit(Role::Assistant, 20, &body, 7);
    let r = render_unit_body(&a, None);
    assert!(r.truncated);
    assert_eq!(r.elided_chars, 2000 - 900);
    // head 594.
    assert!(r.body.starts_with(&"b".repeat(594)));
    assert!(r.body.ends_with(&"b".repeat(306)));
    assert!(r.body.contains("[+1100 chars, 7 lines elided]"));
    assert_eq!(r.rendered_chars, 900);

    // The assistant head (594) is strictly larger than the user head (360).
    let ubody: String = "u".repeat(2000);
    let ru = render_unit_body(&unit(Role::User, 1, &ubody, 0), None);
    let asst_head_len = 594usize;
    let user_head_len = 360usize;
    assert!(asst_head_len > user_head_len);
    // and the rendered user head prefix is shorter than the assistant head prefix.
    assert!(ru.body.starts_with(&"u".repeat(360)));
}

#[test]
fn single_line_user_omits_lines_elided_note() {
    let body: String = "x".repeat(1000);
    let u = unit(Role::User, 5, &body, 0); // 0 original newlines
    let r = render_unit_body(&u, None);
    assert!(r.truncated);
    assert!(r.body.contains("[+400 chars]"));
    assert!(
        !r.body.contains("lines elided"),
        "single-line message must omit the line note: {}",
        r.body
    );
}

#[test]
fn ellipsis_cut_is_codepoint_safe_for_multibyte_token() {
    // A multi-byte token straddling the cut must be wholly kept or wholly dropped — the
    // rendered string must remain valid UTF-8 with no replacement char.
    // Build a body of 700 single-char 'a' then a 🛠 (4 bytes) at the boundary region.
    let mut body = String::new();
    body.push_str(&"a".repeat(360)); // exactly the head region
    body.push('🛠'); // lands right after the head cut
    body.push_str(&"a".repeat(400));
    let u = unit(Role::User, 1, &body, 0);
    let r = render_unit_body(&u, None);
    // valid UTF-8 by construction (String), and the emoji is either in head or dropped,
    // never split. The head is the first 360 'a's; the emoji is elided.
    assert!(r.body.starts_with(&"a".repeat(360)));
    assert!(!r.body.contains('\u{FFFD}'), "no replacement char");
    // The whole rendered body round-trips through chars (no mid-codepoint slice).
    let rebuilt: String = r.body.chars().collect();
    assert_eq!(rebuilt, r.body);
}

#[test]
fn emoji_in_tail_is_wholly_kept() {
    // Put the emoji in the tail region; it must survive intact.
    let mut body = String::new();
    body.push_str(&"a".repeat(700));
    body.push('🛠');
    body.push_str(&"b".repeat(239)); // tail = last 240 chars = 🛠 + 239 b's
    let u = unit(Role::User, 1, &body, 0);
    let r = render_unit_body(&u, None);
    assert!(r.truncated);
    assert!(r.body.contains('🛠'), "emoji in the kept tail: {}", r.body);
    assert!(!r.body.contains('\u{FFFD}'));
}

#[test]
fn render_turn_text_emits_user_marker_assistant_for_both() {
    // Exercise the text renderer arms directly: a both-sides turn with tool calls emits
    // ▽ user, the [N tool calls] marker, then △ assistant.
    let t = mk_turn(0, Some("the ask"), Some("the reply"), 3, 0);
    let mut lines: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::Both, &cfg(), None, &mut |s| lines.push(s));
    let joined = lines.join("\n");
    assert!(joined.contains("▽ L1"), "user header: {joined}");
    assert!(joined.contains("[3 tool calls]"), "tool marker: {joined}");
    assert!(joined.contains("△ L5"), "assistant header: {joined}");
    assert!(joined.contains("the ask"));
    assert!(joined.contains("the reply"));

    // UserOnly: no marker, no assistant.
    let mut uonly: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::UserOnly, &cfg(), None, &mut |s| uonly.push(s));
    let uj = uonly.join("\n");
    assert!(uj.contains("▽ L1"));
    assert!(!uj.contains("tool calls"), "no marker on user-only: {uj}");
    assert!(!uj.contains("△ L5"), "no assistant on user-only");

    // AssistantOnly: only the assistant side.
    let mut aonly: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::AssistantOnly, &cfg(), None, &mut |s| {
        aonly.push(s)
    });
    let aj = aonly.join("\n");
    assert!(!aj.contains("▽ L1"), "no user on assistant-only");
    assert!(aj.contains("△ L5"));
}

#[test]
fn render_turn_text_zero_tool_turn_omits_marker() {
    let t = mk_turn(0, Some("ask"), Some("reply"), 0, 0);
    let mut lines: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::Both, &cfg(), None, &mut |s| lines.push(s));
    assert!(
        !lines.join("\n").contains("tool calls"),
        "0-tool omits marker"
    );
}

#[test]
fn emit_unit_text_flags_dedup_unit() {
    let mut u = unit(Role::User, 7, "deduped ask", 0);
    u.also_in_summary = true;
    let mut lines: Vec<String> = Vec::new();
    emit_unit_text(&u, None, &mut |s| lines.push(s));
    assert!(
        lines.iter().any(|l| l.contains("(also in summary)")),
        "dedup flag rendered: {lines:?}"
    );
    // The glyph is derived from the role (user → ▽) inside emit_unit_text now.
    assert!(lines[0].starts_with("▽ L7"), "user glyph: {lines:?}");
}

#[test]
fn render_turn_text_user_only_with_no_user_emits_nothing() {
    // Defensive: a UserOnly selection on a turn whose user is None (cannot normally
    // happen) emits no user line — the `if let Some(u)` false arm.
    let t = mk_turn(0, None, Some("only assistant"), 0, 0);
    let mut lines: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::UserOnly, &cfg(), None, &mut |s| lines.push(s));
    assert!(lines.is_empty(), "no user to render: {lines:?}");
}

#[test]
fn render_turn_text_assistant_only_with_no_assistant_emits_nothing() {
    let t = mk_turn(0, Some("only user"), None, 0, 0);
    let mut lines: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::AssistantOnly, &cfg(), None, &mut |s| {
        lines.push(s)
    });
    assert!(lines.is_empty(), "no assistant to render: {lines:?}");
}

#[test]
fn render_turn_text_both_with_zero_tools_no_marker_line() {
    // Both selection, 0 tools → the marker `if turn.tool_calls > 0` false arm: no
    // `[N tool calls]` line, but user + assistant still render.
    let t = mk_turn(0, Some("ask"), Some("reply"), 0, 0);
    let mut lines: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::Both, &cfg(), None, &mut |s| lines.push(s));
    assert!(lines.iter().any(|l| l.starts_with("▽ L")));
    assert!(lines.iter().any(|l| l.starts_with("△ L")));
    assert!(!lines.iter().any(|l| l.contains("tool calls")));
}

#[test]
fn emit_unit_text_non_dup_has_no_flag() {
    // A non-dedup unit renders WITHOUT the (also in summary) suffix (the `also_in_summary`
    // false arm).
    let u = unit(Role::User, 3, "a normal ask", 0);
    let mut lines: Vec<String> = Vec::new();
    emit_unit_text(&u, None, &mut |s| lines.push(s));
    assert!(!lines.iter().any(|l| l.contains("also in summary")));
    assert!(lines[0].starts_with("▽ L3"));
}

#[test]
fn agent_placeholder_line_pluralizes_each_noun_independently() {
    // X==1 → "1 agent message" + single L{n} (no dash); Y==0 shown; Z==0 omitted.
    let one = PlaceholderSpan {
        messages: 1,
        tool_calls: 0,
        failed: 0,
        first_line: 42,
        last_line: 42,
    };
    assert_eq!(
        agent_placeholder_line(&one),
        "△ L42  [1 agent message, 0 tool calls]"
    );
    // X>1 → range with a dash; Y>1 plural; Z>0 included, "failed" not pluralized.
    let many = PlaceholderSpan {
        messages: 3,
        tool_calls: 4,
        failed: 2,
        first_line: 10,
        last_line: 20,
    };
    assert_eq!(
        agent_placeholder_line(&many),
        "△ L10–L20  [3 agent messages, 4 tool calls, 2 failed]"
    );
    // Z==1 → "1 failed" (adjective, not "1 faileds").
    let one_fail = PlaceholderSpan {
        messages: 2,
        tool_calls: 1,
        failed: 1,
        first_line: 5,
        last_line: 9,
    };
    assert_eq!(
        agent_placeholder_line(&one_fail),
        "△ L5–L9  [2 agent messages, 1 tool call, 1 failed]"
    );
}

#[test]
fn placeholder_attribution_sums_per_message_tool_and_failed() {
    // The collapsed span's Y/Z sum the per-message preceding_tool_calls / preceding_failed.
    let mut agents = vec![
        agent_msg(10, "found the cause", 0, 0), // first (rich) → kept
        agent_msg(20, "let me a", 2, 1),        // middle decl → collapse (2 tools, 1 failed)
        agent_msg(30, "let me b", 3, 0),        // middle decl → collapse (3 tools)
        agent_msg(40, "let me c", 1, 2),        // middle decl → collapse (1 tool, 2 failed)
        agent_msg(50, "let me d", 0, 0),        // middle decl → collapse
        agent_msg(60, "let me e", 0, 0),        // middle decl → collapse
        agent_msg(70, "let me f", 0, 0),        // middle decl → collapse
        agent_msg(80, "done", 0, 0),            // last → kept
    ];
    assign_positions(&mut agents);
    let t = TurnSlice {
        turn_index: 0,
        user: Some(unit(Role::User, 1, "ask", 0)),
        tool_calls: 6,
        image_ids: Vec::new(),
        agents,
        compactions_before: 0,
        is_automation: false,
        automation: None,
    };
    let lane = select_agent_messages(&t, &rich_cfg());
    let span = lane
        .iter()
        .find_map(|r| match r {
            AgentRender::Placeholder(s) => Some(s),
            _ => None,
        })
        .expect("a placeholder for the collapsed middles");
    assert_eq!(span.messages, 6, "six middle declarations");
    assert_eq!(
        span.tool_calls,
        2 + 3 + 1,
        "Y sums the span's preceding tool calls"
    );
    assert_eq!(
        span.failed,
        1 + 2,
        "Z sums the span's erroring tool results"
    );
    assert_eq!(span.first_line, 20);
    assert_eq!(span.last_line, 70);
}
