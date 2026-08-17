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
