//! Summary fingerprinting and live-region dedup demotion.

use super::*;

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
fn dedup_demoted_turn_sorts_after_non_dup_in_phase1() {
    // Two complete live turns, one dedup-flagged: at a budget that fits only ONE, the
    // NON-dup turn must win Phase 1 (dedup_pass false before true).
    let dup_text = "the duplicate ask the summary already has verbatim in full here";
    let dup_turn = mk_turn(0, Some(dup_text), Some("dup reply"), 0, 0);
    // The unique turn carries a LARGE (capped) user body so a single unique pair dominates
    // the cheap dup pair - this lets the budget sit cleanly between "one unique pair + the
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
    // Flag the rich middle as also_in_summary - richness must still keep it.
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
