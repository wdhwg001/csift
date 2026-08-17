use super::*;

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
    assert_eq!(
        parse_turn_range("0..0").unwrap().resolve(100, false),
        (0, 0)
    );
    assert_eq!(
        parse_turn_range("10..20").unwrap().resolve(100, false),
        (10, 20)
    );
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
