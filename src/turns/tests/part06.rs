use super::*;

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

#[test]
fn cost_invariant_holds_with_placeholders_under_rich_and_all() {
    // Falsifiable: render a turn's assistant lane to text under rich (with a placeholder)
    // and under all, count emitted chars, assert == summed(unit_cost(kept) +
    // placeholder_cost(span)). The summed-cost == summed-emitted invariant extended.
    let t = mk_turn_agents(
        0,
        Some("ask"),
        &[
            "found the cause",
            "let me a",
            "let me b",
            "let me c",
            "let me d",
            "let me e",
            "let me f",
            "12 passed in src/x.rs:9",
            "done",
        ],
        0,
    );
    for c in [rich_cfg(), {
        let mut c = rich_cfg();
        c.mode = AgentMsgMode::All;
        c
    }] {
        // Emit the lane to text exactly as the renderer does (+ newline per line).
        let mut emitted = String::new();
        for entry in select_agent_messages(&t, &c) {
            match entry {
                AgentRender::Kept(a) => emit_unit_text(&a.unit, None, &mut |s| {
                    emitted.push_str(&s);
                    emitted.push('\n');
                }),
                AgentRender::Placeholder(s) => {
                    emitted.push_str(&agent_placeholder_line(&s));
                    emitted.push('\n');
                }
            }
        }
        assert_eq!(
            emitted.chars().count(),
            assistant_lane_cost(&t, &c),
            "emitted lane chars != assistant_lane_cost under mode {:?}",
            c.mode
        );
    }
}

#[test]
fn richness_cfg_default_is_longest() {
    // The default config is Longest — keep the longest agent message + the first-if-
    // substantive + the rich middles (NOT the old `agents.last()` single-EOT default).
    let d = RichnessCfg::default();
    assert_eq!(d.mode, AgentMsgMode::Longest);
    assert_eq!(d.run_threshold, 6);
    assert_eq!(d.rich_min_chars, 280);
    assert_eq!(d.declaration_max_chars, 200);
    assert!(d.keep_first);
}

#[test]
fn dedup_flagged_middle_still_richness_gated() {
    // A dedup-flagged (also_in_summary) middle is subject to the SAME richness rule:
    // rich+flagged → kept (carrying the flag); not-rich+flagged → collapsed. The LAST
    // unit is never collapsed by richness (demote-not-drop still protects it).
    let mut agents = vec![
        agent_msg(10, "the opening plan", 0, 0),
        agent_msg(20, "found 12 in src/x.rs:9", 0, 0), // rich middle
        agent_msg(30, "let me a", 0, 0),
        agent_msg(40, "let me b", 0, 0),
        agent_msg(50, "let me c", 0, 0),
        agent_msg(60, "let me d", 0, 0),
        agent_msg(70, "let me e", 0, 0),
        agent_msg(80, "final", 0, 0),
    ];
    // Flag the rich middle as also_in_summary — richness must still keep it.
    agents[1].unit.also_in_summary = true;
    assign_positions(&mut agents);
    let t = TurnSlice {
        turn_index: 0,
        user: None,
        tool_calls: 0,
        image_ids: Vec::new(),
        agents,
        compactions_before: 0,
        is_automation: false,
        automation: None,
    };
    let lane = select_agent_messages(&t, &rich_cfg());
    let kept_flagged = lane
        .iter()
        .any(|r| matches!(r, AgentRender::Kept(a) if a.unit.also_in_summary));
    assert!(
        kept_flagged,
        "a rich dedup-flagged middle is kept carrying its flag"
    );
}

// ── Fan-out scope summary + top-level/subagent id classification ──

#[test]
fn shared_uuid_validator_distinguishes_uuid_from_bare_hex() {
    // turns no longer rolls its own `is_top_level_session_id`; it now discriminates via the
    // authoritative path-derived `is_subagent` field, and any id-SHAPE check reuses the ONE
    // canonical validator `path::is_session_uuid` (the DRY fix). Spot-check that validator's
    // contract holds for the id forms turns' tests construct.
    assert!(crate::path::is_session_uuid(
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
    ));
    assert!(!crate::path::is_session_uuid("a00ea52f023afd9ce"));
    // Wrong group shape / non-hex → not a uuid.
    assert!(!crate::path::is_session_uuid("0a1b2c3d-4e5f-4a6b-8c7d"));
    assert!(!crate::path::is_session_uuid(
        "zzzzzzzz-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
    ));
}

/// A ScanResult with a chosen session id + a single trivial round-trip turn (so its plan
/// is non-empty under any budget).
fn scan_named(session_id: &str) -> ScanResult {
    // Keep the id-domain fields self-consistent with the chosen id form: a non-uuid (bare
    // hex) id is a subagent transcript. parent_session_id is the id itself for a top-level
    // session (a bare-hex test id has no real parent path, so it points at itself here).
    let is_subagent = !crate::path::is_session_uuid(session_id);
    ScanResult {
        session_id: session_id.to_string(),
        is_subagent,
        parent_session_id: session_id.to_string(),
        turns: vec![mk_turn(0, Some("ask"), Some("reply"), 1, 0)],
        summaries: Vec::new(),
        skipped_lines: 0,
    }
}

#[test]
fn scope_summary_counts_top_level_and_subagents() {
    // One top-level uuid + two bare-hex subagents, all rendering.
    let sessions = vec![
        scan_named("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"),
        scan_named("a00ea52f023afd9ce"),
        scan_named("a01086fb826e1ab0e"),
    ];
    let plans: Vec<SessionPlan> = sessions
        .iter()
        .map(|sr| plan_session(sr, 8000, 0.5, 0, &cfg()))
        .collect();
    let sc = scope_summary(&sessions, &plans);
    assert_eq!(sc.in_scope, 3);
    assert_eq!(sc.in_scope_top, 1);
    assert_eq!(sc.in_scope_sub, 2);
    assert_eq!(sc.rendered, 3);
}

#[test]
fn scope_summary_reports_true_scope_not_rendered() {
    // CRITICAL: a session whose plan selects nothing (budget too small) is STILL counted in
    // the TRUE scope and its top-level/subagent split — only `rendered` shrinks. This is what
    // stops a targeted top-level uuid from reading as `0 top-level` and a budget knob from
    // silently rewriting "scope".
    let mut empty = scan_named("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d");
    empty.turns.clear(); // its plan will be empty (nothing fits)
    let rendered_sub = scan_named("a00ea52f023afd9ce");
    let sessions = vec![empty, rendered_sub];
    let plans: Vec<SessionPlan> = sessions
        .iter()
        .map(|sr| plan_session(sr, 8000, 0.5, 0, &cfg()))
        .collect();
    let sc = scope_summary(&sessions, &plans);
    // Both sessions are in scope; the top-level one is counted even though it rendered nothing.
    assert_eq!(sc.in_scope, 2);
    assert_eq!(
        sc.in_scope_top, 1,
        "targeted top-level must NOT read as 0 top-level"
    );
    assert_eq!(sc.in_scope_sub, 1);
    assert_eq!(sc.rendered, 1, "only the subagent fit the budget");
}
