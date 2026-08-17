//! Per-record hit collection across block types, AUQ duals, and category surfacing.

use super::*;

#[test]
fn collect_hits_thinking_category() {
    let r = rec(
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"the carry holds a partial line"},{"type":"text","text":"done"}]}}"#,
    );
    let m = build_matcher(&args("carry")).unwrap();
    let mut hits = Vec::new();
    collect_record_hits(
        &r,
        LabelFilter::new(&["agent.thinking".to_string()], &[]),
        &m,
        false,
        EXCERPT_MAX,
        &PlanIndex::default(),
        &HashMap::new(),
        &ClassifyCtx::top_level(),
        &mut hits,
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].class, Class::AgentThinking);
    assert!(hits[0].excerpt.contains("carry"));
}

#[test]
fn collect_hits_agent_text_only_from_assistant() {
    let r = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"the answer is foo"}]}}"#,
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
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].class, Class::AgentMessage);
}

#[test]
fn collect_hits_tool_use_matches_name_and_input() {
    let r = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
    );
    let m = build_matcher(&args("AskUserQuestion")).unwrap();
    let mut hits = Vec::new();
    collect_record_hits(
        &r,
        LabelFilter::new(&["agent.tool.use".to_string()], &[]),
        &m,
        false,
        EXCERPT_MAX,
        &PlanIndex::default(),
        &HashMap::new(),
        &ClassifyCtx::top_level(),
        &mut hits,
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].tool_name.as_deref(), Some("AskUserQuestion"));
}

#[test]
fn collect_hits_mcp_elicitation_system_marker() {
    // §3.10: an MCP-elicitation pending marker is a `system` record with NO tool_use
    // block - `search` must still find it via its top-level `content` string (the gap the
    // §3.10 arm closes), tagged `Tool` and named by `csiftKind`.
    let r = rec(
        r#"{"type":"system","subtype":"mcp_elicitation","timestamp":"2026-06-27T02:00:00.000Z","content":"MCP elicitation [gdrive] (url): authorize wibblewobble access","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"mcp-elicitation","csiftKey":"el-1","csiftMcpServer":"gdrive"}"#,
    );
    let m = build_matcher(&args("wibblewobble")).unwrap();
    let mut hits = Vec::new();
    collect_record_hits(
        &r,
        LabelFilter::new(&["agent.tool.use".to_string()], &[]),
        &m,
        false,
        EXCERPT_MAX,
        &PlanIndex::default(),
        &HashMap::new(),
        &ClassifyCtx::top_level(),
        &mut hits,
    );
    assert_eq!(
        hits.len(),
        1,
        "MCP system marker must produce exactly one hit"
    );
    assert_eq!(hits[0].class, Class::AgentToolUse);
    assert_eq!(hits[0].tool_name.as_deref(), Some("mcp-elicitation"));
    assert!(hits[0].excerpt.contains("wibblewobble"));
}

#[test]
fn collect_hits_auq_marker_does_not_double_emit() {
    // §3.10: an AskUserQuestion pending marker DOES carry a tool_use block, so it matches
    // via the `Block::ToolUse` arm. The §3.10 non-tool_use arm is guarded to markers with
    // NO tool_use block, so this must yield EXACTLY ONE hit (not two).
    let r = rec(
        r#"{"type":"assistant","timestamp":"2026-06-27T01:00:00.000Z","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"k1","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"k1","name":"AskUserQuestion","input":{"questions":[{"question":"pick wibblewobble?"}]}}]}}"#,
    );
    let m = build_matcher(&args("wibblewobble")).unwrap();
    let mut hits = Vec::new();
    collect_record_hits(
        &r,
        LabelFilter::new(&["agent.tool.use".to_string()], &[]),
        &m,
        false,
        EXCERPT_MAX,
        &PlanIndex::default(),
        &HashMap::new(),
        &ClassifyCtx::top_level(),
        &mut hits,
    );
    assert_eq!(
        hits.len(),
        1,
        "AQ marker must not double-emit via the §3.10 arm"
    );
    assert_eq!(hits[0].tool_name.as_deref(), Some("AskUserQuestion"));
}

#[test]
fn collect_hits_auq_answer_under_user() {
    let r = rec(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"User has answered your questions: \"Q\"=\"chosen\". You can now continue."}]}}"#,
    );
    let m = build_matcher(&args("chosen")).unwrap();
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
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].class, Class::UserAnswer);
}

#[test]
fn tool_result_carrier_not_a_user_hit_when_plain() {
    let r = rec(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"plain output here"}]}}"#,
    );
    let m = build_matcher(&args("output")).unwrap();
    let mut hits = Vec::new();
    // User category must NOT surface a plain tool_result carrier.
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
    assert_eq!(hits.len(), 0);
    // But the tool-response category does.
    let mut hits2 = Vec::new();
    collect_record_hits(
        &r,
        LabelFilter::new(&["agent.tool.result".to_string()], &[]),
        &m,
        false,
        EXCERPT_MAX,
        &PlanIndex::default(),
        &HashMap::new(),
        &ClassifyCtx::top_level(),
        &mut hits2,
    );
    assert_eq!(hits2.len(), 1);
    assert_eq!(hits2[0].class, Class::AgentToolResult);
}

#[test]
fn auq_answer_surfaces_under_user_category_end_to_end() {
    let lines = vec![
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pick one"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
        r#"{"type":"user","uuid":"ans","parentUuid":"a0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"the bold option\". You can now continue."}]}}"#,
    ];
    let mut a = args("bold option");
    a.labels = vec!["user".to_string()];
    let ex = search(&lines, &a);
    // The AUQ answer is surfaced under `user` (it rides on a carrier) AND - the
    // sanctioned behavior change (§6.4) - it now OPENS a new turn (the answer is a
    // genuine user message that was previously missed as a boundary). So the hit
    // lands in turn 1 (the genuine "pick one" opener is turn 0).
    assert_eq!(ex.len(), 1);
    assert_eq!(ex[0].turn_index, 1);
    assert!(ex[0]
        .hits
        .iter()
        .any(|h| h.class == Class::UserAnswer && h.excerpt.contains("bold option")));
}

#[test]
fn auq_alternate_phrasing_surfaces_under_user_end_to_end() {
    // DEFECT 1: the dominant real-data phrasing ("Your questions have been
    // answered: …") must surface under `user`, exactly like the other phrasing.
    // (Previously the single hardcoded marker missed it entirely.)
    let lines = vec![
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pick one"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
        r#"{"type":"user","uuid":"ans","parentUuid":"a0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"Your questions have been answered: \"which?\"=\"the teal option\". You can now continue with these answers in mind."}]}}"#,
    ];
    let mut a = args("teal option");
    a.labels = vec!["user".to_string()];
    let ex = search(&lines, &a);
    assert_eq!(
        ex.len(),
        1,
        "alternate AUQ phrasing must surface a user hit"
    );
    // §6.4 behavior change: the answer opens its own turn (turn 1), after the
    // genuine "pick one" opener (turn 0).
    assert_eq!(ex[0].turn_index, 1);
    assert!(ex[0]
        .hits
        .iter()
        .any(|h| h.class == Class::UserAnswer && h.excerpt.contains("teal option")));
}

#[test]
fn auq_answer_not_double_counted_no_category_filter() {
    // DEFECT 3: with NO -t filter (all categories active) an AUQ answer must be
    // surfaced ONCE (under `user`), NOT twice (also `tool-response`). SPEC §5.
    let lines = vec![
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pick one"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
        r#"{"type":"user","uuid":"ans","parentUuid":"a0","timestamp":"2026-06-07T05:00:30.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"zzqq choice\". You can now continue."}]}}"#,
    ];
    // No selector filter → every label eligible.
    let ex = search(&lines, &args("zzqq"));
    assert_eq!(ex.len(), 1);
    let cats: Vec<Class> = ex[0].hits.iter().map(|h| h.class).collect();
    assert_eq!(
        cats,
        vec![Class::UserAnswer],
        "AUQ answer must appear ONCE as the richest view `user.answer`, not also agent.tool.result"
    );
}

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
    // that does not also name `user` - it is genuinely a tool_result.
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
