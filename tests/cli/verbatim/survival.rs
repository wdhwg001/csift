//! The SURVIVAL AXIS on `verbatim`: LIVE turns are replayed, abandoned ones are counted
//! and pointed at, and the reconstruction still crosses a compaction cut on purpose.

use crate::harness::*;

const VENC: &str = "-Users-dev-example-project";
const VSESS: &str = "7a6b5c4d-3e2f-4108-9a7b-6c5d4e3f2a19";

fn rows(out: &str) -> Vec<serde_json::Value> {
    out.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// L1 prompt, L2 reply, L3 a hook attachment, L4 the REWOUND prompt, L5 its reply,
/// L6 the resend from the same parent, L7 its reply.
fn rewind_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{VENC}/{VSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting the lagoon"}]}}"#, "\n",
            r#"{"type":"attachment","uuid":"x0","parentUuid":"a0","timestamp":"2026-06-07T05:00:50.000Z","attachment":{"type":"hook_additional_context","content":["session context"]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"x0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging the northern channel"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"x0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"survey the southern shoal instead"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"surveying the southern shoal"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn a_rewound_turn_is_counted_and_pointed_at_never_replayed() {
    let h = rewind_home();
    let out = h.run(&["verbatim", at(VSESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("chart the lagoon") && out.stdout.contains("southern shoal"),
        "the surviving conversation is replayed:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("dredge the northern channel"),
        "a rewound turn is never replayed - the summariser never saw it either:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("abandoned  1 turn(s) off the surviving conversation, not replayed: L4"),
        "the note counts it and names its line:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains(&format!("csift show @{VSESS} --line 4")),
        "and hands over the exact fetch:\n{}",
        out.stdout
    );
}

#[test]
fn the_json_summary_carries_the_abandoned_count_and_units_carry_survival() {
    let h = rewind_home();
    let j = h.run(&["verbatim", at(VSESS).as_str(), "--format", "json"]);
    assert!(j.success, "stderr: {}", j.stderr);
    let all = rows(&j.stdout);
    let summary = all
        .iter()
        .find(|v| v["kind"] == "summary")
        .expect("summary row");
    assert_eq!(summary["abandoned_turns"], 1, "{}", j.stdout);
    let units: Vec<&serde_json::Value> = all.iter().filter(|v| v["kind"] == "turn").collect();
    assert!(!units.is_empty(), "{}", j.stdout);
    assert!(
        units.iter().all(|u| u["survival"] == "live"),
        "a replayed unit is live by construction:\n{}",
        j.stdout
    );
    assert!(
        !units.iter().any(|u| u["line"] == 4),
        "the rewound opener is not a unit:\n{}",
        j.stdout
    );
}

#[test]
fn verbatim_turn_numbers_are_the_numbers_search_prints() {
    // The parity pin: one numbering across the tree. `search --count-by turn` names the
    // live turns; every verbatim unit's turn_index must be one of them.
    let h = rewind_home();
    let census = h.run(&[
        "search",
        "",
        at(VSESS).as_str(),
        "--count-by",
        "turn",
        "--format",
        "json",
    ]);
    let mut search_turns: Vec<u64> = rows(&census.stdout)
        .into_iter()
        .filter(|v| v["axis"] == "turn" && v["key"].is_string())
        .filter_map(|v| {
            v["key"]
                .as_str()
                .and_then(|k| k.trim_start_matches('t').parse::<u64>().ok())
        })
        .collect();
    search_turns.sort_unstable();
    assert_eq!(
        search_turns,
        vec![0, 1],
        "two live turns: {}",
        census.stdout
    );

    let j = h.run(&["verbatim", at(VSESS).as_str(), "--format", "json"]);
    let mut verbatim_turns: Vec<u64> = rows(&j.stdout)
        .into_iter()
        .filter(|v| v["kind"] == "turn")
        .filter_map(|v| v["turn_index"].as_u64())
        .collect();
    verbatim_turns.sort_unstable();
    verbatim_turns.dedup();
    assert_eq!(
        verbatim_turns, search_turns,
        "verbatim numbers the same turns search does:\n{}\n{}",
        j.stdout, census.stdout
    );
}

#[test]
fn a_pre_cut_unit_is_still_reconstructed_and_flagged() {
    // Claude Code's own loader stops at a compaction boundary; csift reads ACROSS it,
    // because restoring what a compaction clipped is the whole command.
    let h = Home::new();
    h.write(
        &format!("{VENC}/{VSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting the lagoon"}]}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"a0","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preTokens":900}}"#, "\n",
            r#"{"type":"user","uuid":"s1","parentUuid":"b1","isCompactSummary":true,"timestamp":"2026-06-07T05:10:01.000Z","message":{"role":"user","content":"summary of the lagoon work"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"s1","timestamp":"2026-06-07T05:11:00.000Z","message":{"role":"user","content":"survey the southern shoal"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:11:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"surveying the southern shoal"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["verbatim", at(VSESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("chart the lagoon"),
        "the clipped turn is exactly what verbatim exists to restore:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("abandoned  "),
        "a cut is not an abandonment:\n{}",
        out.stdout
    );
    let j = h.run(&["verbatim", at(VSESS).as_str(), "--format", "json"]);
    let units = rows(&j.stdout);
    let pre = units
        .iter()
        .find(|v| v["kind"] == "turn" && v["line"] == 1)
        .expect("the pre-cut opener");
    assert_eq!(pre["survival"], "pre-cut", "{}", j.stdout);
    let post = units
        .iter()
        .find(|v| v["kind"] == "turn" && v["line"] == 5)
        .expect("the post-cut opener");
    assert_eq!(post["survival"], "live", "{}", j.stdout);
    let summary = units
        .iter()
        .find(|v| v["kind"] == "summary")
        .expect("summary row");
    assert_eq!(summary["abandoned_turns"], 0, "{}", j.stdout);
}

#[test]
fn an_esc_recalled_draft_is_counted_with_the_rewound_ones() {
    // A draft never drew a reply; a rewound turn did. Both are off the surviving
    // conversation, so both are counted and neither is replayed.
    let h = Home::new();
    h.write(
        &format!("{VENC}/{VSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting the lagoon"}]}}"#, "\n",
            r#"{"type":"user","uuid":"d1","parentUuid":"a0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the chanel"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","timestamp":"2026-06-07T05:01:30.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:35.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging the northern channel"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["verbatim", at(VSESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("dredge the chanel"),
        "the recalled text is not part of the conversation:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("dredge the northern channel"),
        "the sent one is:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("abandoned  1 turn(s) off the surviving conversation, not replayed: L3"),
        "{}",
        out.stdout
    );
}
