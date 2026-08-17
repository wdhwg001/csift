//! search scope: span interleaving, banners, noise rejection, malformed accounting.

use crate::harness::*;

#[test]
fn search_unknown_session_errors() {
    let h = populated_home();
    let out = h.run(&[
        "search",
        "x",
        at("00000000-0000-0000-0000-000000000000").as_str(),
    ]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("no session file found"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn search_empty_session_file_is_skipped() {
    // A project whose session file is EMPTY → search_one_file's `mmap_bytes → None`
    // arm (empty file). The search succeeds with no matches.
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), ""); // zero-byte session
    let out = h.run(&["search", "anything"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no matching exchanges"),
        "got: {}",
        out.stdout
    );
}

#[test]
fn search_skips_non_transcript_noise_lines() {
    // A session padded with attachment / file-history-snapshot / queue-operation lines (no
    // role marker) → search's pre-JSON category prefilter drops them (the
    // `!line_is_transcript_candidate` TRUE arm) while still matching the real turn. (The
    // `compact_boundary` line IS kept by the prefilter now (D7), but carries no `carry` literal
    // and no compactMetadata here, so it produces no spurious hit.)
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"attachment","data":{"x":1}}"#, "\n",
            r#"{"type":"file-history-snapshot","snapshot":{}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","preTokens":1}"#, "\n",
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"real turn with carry token"}}"#, "\n",
            r#"{"type":"queue-operation","op":"x"}"#, "\n",
        ),
    );
    let out = h.run(&["search", "carry", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("matched 1"), "got: {}", out.stdout);
}

#[test]
fn search_timeline_interleaves_subagents_with_top_level_by_timestamp() {
    // The combined timeline is CHRONOLOGICAL, not file-grouped: a subagent exchange whose
    // turn began BETWEEN two parent turns must sort BETWEEN them — even though the subagent
    // file is scanned after the parent file. Parent turns at T=00 and T=10, subagent turn at
    // T=05 → expected envelope order 00 (parent) · 05 (SUBAGENT) · 10 (parent).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"ping alpha"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply alpha"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"ping gamma"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:11.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply gamma"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-sub222.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"sub222","uuid":"s0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"user","content":"ping beta"}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa0","parentUuid":"s0","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"assistant","content":[{"type":"text","text":"sub reply beta"}]}}"#, "\n",
        ),
    );

    let out = h.run(&["search", "ping", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let envelopes: Vec<_> = json_lines(&out.stdout)
        .into_iter()
        .filter(|o| o.get("turn_index").is_some())
        .collect();
    assert_eq!(
        envelopes.len(),
        3,
        "parent ×2 + subagent ×1: {}",
        out.stdout
    );

    // Chronological interleave: 00 (parent) · 05 (SUBAGENT, between) · 10 (parent).
    assert_eq!(
        envelopes[0]["ts_utc"],
        serde_json::json!("2026-06-07T05:00:00.000Z")
    );
    assert_eq!(envelopes[0]["is_subagent"], serde_json::json!(false));
    assert_eq!(
        envelopes[1]["ts_utc"],
        serde_json::json!("2026-06-07T05:00:05.000Z")
    );
    assert_eq!(
        envelopes[1]["is_subagent"],
        serde_json::json!(true),
        "subagent sorts BETWEEN the two parent turns, not grouped after them"
    );
    assert_eq!(envelopes[1]["parent_session_id"], serde_json::json!(SESS));
    assert_eq!(
        envelopes[2]["ts_utc"],
        serde_json::json!("2026-06-07T05:00:10.000Z")
    );
    assert_eq!(envelopes[2]["is_subagent"], serde_json::json!(false));

    // ts_local is the same instant rendered in the host TZ (present, non-null).
    assert!(
        envelopes[1]["ts_local"].is_string(),
        "envelope carries ts_local"
    );
}

#[test]
fn gated_no_match_still_counts_malformed_exactly() {
    // Mutation pin on the malformed law's GATE path: a no-match literal query lets the
    // whole-file gate close every file WITHOUT building records — the gated accounting
    // must still report the exact malformed count (a degraded `+=` would drift it).
    let h = Home::new();
    let enc = "-Users-dev-example-project";
    let sess = "66778899-aabb-4000-8000-00000000000b";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"clean line"}}"#, "\n",
            r#"{"type":"user","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"torn"#, "\n", // crash-truncated candidate
            r#"free text garbage line"#, "\n", // non-candidate garbage
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"also clean"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "zzgatedmiss",
        &format!("@{sess}"),
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let last: serde_json::Value =
        serde_json::from_str(out.stdout.lines().next_back().unwrap()).unwrap();
    assert_eq!(last["matched"], serde_json::json!(0));
    assert_eq!(
        last["skipped_lines"],
        serde_json::json!(2),
        "exactly the torn candidate + the garbage line: {}",
        out.stdout
    );
}

#[test]
fn scope_banner_splits_top_level_and_subagent_exactly() {
    // Mutation pin: the banner's top-level/subagent split arithmetic (scope_top is the
    // resolved-set remainder after counting subagent paths).
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["search", "", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("2 sessions in scope (1 top-level + 1 subagent)"),
        "exact scope split: {}",
        out.stdout
    );
}

#[test]
fn search_at_uuid_path_scopes_to_session() {
    // `search "" @<uuid>` scopes to that session via the `@<uuid>` PATH positional (the grammar
    // that replaced the removed bare-uuid-pattern routing). An empty pattern = pure filter over
    // scope, so the session's own exchanges come back.
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["search", "", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("{}·t", &SESS[..8])),
        "scoped search should return the session's exchanges: {}",
        out.stdout
    );
}

#[test]
fn search_bare_uuid_is_a_literal_pattern_not_a_scope() {
    // A BARE uuid (no `@`) as the sole positional is now a LITERAL pattern, NOT a session scope.
    // It is searched verbatim across the corpus and emits no scope-routing note.
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["search", SESS]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("is a session id, not a pattern"),
        "a bare uuid must NOT be routed to a scope anymore; stderr: {}",
        out.stderr
    );
}
