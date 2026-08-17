//! Message selection modes: eot-only, short runs, placeholders, fusion.

use super::*;

#[test]
fn select_eot_only_keeps_only_the_last_agent_message() {
    // The non-breaking default: a multi-agent turn collapses to its last message, no
    // placeholder, byte-identical selection to the pre-expansion single EOT.
    let t = mk_turn_agents(
        0,
        Some("ask"),
        &["let me look", "still working", "the final answer"],
        0,
    );
    let lane = select_agent_messages(&t, &cfg());
    assert_eq!(lane.len(), 1);
    match &lane[0] {
        AgentRender::Kept(a) => assert_eq!(a.unit.text, "the final answer"),
        _ => panic!("expected the last message kept"),
    }
}

#[test]
fn select_one_message_turn_keeps_it_as_last() {
    // A 1-agent-message turn's sole message is BOTH first and last → always kept, no
    // richness eval, even a declaration-shaped one.
    let t = mk_turn_agents(0, Some("ask"), &["let me look into this"], 0);
    let lane = select_agent_messages(&t, &rich_cfg());
    assert_eq!(lane.len(), 1);
    assert!(matches!(lane[0], AgentRender::Kept(_)));
}

#[test]
fn select_short_run_keeps_all_under_threshold() {
    // A run at or below the run threshold (6) keeps every message verbatim (no filtering).
    let texts: Vec<String> = (0..6).map(|i| format!("let me step {i}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let t = mk_turn_agents(0, Some("ask"), &refs, 0);
    let lane = select_agent_messages(&t, &rich_cfg());
    assert_eq!(lane.len(), 6, "6 <= threshold 6 → keep all");
    assert!(lane.iter().all(|r| matches!(r, AgentRender::Kept(_))));
}

#[test]
fn select_rich_first_kept_and_sudden_rich_middle_survives() {
    // A >6 run: a rich first survives, a sudden rich middle survives whole, the pure
    // declarations around it collapse into placeholders split BY the survivor.
    let t = mk_turn_agents(
        0,
        Some("ask"),
        &[
            "found the root cause already",     // first — rich → kept
            "let me try the next thing",        // middle decl → collapse
            "now i will check another",         // middle decl → collapse
            "12 passed 3 failed in src/x.rs:9", // sudden rich middle → kept
            "let me write it up",               // middle decl → collapse
            "next i continue here",             // middle decl → collapse
            "now let me finalize",              // middle decl → collapse
            "the final committed answer",       // last → always kept
        ],
        0,
    );
    let lane = select_agent_messages(&t, &rich_cfg());
    // Kept: first, the sudden-rich middle, the last → 3 kept; the two declaration runs →
    // 2 placeholders (split by the survivor).
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        kept,
        vec![
            "found the root cause already",
            "12 passed 3 failed in src/x.rs:9",
            "the final committed answer",
        ]
    );
    let placeholders = lane
        .iter()
        .filter(|r| matches!(r, AgentRender::Placeholder(_)))
        .count();
    assert_eq!(
        placeholders, 2,
        "two contiguous declaration runs → two placeholders"
    );
}

#[test]
fn select_all_middles_droppable_makes_one_placeholder() {
    // Every middle is a signal-less short declaration → ONE placeholder spanning them all,
    // between the kept first(or its collapse) and the kept last.
    let t = mk_turn_agents(
        0,
        Some("ask"),
        &[
            "the opening plan is to refactor", // first: not droppable (no intent verb) → kept
            "let me a",
            "let me b",
            "let me c",
            "let me d",
            "let me e",
            "let me f",
            "the final answer here", // last → kept
        ],
        0,
    );
    let lane = select_agent_messages(&t, &rich_cfg());
    let spans: Vec<&PlaceholderSpan> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Placeholder(s) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(
        spans.len(),
        1,
        "one contiguous dropped run → one placeholder"
    );
    assert_eq!(spans[0].messages, 6, "the six middle declarations collapse");
}

#[test]
fn select_no_keep_first_collapses_a_declaration_first() {
    // With --no-keep-first a declaration first is decided as a middle → collapsed.
    let mut c = rich_cfg();
    c.keep_first = false;
    let t = mk_turn_agents(
        0,
        Some("ask"),
        &[
            "let me look into this", // first decl → collapsed under no-keep-first
            "let me b",
            "let me c",
            "let me d",
            "let me e",
            "let me f",
            "the final answer", // last → kept
        ],
        0,
    );
    let lane = select_agent_messages(&t, &c);
    // The first is now part of the leading placeholder span.
    match &lane[0] {
        AgentRender::Placeholder(s) => assert!(s.messages >= 1),
        _ => panic!("the declaration first must collapse with --no-keep-first: {lane:?}"),
    }
    // With keep-first (default) the same first is kept by position privilege.
    let kept_lane = select_agent_messages(&t, &rich_cfg());
    assert!(matches!(kept_lane[0], AgentRender::Kept(_)));
}

#[test]
fn select_all_mode_keeps_every_message_no_placeholder() {
    let texts: Vec<String> = (0..10).map(|i| format!("let me step {i}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let t = mk_turn_agents(0, Some("ask"), &refs, 0);
    let mut c = rich_cfg();
    c.mode = AgentMsgMode::All;
    let lane = select_agent_messages(&t, &c);
    assert_eq!(lane.len(), 10);
    assert!(lane.iter().all(|r| matches!(r, AgentRender::Kept(_))));
}

#[test]
fn select_fusion_message_kept_whole_and_char_capped_later() {
    // A fused finding+declaration body trips Arm 2 (the finding) → kept WHOLE at Stage 1;
    // the trailing declaration is shed only by the existing ASST_CAP char-ellipsis later.
    let fused = "root cause confirmed in src/x.rs:42 — now let me write the fix";
    let t = mk_turn_agents(
        0,
        Some("ask"),
        &[
            "let me a", "let me b", "let me c", "let me d", "let me e", "let me f", fused, "done",
        ],
        0,
    );
    let lane = select_agent_messages(&t, &rich_cfg());
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        kept.contains(&fused),
        "the fused finding survives Stage 1: {kept:?}"
    );
    // Stage 2 char-cap is the existing render_unit_body path — verbatim under the cap here.
    let u = unit(Role::Assistant, 1, fused, 0);
    let r = render_unit_body(&u, None);
    assert!(
        !r.truncated,
        "this fused body is under ASST_CAP so it renders whole"
    );
}

#[test]
fn trigger_boundary_six_keeps_all_seven_filters() {
    // The >6 off-by-one: exactly 6 keeps all; exactly 7 filters (last kept, the 5 middles
    // richness-gated, first by privilege).
    let c = rich_cfg();
    let six: Vec<&str> = vec![
        "let me a", "let me b", "let me c", "let me d", "let me e", "let me f",
    ];
    let t6 = mk_turn_agents(0, Some("ask"), &six, 0);
    let lane6 = select_agent_messages(&t6, &c);
    assert_eq!(lane6.len(), 6, "6 > 6 is false → keep all");
    assert!(lane6.iter().all(|r| matches!(r, AgentRender::Kept(_))));

    let seven: Vec<&str> = vec![
        "let me a", "let me b", "let me c", "let me d", "let me e", "let me f", "let me g",
    ];
    let t7 = mk_turn_agents(0, Some("ask"), &seven, 0);
    let lane7 = select_agent_messages(&t7, &c);
    // First kept (privilege) + last kept + the 5 middles collapse into one placeholder.
    let kept = lane7
        .iter()
        .filter(|r| matches!(r, AgentRender::Kept(_)))
        .count();
    let phs = lane7
        .iter()
        .filter(|r| matches!(r, AgentRender::Placeholder(_)))
        .count();
    assert_eq!(kept, 2, "first + last kept under filtering");
    assert_eq!(
        phs, 1,
        "the 5 middle declarations collapse into one placeholder"
    );
}

#[test]
fn eot_only_selection_is_byte_identical_to_single_eot_render() {
    // GOLDEN non-breaking: a multi-agent turn rendered under EotOnly emits EXACTLY the
    // single-EOT text (header + body of the last message), no placeholder, same cost.
    let t = mk_turn_agents(
        3,
        Some("the ask"),
        &["let me look", "found 12 things", "the final reply"],
        0,
    );
    // EotOnly render.
    let mut eot_lines: Vec<String> = Vec::new();
    render_turn_text(&t, SelSides::AssistantOnly, &cfg(), None, &mut |s| {
        eot_lines.push(s)
    });
    // The hand-rolled single-EOT render of the last message only.
    let last = t.assistant_eot().unwrap();
    let mut single: Vec<String> = Vec::new();
    emit_unit_text(last, None, &mut |s| single.push(s));
    assert_eq!(eot_lines, single, "EotOnly == single-EOT render");
    // And the cost matches the single-unit cost (no placeholder, no extra agents).
    assert_eq!(
        turn_cost(&t, SelSides::AssistantOnly, &cfg()),
        unit_cost(last)
    );
}
