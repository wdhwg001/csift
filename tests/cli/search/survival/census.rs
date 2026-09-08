//! What the census does with a record that belongs to no numbered turn, and what the
//! whole-file gate does with the chain.

use super::*;

const CENC: &str = "-Users-dev-example-atlas";
const CSESS: &str = "5a4b3c2d-1e0f-4998-8776-6655443322aa";

/// ONE numbered turn (L1 prompt, L2 reply, L9 the trailing hook attachment) plus a SIX
/// record branch the conversation was rewound past (L3 the prompt, its reply, two tool
/// calls and their two carriers). Nothing on that branch belongs to a turn.
fn one_turn_six_abandoned() -> Home {
    let h = Home::new();
    h.write(
        &format!("{CENC}/{CSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting the lagoon"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging the northern channel"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"a1","timestamp":"2026-06-07T05:01:06.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/Users/dev/chart.md"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","parentUuid":"a2","timestamp":"2026-06-07T05:01:07.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"depth soundings"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a3","parentUuid":"c1","timestamp":"2026-06-07T05:01:08.000Z","message":{"role":"assistant","id":"m3","content":[{"type":"tool_use","id":"t2","name":"Read","input":{"file_path":"/Users/dev/depths.md"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c2","parentUuid":"a3","timestamp":"2026-06-07T05:01:09.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"more soundings"}]}}"#, "\n",
            r#"{"type":"attachment","uuid":"x0","parentUuid":"a0","timestamp":"2026-06-07T05:02:00.000Z","attachment":{"type":"hook_additional_context","content":["session context"]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn count_by_turn_gives_the_abandoned_records_their_own_bucket_never_t0() {
    let h = one_turn_six_abandoned();
    let out = h.run(&["search", "", &at(CSESS), "--count-by", "turn"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // One numbered turn, and the records outside it are named as such rather than folded
    // into turn zero - which is what a numeric placeholder used to do to every one of them.
    assert!(
        out.stdout.contains("t0") && !out.stdout.contains("t1"),
        "exactly one numbered turn:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("abandoned"),
        "the abandoned bucket is named:\n{}",
        out.stdout
    );
    let json = h.run(&[
        "search",
        "",
        &at(CSESS),
        "--count-by",
        "turn",
        "--format",
        "json",
    ]);
    let census: Vec<serde_json::Value> = rows(&json.stdout)
        .into_iter()
        .filter(|v| v["axis"] == "turn" && v.get("key").is_some())
        .collect();
    let abandoned = census
        .iter()
        .find(|v| v["key"].is_null())
        .expect("a null-key census row for the records outside every turn");
    assert_eq!(
        abandoned["records"], 6,
        "six records on the rewound branch:\n{}",
        json.stdout
    );
    let t0 = census
        .iter()
        .find(|v| v["key"] == "t0")
        .expect("the numbered turn");
    assert_eq!(t0["records"], 2, "the live turn keeps its own count only");
}

#[test]
fn a_file_the_whole_file_gate_proves_cannot_match_builds_no_chain() {
    let h = one_turn_six_abandoned();
    // A pattern the file cannot carry: §7f proves it pre-JSON and skips record building
    // entirely, so no spine row is lifted and no survival fact is claimed.
    let gated = h.run(&[
        "search",
        "zzqxnotinthisfilezz",
        &at(CSESS),
        "--format",
        "json",
    ]);
    assert!(gated.success, "stderr: {}", gated.stderr);
    let s = summary(&gated.stdout);
    assert_eq!(s["matched"], 0);
    assert_eq!(s["skipped_lines"], 0, "the malformed count stays exact");
    assert_eq!(
        s["abandoned_records"], 0,
        "the gate skipped the file, so nothing was walked:\n{}",
        gated.stdout
    );
    assert!(
        s["leaf_source"].is_null() && s["boundary_cut_line"].is_null(),
        "no leaf was picked on a file that was never read:\n{}",
        gated.stdout
    );
    // The same file, a pattern it DOES carry: now the chain runs and discloses the branch.
    let scanned = h.run(&["search", "soundings", &at(CSESS), "--format", "json"]);
    let s2 = summary(&scanned.stdout);
    assert_eq!(
        s2["abandoned_records"], 6,
        "the chain is built when the file is actually scanned:\n{}",
        scanned.stdout
    );
    assert!(!s2["leaf_source"].is_null());
}
