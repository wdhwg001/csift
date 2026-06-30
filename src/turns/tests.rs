//! Unit tests for the `turns` engine over small, locale-neutral fixtures.
//!
//! Charset discipline (CLAUDE.md / design §0): fixture strings use accented-Latin +
//! emoji multi-byte tokens only (`café🛠`), the house fixture style. The
//! end-to-end feature exercise on a real-shaped, multi-summary transcript lives in
//! `tests/cli_integration.rs`.

use super::*;

/// Build a [`TurnUnit`] for a role from a plain string (no record), for cost/ellipsis
/// tests. `orig_newlines` lets a test assert the `L lines elided` note.
fn unit(role: Role, line_no: usize, text: &str, orig_newlines: usize) -> TurnUnit {
    TurnUnit {
        line_no,
        role,
        full_chars: text.chars().count(),
        text: text.to_string(),
        orig_newlines,
        ts_utc: Some("2026-06-07T05:00:00.000Z".to_string()),
        also_in_summary: false,
        from_sidecar: false,
        inbound: None,
    }
}

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
    // newline — the SAME chars the renderer emits, so summed cost == summed emitted chars.
    // This is the core of the overshoot fix: the header is the true timestamp-dependent
    // line, NOT a flat-24 guess (which undercharged every unit by ~47 chars).
    let small = unit(Role::User, 1, "hi", 0);
    let hdr = unit_header_line(&small).chars().count();
    assert_eq!(unit_cost(&small), hdr + NEWLINE_COST + 2 + NEWLINE_COST);
    // The real header is far longer than the old flat 24 (glyph + L + role + the full
    // `YYYY-MM-DD HH:MM:SS TZ (RAW_UTC)` timestamp expansion) — proving the old undercharge.
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
    let line = boundary_banner_line(34097);
    assert_eq!(banner_cost(34097), line.chars().count() + NEWLINE_COST);
    assert!(line.contains("compaction boundary"));
    assert!(line.contains("L34097"));
}

#[test]
fn cumulative_banner_cost_is_monotone_and_zero_at_depth_zero() {
    let summaries = vec![
        SummaryInfo {
            line_no: 100,
            fingerprints: vec![],
            body_chars: 10,
        },
        SummaryInfo {
            line_no: 500,
            fingerprints: vec![],
            body_chars: 10,
        },
        SummaryInfo {
            line_no: 900,
            fingerprints: vec![],
            body_chars: 10,
        },
    ];
    assert_eq!(cumulative_banner_cost(&summaries, 0), 0);
    let d1 = cumulative_banner_cost(&summaries, 1);
    let d2 = cumulative_banner_cost(&summaries, 2);
    let d3 = cumulative_banner_cost(&summaries, 3);
    assert!(d1 > 0 && d2 > d1 && d3 > d2, "monotone increasing in depth");
    // Depth 1 charges the NEWEST (max line_no = 900) banner only.
    assert_eq!(d1, banner_cost(900));
    // Depth beyond the summary count saturates at "all banners".
    assert_eq!(cumulative_banner_cost(&summaries, 99), d3);
}

// ── Fingerprint / dedup ──

#[test]
fn fingerprint_normalizes_lowercases_and_caps_at_80() {
    let fp = fingerprint("  Hello   WORLD café  ");
    assert_eq!(fp, "hello world café");
    let long = "a".repeat(200);
    assert_eq!(fingerprint(&long).chars().count(), 80);
    assert_eq!(fingerprint(""), "");
    assert_eq!(fingerprint("   "), "");
}

#[test]
fn quoted_inner_extracts_first_quoted_run() {
    assert_eq!(
        quoted_inner(r#"- "initial greeting about café" (note)"#).as_deref(),
        Some("initial greeting about café")
    );
    assert_eq!(quoted_inner("no quotes here"), None);
    assert_eq!(quoted_inner(r#"one " only"#), None); // unmatched quote
}

#[test]
fn summary_fingerprints_pulls_bullets_and_quotes() {
    let body = "\
6. All user messages:
   - \"the very first ask about the carry logic\"
   - \"second ask: explain the panic path now please\"
9. Optional Next Step:
   The assistant said \"I will run the budget walk and report counts\".";
    let fps = summary_fingerprints(body);
    assert!(fps.iter().any(|f| f.starts_with("the very first ask")));
    assert!(fps.iter().any(|f| f.starts_with("second ask")));
    assert!(fps.iter().any(|f| f.contains("run the budget walk")));
}

#[test]
fn unit_matches_summary_prefix_either_direction() {
    let summary_fps = vec![fingerprint(
        "the very first ask about the carry logic in detail",
    )];
    // A unit clipped shorter than the summary bullet still matches (prefix).
    let short = unit(Role::User, 1, "the very first ask", 0);
    assert!(unit_matches_summary(&short, &summary_fps));
    // A unit longer than the bullet (summary is the prefix) also matches.
    let long = unit(
        Role::User,
        1,
        "the very first ask about the carry logic in detail, and then much more text",
        0,
    );
    assert!(unit_matches_summary(&long, &summary_fps));
    // An unrelated unit does not match.
    let other = unit(Role::User, 1, "a completely different question entirely", 0);
    assert!(!unit_matches_summary(&other, &summary_fps));
    // An empty-fingerprint unit never matches.
    let empty = unit(Role::User, 1, "   ", 0);
    assert!(!unit_matches_summary(&empty, &summary_fps));
}

// ── Turn cost + selection sides ──

/// An EXPLICIT `EotOnly` (single-EOT) richness config. Every cost / plan / render test that
/// predates the multi-agent-message model runs against this so its assertions stay byte-
/// identical to the single-EOT behavior, INDEPENDENT of what the crate default mode is
/// (the default is now `Longest` — see [`longest_cfg`] / `richness_cfg_default_is_longest`).
fn cfg() -> RichnessCfg {
    RichnessCfg {
        mode: AgentMsgMode::EotOnly,
        ..RichnessCfg::default()
    }
}

/// The crate-DEFAULT richness config — `Longest` mode (keep the longest agent message + the
/// first-if-substantive + the rich middles). Drives the default-selection tests.
fn longest_cfg() -> RichnessCfg {
    RichnessCfg::default()
}

/// A `rich`-mode config with the documented Rich-mode defaults (threshold 6, rich-min
/// 280, declaration-max 200, keep-first true) for the multi-agent-message tests.
fn rich_cfg() -> RichnessCfg {
    RichnessCfg {
        mode: AgentMsgMode::Rich,
        ..RichnessCfg::default()
    }
}

/// Build a single [`AgentMsg`] wrapping a unit, with the given per-message attribution.
fn agent_msg(line_no: usize, text: &str, tools: usize, failed: usize) -> AgentMsg {
    AgentMsg {
        unit: unit(Role::Assistant, line_no, text, 0),
        pos: AgentPos::Last, // reassigned by the helper that assembles the run
        preceding_tool_calls: tools,
        preceding_failed: failed,
    }
}

/// Assign First/Middle/Last positions over an agent run (mirrors `build`).
fn assign_positions(agents: &mut [AgentMsg]) {
    let last = agents.len().saturating_sub(1);
    for (i, a) in agents.iter_mut().enumerate() {
        a.pos = if i == last {
            AgentPos::Last
        } else if i == 0 {
            AgentPos::First
        } else {
            AgentPos::Middle
        };
    }
}

fn mk_turn(
    turn_index: usize,
    user: Option<&str>,
    asst: Option<&str>,
    tools: usize,
    comp: usize,
) -> TurnSlice {
    let mut agents: Vec<AgentMsg> = asst
        .map(|t| vec![agent_msg(turn_index * 10 + 5, t, 0, 0)])
        .unwrap_or_default();
    assign_positions(&mut agents);
    TurnSlice {
        turn_index,
        user: user.map(|t| unit(Role::User, turn_index * 10 + 1, t, 0)),
        tool_calls: tools,
        image_ids: Vec::new(),
        agents,
        compactions_before: comp,
        is_automation: false,
        automation: None,
    }
}

/// Build a turn whose agent run is the ORDERED list of `(line_no, text)` agent messages,
/// each with optional per-message tool/failed attribution defaulting to 0. For the
/// richness selection tests.
fn mk_turn_agents(
    turn_index: usize,
    user: Option<&str>,
    agent_texts: &[&str],
    comp: usize,
) -> TurnSlice {
    let mut agents: Vec<AgentMsg> = agent_texts
        .iter()
        .enumerate()
        .map(|(i, t)| agent_msg(turn_index * 100 + i + 5, t, 1, 0))
        .collect();
    assign_positions(&mut agents);
    TurnSlice {
        turn_index,
        user: user.map(|t| unit(Role::User, turn_index * 100 + 1, t, 0)),
        tool_calls: agents.len(),
        image_ids: Vec::new(),
        agents,
        compactions_before: comp,
        is_automation: false,
        automation: None,
    }
}

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
fn is_round_trip_requires_both_sides() {
    assert!(mk_turn(0, Some("a"), Some("b"), 0, 0).is_round_trip());
    assert!(!mk_turn(0, Some("a"), None, 0, 0).is_round_trip());
    assert!(!mk_turn(0, None, Some("b"), 0, 0).is_round_trip());
}

#[test]
fn is_human_round_trip_excludes_automation_notifications() {
    // A HUMAN round-trip qualifies for the HARD FLOOR; an automation-pulse round-trip is a
    // structural round-trip (is_round_trip true) but NOT a human one — so it is excluded
    // from the protected `--round-trip-fraction` lane (the budget-floor consumer).
    let human = mk_turn(0, Some("a"), Some("b"), 0, 0);
    assert!(human.is_round_trip() && human.is_human_round_trip());
    let mut pulse = mk_turn(
        1,
        Some("<task-notification>…</task-notification>"),
        Some("ack"),
        0,
        0,
    );
    pulse.is_automation = true;
    assert!(
        pulse.is_round_trip(),
        "an automation pulse + ack is STRUCTURALLY a round-trip"
    );
    assert!(
        !pulse.is_human_round_trip(),
        "but it is NOT a human round-trip → excluded from the floor"
    );
}

// ── Budget allocation (the load-bearing 50% floor) ──

/// Build a session of `n` complete round-trips + a trailing assistant-heavy block, to
/// drive the 50%-floor regression: a naive recency walk would starve users.
fn scan_with_turns(turns: Vec<TurnSlice>, summaries: Vec<SummaryInfo>) -> ScanResult {
    ScanResult {
        session_id: "s".to_string(),
        is_subagent: false,
        parent_session_id: "s".to_string(),
        turns,
        summaries,
        skipped_lines: 0,
    }
}

#[test]
fn rt_floor_recovers_a_user_turn_despite_assistant_heavy_tail() {
    // Mirror the pulse finding: an assistant-heavy tail (3 huge assistant-only "turns"
    // near EOF) plus older complete round-trips. Without the 50% floor a recency walk
    // would spend the whole budget on the assistant monologue and recover 0 users.
    let huge = "h".repeat(5000);
    let mut turns = Vec::new();
    // older complete round-trips (turn 0..3)
    for i in 0..4 {
        turns.push(mk_turn(
            i,
            Some(&format!("user ask number {i}")),
            Some(&format!("reply {i}")),
            1,
            0,
        ));
    }
    // assistant-heavy tail: complete turns whose assistant side is enormous (turn 4..7)
    for i in 4..7 {
        turns.push(mk_turn(i, Some(&format!("short {i}")), Some(&huge), 2, 0));
    }
    let sr = scan_with_turns(turns, Vec::new());
    let plan = plan_session(&sr, 8000, 0.5, 0, &cfg());
    let (n_user, _n_asst) = count_sides(&plan, &cfg());
    assert!(
        n_user >= 1,
        "the 50% floor must recover >=1 user turn, got {n_user}"
    );
    // And the rendered chars stay within budget.
    assert!(
        plan.rendered_chars <= 8000,
        "budget respected: {}",
        plan.rendered_chars
    );
}

#[test]
fn budget_respected_and_monotonic() {
    let mut turns = Vec::new();
    for i in 0..20 {
        turns.push(mk_turn(
            i,
            Some(&format!("the user asks question {i} about the carry logic")),
            Some(&format!("the assistant replies in detail to question {i}")),
            i % 3,
            0,
        ));
    }
    let sr = scan_with_turns(turns, Vec::new());
    let big = plan_session(&sr, 40000, 0.5, 0, &cfg());
    let small = plan_session(&sr, 4000, 0.5, 0, &cfg());
    assert!(big.rendered_chars <= 40000);
    assert!(small.rendered_chars <= 4000);
    assert!(small.selected.len() <= big.selected.len());
    // Monotonic: the smaller budget's selected turn-indices are a subset of the bigger.
    let big_set: std::collections::BTreeSet<usize> =
        big.selected.iter().map(|s| s.turn_index).collect();
    for s in &small.selected {
        assert!(
            big_set.contains(&s.turn_index),
            "turn {} in small but not big (recency-first must be stable)",
            s.turn_index
        );
    }
}

#[test]
fn higher_rt_fraction_takes_more_complete_pairs() {
    let mut turns = Vec::new();
    for i in 0..20 {
        turns.push(mk_turn(
            i,
            Some(&format!("user question {i} text here")),
            Some(&format!("assistant answer {i} text here")),
            1,
            0,
        ));
    }
    let sr = scan_with_turns(turns, Vec::new());
    let count_both = |p: &SessionPlan| {
        p.selected
            .iter()
            .filter(|s| matches!(s.sides, SelSides::Both))
            .count()
    };
    let low = plan_session(&sr, 6000, 0.3, 0, &cfg());
    let high = plan_session(&sr, 6000, 0.8, 0, &cfg());
    assert!(
        count_both(&high) >= count_both(&low),
        "higher rt-fraction must not yield fewer complete pairs: {} vs {}",
        count_both(&high),
        count_both(&low)
    );
}

#[test]
fn determinism_identical_plan_for_identical_input() {
    let mut turns = Vec::new();
    for i in 0..12 {
        turns.push(mk_turn(
            i,
            Some(&format!("u{i}")),
            Some(&format!("a{i}")),
            1,
            0,
        ));
    }
    let sr = scan_with_turns(turns, Vec::new());
    let p1 = plan_session(&sr, 5000, 0.5, 0, &cfg());
    let p2 = plan_session(&sr, 5000, 0.5, 0, &cfg());
    let idx = |p: &SessionPlan| p.selected.iter().map(|s| s.turn_index).collect::<Vec<_>>();
    assert_eq!(idx(&p1), idx(&p2));
    assert_eq!(p1.rendered_chars, p2.rendered_chars);
}

// ── Multi-compaction spanning + dedup demotion ──

fn summary(line_no: usize, fps: Vec<&str>, body_chars: usize) -> SummaryInfo {
    SummaryInfo {
        line_no,
        fingerprints: fps.into_iter().map(fingerprint).collect(),
        body_chars,
    }
}

#[test]
fn spanned_boundary_count_is_max_compactions_before() {
    let turns = vec![
        mk_turn(0, Some("a"), Some("b"), 0, 2), // behind 2 summaries
        mk_turn(1, Some("c"), Some("d"), 0, 1), // behind newest summary
        mk_turn(2, Some("e"), Some("f"), 0, 0), // live region
    ];
    let selected = vec![
        Selected {
            turn_index: 0,
            sides: SelSides::Both,
        },
        Selected {
            turn_index: 1,
            sides: SelSides::Both,
        },
        Selected {
            turn_index: 2,
            sides: SelSides::Both,
        },
    ];
    // The oldest selected turn (cb=2) means the ascending render crosses 2 boundaries.
    assert_eq!(spanned_boundary_count(&turns, &selected), 2);
    // Only the live turn selected → 0 boundaries spanned.
    let live_only = vec![Selected {
        turn_index: 2,
        sides: SelSides::Both,
    }];
    assert_eq!(spanned_boundary_count(&turns, &live_only), 0);
    // Empty selection → 0.
    assert_eq!(spanned_boundary_count(&turns, &[]), 0);
}

#[test]
fn dedup_demotes_live_region_match_but_not_pre_boundary() {
    // A live-region (compactions_before==0) user turn whose text matches a summary
    // bullet is flagged + demoted; an identical-text turn BEFORE an older boundary is
    // pure restoration (never deduped).
    let dup_text = "the very first ask about the carry logic";
    let turns = vec![
        mk_turn(0, Some(dup_text), Some("old reply"), 0, 1), // pre-boundary: not deduped
        mk_turn(1, Some(dup_text), Some("live reply"), 0, 0), // live: deduped
    ];
    let sums = vec![summary(900, vec![dup_text], 12000)];
    let sr = scan_with_turns(turns, sums);
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    assert_eq!(
        plan.dedup_demoted, 1,
        "exactly the live-region match is demoted"
    );
    // The pre-boundary identical turn is still selected (not dropped) and NOT flagged.
    assert!(plan.selected.iter().any(|s| s.turn_index == 0));
}

#[test]
fn dedup_demoted_turn_is_selected_after_non_dups_not_dropped() {
    // Even a dedup-flagged turn is still selected (demote, not delete) when budget allows.
    let dup_text = "shared question text that the summary already has verbatim here";
    let turns = vec![
        mk_turn(
            0,
            Some("unique earlier question"),
            Some("earlier reply"),
            0,
            0,
        ),
        mk_turn(1, Some(dup_text), Some("a later reply"), 0, 0),
    ];
    let sums = vec![summary(900, vec![dup_text], 9000)];
    let sr = scan_with_turns(turns, sums);
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    assert!(plan.dedup_demoted >= 1);
    // Both turns selected (generous budget): the dup is demoted, never dropped.
    assert!(plan.selected.iter().any(|s| s.turn_index == 1));
    assert!(plan.selected.iter().any(|s| s.turn_index == 0));
}

#[test]
fn max_compactions_caps_reach() {
    let turns = vec![
        mk_turn(0, Some("a"), Some("b"), 0, 3),
        mk_turn(1, Some("c"), Some("d"), 0, 2),
        mk_turn(2, Some("e"), Some("f"), 0, 1),
        mk_turn(3, Some("g"), Some("h"), 0, 0),
    ];
    let sr = scan_with_turns(turns, Vec::new());
    let capped = plan_session(&sr, 40000, 0.5, 1, &cfg());
    // Only turns with compactions_before <= 1 survive.
    for s in &capped.selected {
        assert!(s.turn_index >= 2, "turn {} beyond cap leaked", s.turn_index);
    }
    assert!(capped.spanned_boundaries <= 1);
}

// ── compact_summary_body / raw_body_newlines via real Record parse ──

fn rec(json: &str) -> Record {
    serde_json::from_str(json).expect("valid record")
}

#[test]
fn compact_summary_body_reads_string_content_only() {
    let s = rec(
        r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"the summary body text"}}"#,
    );
    assert_eq!(
        compact_summary_body(&s).as_deref(),
        Some("the summary body text")
    );
    // A block-bodied (surprise) summary → None, not a guess.
    let blocks = rec(
        r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":[{"type":"text","text":"x"}]}}"#,
    );
    assert!(compact_summary_body(&blocks).is_none());
    // No message → None.
    let bare = rec(r#"{"type":"user","isCompactSummary":true}"#);
    assert!(compact_summary_body(&bare).is_none());
}

#[test]
fn raw_body_newlines_counts_user_and_assistant_bodies() {
    let u = rec(
        r#"{"type":"user","message":{"role":"user","content":"line one\nline two\nline three"}}"#,
    );
    assert_eq!(raw_body_newlines(&u), 2);
    let a = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"first"},{"type":"text","text":"second"}]}}"#,
    );
    // two text blocks joined by \n → 1 newline.
    assert_eq!(raw_body_newlines(&a), 1);
    // No message → 0.
    let bare = rec(r#"{"type":"system","subtype":"x"}"#);
    assert_eq!(raw_body_newlines(&bare), 0);
    // message but no content → 0.
    let nocontent = rec(r#"{"type":"user","message":{"role":"user"}}"#);
    assert_eq!(raw_body_newlines(&nocontent), 0);
}

// ── build(): turn slices + tool-call counts from line-numbered records ──

#[test]
fn build_produces_round_trip_with_tool_count_and_compaction() {
    // turn 0: user, assistant tool_use x2, assistant text. Then a summary. Then turn 1.
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(
                r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"first ask about café"}}"#,
            ),
        ),
        (
            2,
            rec(
                r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{}},{"type":"tool_use","id":"t2","name":"Bash","input":{}}]}}"#,
            ),
        ),
        (
            3,
            rec(
                r#"{"type":"assistant","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"the reply to the first ask"}]}}"#,
            ),
        ),
        (
            4,
            rec(
                r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"6. All user messages:\n   - \"first ask about café\""}}"#,
            ),
        ),
        (
            5,
            rec(
                r#"{"type":"user","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"second ask after compaction"}}"#,
            ),
        ),
        (
            6,
            rec(
                r#"{"type":"assistant","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"the second reply"}]}}"#,
            ),
        ),
    ];
    let (turns, summaries) = build(&records, &[]);
    assert_eq!(turns.len(), 2);
    // turn 0: round-trip, 2 tool calls, before the (one) summary → compactions_before 1.
    assert!(turns[0].is_round_trip());
    assert_eq!(turns[0].tool_calls, 2);
    assert_eq!(turns[0].compactions_before, 1);
    assert_eq!(turns[0].user.as_ref().unwrap().line_no, 1);
    assert_eq!(turns[0].assistant_eot().unwrap().line_no, 3);
    // turn 1: live region, 0 tool calls, after the summary.
    assert_eq!(turns[1].compactions_before, 0);
    assert_eq!(turns[1].tool_calls, 0);
    // one summary captured with a fingerprint of the §6 bullet.
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].line_no, 4);
    assert!(summaries[0]
        .fingerprints
        .iter()
        .any(|f| f.starts_with("first ask about café")));
}

#[test]
fn build_pure_tool_call_turn_has_no_assistant_eot() {
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(r#"{"type":"user","message":{"role":"user","content":"do the thing"}}"#),
        ),
        (
            2,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#,
            ),
        ),
    ];
    let (turns, _s) = build(&records, &[]);
    assert_eq!(turns.len(), 1);
    assert!(turns[0].user.is_some());
    assert!(
        turns[0].assistant_eot().is_none(),
        "pure tool-call turn has no EOT text"
    );
    assert_eq!(turns[0].tool_calls, 1);
    assert!(!turns[0].is_round_trip());
}

// ── boundary banner crossing logic ──

#[test]
fn crossed_summaries_first_turn_crosses_nothing() {
    // 3 summaries at lines 100<200<300 → ranks (newest=1): 300=r1, 200=r2, 100=r3.
    let sums = vec![
        summary(100, vec![], 10),
        summary(200, vec![], 20),
        summary(300, vec![], 30),
    ];
    // FIRST turn (from=None): the cursor seeds at its own depth → crosses NOTHING (a
    // summary older than every selected turn has no restored turn below it).
    assert!(crossed_summaries(&sums, None, 2).is_empty());
    assert!(crossed_summaries(&sums, None, 0).is_empty());
}

#[test]
fn crossed_summaries_step_emits_each_boundary_once() {
    let sums = vec![
        summary(100, vec![], 10),
        summary(200, vec![], 20),
        summary(300, vec![], 30),
    ];
    // Moving from a turn at cb=2 to a turn at cb=0: crosses ranks (0, 2] = r1 (300) and
    // r2 (200), ascending → [200, 300].
    let crossed = crossed_summaries(&sums, Some(2), 0);
    assert_eq!(
        crossed.iter().map(|s| s.line_no).collect::<Vec<_>>(),
        vec![200, 300]
    );
    // Moving from cb=2 to cb=1: crosses only rank 2 (line 200).
    let one = crossed_summaries(&sums, Some(2), 1);
    assert_eq!(one.iter().map(|s| s.line_no).collect::<Vec<_>>(), vec![200]);
    // No movement (same cb) → nothing; moving DEEPER (to >= from) → nothing.
    assert!(crossed_summaries(&sums, Some(1), 1).is_empty());
    assert!(crossed_summaries(&sums, Some(1), 2).is_empty());
}

#[test]
fn boundary_banners_total_equals_max_cb_across_walk() {
    // Full ascending walk: first turn at cb=2 (crosses nothing), then cb=1, cb=0. Total
    // banners == the greatest cb (2), each summary within the span emitted exactly once.
    let sums = vec![
        summary(100, vec![], 10),
        summary(200, vec![], 20),
        summary(300, vec![], 30),
    ];
    let mut prev: Option<usize> = None;
    let mut emitted: Vec<usize> = Vec::new();
    for cb in [2usize, 1, 0] {
        for s in crossed_summaries(&sums, prev, cb) {
            emitted.push(s.line_no);
        }
        prev = Some(cb);
    }
    // The two summaries WITHIN the selected span (ranks 1,2 = lines 300,200), each once.
    // The oldest summary (line 100, rank 3) is OLDER than every selected turn → never
    // bannered.
    assert_eq!(emitted.len(), 2, "banners == max cb (2): {emitted:?}");
    let mut sorted = emitted.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, vec![200, 300]);
}

#[test]
fn line_is_turn_candidate_superset_of_assistant_text() {
    // A pure-text assistant record (no Edit/Write/Read/Bash) must pass — the broadened
    // prefilter is the design's required deviation.
    let pure_asst = br#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"just prose, no tools"}]}}"#;
    assert!(line_is_turn_candidate(pure_asst));
    let user = br#"{"type":"user","message":{"role":"user","content":"hi"}}"#;
    assert!(line_is_turn_candidate(user));
    let summary =
        br#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"s"}}"#;
    assert!(line_is_turn_candidate(summary));
    // A system metrics record with none of the markers → skipped.
    let sys = br#"{"type":"system","subtype":"turn_duration","durationMs":5}"#;
    assert!(!line_is_turn_candidate(sys));
}

#[test]
fn window_admits_turn_range_and_time() {
    let tr = Some((2usize, 5usize));
    let unbounded = TimeWindow::default();
    assert!(window_admits(
        3,
        Some("2026-06-07T05:00:00Z"),
        tr,
        &unbounded
    ));
    assert!(!window_admits(
        1,
        Some("2026-06-07T05:00:00Z"),
        tr,
        &unbounded
    ));
    assert!(!window_admits(6, None, tr, &unbounded));
    // No turn-range, bounded time excludes timestamp-less.
    let bounded = TimeWindow::from_args(Some("2026-06-01"), None).unwrap();
    assert!(!window_admits(0, None, None, &bounded));
}

#[test]
fn parse_turn_range_parses_and_rejects() {
    assert_eq!(parse_turn_range("2..5").unwrap(), (2, 5));
    assert!(parse_turn_range("5..2").is_err());
    assert!(parse_turn_range("noformat").is_err());
    assert!(parse_turn_range("a..b").is_err());
}

// ── Branch-completeness: selection edge paths + renderers ──

#[test]
fn phase1_giant_first_round_trip_is_clamped_in() {
    // The most-recent complete turn is larger than the WHOLE rt reservation. It must
    // still be selected (the most-recent exchange is load-bearing), clamped — the
    // `spent_rt == 0 && c <= rt_budget` / `> rt_budget` arms of Phase 1.
    let huge_u = "u".repeat(5000);
    let huge_a = "a".repeat(5000);
    let turns = vec![mk_turn(0, Some(&huge_u), Some(&huge_a), 3, 0)];
    let sr = scan_with_turns(turns, Vec::new());
    // A tiny budget whose rt half is smaller than the single capped round-trip cost.
    let plan = plan_session(&sr, 2000, 0.5, 0, &cfg());
    assert_eq!(
        plan.selected.len(),
        1,
        "the giant most-recent turn is included"
    );
    assert!(matches!(plan.selected[0].sides, SelSides::Both));
}

#[test]
fn phase2_fills_user_only_when_round_trip_does_not_fit() {
    // turn 2 (most recent, small complete) is taken whole in Phase 1; turn 1 (older, a
    // complete pair whose ASSISTANT is huge) does not fit the remaining pool as a whole
    // pair, so Phase 2 takes its USER side only (user-first preference); turn 0 small.
    let turns = vec![
        mk_turn(
            0,
            Some("oldest short ask"),
            Some("oldest short reply"),
            0,
            0,
        ),
        mk_turn(
            1,
            Some("middle user ask kept verbatim"),
            Some(&"a".repeat(3000)),
            0,
            0,
        ),
        mk_turn(
            2,
            Some("most recent short ask"),
            Some("most recent short reply"),
            0,
            0,
        ),
    ];
    let sr = scan_with_turns(turns, Vec::new());
    // Budget tuned so the small complete pairs + turn 1's user-only fit, but turn 1's
    // 900-capped assistant side does not.
    let plan = plan_session(&sr, 700, 0.5, 0, &cfg());
    assert!(
        plan.rendered_chars <= 700,
        "budget respected: {}",
        plan.rendered_chars
    );
    assert!(
        plan.selected
            .iter()
            .any(|s| s.turn_index == 1 && matches!(s.sides, SelSides::UserOnly)),
        "the recent turn's user side is filled (assistant too big): {:?}",
        plan.selected
            .iter()
            .map(|s| (s.turn_index, s.sides))
            .collect::<Vec<_>>()
    );
}

#[test]
fn partial_user_only_turn_is_selected_as_user_only() {
    // A turn with NO assistant EOT (pure tool-call) is partial → Phase 2 takes its user
    // side only (the `else if t.user.is_some()` arm).
    let turns = vec![
        mk_turn(0, Some("complete ask"), Some("complete reply"), 0, 0),
        mk_turn(1, Some("partial ask with no eot"), None, 2, 0),
    ];
    let sr = scan_with_turns(turns, Vec::new());
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    assert!(
        plan.selected
            .iter()
            .any(|s| s.turn_index == 1 && matches!(s.sides, SelSides::UserOnly)),
        "partial (no-EOT) turn taken user-only"
    );
}

#[test]
fn partial_assistant_only_turn_is_selected_as_assistant_only() {
    // A turn with NO user opener (a synthetic/orphan lead with only assistant text) →
    // the `else if t.assistant_eot().is_some()` arm.
    let turns = vec![
        mk_turn(
            0,
            None,
            Some("orphan assistant reply with no user opener"),
            0,
            0,
        ),
        mk_turn(1, Some("normal ask"), Some("normal reply"), 0, 0),
    ];
    let sr = scan_with_turns(turns, Vec::new());
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    assert!(
        plan.selected
            .iter()
            .any(|s| s.turn_index == 0 && matches!(s.sides, SelSides::AssistantOnly)),
        "orphan assistant-only turn taken assistant-only"
    );
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
fn count_sides_counts_each_selection_kind() {
    // Each selected turn must exist in `plan.turns` (the count walks the real agent lane —
    // a single-agent turn contributes one assistant unit). Both → +1 user +1 asst;
    // UserOnly → +1 user; AssistantOnly → +1 asst.
    let plan = SessionPlan {
        selected: vec![
            Selected {
                turn_index: 0,
                sides: SelSides::Both,
            },
            Selected {
                turn_index: 1,
                sides: SelSides::UserOnly,
            },
            Selected {
                turn_index: 2,
                sides: SelSides::AssistantOnly,
            },
        ],
        turns: vec![
            mk_turn(0, Some("u0"), Some("a0"), 0, 0),
            mk_turn(1, Some("u1"), Some("a1"), 0, 0),
            mk_turn(2, Some("u2"), Some("a2"), 0, 0),
        ],
        spanned_boundaries: 0,
        rendered_chars: 0,
        newest_summary_line: None,
        dedup_demoted: 0,
    };
    assert_eq!(count_sides(&plan, &cfg()), (2, 2));
}

#[test]
fn turn_cost_assistant_only_costs_only_assistant_side() {
    let t = mk_turn(0, Some("ask"), Some("reply"), 5, 0);
    // AssistantOnly cost == just the assistant unit cost (no user, no marker).
    let a = t.assistant_eot().unwrap();
    assert_eq!(turn_cost(&t, SelSides::AssistantOnly, &cfg()), unit_cost(a));
}

#[test]
fn summary_fingerprints_handles_bare_unquoted_bullet() {
    // A §6 bullet with NO quotes → the fingerprint falls back to the whole bullet body
    // (the `unwrap_or_else(|| fingerprint(rest))` arm).
    let body = "6. All user messages:\n   - a bare unquoted bullet about the carry";
    let fps = summary_fingerprints(body);
    assert!(
        fps.iter().any(|f| f.starts_with("a bare unquoted bullet")),
        "bare bullet fingerprinted: {fps:?}"
    );
}

#[test]
fn build_orphan_assistant_lead_has_no_user() {
    // Records that lead with an assistant before any genuine user → a synthetic turn 0
    // with assistant_eot but no user (group_turn_indices seeds turn 0 with the lead).
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"orphan lead reply"}]}}"#,
            ),
        ),
        (
            2,
            rec(r#"{"type":"user","message":{"role":"user","content":"the first real ask"}}"#),
        ),
        (
            3,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"real reply"}]}}"#,
            ),
        ),
    ];
    let (turns, _s) = build(&records, &[]);
    // The orphan lead folds into turn 0 (the first real user turn), so turn 0 has the
    // user AND carries the orphan assistant text as its EOT (last assistant in the turn).
    assert_eq!(turns.len(), 1);
    assert!(turns[0].user.is_some());
    assert!(turns[0].assistant_eot().is_some());
}

#[test]
fn empty_session_plans_to_nothing() {
    let sr = scan_with_turns(Vec::new(), Vec::new());
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    assert!(plan.selected.is_empty());
    assert_eq!(plan.spanned_boundaries, 0);
    assert_eq!(plan.rendered_chars, 0);
}

#[test]
fn dedup_no_op_when_no_summary() {
    // No summary at all → dedup is a no-op, nothing flagged.
    let turns = vec![mk_turn(0, Some("ask"), Some("reply"), 0, 0)];
    let sr = scan_with_turns(turns, Vec::new());
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    assert_eq!(plan.dedup_demoted, 0);
    assert!(plan.newest_summary_line.is_none());
}

#[test]
fn end_to_end_live_dedup_through_build_and_plan() {
    // A LIVE-region (compactions_before==0) turn whose user text matches the newest
    // summary's §6 bullet must be flagged + demoted through the full build → plan path.
    // The pre-boundary turn (cb=1) carrying nearby text is NOT deduped (older context).
    let records: Vec<(usize, Record)> = vec![
        (1, rec(r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pre-boundary ask"}}"#)),
        (2, rec(r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"pre reply"}]}}"#)),
        (3, rec("{\"type\":\"user\",\"isCompactSummary\":true,\"message\":{\"role\":\"user\",\"content\":\"6. All user messages:\\n   - \\\"the live duplicate ask verbatim\\\"\\n9. Optional Next Step:\\n   x\"}}")),
        (4, rec(r#"{"type":"user","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"the live duplicate ask verbatim"}}"#)),
        (5, rec(r#"{"type":"assistant","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"live reply"}]}}"#)),
    ];
    let (turns, summaries) = build(&records, &[]);
    assert_eq!(summaries.len(), 1);
    assert!(summaries[0]
        .fingerprints
        .iter()
        .any(|f| f.starts_with("the live duplicate ask verbatim")));
    // turn 0 is pre-boundary (cb=1), turn 1 is live (cb=0).
    assert_eq!(turns[0].compactions_before, 1);
    assert_eq!(turns[1].compactions_before, 0);
    let sr = scan_with_turns(turns, summaries);
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    assert_eq!(
        plan.dedup_demoted, 1,
        "exactly the live-region match is demoted"
    );
    assert_eq!(plan.newest_summary_line, Some(3));
}

// ── More branch-completeness: clamp arms, empty fingerprints, dedup edge ──

#[test]
fn giant_round_trip_bigger_than_unit_budget_is_not_force_included_both() {
    // A single complete turn whose CAPPED cost exceeds even the whole UNIT budget must
    // NOT be force-included as a both-sides pair — doing so would overshoot the budget,
    // which is the exact bug this fix removes. Phase 2 instead takes the cheaper single
    // (user-first) side that DOES fit, preserving the ≤-budget guarantee.
    let huge_u = "u".repeat(5000);
    let huge_a = "a".repeat(5000);
    let pair = mk_turn(0, Some(&huge_u), Some(&huge_a), 9, 0);
    // Budget that fits the (capped ~600) user side alone but NOT the full ~1560 pair.
    let user_side = turn_cost(&pair, SelSides::UserOnly, &cfg());
    let both = turn_cost(&pair, SelSides::Both, &cfg());
    let sr = scan_with_turns(vec![pair], Vec::new());
    // doc-header reservation is tiny (no summaries); pick a budget between the user side
    // and the full pair, with headroom for the header block.
    let budget = user_side + 400;
    assert!(budget < both, "budget must be below the full-pair cost");
    let plan = plan_session(&sr, budget, 0.5, 0, &cfg());
    assert_eq!(
        plan.selected.len(),
        1,
        "the single turn is still represented"
    );
    assert!(
        matches!(plan.selected[0].sides, SelSides::UserOnly),
        "the cheaper user side fits where the full pair does not: {:?}",
        plan.selected[0].sides
    );
    // And the whole emitted document still respects budget by construction.
    assert!(
        plan.rendered_chars <= budget,
        "{} <= {budget}",
        plan.rendered_chars
    );
}

#[test]
fn giant_round_trip_fits_full_budget_not_rt_half_is_force_included() {
    // A complete turn whose cost is > the 50% round-trip reservation but <= the whole
    // (post-header-block) unit budget → the `spent_units == 0` force-include arm: it is
    // taken as a both-sides pair even though it blows past rt_budget, because the FULL
    // document still fits the budget. Sized off the real `turn_cost`, not a magic number.
    let huge_u = "u".repeat(2000);
    let huge_a = "a".repeat(2000);
    let pair = mk_turn(0, Some(&huge_u), Some(&huge_a), 0, 0);
    let both = turn_cost(&pair, SelSides::Both, &cfg());
    let sr = scan_with_turns(vec![pair], Vec::new());
    // Budget that, after the (tiny, no-summary) header-block reservation, admits the whole
    // pair — but whose 50% rt reservation does NOT (so the rt-half branch is skipped and
    // the force-include arm fires). A generous +1000 covers the header block reservation.
    let budget = both + 1000;
    assert!(
        (budget as f64 * 0.5) < both as f64,
        "rt_budget (~half) must be below the pair cost so the force-include arm is hit"
    );
    let plan = plan_session(&sr, budget, 0.5, 0, &cfg());
    assert_eq!(plan.selected.len(), 1);
    assert!(matches!(plan.selected[0].sides, SelSides::Both));
    assert!(
        plan.rendered_chars <= budget,
        "{} <= {budget}",
        plan.rendered_chars
    );
}

#[test]
fn dedup_skips_empty_text_unit() {
    // A live unit whose text is whitespace-only → its fingerprint is empty → never a
    // dedup match (the `unit_fp.is_empty()` true arm), so it is not flagged.
    let summary_fps = vec![fingerprint("a real summary bullet about the carry")];
    let blank = unit(Role::User, 1, "   ", 0);
    assert!(!unit_matches_summary(&blank, &summary_fps));
}

#[test]
fn summary_fingerprints_skips_empty_quote() {
    // A bullet whose quoted run is empty (`- ""`) and a bare bullet that normalizes to
    // empty → the `!fp.is_empty()` false arms (nothing pushed).
    let body = "6. All user messages:\n   - \"\"\n   -   ";
    let fps = summary_fingerprints(body);
    assert!(
        fps.is_empty(),
        "empty bullets produce no fingerprints: {fps:?}"
    );
}

#[test]
fn build_summary_with_block_body_is_not_captured() {
    // A summary record with a BLOCK body (not a string) → compact_summary_body returns
    // None → the summary is NOT captured (the `if let Some(body)` false arm in build).
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(r#"{"type":"user","message":{"role":"user","content":"ask"}}"#),
        ),
        (
            2,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"reply"}]}}"#,
            ),
        ),
        // a block-bodied "summary" (a genuine surprise) → skipped as a summary source.
        (
            3,
            rec(
                r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":[{"type":"text","text":"block body summary"}]}}"#,
            ),
        ),
    ];
    let (turns, summaries) = build(&records, &[]);
    assert!(summaries.is_empty(), "block-bodied summary not captured");
    // The turn still builds; the block summary contributes no boundary.
    assert_eq!(turns[0].compactions_before, 0);
}

#[test]
fn budget_unit_token_conversion_via_run_is_four_x() {
    // The TOKEN_CHARS constant is 4.0 — the documented heuristic.
    assert_eq!(TOKEN_CHARS, 4.0);
}

#[test]
fn dedup_demoted_turn_sorts_after_non_dup_in_phase1() {
    // Two complete live turns, one dedup-flagged: at a budget that fits only ONE, the
    // NON-dup turn must win Phase 1 (dedup_pass false before true).
    let dup_text = "the duplicate ask the summary already has verbatim in full here";
    let dup_turn = mk_turn(0, Some(dup_text), Some("dup reply"), 0, 0);
    // The unique turn carries a LARGE (capped) user body so a single unique pair dominates
    // the cheap dup pair — this lets the budget sit cleanly between "one unique pair + the
    // header-block reservation" and "both pairs", isolating the dedup-ORDER decision.
    let big_ask = format!("a unique fresh ask {}", "q".repeat(700));
    let uniq_turn = mk_turn(1, Some(&big_ask), Some("unique reply"), 0, 0);
    // Both turns are live (cb=0) so no banner is charged; sums drives the dedup flag only.
    let one_pair = turn_cost(&uniq_turn, SelSides::Both, &cfg());
    let dup_pair = turn_cost(&dup_turn, SelSides::Both, &cfg());
    let sums = vec![summary(900, vec![dup_text], 9000)];
    let sr = scan_with_turns(vec![dup_turn, uniq_turn], sums);
    // Selection runs against `available = budget - header_block`. The window that fits
    // EXACTLY one pair is `available ∈ [one_pair, one_pair + dup_pair)`. Pick `available`
    // at the midpoint of that window, then add back the (budget-dependent) header block.
    // Iterate once to settle the header-block reservation (it depends on the budget's
    // digit width); converges immediately for these magnitudes.
    let want_available = one_pair + dup_pair / 2;
    let mut budget = want_available + doc_header_block_max_chars(&sr, 40000);
    budget = want_available + doc_header_block_max_chars(&sr, budget);
    let available = budget - doc_header_block_max_chars(&sr, budget);
    assert!(
        available >= one_pair && available < one_pair + dup_pair,
        "available {available} must fit exactly one pair (one={one_pair}, dup={dup_pair})"
    );
    let plan = plan_session(&sr, budget, 0.5, 0, &cfg());
    assert_eq!(plan.dedup_demoted, 1);
    // The NON-dup turn (index 1) is selected before the dup (index 0).
    assert!(
        plan.selected.iter().any(|s| s.turn_index == 1),
        "non-dup turn wins the tight budget: {:?}",
        plan.selected
            .iter()
            .map(|s| s.turn_index)
            .collect::<Vec<_>>()
    );
}

// ── Render-helper + raw-body branch completeness ──

#[test]
fn raw_body_newlines_block_with_empty_text_blocks() {
    // A block body where one text block is blank → it is skipped (the `!text.trim()`
    // false arm); only the non-blank block contributes.
    let r = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"   "},{"type":"text","text":"real\nline"}]}}"#,
    );
    assert_eq!(raw_body_newlines(&r), 1);
    // A block body with a non-text block (tool_use) interleaved → only text counts.
    let r2 = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}},{"type":"text","text":"a\nb\nc"}]}}"#,
    );
    assert_eq!(raw_body_newlines(&r2), 2);
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
fn turn_cost_user_only_costs_only_user_side() {
    let t = mk_turn(0, Some("the ask"), Some("the reply"), 5, 0);
    let u = t.user.as_ref().unwrap();
    // UserOnly: just the user unit cost (no marker, no assistant).
    assert_eq!(turn_cost(&t, SelSides::UserOnly, &cfg()), unit_cost(u));
}

// ── Prefilter arm-by-arm + build None arms ──

#[test]
fn line_is_turn_candidate_each_arm_in_isolation() {
    // Each arm matched by a line that triggers ONLY that probe (so the OR short-circuits
    // at a different position each time → every arm's true side is exercised).
    assert!(line_is_turn_candidate(br#"x "role":"user" x"#)); // arm 1
    assert!(line_is_turn_candidate(br#"x "role":"assistant" x"#)); // arm 2
    assert!(line_is_turn_candidate(br#"x "type":"assistant" x"#)); // arm 3
    assert!(line_is_turn_candidate(b"x isCompactSummary x")); // arm 4
    assert!(line_is_turn_candidate(b"x tool_use x")); // arm 5
                                                      // A line matching NONE → all arms false.
    assert!(!line_is_turn_candidate(
        b"{\"type\":\"system\",\"subtype\":\"x\"}"
    ));
    assert!(!line_is_turn_candidate(b""));
}

#[test]
fn build_skips_non_genuine_user_opener() {
    // A turn whose opening record is genuine-user but genuine_user_text returns text;
    // also a record that is a tool_result carrier (not genuine) must NOT become a user
    // opener (the `is_genuine_user` false path inside the loop).
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(r#"{"type":"user","message":{"role":"user","content":"real opener"}}"#),
        ),
        (
            2,
            rec(
                r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"carrier"}]}}"#,
            ),
        ),
        (
            3,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"reply"}]}}"#,
            ),
        ),
    ];
    let (turns, _s) = build(&records, &[]);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].user.as_ref().unwrap().text, "real opener");
    assert!(turns[0].assistant_eot().is_some());
}

#[test]
fn raw_body_newlines_no_message_and_no_content() {
    // No message at all → 0 (the `let Some(msg) else` arm).
    let no_msg = rec(r#"{"type":"system","subtype":"x"}"#);
    assert_eq!(raw_body_newlines(&no_msg), 0);
    // message present, no content → 0 (the `let Some(content) else` arm).
    let no_content = rec(r#"{"type":"user","message":{"role":"user"}}"#);
    assert_eq!(raw_body_newlines(&no_content), 0);
}

#[test]
fn compact_summary_body_none_when_no_message() {
    let bare = rec(r#"{"type":"user","isCompactSummary":true}"#);
    assert!(compact_summary_body(&bare).is_none());
}

#[test]
fn phase2_empty_turn_no_user_no_assistant_selects_nothing() {
    // A degenerate turn with NEITHER user NOR assistant (the `&[]` candidates arm at
    // L782's else) → it is never selected; a real turn beside it is.
    let turns = vec![
        mk_turn(0, None, None, 0, 0),
        mk_turn(1, Some("real ask"), Some("real reply"), 0, 0),
    ];
    let sr = scan_with_turns(turns, Vec::new());
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    assert!(
        !plan.selected.iter().any(|s| s.turn_index == 0),
        "the empty turn is never selected"
    );
    assert!(plan.selected.iter().any(|s| s.turn_index == 1));
}

#[test]
fn unit_matches_summary_no_match_returns_false() {
    // A non-empty unit fingerprint that matches NO summary fingerprint → the `.any()`
    // false result (L845/L849 false sides).
    let summary_fps = vec![fingerprint(
        "a summary bullet entirely unrelated to the unit",
    )];
    let u = unit(
        Role::User,
        1,
        "a completely different question about something else",
        0,
    );
    assert!(!unit_matches_summary(&u, &summary_fps));
    // An empty summary fingerprint in the list must not spuriously match (the `!sfp
    // .is_empty()` false guard).
    let with_empty = vec![String::new(), fingerprint("real bullet text here")];
    assert!(!unit_matches_summary(&u, &with_empty));
}

#[test]
fn parse_turn_range_valid_range_hi_ge_lo() {
    // hi >= lo → the `hi < lo` FALSE arm (the success path, distinct from the reject).
    assert_eq!(parse_turn_range("0..0").unwrap(), (0, 0));
    assert_eq!(parse_turn_range("10..20").unwrap(), (10, 20));
}

#[test]
fn shown_user_and_assistant_cover_all_combinations() {
    let complete = mk_turn(0, Some("u"), Some("a"), 0, 0);
    // The assistant LANE (shown_agent_lane) is non-empty exactly when the side is shown
    // AND the turn has agent messages; empty otherwise (replaces the old shown_assistant).
    let lane = |t: &TurnSlice, s: SelSides| !shown_agent_lane(t, s, &cfg()).is_empty();
    // Both → both sides shown.
    assert!(shown_user(&complete, SelSides::Both).is_some());
    assert!(lane(&complete, SelSides::Both));
    // UserOnly → user shown, assistant hidden (shows_assistant false).
    assert!(shown_user(&complete, SelSides::UserOnly).is_some());
    assert!(!lane(&complete, SelSides::UserOnly));
    // AssistantOnly → assistant shown, user hidden.
    assert!(shown_user(&complete, SelSides::AssistantOnly).is_none());
    assert!(lane(&complete, SelSides::AssistantOnly));
    // A turn missing a side → empty lane even when the selection would show it.
    let no_asst = mk_turn(0, Some("u"), None, 0, 0);
    assert!(!lane(&no_asst, SelSides::Both));
    let no_user = mk_turn(0, None, Some("a"), 0, 0);
    assert!(shown_user(&no_user, SelSides::Both).is_none());
    // The boolean helpers.
    assert!(shows_user(SelSides::Both) && shows_user(SelSides::UserOnly));
    assert!(!shows_user(SelSides::AssistantOnly));
    assert!(shows_assistant(SelSides::Both) && shows_assistant(SelSides::AssistantOnly));
    assert!(!shows_assistant(SelSides::UserOnly));
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
fn summary_fingerprints_skips_empty_prose_quote() {
    // A §9-style prose line whose quoted run is empty (`said ""`) → the L509 `!fp
    // .is_empty()` false arm (nothing pushed for that line).
    let body = "9. Optional Next Step:\n   The assistant said \"\".";
    let fps = summary_fingerprints(body);
    assert!(
        fps.is_empty(),
        "empty prose quote yields no fingerprint: {fps:?}"
    );
}

#[test]
fn build_skips_non_candidate_records_in_scan() {
    // The build path itself runs over already-parsed records; the prefilter's skip is
    // exercised at scan time. Verify build tolerates a `system` record interleaved as a
    // turn member (it contributes no user/assistant/summary).
    let records: Vec<(usize, Record)> = vec![
        (
            1,
            rec(r#"{"type":"user","message":{"role":"user","content":"ask"}}"#),
        ),
        (
            2,
            rec(r#"{"type":"system","subtype":"turn_duration","durationMs":5}"#),
        ),
        (
            3,
            rec(
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"reply"}]}}"#,
            ),
        ),
    ];
    let (turns, summaries) = build(&records, &[]);
    assert_eq!(turns.len(), 1);
    assert!(turns[0].is_round_trip());
    assert!(summaries.is_empty());
}

// ════════════════════════════════════════════════════════════════════════════
// Multi-agent-message model — richness function, selection, placeholder
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn agent_msg_is_rich_each_signal_arm_flips_a_short_body() {
    let c = rich_cfg();
    // ARM 2a — number-of-substance: a count adjacent to a substance noun.
    assert!(agent_msg_is_rich("12 passed 3 failed", &c));
    assert!(agent_msg_is_rich("ran 45 tests", &c));
    // ARM 2a — an N / M ratio (no noun needed).
    assert!(agent_msg_is_rich("now at 12/40 done", &c));
    assert!(agent_msg_is_rich("3 of 5 complete", &c));
    // ARM 2b — commit-hash-like hex (must carry an a–f letter).
    assert!(agent_msg_is_rich("fix landed in a1b2c3d", &c));
    assert!(agent_msg_is_rich("see deadbeef now", &c));
    // ARM 2c — file:line ref and a src/ path.
    assert!(agent_msg_is_rich("the bug is at src/turns.rs:402", &c));
    assert!(agent_msg_is_rich("edited src/cli.rs today", &c));
    // ARM 2d — backtick code path.
    assert!(agent_msg_is_rich("the `agents` vec holds it", &c));
    // ARM 2e — finding/decision lexeme.
    assert!(agent_msg_is_rich("root cause confirmed here", &c));
    assert!(agent_msg_is_rich("found the real issue", &c));
    assert!(agent_msg_is_rich("regression verified", &c));
    // ARM 1 — length gate: a >=280-char signal-less body is rich on length alone.
    let long = "z".repeat(280);
    assert!(agent_msg_is_rich(&long, &c));
}

#[test]
fn agent_msg_is_rich_rejects_a_short_signalless_declaration() {
    let c = rich_cfg();
    // A short, signal-less intent-verb opener is NOT rich.
    assert!(!agent_msg_is_rich("let me read the file", &c));
    assert!(!agent_msg_is_rich("now i will look into this", &c));
    // A plain decimal (no a–f) is NOT a commit hash, and "1" alone has no substance noun.
    assert!(!agent_msg_is_rich("step 1 next", &c));
}

#[test]
fn agent_msg_is_rich_is_codepoint_safe_for_multibyte_with_a_digit() {
    // REGRESSION: a digit adjacent to multi-byte text used to panic — the ±16-byte
    // number-of-substance window sliced mid-codepoint. The window bounds must snap to a
    // char boundary; this must NOT panic, for a 2-digit number AND a single digit.
    let c = rich_cfg();
    // A multi-byte line with a time/number right next to 4-byte chars (no substance noun
    // → not rich, but the point is it must not panic).
    let _ = agent_msg_is_rich("🤖 07:40 watching 🚀, 9 left to go", &c);
    let _ = agent_msg_is_rich("🤖 confirmed 42 times, root cause at src/x.rs:9", &c);
    let _ = agent_msg_is_rich("🤖 step 7 done", &c);
    // A multi-byte phrase with a number that DOES carry a finding lexeme is rich.
    assert!(agent_msg_is_rich("🤖 root cause confirmed at line 42", &c));
    // The droppable predicate is codepoint-safe too (it calls agent_msg_is_rich first).
    let _ = agent_msg_is_droppable("🤖 looking at the 07:40 log", &c);
}

#[test]
fn agent_msg_is_droppable_and_keep_on_doubt() {
    let c = rich_cfg();
    // Droppable: short + intent-verb opener + no signal.
    assert!(agent_msg_is_droppable("let me read the file", &c));
    assert!(agent_msg_is_droppable("now i will open this file", &c));
    // NOT droppable — rich wins even with an intent-verb opener (fusion case).
    assert!(!agent_msg_is_droppable(
        "let me note: root cause confirmed in src/x.rs:42",
        &c
    ));
    // KEEP-ON-DOUBT: a sub-280 signal-less body WITHOUT an intent-verb opener is KEPT
    // (neither rich nor droppable → falls through → kept).
    assert!(!agent_msg_is_rich(
        "the boundary handling here is subtle",
        &c
    ));
    assert!(!agent_msg_is_droppable(
        "the boundary handling here is subtle",
        &c
    ));
    // A signal-less intent-verb opener AT/ABOVE the declaration length is NOT droppable.
    let long_decl = format!("let me {}", "x".repeat(210));
    assert!(!agent_msg_is_droppable(&long_decl, &c));
}

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

/// A deterministic body of exactly `n` ASCII chars whose tail token is unique (`TAGn`), so
/// a test can assert WHICH message survived by substring without coupling to the filler.
fn body_chars(tag: &str, n: usize) -> String {
    let suffix = format!(" {tag}");
    let fill = n.saturating_sub(suffix.chars().count());
    let mut s = "x".repeat(fill);
    s.push_str(&suffix);
    s
}

// ── Default `Longest` mode (the user-specified new default) ──

#[test]
fn longest_default_keeps_the_longest_not_the_last() {
    // THE HEADLINE CASE. A turn = [a long substantive Rich Response, a ~50-char throwaway
    // wrap-up]. The OLD default kept `agents.last()` → the wrap-up, silently dropping the
    // substance. The NEW default keeps the LONGEST → the Rich Response; the short non-rich
    // wrap-up collapses into a placeholder. The Rich Response is the FIRST message here, so
    // this proves the default is "longest", NOT "last" and NOT merely "first".
    let rich_response = body_chars("RICHRESP", 600); // longest, > rich_min_chars
    let wrap_up = "Done — let me know if you'd like anything else."; // ~48 chars, not rich
    assert!(wrap_up.chars().count() < 60);
    let t = mk_turn_agents(0, Some("ask"), &[&rich_response, wrap_up], 0);
    let lane = select_agent_messages(&t, &longest_cfg());
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(kept.len(), 1, "only the longest survives: {lane:?}");
    assert!(
        kept[0].contains("RICHRESP"),
        "the LONGEST (Rich Response) is kept, not the wrap-up: {kept:?}"
    );
    // The wrap-up collapsed into exactly one placeholder.
    let phs = lane
        .iter()
        .filter(|r| matches!(r, AgentRender::Placeholder(_)))
        .count();
    assert_eq!(phs, 1, "the throwaway wrap-up collapses into a placeholder");
}

#[test]
fn longest_default_longest_is_a_middle_message() {
    // The substantive message is a MIDDLE (the realistic shape: a short opener, the big
    // Rich Response in the middle, a short wrap-up). Default keeps the middle longest;
    // the short non-rich opener + wrap-up both collapse.
    let opener = "Let me look into this."; // short, not substantive, not rich
    let middle = body_chars("MIDRESP", 700); // the longest → kept
    let wrap_up = "All set."; // tiny → collapse
    let t = mk_turn_agents(0, Some("ask"), &[opener, &middle, wrap_up], 0);
    let lane = select_agent_messages(&t, &longest_cfg());
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(kept.len(), 1, "only the middle longest survives: {lane:?}");
    assert!(kept[0].contains("MIDRESP"), "the middle Rich Response wins");
}

#[test]
fn longest_default_also_keeps_a_substantive_first() {
    // The FIRST is ALSO kept when substantive (>= rich_min_chars), even though it is not
    // the longest. Here: a long-but-not-longest first (states the plan), the longest in the
    // middle, a short wrap-up. Kept = {first, longest middle}; the wrap-up collapses.
    let first = body_chars("PLANFIRST", 400); // substantive (>= 280) but < the longest
    let middle = body_chars("BIGMID", 900); // the longest
    let wrap_up = "ok done"; // tiny → collapse
    let t = mk_turn_agents(0, Some("ask"), &[&first, &middle, wrap_up], 0);
    let lane = select_agent_messages(&t, &longest_cfg());
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        kept.iter().any(|k| k.contains("PLANFIRST")),
        "a substantive first is ALSO kept: {kept:?}"
    );
    assert!(
        kept.iter().any(|k| k.contains("BIGMID")),
        "the longest is always kept: {kept:?}"
    );
    assert_eq!(kept.len(), 2, "exactly first + longest: {lane:?}");
}

#[test]
fn longest_default_drops_a_non_substantive_first() {
    // A SHORT first (below rich_min_chars, not rich) is NOT kept by position — only the
    // longest survives. Distinguishes `Longest` from `Rich`'s unconditional keep-first.
    let first = "let me start"; // short + not rich → not kept
    let middle = body_chars("ONLYLONG", 600); // the longest
    let wrap_up = "fin"; // tiny
    let t = mk_turn_agents(0, Some("ask"), &[first, &middle, wrap_up], 0);
    let lane = select_agent_messages(&t, &longest_cfg());
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(kept.len(), 1, "non-substantive first is dropped: {lane:?}");
    assert!(kept[0].contains("ONLYLONG"));
}

#[test]
fn longest_default_keeps_a_rich_middle_with_a_major_finding() {
    // A MIDDLE that is RICH by SIGNAL (not length) — a file:line + ratio finding — is kept
    // even though it is not the longest, because major findings can live mid-run. Here the
    // longest is the final answer; the rich middle ALSO survives.
    let opener = "starting now"; // short → collapse
    let finding = "12 passed 3 failed in src/x.rs:9"; // rich by signal, not longest
    let longest = body_chars("FINALANS", 500); // the longest → kept (last)
    let t = mk_turn_agents(0, Some("ask"), &[opener, finding, &longest], 0);
    let lane = select_agent_messages(&t, &longest_cfg());
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        kept.iter().any(|k| k.contains("src/x.rs:9")),
        "the rich middle finding survives: {kept:?}"
    );
    assert!(
        kept.iter().any(|k| k.contains("FINALANS")),
        "the longest survives: {kept:?}"
    );
}

#[test]
fn longest_default_single_message_turn_keeps_it() {
    // A 1-message turn keeps its sole message regardless of richness (it is the longest).
    let t = mk_turn_agents(0, Some("ask"), &["let me look into this"], 0);
    let lane = select_agent_messages(&t, &longest_cfg());
    assert_eq!(lane.len(), 1);
    assert!(matches!(lane[0], AgentRender::Kept(_)));
}

#[test]
fn longest_default_tie_breaks_to_the_last_maximum() {
    // All messages equal length → `max_by_key` returns the LAST maximum, so the default
    // coincides with the old `agents.last()` pick on an all-equal run (documented tie rule).
    // None are rich/substantive, so ONLY the tie-winning last survives.
    let a = "alpha beta gamma"; // 16 chars
    let b = "delta epsilon ze"; // 16 chars
    let c = "eta theta iota k"; // 16 chars
    assert_eq!(a.chars().count(), b.chars().count());
    assert_eq!(b.chars().count(), c.chars().count());
    let t = mk_turn_agents(0, Some("ask"), &[a, b, c], 0);
    let lane = select_agent_messages(&t, &longest_cfg());
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        kept,
        vec![c],
        "tie → the LAST maximum (== old agents.last())"
    );
}

#[test]
fn longest_default_collapses_contiguous_runs_into_separate_placeholders() {
    // The placeholder fusing is shared with Rich: two contiguous dropped runs split by a
    // surviving rich middle → TWO placeholders. Longest = a long final answer; a rich
    // middle survives between two declaration runs.
    let t = mk_turn_agents(
        0,
        Some("ask"),
        &[
            "let me a",                     // drop
            "let me b",                     // drop
            "found 12 cases in src/z.rs:3", // rich middle → kept
            "let me c",                     // drop
            "let me d",                     // drop
            &body_chars("THEANSWER", 400),  // longest (last) → kept
        ],
        0,
    );
    let lane = select_agent_messages(&t, &longest_cfg());
    let phs = lane
        .iter()
        .filter(|r| matches!(r, AgentRender::Placeholder(_)))
        .count();
    assert_eq!(
        phs, 2,
        "two declaration runs split by the rich middle: {lane:?}"
    );
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert!(kept.iter().any(|k| k.contains("src/z.rs:3")));
    assert!(kept.iter().any(|k| k.contains("THEANSWER")));
}

#[test]
fn longest_default_keep_flag_tunes_the_substantive_first_gate() {
    // `--agent-rich-min-chars` (rich_min_chars) is the tuning knob: a first of 300 chars is
    // substantive at the default 280 (kept) but NOT at a raised 500 (dropped). Same turn,
    // two configs → the flag changes the survivor set.
    let first = body_chars("TUNEFIRST", 300);
    let longest = body_chars("TUNELONG", 800);
    let wrap_up = "bye";
    let t = mk_turn_agents(0, Some("ask"), &[&first, &longest, wrap_up], 0);

    let default_kept: Vec<String> = select_agent_messages(&t, &longest_cfg())
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        default_kept.iter().any(|k| k.contains("TUNEFIRST")),
        "300-char first IS substantive at the default 280 gate"
    );

    let raised = RichnessCfg {
        rich_min_chars: 500,
        ..longest_cfg()
    };
    let raised_kept: Vec<String> = select_agent_messages(&t, &raised)
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !raised_kept.iter().any(|k| k.contains("TUNEFIRST")),
        "the same 300-char first is NOT substantive once the gate is raised to 500"
    );
    assert!(
        raised_kept.iter().any(|k| k.contains("TUNELONG")),
        "the longest still survives regardless of the gate"
    );
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

/// Build an AUTOMATION-opener turn of a given kind (a `<task-notification>` pulse) for the
/// per-class breakdown tests. `user` is the rendered opener text; the slice is flagged
/// `is_automation` and carries a parsed trigger of `kind`.
fn mk_automation_turn(
    turn_index: usize,
    kind: crate::model::AutomationKind,
    user: &str,
) -> TurnSlice {
    let mut t = mk_turn(turn_index, Some(user), Some("ack"), 0, 0);
    t.is_automation = true;
    t.automation = Some(crate::model::AutomationTrigger {
        kind,
        task_id: Some(format!("id{turn_index}")),
        status: Some("completed".to_string()),
        summary: Some(user.to_string()),
        event: None,
    });
    t
}

#[test]
fn automation_by_kind_breaks_down_per_class_not_lumped() {
    use crate::model::AutomationKind::*;
    // A session mixing 2 background-command + 1 agent + 1 monitor automation pulses plus a
    // human turn. The breakdown must report the composition, not a lumped scalar.
    let sr = scan_with_turns(
        vec![
            mk_turn(0, Some("human ask"), Some("human reply"), 1, 0),
            mk_automation_turn(1, BackgroundCommand, "bg one done"),
            mk_automation_turn(2, BackgroundCommand, "bg two done"),
            mk_automation_turn(3, Agent, "agent done"),
            mk_automation_turn(4, Monitor, "monitor fired"),
        ],
        Vec::new(),
    );
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    let by = automation_by_kind(std::slice::from_ref(&plan));
    // Order is [BackgroundCommand, Agent, Workflow, Monitor, Task].
    assert_eq!(by, [2, 1, 0, 1, 0]);
    let text = automation_breakdown_text(&by);
    assert_eq!(text, "2 background-command, 1 agent, 1 monitor");
    // The lumped total still agrees with the per-class sum.
    assert_eq!(count_automation(&plan), by.iter().sum::<usize>());
}

#[test]
fn automation_by_kind_covers_workflow_task_and_unparsed_fallback() {
    use crate::model::AutomationKind::*;
    // Exercise the remaining classes (Workflow, Task) AND the `automation == None` fallback —
    // an `is_automation` turn whose trigger failed to parse is attributed to `task`.
    let mut unparsed = mk_turn(3, Some("mystery pulse"), Some("ack"), 0, 0);
    unparsed.is_automation = true; // flagged, but .automation stays None
    let sr = scan_with_turns(
        vec![
            mk_automation_turn(0, Workflow, "wf done"),
            mk_automation_turn(1, Task, "task done"),
            mk_automation_turn(2, Workflow, "wf two done"),
            unparsed,
        ],
        Vec::new(),
    );
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    let by = automation_by_kind(std::slice::from_ref(&plan));
    // [BackgroundCommand, Agent, Workflow, Monitor, Task] — 2 workflow, 1 task (parsed) + 1
    // task (the None-fallback) = 2 task.
    assert_eq!(by, [0, 0, 2, 0, 2]);
    assert_eq!(automation_breakdown_text(&by), "2 workflow, 2 task");
}

#[test]
fn automation_breakdown_text_empty_when_no_triggers() {
    assert_eq!(automation_breakdown_text(&[0, 0, 0, 0, 0]), "");
}

#[test]
fn automation_in_scope_counts_every_notification_regardless_of_selection() {
    use crate::model::AutomationKind::*;
    // A monitor-heavy session: many monitor pulses + a couple workflow ones. Plan it under a
    // budget too small to select them all; `automation_in_scope_by_kind` must still report the
    // WHOLE-session composition (the fix for a header reading `monitor:0` on a monitor-dominated
    // session), whereas the SELECTED `automation_by_kind` may report fewer.
    let mut turns = vec![mk_turn(0, Some("human ask"), Some("human reply"), 1, 0)];
    for i in 1..=6 {
        turns.push(mk_automation_turn(i, Monitor, "monitor tick fired"));
    }
    turns.push(mk_automation_turn(7, Workflow, "wf done"));
    let sr = scan_with_turns(turns, Vec::new());
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    // In-scope counts ALL pulses: [bg, agent, workflow, monitor, task] = 6 monitor + 1 workflow.
    let in_scope = automation_in_scope_by_kind(std::slice::from_ref(&plan));
    assert_eq!(in_scope, [0, 0, 1, 6, 0]);
    assert_eq!(in_scope.iter().sum::<usize>(), 7);
    // The selected breakdown is a SUBSET of the in-scope one (never larger in any class).
    let selected = automation_by_kind(std::slice::from_ref(&plan));
    for (sel, scope) in selected.iter().zip(in_scope.iter()) {
        assert!(sel <= scope, "selected per-class must not exceed in-scope");
    }
}

#[test]
fn automation_in_scope_empty_when_no_automation() {
    // A purely-human session has no in-scope automation in any class.
    let sr = scan_with_turns(
        vec![mk_turn(0, Some("ask"), Some("reply"), 0, 0)],
        Vec::new(),
    );
    let plan = plan_session(&sr, 40000, 0.5, 0, &cfg());
    assert_eq!(
        automation_in_scope_by_kind(std::slice::from_ref(&plan)),
        [0, 0, 0, 0, 0]
    );
}

#[test]
fn automation_by_kind_skips_non_user_and_missing_turns() {
    // Exercise the two guard arms in `automation_by_kind`: an AssistantOnly selection (does not
    // SHOW the user side → skipped) and a selection pointing at a turn_index that is not present
    // in `plan.turns` (find_turn None → skipped). Neither contributes to the breakdown.
    let mut auto = mk_turn(0, Some("pulse"), Some("ack"), 0, 0);
    auto.is_automation = true;
    auto.automation = Some(crate::model::AutomationTrigger {
        kind: crate::model::AutomationKind::Agent,
        task_id: Some("id0".to_string()),
        status: Some("completed".to_string()),
        summary: Some("pulse".to_string()),
        event: None,
    });
    let plan = SessionPlan {
        selected: vec![
            // AssistantOnly over the automation turn → the !shows_user guard skips it.
            Selected {
                turn_index: 0,
                sides: SelSides::AssistantOnly,
            },
            // A Both selection at a turn_index NOT in `turns` → the find_turn None guard skips it.
            Selected {
                turn_index: 99,
                sides: SelSides::Both,
            },
        ],
        turns: vec![auto],
        spanned_boundaries: 0,
        rendered_chars: 0,
        newest_summary_line: None,
        dedup_demoted: 0,
    };
    let by = automation_by_kind(std::slice::from_ref(&plan));
    assert_eq!(by, [0, 0, 0, 0, 0], "both selections must be skipped");
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
fn turn_carries_parsed_automation_trigger_for_json() {
    // The scan path stores the parsed trigger on the slice when the opener is a
    // <task-notification>; the JSON emitter reads `trigger_kind`/`task_id`/`status` off it.
    let line = r#"{"type":"user","message":{"role":"user","content":"<task-notification><task-id>wf_42</task-id><status>completed</status><summary>Dynamic workflow \"x\" completed</summary></task-notification>"}}"#;
    let rec: crate::model::Record = serde_json::from_str(line).expect("record");
    let trig = rec.automation_trigger().expect("a trigger");
    assert_eq!(trig.kind.slug(), "workflow");
    assert_eq!(trig.task_id.as_deref(), Some("wf_42"));
    assert_eq!(trig.status.as_deref(), Some("completed"));
}

// ── --slice chunked output (slice_into_windows) ──

#[test]
fn slice_windows_concatenate_back_to_the_source() {
    let doc = "line one\nline two\nthree\nfour five six\n";
    let chunks = slice_into_windows(doc, 12);
    assert_eq!(chunks.concat(), doc, "lossless reassembly across slices");
    for c in &chunks {
        assert!(c.chars().count() <= 12, "chunk over window: {c:?}");
    }
    assert!(chunks.len() > 1, "doc spans multiple windows");
}

#[test]
fn slice_windows_count_chars_not_bytes() {
    // Each `🛠` is 4 BYTES but 1 CHARACTER. A 6-char window fits 5 wrenches + newline
    // (21 bytes), proving the window counts Unicode scalars — the unit Claude Code's
    // additionalContext cap uses — not bytes (a byte budget would split after the first).
    let line = "🛠🛠🛠🛠🛠\n"; // 6 chars, 21 bytes
    let chunks = slice_into_windows(line, 6);
    assert_eq!(
        chunks.len(),
        1,
        "6 chars fit one 6-char window despite 21 bytes"
    );
    assert_eq!(chunks[0], line);
}

#[test]
fn slice_windows_hard_split_an_oversized_line_on_char_boundaries() {
    // A single line longer than the window is hard-split so NO chunk exceeds it — and never
    // mid-`🛠` (char boundary). Window 2, line of 5 wrenches (no trailing newline).
    let line = "🛠🛠🛠🛠🛠";
    let chunks = slice_into_windows(line, 2);
    assert_eq!(chunks.concat(), line, "lossless even when hard-splitting");
    for c in &chunks {
        assert!(c.chars().count() <= 2);
        assert!(c.chars().all(|ch| ch == '🛠'), "no broken char: {c:?}");
    }
    assert_eq!(chunks, vec!["🛠🛠", "🛠🛠", "🛠"]);
}

#[test]
fn slice_windows_empty_input_yields_no_chunks() {
    assert!(slice_into_windows("", 10).is_empty());
}
