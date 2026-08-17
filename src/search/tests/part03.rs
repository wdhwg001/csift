use super::*;

#[test]
fn auq_answer_text_none_when_no_blocks_or_no_marker() {
    // A record with string content → no blocks → None (the `blocks()?` arm).
    let r = rec(r#"{"type":"user","message":{"role":"user","content":"plain string"}}"#);
    assert!(auq_answer_text(&r).is_none());
    // A carrier whose tool_result is NOT an AUQ answer → None (loop falls through).
    let r2 = rec(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"just normal output"}]}}"#,
    );
    assert!(auq_answer_text(&r2).is_none());
}

#[test]
fn auq_answer_text_skips_non_tool_result_blocks() {
    // The helper's loop must skip a non-ToolResult block (the `if let
    // Block::ToolResult` FALSE arm) and still find the AUQ answer in a later one.
    let r = rec(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"image","source":{}},{"type":"tool_result","tool_use_id":"x","content":"User has answered your questions: \"q\"=\"the picked one\"."}]}}"#,
    );
    assert_eq!(
        auq_answer_text(&r).as_deref(),
        Some("User has answered your questions: \"q\"=\"the picked one\".")
    );
}

#[test]
fn auq_answer_under_user_present_but_pattern_does_not_match() {
    // is_auq_answer is true and auq_answer_text returns Some, but the regex does
    // NOT match the answer → the `matcher.is_match(&text)` FALSE arm: no hit.
    let r = rec(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"User has answered your questions: \"q\"=\"alpha\"."}]}}"#,
    );
    // Pattern present in NEITHER a genuine-user text (there is none) NOR the answer.
    let m = build_matcher(&args("zzzznomatch")).unwrap();
    let mut hits = Vec::new();
    collect_record_hits(
        &r,
        LabelFilter::new(&["user".to_string()], &[]),
        &m,
        false,
        EXCERPT_MAX,
        &PlanIndex::default(),
        &HashMap::new(),
        &ClassifyCtx::top_level(),
        &mut hits,
    );
    assert!(
        hits.is_empty(),
        "non-matching AUQ answer yields no user hit"
    );
}

#[test]
fn truncate_excerpt_long_and_short() {
    assert_eq!(truncate_excerpt("short"), "short");
    let s = "y".repeat(EXCERPT_MAX + 3);
    let out = truncate_excerpt(&s);
    assert!(out.ends_with("… (+3 chars)"), "got: {out}");
}

#[test]
fn match_excerpt_centers_on_a_deep_match() {
    // The needle sits ~800 chars in — far past EXCERPT_MAX. The OLD head-only
    // excerpt hid it entirely (the bug that forced raw-jsonl reads); centering
    // must surface it, with explicit clipping markers on both sides.
    // Synthetic multi-byte placeholder (neutral emoji) + neutral padding.
    let needle = "🤖🎉✅🚀🌟";
    let text = format!("{}{needle}{}", "🔵".repeat(800), "🟥".repeat(800));
    let m = build_matcher(&args(needle)).unwrap();
    let span = m.locate(&text).expect("matches").expect("has a span");
    let (ex, truncated) = match_excerpt(&text, Some(span), EXCERPT_MAX);
    assert!(ex.contains(needle), "excerpt must show the match: {ex}");
    assert!(
        ex.starts_with('…'),
        "content precedes the window → leading …: {ex}"
    );
    assert!(
        ex.contains("chars)"),
        "content follows → trailing count: {ex}"
    );
    assert!(truncated, "a clipped match-centered window is truncated");
}

#[test]
fn match_excerpt_short_message_is_shown_whole() {
    let text = "a short hit here";
    let m = build_matcher(&args("hit")).unwrap();
    let span = m.locate(text).unwrap();
    let (ex, truncated) = match_excerpt(text, span, EXCERPT_MAX);
    assert_eq!(ex, "a short hit here");
    assert!(!truncated, "a message that fits the cap is not truncated");
}

#[test]
fn match_excerpt_early_match_keeps_the_head() {
    let text = format!("needle {}", "z".repeat(EXCERPT_MAX));
    let m = build_matcher(&args("needle")).unwrap();
    let span = m.locate(&text).unwrap();
    let (ex, truncated) = match_excerpt(&text, span, EXCERPT_MAX);
    assert!(!ex.starts_with('…'), "match at char 0 → no leading …: {ex}");
    assert!(ex.starts_with("needle"), "got: {ex}");
    assert!(truncated, "the tail past the window was dropped");
}

#[test]
fn match_excerpt_pure_filter_falls_back_to_head() {
    let text = "X".repeat(EXCERPT_MAX + 50);
    let m = build_matcher(&args("")).unwrap(); // empty pattern = pure filter
    let span = m.locate(&text).expect("pure filter matches");
    assert_eq!(span, None, "pure filter has no locatable span");
    let (ex, truncated) = match_excerpt(&text, span, EXCERPT_MAX);
    assert!(!ex.starts_with('…'), "head form has no leading …");
    assert!(ex.ends_with("… (+50 chars)"), "got: {ex}");
    assert!(truncated, "the head form clipped 50 chars");
}

#[test]
fn match_excerpt_full_budget_emits_whole_message() {
    // `--no-truncate` passes `usize::MAX` as the budget: a message longer than EXCERPT_MAX is
    // emitted whole, with NO truncation marker — whereas the default budget truncates.
    let n = EXCERPT_MAX + 200;
    let text = "🤖".repeat(n);
    let (capped, capped_truncated) = match_excerpt(&text, None, EXCERPT_MAX);
    assert!(
        capped.contains("… (+"),
        "default budget truncates: {capped}"
    );
    assert!(capped_truncated, "default budget reports truncation");
    let (full, full_truncated) = match_excerpt(&text, None, usize::MAX);
    assert!(
        !full.contains("… (+"),
        "full budget has no truncation marker"
    );
    assert!(
        !full_truncated,
        "--no-truncate's usize::MAX budget never truncates — the signal the caution note keys on"
    );
    assert_eq!(full.chars().count(), n, "full text length preserved");
}

#[test]
fn sibling_cap_policy_is_fixed_and_message_classes_uncapped() {
    // Message classes always render (None = uncapped); chattier machinery is capped.
    assert_eq!(sibling_cap(Class::UserMessage), None);
    assert_eq!(sibling_cap(Class::AgentMessage), None);
    assert_eq!(sibling_cap(Class::CommInbox), None);
    assert_eq!(sibling_cap(Class::AgentThinking), Some(2));
    assert_eq!(sibling_cap(Class::AgentToolUse), Some(3));
    assert_eq!(sibling_cap(Class::AgentToolResult), Some(3));
    assert_eq!(sibling_cap(Class::CommandStdout), Some(2));
}

#[test]
fn collect_record_hits_can_hit_false_is_skipped_via_collect_turn_hits() {
    // A record marked `can_hit:false` is skipped before any regex work in
    // collect_turn_hits (the `if !kept.can_hit { continue }` arm).
    let m = build_matcher(&args("Carry")).unwrap(); // case-sensitive → has prefilter
                                                    // A line lacking the literal → can_hit=false.
    let raw = br#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"nothing relevant"}]}}"#;
    let kept = Kept {
        rec: serde_json::from_slice(raw).unwrap(),
        can_hit: m.line_may_match(raw),
        line_no: 1,
        from_sidecar: false,
    };
    assert!(!kept.can_hit);
    let turn = Turn {
        index: 0,
        records: vec![&kept],
    };
    let tw = TimeWindow::default();
    let (hits, hit_idxs) = collect_turn_hits(
        &turn,
        LabelFilter::all(),
        &m,
        &tw,
        false,
        EXCERPT_MAX,
        &PlanIndex::default(),
        &HashMap::new(),
        None,
        &test_env(),
    );
    assert!(hits.is_empty(), "a can_hit=false record yields no hits");
    assert!(hit_idxs.is_empty(), "no record produced a hit");
}

#[test]
fn collect_turn_hits_excludes_record_outside_time_window() {
    // A record whose timestamp is outside a bounded window is skipped (the
    // `!time_window.contains(...)` arm), even when it would otherwise match.
    let m = build_matcher(&args("carry")).unwrap();
    let raw = br#"{"type":"assistant","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"the carry"}]}}"#;
    let kept = Kept {
        rec: serde_json::from_slice(raw).unwrap(),
        can_hit: m.line_may_match(raw),
        line_no: 1,
        from_sidecar: false,
    };
    let turn = Turn {
        index: 0,
        records: vec![&kept],
    };
    // Window starting AFTER the record's timestamp → excluded.
    let tw = TimeWindow::from_args(Some("2026-06-07T06:00:00Z"), None).unwrap();
    assert!(collect_turn_hits(
        &turn,
        LabelFilter::all(),
        &m,
        &tw,
        false,
        EXCERPT_MAX,
        &PlanIndex::default(),
        &HashMap::new(),
        None,
        &test_env()
    )
    .0
    .is_empty());
    // An unbounded window admits it.
    let tw2 = TimeWindow::default();
    assert!(!collect_turn_hits(
        &turn,
        LabelFilter::all(),
        &m,
        &tw2,
        false,
        EXCERPT_MAX,
        &PlanIndex::default(),
        &HashMap::new(),
        None,
        &test_env()
    )
    .0
    .is_empty());
}

#[test]
fn reconstruct_synthetic_lead_records_merge_into_first_real_turn() {
    // A file whose FIRST records are NOT genuine users (leading tool noise) must
    // fold into turn 0 once the first genuine user appears, and turns re-index
    // 0-based on genuine users (the synthetic_lead re-index branch).
    let lines = vec![
        // leading non-user noise (a tool_result carrier) — synthetic lead.
        r#"{"type":"user","uuid":"lead","timestamp":"2026-06-07T04:59:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"orphan carry note"}]}}"#,
        // first genuine user (turn 0).
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"real first about carry"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"answer carry"}]}}"#,
        // second genuine user (turn 1).
        r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"second carry"}}"#,
    ];
    let ex = search(&lines, &args("carry"));
    let indices: Vec<usize> = ex.iter().map(|e| e.turn_index).collect();
    assert_eq!(indices, vec![0, 1], "synthetic lead folds into turn 0");
    // The orphan lead record is a MEMBER of turn 0's round-trip.
    assert!(ex[0].record_uuids.contains(&"lead".to_string()));
}

#[test]
fn reconstruct_only_synthetic_lead_no_genuine_user() {
    // A file with ONLY non-genuine records (no genuine user ever) → a single
    // standalone turn 0 holding the orphans (the `else` seed-turn-0 arm, and the
    // `turns.len() > 1` false guard so no re-index).
    let lines = vec![
        r#"{"type":"user","uuid":"o0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"orphan carry one"}]}}"#,
        r#"{"type":"user","uuid":"o1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"y","content":"orphan carry two"}]}}"#,
    ];
    // Search tool-response category so the orphan carriers can produce a hit.
    let mut a = args("carry");
    a.labels = vec!["agent.tool.result".to_string()];
    let ex = search(&lines, &a);
    assert_eq!(ex.len(), 1);
    assert_eq!(ex[0].turn_index, 0);
}

#[test]
fn parse_turn_range_equal_bounds_ok() {
    // hi == lo is valid (single turn); only hi < lo errors.
    assert_eq!(
        parse_turn_range("5..5").unwrap().resolve(100, false),
        (5, 5)
    );
}

#[test]
fn turn_range_excludes_below_lo_and_above_hi() {
    // The two-turn fixture: a range `0..0` keeps turn 0 and excludes turn 1 via
    // the `turn.index > hi` arm (complementing the `< lo` arm other tests cover).
    let mut a = args("");
    a.turn_range = Some("0..0".to_string());
    let ex = search(&fixture(), &a);
    let indices: Vec<usize> = ex.iter().map(|e| e.turn_index).collect();
    assert_eq!(
        indices,
        vec![0],
        "only turn 0; turn 1 excluded by the > hi arm"
    );
}

#[test]
fn collect_record_hits_resolve_persisted_with_no_pointer_keeps_inline() {
    // resolve_persisted=true but the tool_result has NO persisted pointer → the
    // `persisted_output_path()` None arm: the inline text is matched as-is.
    let r = rec(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"plain inline output with token zzinline"}]}}"#,
    );
    let m = build_matcher(&args("zzinline")).unwrap();
    let mut hits = Vec::new();
    collect_record_hits(
        &r,
        LabelFilter::new(&["agent.tool.result".to_string()], &[]),
        &m,
        true,
        EXCERPT_MAX,
        &PlanIndex::default(),
        &HashMap::new(),
        &ClassifyCtx::top_level(),
        &mut hits,
    );
    assert_eq!(
        hits.len(),
        1,
        "inline text still matches when there is no pointer"
    );
    assert_eq!(hits[0].class, Class::AgentToolResult);
}

#[test]
fn agent_text_block_only_from_assistant_not_user_text_block() {
    // A USER record with a text block must NOT surface under `agent` (the
    // `rec.is_type("assistant")` false arm of the agent-text branch).
    let r = rec(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"user text foo"}]}}"#,
    );
    let m = build_matcher(&args("foo")).unwrap();
    let mut hits = Vec::new();
    collect_record_hits(
        &r,
        LabelFilter::new(&["agent.message".to_string()], &[]),
        &m,
        false,
        EXCERPT_MAX,
        &PlanIndex::default(),
        &HashMap::new(),
        &ClassifyCtx::top_level(),
        &mut hits,
    );
    assert!(hits.is_empty(), "a user text block is not an agent hit");
}

#[test]
fn auq_answer_still_surfaces_under_tool_response_alone() {
    // The de-dup must NOT hide the AUQ answer from a `-t agent.tool.result` filter
    // that does not also name `user` — it is genuinely a tool_result.
    let lines = vec![
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pick one"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
        r#"{"type":"user","uuid":"ans","parentUuid":"a0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"zzqq choice\". You can now continue."}]}}"#,
    ];
    let mut a = args("zzqq");
    a.labels = vec!["agent.tool.result".to_string()];
    let ex = search(&lines, &a);
    assert_eq!(ex.len(), 1);
    assert!(ex[0].hits.iter().all(|h| h.class == Class::AgentToolResult));
    assert_eq!(ex[0].hits.len(), 1, "exactly one tool-response hit");
}
