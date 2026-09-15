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
    // The three newlines sit at 250 / 500 / 750 (evenly spread by the helper); the cut
    // removes [360, 760), so the two at 500 and 750 are gone and the one at 250 survives
    // inside the kept head.
    assert_eq!(r.elided_lines, 2);
    // head 360, tail 240 (600 cap, 0.60 head frac).
    assert!(r.body.starts_with(&"a".repeat(360)));
    assert!(r.body.ends_with(&"a".repeat(240)));
    assert!(r.body.contains("[+400 chars, 2 lines elided]"));
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
    // Seven newlines evenly spread → 250, 500, …, 1750; the cut removes [594, 1694), so the
    // four at 750/1000/1250/1500 are elided (250 and 500 sit in the head, 1750 in the tail).
    assert!(r.body.contains("[+1100 chars, 4 lines elided]"));
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
fn lines_elided_counts_only_the_newlines_the_cut_removed() {
    // A 1000-char user body with FIVE original newlines placed so the removed span
    // [360, 760) contains exactly TWO of them: 100 and 200 sit in the kept head, 400 and 700
    // inside the cut, 800 in the kept tail.
    let body: String = "a".repeat(1000);
    let u = unit_at(Role::User, 10, &body, &[100, 200, 400, 700, 800]);
    let r = render_unit_body(&u, None);
    assert!(r.truncated);
    assert_eq!(r.elided_chars, 400);
    assert_eq!(
        r.elided_lines, 2,
        "only the newlines strictly inside [360,760) are elided: {}",
        r.body
    );
    assert!(
        r.body.contains("[+400 chars, 2 lines elided]"),
        "{}",
        r.body
    );

    // The SAME message uncut (a cap above its length) reports 0 and prints no note at all.
    let uncut = render_unit_body(&u, Some(1000));
    assert!(!uncut.truncated);
    assert_eq!(uncut.elided_lines, 0);
    assert!(!uncut.body.contains("lines elided"), "{}", uncut.body);

    // And a WIDER cut of the same message reports MORE lines - the figure follows the cut,
    // which is exactly what the whole-message count could not do. Cap 300 → head 180, tail
    // 120, removed [180, 880): four newlines (200, 400, 700, 800).
    let wider = render_unit_body(&u, Some(300));
    assert_eq!(wider.elided_chars, 700);
    assert_eq!(wider.elided_lines, 4, "{}", wider.body);
}

#[test]
fn elided_newlines_counts_the_half_open_span() {
    // The span is [cut_from, cut_to): the lower bound is INCLUDED, the upper EXCLUDED, so a
    // newline exactly at either edge lands on the documented side.
    let at = [10u32, 20, 30, 40];
    assert_eq!(elided_newlines(&at, 20, 40), 2, "20 in, 40 out");
    assert_eq!(elided_newlines(&at, 0, 41), 4);
    assert_eq!(elided_newlines(&at, 41, 99), 0);
    // Collapsed-together newlines share one offset and are each counted.
    assert_eq!(elided_newlines(&[15, 15, 15], 10, 20), 3);
    // An empty or inverted span removes nothing.
    assert_eq!(elided_newlines(&at, 20, 20), 0);
    assert_eq!(elided_newlines(&at, 40, 20), 0);
    assert_eq!(elided_newlines(&[], 0, 99), 0);
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
    // A multi-byte token straddling the cut must be wholly kept or wholly dropped - the
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
    // happen) emits no user line - the `if let Some(u)` false arm.
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

/// A [`PlaceholderSpan`] with the X/Y/Z counts, the line range, the folded char total and no
/// previews - the shape the marker-wording tests exercise.
fn span(
    messages: usize,
    tool_calls: usize,
    failed: usize,
    first_line: usize,
    last_line: usize,
    chars: usize,
) -> PlaceholderSpan {
    PlaceholderSpan {
        messages,
        tool_calls,
        failed,
        first_line,
        last_line,
        chars,
        previews: Vec::new(),
    }
}

#[test]
fn agent_placeholder_line_pluralizes_each_noun_independently() {
    // X==1 → "1 agent message" + single L{n} (no dash); Y==0 shown; Z==0 omitted; N is the
    // summed chars of the collapsed bodies.
    let one = span(1, 0, 0, 42, 42, 137);
    assert_eq!(
        agent_placeholder_line(&one),
        "△ L42  [1 agent message collapsed, 137 chars, 0 tool calls]"
    );
    // X>1 → range with a dash; Y>1 plural; Z>0 included, "failed" not pluralized.
    let many = span(3, 4, 2, 10, 20, 512);
    assert_eq!(
        agent_placeholder_line(&many),
        "△ L10–L20  [3 agent messages collapsed, 512 chars, 4 tool calls, 2 failed]"
    );
    // Z==1 → "1 failed" (adjective, not "1 faileds").
    let one_fail = span(2, 1, 1, 5, 9, 64);
    assert_eq!(
        agent_placeholder_line(&one_fail),
        "△ L5–L9  [2 agent messages collapsed, 64 chars, 1 tool call, 1 failed]"
    );
}

#[test]
fn a_fold_previews_only_its_substantive_members() {
    // The preview gate is `full_chars >= COLLAPSED_PREVIEW_MIN_CHARS` (210): 209 chars earns
    // none, 210 earns one. The excerpt is the first 60 chars and states its own remainder.
    let under = unit(Role::Assistant, 11, &body_chars("SHORT", 209), 0);
    assert_eq!(under.full_chars, 209);
    assert!(collapsed_preview(&under).is_none());

    let over = unit(Role::Assistant, 12, &body_chars("LONG", 210), 0);
    assert_eq!(over.full_chars, 210);
    let p = collapsed_preview(&over).expect("a preview at the threshold");
    assert_eq!(p.line, 12);
    assert_eq!(
        p.excerpt.chars().count(),
        60 + "… (+150 chars)".chars().count()
    );
    assert!(p.excerpt.contains("… (+150 chars)"), "{}", p.excerpt);
    assert_eq!(
        collapsed_preview_line(&p),
        format!("    L12  {}", p.excerpt)
    );
}

#[test]
fn a_fold_marker_and_its_preview_lines_are_what_the_cost_charges() {
    // `agent_placeholder_lines` is the ONE list the renderer emits and the cost model charges,
    // so the two can never disagree: marker first, then one preview per substantive member.
    let mut s = span(2, 3, 0, 7, 9, 430);
    s.previews = vec![
        CollapsedPreview {
            line: 7,
            excerpt: "first collapsed head".to_string(),
        },
        CollapsedPreview {
            line: 9,
            excerpt: "second collapsed head".to_string(),
        },
    ];
    let lines = agent_placeholder_lines(&s);
    assert_eq!(lines.len(), 3, "marker + two previews: {lines:?}");
    assert_eq!(lines[0], agent_placeholder_line(&s));
    assert_eq!(lines[1], "    L7  first collapsed head");
    assert_eq!(lines[2], "    L9  second collapsed head");
    let emitted: usize = lines.iter().map(|l| l.chars().count() + 1).sum();
    assert_eq!(agent_placeholder_cost(&s), emitted);
    // With no previews the cost is the marker line alone.
    let bare = span(2, 3, 0, 7, 9, 430);
    assert_eq!(
        agent_placeholder_cost(&bare),
        agent_placeholder_line(&bare).chars().count() + 1
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
    // N sums the collapsed bodies' own char counts ("let me a".."let me f", 8 chars each).
    assert_eq!(span.chars, 6 * "let me a".chars().count());
    // All six are far under the preview threshold, so the fold previews nothing.
    assert!(span.previews.is_empty(), "{:?}", span.previews);
}
