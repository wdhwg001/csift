use super::*;

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
    // No turn, bounded time excludes timestamp-less.
    let bounded = TimeWindow::from_args(Some("2026-06-01"), None).unwrap();
    assert!(!window_admits(0, None, None, &bounded));
}

#[test]
fn parse_turn_range_parses_and_rejects() {
    assert_eq!(
        parse_turn_range("2..5").unwrap().resolve(100, false),
        (2, 5)
    );
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
