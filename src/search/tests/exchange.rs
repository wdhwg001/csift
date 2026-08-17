//! Exchange assembly: turn delimiting, chronological ordering, label/turn/time windows.

use super::*;

#[test]
fn turn_range_parsing() {
    assert_eq!(
        parse_turn_range("10..20").unwrap().resolve(100, false),
        (10, 20)
    );
    assert_eq!(
        parse_turn_range("0..0").unwrap().resolve(100, false),
        (0, 0)
    );
    assert!(parse_turn_range("20..10").is_err());
    assert!(parse_turn_range("notarange").is_err());
    assert!(parse_turn_range("a..b").is_err());
}

// ── End-to-end exchange reconstruction over a synthetic multi-turn fixture ──
//
// Records as they appear in a real jsonl (file order). Two genuine-user turns;
// each expands into the assistant chain + a tool round-trip. Interleaved noise:
// an isMeta pseudo-turn and a tool_result carrier — NEITHER may start a turn.

#[test]
fn started_utc_is_none_when_opener_and_hits_lack_timestamps() {
    // A genuine-user opener with NO `timestamp` and no later hit ts → started_utc is None
    // (the `.or_else(hits…)` fallback finds nothing). Such an exchange sorts LAST in the
    // combined timeline (the timestamp_sort_key None arm, asserted separately).
    let lines = vec![
        r#"{"type":"user","uuid":"u0","message":{"role":"user","content":"no ts but matches carry"}}"#,
    ];
    let ex = search(&lines, &args("carry"));
    assert_eq!(ex.len(), 1);
    assert!(
        ex[0].started_utc.is_none(),
        "no opener ts, no hit ts → None"
    );
}

#[test]
fn started_utc_falls_back_to_first_hit_when_opener_lacks_timestamp() {
    // The opener carries no ts, but a later agent record in the turn does → started_utc falls
    // back to that hit's timestamp rather than staying None.
    let lines = vec![
        r#"{"type":"user","uuid":"u0","message":{"role":"user","content":"opener carry no ts"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:09.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply carry"}]}}"#,
    ];
    let mut a = args("carry");
    a.labels = vec!["agent.message".to_string()];
    let ex = search(&lines, &a);
    assert_eq!(ex.len(), 1);
    assert_eq!(
        ex[0].started_utc.as_deref(),
        Some("2026-06-07T05:00:09.000Z"),
        "fallback to the first hit's timestamp"
    );
}

#[test]
fn timestamp_sort_key_orders_timestamped_first_none_last() {
    // ISO-8601 UTC strings sort chronologically as text; a None timestamp sorts LAST.
    let early = timestamp_sort_key(Some("2026-06-07T05:00:00.000Z"));
    let late = timestamp_sort_key(Some("2026-06-07T05:00:05.000Z"));
    let none = timestamp_sort_key(None);
    assert!(early < late, "earlier ts sorts first");
    assert!(late < none, "any timestamp sorts before None");
    assert!(early < none);
}

#[test]
fn label_not_excludes_and_composes_with_include() {
    // Premise: without -T the pattern DOES hit a thinking block.
    let mut plain = args("straddling");
    plain.labels = Vec::new();
    let ex0 = search(&fixture(), &plain);
    assert!(
        ex0.iter()
            .flat_map(|e| &e.hits)
            .any(|h| h.class == Class::AgentThinking),
        "fixture premise: 'straddling' hits agent.thinking"
    );
    // -T agent.thinking (no -t): ALL minus thinking — the thinking hit is gone.
    let mut a = args("straddling");
    a.labels_not = vec!["agent.thinking".to_string()];
    let ex = search(&fixture(), &a);
    assert!(
        ex.iter()
            .flat_map(|e| &e.hits)
            .all(|h| h.class != Class::AgentThinking),
        "-T agent.thinking must drop thinking hits"
    );
    // -t agent -T agent.thinking: the agent role minus its thinking leaf.
    let mut b = args("the");
    b.labels = vec!["agent".to_string()];
    b.labels_not = vec!["agent.thinking".to_string()];
    let exb = search(&fixture(), &b);
    for h in exb.iter().flat_map(|e| &e.hits) {
        assert!(
            h.class.path().starts_with("agent") && h.class != Class::AgentThinking,
            "surviving hits stay within -t minus -T; got {}",
            h.class.path()
        );
    }
}

#[test]
fn label_not_renders_the_richest_surviving_view() {
    // A SendMessage tool_use is dual-labeled (comm.sent > tool.use) and normally renders
    // the richer comm view; excluding the comm branch re-renders it under the SURVIVING
    // agent.tool.use label instead of vanishing.
    let lines = vec![
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"SendMessage","input":{"to":"Peer","message":"zzpayload"}}]}}"#,
    ];
    let plain = args("zzpayload");
    let ex = search(&lines, &plain);
    assert_eq!(
        ex.iter().flat_map(|e| &e.hits).next().map(|h| h.class),
        Some(Class::CommSent),
        "the unfiltered view is the richer comm label"
    );
    let mut b = args("zzpayload");
    b.labels_not = vec!["agent.communication".to_string()];
    let exb = search(&lines, &b);
    assert_eq!(
        exb.iter().flat_map(|e| &e.hits).next().map(|h| h.class),
        Some(Class::AgentToolUse),
        "with the comm branch excluded the record renders as the surviving tool.use"
    );
}

#[test]
fn turn_delimiting_two_genuine_users_only() {
    // A regex that matches in both turns; isMeta + carrier must not add turns.
    let ex = search(&fixture(), &args("the"));
    let indices: Vec<usize> = ex.iter().map(|e| e.turn_index).collect();
    assert_eq!(
        indices,
        vec![0, 1],
        "exactly two turns, 0-based, no meta turn"
    );
}

#[test]
fn exchange_returns_full_round_trip() {
    // Match only the thinking block; the emitted exchange's record_uuids must
    // include the WHOLE turn 0 chain (user, thinking, tool_use, carrier, agent),
    // proving the complete round-trip is stitched, not just the matched record.
    let mut a = args("straddling");
    a.labels = vec!["agent.thinking".to_string()];
    let ex = search(&fixture(), &a);
    assert_eq!(ex.len(), 1);
    assert_eq!(ex[0].turn_index, 0);
    assert_eq!(ex[0].hits.len(), 1);
    assert_eq!(ex[0].hits[0].class, Class::AgentThinking);
    // Full turn membership (the carry's complete round-trip).
    let uuids = &ex[0].record_uuids;
    for expected in ["u0", "a0t", "a0u", "c0", "a0f"] {
        assert!(uuids.contains(&expected.to_string()), "missing {expected}");
    }
    // The isMeta record belongs to NEITHER turn's body (it sits after turn 0's
    // agent but before turn 1's user; our grouping appends it to turn 0 as a
    // member — but it is not a turn delimiter). It must be present as a member
    // of turn 0 (a sibling record), never as its own turn.
    assert!(uuids.contains(&"meta".to_string()));
}

#[test]
fn category_filter_restricts_hits() {
    // `-t agent` over a pattern present in both an agent text and a thinking
    // block must only surface the agent hit.
    let mut a = args("carry");
    a.labels = vec!["agent.message".to_string()];
    let ex = search(&fixture(), &a);
    // "carry" appears in turn 0's thinking AND agent text; with -t agent only
    // the agent hit is emitted (one exchange, one agent hit).
    assert_eq!(ex.len(), 1);
    assert!(ex[0].hits.iter().all(|h| h.class == Class::AgentMessage));
}

#[test]
fn turn_range_filter_selects_turn() {
    let mut a = args("");
    a.turn_range = Some("1..1".to_string());
    let ex = search(&fixture(), &a);
    assert_eq!(ex.len(), 1);
    assert_eq!(ex[0].turn_index, 1);
}

#[test]
fn time_window_filters_by_timestamp() {
    // turn 1's records are at 06:00; a since=06:00 window drops turn 0 entirely
    // (its records are at 05:00) and the isMeta member at 05:01.
    let mut a = args("");
    a.since = Some("2026-06-07T06:00:00Z".to_string());
    let ex = search(&fixture(), &a);
    let indices: Vec<usize> = ex.iter().map(|e| e.turn_index).collect();
    assert_eq!(indices, vec![1], "only the 06:00 turn survives the window");
}

#[test]
fn empty_pattern_pure_filter_emits_all_turns() {
    let ex = search(&fixture(), &args(""));
    assert_eq!(ex.len(), 2, "empty pattern matches every turn");
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
