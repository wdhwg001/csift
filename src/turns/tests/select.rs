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
            "found the root cause already",     // first - rich → kept
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
    // Stage 2 char-cap is the existing render_unit_body path - verbatim under the cap here.
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

// ── Same-prefix re-send (v0.12.3) ──

/// Two agent messages where the LATER carries the earlier's whole body plus an addendum -
/// both over `DEDUP_PREFIX` chars, so the containment rule fires.
fn resend_pair_turn(addendum: &str) -> TurnSlice {
    let first = format!(
        "the committed answer runs long enough to clear the eighty-char fingerprint gate {}",
        "and keeps going for a while"
    );
    let second = format!("{first}{addendum}");
    mk_turn_agents(0, Some("ask"), &[&first, &second], 0)
}

#[test]
fn a_later_message_carrying_the_whole_earlier_body_supersedes_it() {
    let t = resend_pair_turn(" plus the addendum the hook asked for");
    let lane = select_agent_messages(&t, &{
        let mut c = longest_cfg();
        c.mode = AgentMsgMode::All;
        c
    });
    assert_eq!(lane.len(), 2, "both messages still occupy a lane slot");
    let (msg, by_line) = match &lane[0] {
        AgentRender::Superseded { msg, by_line } => (msg, *by_line),
        other => panic!("the earlier message must be superseded, got {other:?}"),
    };
    assert_eq!(msg.unit.line_no, t.agents[0].unit.line_no);
    assert_eq!(
        by_line, t.agents[1].unit.line_no,
        "the LATER message survives"
    );
    assert!(
        matches!(&lane[1], AgentRender::Kept(a) if a.unit.line_no == t.agents[1].unit.line_no),
        "the survivor is rendered in full"
    );
    // The marker names the survivor's line and the suppressed body's own char count, and the
    // cost charged is that one line - not the body.
    let line = superseded_line(msg, by_line);
    assert_eq!(
        line,
        format!(
            "△ L{}  [superseded by the same-prefix re-send at L{}, {} chars]",
            msg.unit.line_no, by_line, msg.unit.full_chars
        )
    );
    assert_eq!(superseded_cost(msg, by_line), line.chars().count() + 1);
    assert!(
        superseded_cost(msg, by_line) < unit_cost(&msg.unit),
        "the marker must be cheaper than the body it replaces"
    );
}

#[test]
fn a_shared_prefix_without_containment_is_not_a_resend() {
    // The two bodies share far more than 80 chars and then DIVERGE, so the later does NOT
    // carry the earlier: both stay kept. This is the measured majority case (52 of 53 corpus
    // pairs), and folding it would drop prose the survivor never said.
    let head =
        "the committed answer runs long enough to clear the eighty-char fingerprint gate here";
    let first = format!("{head} and then says ALPHA about the first branch");
    let second = format!("{head} and then says BETA about the second branch");
    let t = mk_turn_agents(0, Some("ask"), &[&first, &second], 0);
    let lane = select_agent_messages(&t, &{
        let mut c = longest_cfg();
        c.mode = AgentMsgMode::All;
        c
    });
    assert!(
        lane.iter().all(|r| matches!(r, AgentRender::Kept(_))),
        "a divergent pair is not a re-send: {lane:?}"
    );
}

#[test]
fn a_body_under_the_fingerprint_length_is_never_a_resend() {
    // Two identical SHORT messages: their first-80-chars are equal only because both bodies
    // are shorter than 80, where the fingerprint is not the strict discriminator its own doc
    // claims - and the marker would cost more than the body. Both stay kept.
    let t = mk_turn_agents(0, Some("ask"), &["Done.", "Done."], 0);
    let lane = select_agent_messages(&t, &{
        let mut c = longest_cfg();
        c.mode = AgentMsgMode::All;
        c
    });
    assert_eq!(lane.len(), 2);
    assert!(
        lane.iter().all(|r| matches!(r, AgentRender::Kept(_))),
        "{lane:?}"
    );
    // The gate is the head helper itself: under DEDUP_PREFIX chars it yields nothing.
    assert!(resend_head(&t.agents[0].unit).is_none());
    let long = unit(Role::Assistant, 1, &"z".repeat(DEDUP_PREFIX), 0);
    assert_eq!(
        resend_head(&long).map(|h| h.chars().count()),
        Some(DEDUP_PREFIX),
        "exactly at the threshold the head is the full fingerprint width"
    );
}

#[test]
fn a_resend_whose_survivor_was_folded_keeps_both_bodies_out_of_one_marker() {
    // The survivor must itself be KEPT: if the later message is inside a placeholder, marking
    // the earlier would hide BOTH bodies behind markers that point at each other. Under
    // `eot-only` only the LAST message is in the lane, so a two-entry group can never form.
    let t = resend_pair_turn(" plus an addendum");
    let lane = select_agent_messages(&t, &cfg());
    assert_eq!(lane.len(), 1);
    assert!(matches!(lane[0], AgentRender::Kept(_)), "{lane:?}");
}

#[test]
fn three_same_prefix_messages_supersede_onto_the_last() {
    // A turn that re-sent twice: both earlier copies point at the LATEST container.
    let head =
        "the committed answer runs long enough to clear the eighty-char fingerprint gate here";
    let a = head.to_string();
    let b = format!("{head} plus one");
    let c = format!("{head} plus one plus two");
    let t = mk_turn_agents(0, Some("ask"), &[&a, &b, &c], 0);
    let lane = select_agent_messages(&t, &{
        let mut cfg = longest_cfg();
        cfg.mode = AgentMsgMode::All;
        cfg
    });
    let last_line = t.agents[2].unit.line_no;
    let marked: Vec<usize> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Superseded { by_line, .. } => Some(*by_line),
            _ => None,
        })
        .collect();
    assert_eq!(marked, vec![last_line, last_line], "{lane:?}");
}

#[test]
fn the_lane_cost_equals_what_a_superseded_lane_emits() {
    // summed-cost == summed-emitted with a marker in the lane.
    let t = resend_pair_turn(" plus the addendum");
    let c = {
        let mut c = longest_cfg();
        c.mode = AgentMsgMode::All;
        c
    };
    let mut emitted = String::new();
    for entry in select_agent_messages(&t, &c) {
        match entry {
            AgentRender::Kept(a) => emit_unit_text(&a.unit, None, &mut |s| {
                emitted.push_str(&s);
                emitted.push('\n');
            }),
            AgentRender::Placeholder(s) => {
                for line in agent_placeholder_lines(&s) {
                    emitted.push_str(&line);
                    emitted.push('\n');
                }
            }
            AgentRender::Superseded { msg, by_line } => {
                emitted.push_str(&superseded_line(msg, by_line));
                emitted.push('\n');
            }
        }
    }
    assert_eq!(emitted.chars().count(), assistant_lane_cost(&t, &c));
}
