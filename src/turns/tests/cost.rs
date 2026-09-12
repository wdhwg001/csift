//! Character-cost accounting: unit, marker, banner, and turn costs; render floors.

use super::*;

#[test]
fn marker_cost_zero_for_no_tool_calls() {
    assert_eq!(marker_cost(0), 0);
    // "  [3 tool calls]\n" = 2 + 1 + 1 + 12 + 1 = count chars.
    let expect = "  [3 tool calls]\n".chars().count();
    assert_eq!(marker_cost(3), expect);
    // A big count costs more.
    assert!(marker_cost(231) > marker_cost(3));
}

#[test]
fn unit_cost_charges_real_header_line_plus_newlines() {
    // The cost is the EXACT rendered header line + its newline + the rendered body + its
    // newline - the SAME chars the renderer emits, so summed cost == summed emitted chars.
    // This is the core of the overshoot fix: the header is the true timestamp-dependent
    // line, NOT a flat-24 guess (which undercharged every unit by ~47 chars).
    let small = unit(Role::User, 1, "hi", 0);
    let hdr = unit_header_line(&small).chars().count();
    assert_eq!(unit_cost(&small), hdr + NEWLINE_COST + 2 + NEWLINE_COST);
    // The real header is far longer than the old flat 24 (glyph + L + role + the full
    // `YYYY-MM-DD HH:MM:SS TZ (RAW_UTC)` timestamp expansion) - proving the old undercharge.
    assert!(
        hdr > 24,
        "real header line is {hdr} chars, the removed flat HEADER_COST=24 undercharged it"
    );

    // A huge user unit costs header + newline + 600 (cap body) + newline.
    let big = unit(Role::User, 1, &"z".repeat(5000), 2);
    let r = render_unit_body(&big, None);
    let big_hdr = unit_header_line(&big).chars().count();
    assert_eq!(
        unit_cost(&big),
        big_hdr + NEWLINE_COST + r.body.chars().count() + NEWLINE_COST
    );
    assert!(
        r.body.chars().count() < 700,
        "body is capped near 600 (cap + the elision marker scaffold)"
    );
}

#[test]
fn unit_cost_equals_real_emitted_chars_for_the_unit() {
    // Falsifiable: render the unit to TEXT exactly as the renderer does (header line +
    // body line, each + '\n') and assert the emitted char count == unit_cost. If this ever
    // drifts the budget accounting is lying again.
    for (role, text, nl) in [
        (Role::User, "café🛠 a question".to_string(), 0usize),
        (Role::Assistant, "z".repeat(4000), 9usize),
    ] {
        let u = unit(role, 42, &text, nl);
        let mut emitted = String::new();
        emit_unit_text(&u, None, &mut |s| {
            emitted.push_str(&s);
            emitted.push('\n');
        });
        assert_eq!(
            emitted.chars().count(),
            unit_cost(&u),
            "emitted text {:?} chars != unit_cost {} for {:?}",
            emitted.chars().count(),
            unit_cost(&u),
            role
        );
    }
}

#[test]
fn banner_cost_equals_real_emitted_banner_chars() {
    // The boundary banner the renderer emits (+ its '\n') must equal the charged cost.
    let line = boundary_banner_line(34097, None);
    assert_eq!(
        banner_cost(34097, None),
        line.chars().count() + NEWLINE_COST
    );
    assert!(line.contains("compaction boundary"));
    assert!(line.contains("L34097"));
    // C-33: the same equality must hold for a summarize-mode banner, whose extra segment
    // widens the line - a reservation computed without the mode would under-charge and the
    // budget would overrun by exactly that segment.
    for mode in [
        SummarizeMode::Compact,
        SummarizeMode::FromHere,
        SummarizeMode::UpToHere,
    ] {
        let l = boundary_banner_line(34097, Some(mode));
        assert_eq!(
            banner_cost(34097, Some(mode)),
            l.chars().count() + NEWLINE_COST,
            "charged cost must equal the emitted banner for {mode:?}"
        );
    }
    // A plain compaction's banner is byte-identical to the unpaired one; the two summarize
    // modes are strictly wider and differ from each other.
    assert_eq!(
        boundary_banner_line(34097, Some(SummarizeMode::Compact)),
        line
    );
    let up = boundary_banner_line(34097, Some(SummarizeMode::UpToHere));
    let from = boundary_banner_line(34097, Some(SummarizeMode::FromHere));
    assert!(up.contains("summarize up_to") && from.contains("summarize from"));
    assert!(up.chars().count() > line.chars().count() && up != from);
}

#[test]
fn cumulative_banner_cost_is_monotone_and_zero_at_depth_zero() {
    let summaries = vec![
        SummaryInfo {
            line_no: 100,
            fingerprints: vec![],
            body_chars: 10,
            mode: None,
        },
        SummaryInfo {
            line_no: 500,
            fingerprints: vec![],
            body_chars: 10,
            mode: None,
        },
        SummaryInfo {
            line_no: 900,
            fingerprints: vec![],
            body_chars: 10,
            mode: None,
        },
    ];
    assert_eq!(cumulative_banner_cost(&summaries, 0), 0);
    let d1 = cumulative_banner_cost(&summaries, 1);
    let d2 = cumulative_banner_cost(&summaries, 2);
    let d3 = cumulative_banner_cost(&summaries, 3);
    assert!(d1 > 0 && d2 > d1 && d3 > d2, "monotone increasing in depth");
    // Depth 1 charges the NEWEST (max line_no = 900) banner only.
    assert_eq!(d1, banner_cost(900, None));
    // Depth beyond the summary count saturates at "all banners".
    assert_eq!(cumulative_banner_cost(&summaries, 99), d3);
}

// ── Turn cost + selection sides ──

#[test]
fn turn_cost_both_charges_marker_single_side_does_not() {
    let t = mk_turn(0, Some("ask"), Some("reply"), 3, 0);
    let both = turn_cost(&t, SelSides::Both, &cfg());
    let user_only = turn_cost(&t, SelSides::UserOnly, &cfg());
    let asst_only = turn_cost(&t, SelSides::AssistantOnly, &cfg());
    // Both = user + marker + assistant.
    assert_eq!(both, user_only + marker_cost(3) + (asst_only));
    // A zero-tool turn charges no marker on the both selection.
    let t0 = mk_turn(0, Some("ask"), Some("reply"), 0, 0);
    assert_eq!(
        turn_cost(&t0, SelSides::Both, &cfg()),
        turn_cost(&t0, SelSides::UserOnly, &cfg())
            + turn_cost(&t0, SelSides::AssistantOnly, &cfg())
    );
}

#[test]
fn turn_cost_assistant_only_costs_only_assistant_side() {
    let t = mk_turn(0, Some("ask"), Some("reply"), 5, 0);
    // AssistantOnly cost == just the assistant unit cost (no user, no marker).
    let a = t.assistant_eot().unwrap();
    assert_eq!(turn_cost(&t, SelSides::AssistantOnly, &cfg()), unit_cost(a));
}

#[test]
fn turn_cost_user_only_costs_only_user_side() {
    let t = mk_turn(0, Some("the ask"), Some("the reply"), 5, 0);
    let u = t.user.as_ref().unwrap();
    // UserOnly: just the user unit cost (no marker, no assistant).
    assert_eq!(turn_cost(&t, SelSides::UserOnly, &cfg()), unit_cost(u));
}

#[test]
fn turn_cost_partial_turns_skip_absent_sides() {
    // turn_cost over a turn MISSING a side → the `if let Some` None arms in turn_cost
    // (L616/L626): a user-only turn costs only the user; an assistant-only only the asst.
    let user_only = mk_turn(0, Some("just a user ask"), None, 3, 0);
    // Both selection on a user-only turn: marker is charged (tool_calls>0) but no asst.
    let u = user_only.user.as_ref().unwrap();
    assert_eq!(
        turn_cost(&user_only, SelSides::Both, &cfg()),
        unit_cost(u) + marker_cost(3)
    );
    let asst_only = mk_turn(0, None, Some("just a reply"), 0, 0);
    let a = asst_only.assistant_eot().unwrap();
    assert_eq!(turn_cost(&asst_only, SelSides::Both, &cfg()), unit_cost(a));
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
        assert_eq!(
            emitted.chars().count(),
            assistant_lane_cost(&t, &c),
            "emitted lane chars != assistant_lane_cost under mode {:?}",
            c.mode
        );
    }
}

#[test]
fn cost_invariant_holds_when_a_fold_carries_preview_lines() {
    // The DEFAULT `longest` mode collapses a signal-free middle under `rich_min_chars` (280),
    // so a 240-char middle is folded AND long enough to earn a preview line: the invariant has
    // to hold with the extra lines charged, not only with a bare marker.
    let long_middle = body_chars("MIDDLE", 240);
    // The last message must be the LONGEST (else the 240-char middle wins that privilege and
    // is kept instead of folded).
    let longest_last = body_chars("ANSWER", 400);
    let t = mk_turn_agents(
        0,
        Some("ask"),
        &["a short opener", &long_middle, &longest_last],
        0,
    );
    let c = longest_cfg();
    let lane = select_agent_messages(&t, &c);
    let previews: usize = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Placeholder(s) => Some(s.previews.len()),
            _ => None,
        })
        .sum();
    assert!(
        previews >= 1,
        "the 240-char middle earns a preview: {lane:?}"
    );
    let mut emitted = String::new();
    for entry in lane {
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

#[test]
fn min_render_chars_is_a_lower_bound_or_none_for_empty() {
    // A non-empty session reports a positive lower bound; an empty one reports None.
    let full = scan_with_turns(
        vec![mk_turn(0, Some("ask"), Some("reply"), 1, 0)],
        Vec::new(),
    );
    let min = min_render_chars(&full, 40000, &cfg());
    assert!(min.is_some_and(|m| m > 0));
    let mut empty = full;
    empty.turns.clear();
    assert_eq!(min_render_chars(&empty, 40000, &cfg()), None);

    // A user-ONLY turn (no agents) and an agent-ONLY turn (no user) each take only their
    // respective fold arm of the cheapest-side computation; both still yield a positive bound.
    let user_only = scan_with_turns(vec![mk_turn(0, Some("ask"), None, 0, 0)], Vec::new());
    assert!(min_render_chars(&user_only, 40000, &cfg()).is_some_and(|m| m > 0));
    let agent_only = scan_with_turns(vec![mk_turn(0, None, Some("reply"), 0, 0)], Vec::new());
    assert!(min_render_chars(&agent_only, 40000, &cfg()).is_some_and(|m| m > 0));
}

#[test]
fn min_render_chars_none_when_turn_has_no_sides() {
    // A turn with NEITHER a user side NOR any agent message contributes `usize::MAX` to the
    // min fold; with that being the only turn, `min_render_chars` returns None (the
    // `cheapest == usize::MAX` guard), distinct from a positive lower bound.
    let sideless = TurnSlice {
        turn_index: 0,
        user: None,
        tool_calls: 0,
        image_ids: Vec::new(),
        agents: Vec::new(),
        compactions_before: 0,
        is_automation: false,
        automation: None,
    };
    let sr = scan_with_turns(vec![sideless], Vec::new());
    assert_eq!(min_render_chars(&sr, 40000, &cfg()), None);
}

#[test]
fn the_image_marker_line_and_the_cost_reserved_for_it_agree_to_the_character() {
    // The renderer emits this line and the budget reserves for it; the two are the same
    // function plus one newline, so a drifting reservation shows up as a plan that either
    // overruns the budget or leaves a line's worth of it unused on every turn with images.
    let one = vec!["img-a".to_string()];
    let two = vec!["img-a".to_string(), "img-b".to_string()];
    assert_eq!(image_marker_line(&one), "  [1 image: img-a]");
    assert_eq!(
        image_marker_line(&two),
        "  [2 images: img-a, img-b]",
        "the noun follows the count"
    );
    assert_eq!(
        image_marker_cost(&one),
        image_marker_line(&one).chars().count() + 1,
        "the rendered line plus its newline"
    );
    assert_eq!(image_marker_cost(&one), 19);
    assert_eq!(image_marker_cost(&two), 27);
    assert_eq!(image_marker_cost(&[]), 0, "no images, no line, no cost");
}
