//! A gated ATTACHMENT the scan cannot search is still a node of the chain.
//!
//! The two default-on candidate needles admit attachment lines a flagless scan has no
//! leaf for: the v0.11.0 channel literal, and the D7 `compact_boundary` value substring,
//! which keeps any payload that merely mentions the word. Such a record is demoted to a
//! structural spine row rather than removed, because the survival axis walks `parentUuid`
//! THROUGH attachment records - drop one and the walk cannot resolve the parent below it,
//! its floor freezes at the break, and the whole region above reads `pre-cut`.

use super::*;

const SENC: &str = "-Users-dev-example-lagoon";
const SSESS: &str = "5c3d2b1a-7e6f-4d5c-9b8a-2f1e0d9c8b7a";

/// L1 the prompt, L2 its reply, L3 an attachment whose payload MENTIONS
/// `compact_boundary` (the D7 needle admits it on a flagless scan; the payload type is
/// not `hook_additional_context`, so no leaf a flagless scan can reach renders it), L4 a
/// prompt parented to that attachment, L5 its reply, L6 the `last-prompt` leaf marker.
///
/// The 60 SECOND spacing is load-bearing: `walk::repair` bridges a hole to a record at
/// most 5 s earlier, so a tighter fixture hides the break behind the repair.
fn mention_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{SENC}/{SSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"charting the lagoon"}]}}"#, "\n",
            r#"{"type":"attachment","uuid":"at1","parentUuid":"a1","timestamp":"2026-06-07T05:02:00.000Z","attachment":{"type":"task_reminder","content":"render compact_boundary metadata"}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"at1","timestamp":"2026-06-07T05:03:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:04:00.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"dredging the northern channel"}]}}"#, "\n",
            r#"{"type":"last-prompt","leafUuid":"a2"}"#, "\n",
        ),
    );
    h
}

/// Every emitted unit's `line -> survival`, whatever surface produced it: a `search`
/// exchange row nests its units under `hits`, a `show` record row IS the unit.
fn survival_by_line(out: &str) -> Vec<(u64, String)> {
    let mut v: Vec<(u64, String)> = Vec::new();
    for row in rows(out) {
        let units = match row["hits"].as_array() {
            Some(hits) => hits.clone(),
            None if row["kind"] == "record" => vec![row.clone()],
            None => continue,
        };
        for u in units {
            if let (Some(line), Some(s)) = (u["line"].as_u64(), u["survival"].as_str()) {
                v.push((line, s.to_string()));
            }
        }
    }
    v.sort();
    v.dedup();
    v
}

#[test]
fn an_attachment_the_flagless_scan_cannot_search_still_carries_the_chain() {
    let h = mention_home();
    let out = h.run(&["search", "", &at(SSESS), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);

    // The break was ABOVE the attachment: with the record deleted, the walk from the leaf
    // reached L5 and L4, failed to resolve `at1`, and froze - marking L1 and L2 `pre-cut`
    // on a transcript that never compacted.
    let surv = survival_by_line(&out.stdout);
    assert_eq!(
        surv,
        vec![
            (1, "live".to_string()),
            (2, "live".to_string()),
            (4, "live".to_string()),
            (5, "live".to_string()),
        ],
        "every conversation record is live - nothing here was ever cut:\n{}",
        out.stdout
    );

    // And the summary agrees with itself: no boundary, so no cut and nothing abandoned.
    let s = summary(&out.stdout);
    assert!(
        s["boundary_cut_line"].is_null(),
        "no boundary record exists in this transcript: {s}"
    );
    assert_eq!(s["abandoned_records"], 0, "nothing is off the chain: {s}");
    assert_eq!(s["rewound_turns"], 0, "and nothing was rewound: {s}");

    // The demotion is not a leak: the attachment emits no unit of its own, and the label
    // census never learns it exists.
    assert!(
        !surv.iter().any(|(line, _)| *line == 3),
        "the gated attachment emits nothing on a flagless scan:\n{}",
        out.stdout
    );
    let census = h.run(&["search", "", &at(SSESS), "--count-by", "label"]);
    assert!(census.success, "stderr: {}", census.stderr);
    assert!(
        !census.stdout.contains("harness.meta.attachment")
            && !census.stdout.contains("harness.meta.hook"),
        "no attachment leaf reaches a flagless census:\n{}",
        census.stdout
    );
    // The TURN axis says the same thing from the other side: the demoted line joins no
    // turn key, and the two live turns keep the record counts they had.
    let by_turn = h.run(&[
        "search",
        "",
        &at(SSESS),
        "--count-by",
        "turn",
        "--format",
        "json",
    ]);
    assert!(by_turn.success, "stderr: {}", by_turn.stderr);
    let keys: Vec<(String, u64)> = rows(&by_turn.stdout)
        .into_iter()
        .filter(|r| r["kind"] == "census")
        .filter_map(|r| Some((r["key"].as_str()?.to_string(), r["records"].as_u64()?)))
        .collect();
    assert_eq!(
        keys,
        vec![("t0".to_string(), 2), ("t1".to_string(), 2)],
        "two turn keys, two records each - the demoted line adds none:\n{}",
        by_turn.stdout
    );
    let s = summary(&by_turn.stdout);
    assert_eq!(s["matched_records"], 4, "four records censused: {s}");
    assert_eq!(s["excluded_records"], 0, "none outside the axis: {s}");
}

#[test]
fn the_chain_answer_is_the_same_through_every_entry() {
    // The gates decide what a scan can SEARCH. They must never decide what the chain
    // SEES: a record demoted for one query and parsed whole for another has to reach the
    // walk either way, so every entry into this transcript reports the same survival.
    let h = mention_home();
    let entries: [(&str, Vec<String>); 5] = [
        (
            "flagless",
            vec![
                "search".into(),
                String::new(),
                at(SSESS),
                "--format".into(),
                "json".into(),
            ],
        ),
        (
            "-t user",
            vec![
                "search".into(),
                String::new(),
                at(SSESS),
                "-t".into(),
                "user".into(),
                "--format".into(),
                "json".into(),
            ],
        ),
        (
            "--attachments",
            vec![
                "search".into(),
                String::new(),
                at(SSESS),
                "--attachments".into(),
                "--format".into(),
                "json".into(),
            ],
        ),
        (
            "--additional-context",
            vec![
                "search".into(),
                String::new(),
                at(SSESS),
                "--additional-context".into(),
                "--format".into(),
                "json".into(),
            ],
        ),
        (
            "show --line 1..5",
            vec![
                "show".into(),
                at(SSESS),
                "--line".into(),
                "1..5".into(),
                "--format".into(),
                "json".into(),
            ],
        ),
    ];

    let mut seen: Vec<(&str, Vec<(u64, String)>)> = Vec::new();
    let mut chain_summaries: Vec<(&str, serde_json::Value)> = Vec::new();
    for (name, argv) in &entries {
        let args: Vec<&str> = argv.iter().map(String::as_str).collect();
        let out = h.run(&args);
        assert!(out.success, "{name} stderr: {}", out.stderr);
        seen.push((name, survival_by_line(&out.stdout)));
        // `show` reports no chain totals of its own; the four search entries do.
        if name != &"show --line 1..5" {
            let s = summary(&out.stdout);
            chain_summaries.push((
                name,
                serde_json::json!({
                    "leaf_source": s["leaf_source"].clone(),
                    "boundary_cut_line": s["boundary_cut_line"].clone(),
                    "abandoned_records": s["abandoned_records"].clone(),
                    "rewound_turns": s["rewound_turns"].clone(),
                    "replay_copies": s["replay_copies"].clone(),
                }),
            ));
        }
    }

    // Survival on every line the two entries share. The SETS differ by design (a leaf a
    // gate closed emits nothing), the ANSWERS may not.
    let (base_name, base) = &seen[0];
    for (name, other) in &seen[1..] {
        for (line, state) in other {
            if let Some((_, want)) = base.iter().find(|(l, _)| l == line) {
                assert_eq!(
                    state, want,
                    "{name} reads L{line} as {state} where {base_name} reads {want}"
                );
            }
        }
    }
    // Everything the chain totals up is one answer per transcript, not per query.
    let (first_name, first) = &chain_summaries[0];
    for (name, other) in &chain_summaries[1..] {
        assert_eq!(other, first, "{name} chain totals differ from {first_name}");
    }
    // ... and that one answer is the uncut transcript.
    assert_eq!(first["boundary_cut_line"], serde_json::Value::Null);
    assert_eq!(first["abandoned_records"], 0);
}

const PENC: &str = "-Users-dev-example-harbor";
const PSESS: &str = "7b6a5c4d-3e2f-4a1b-9c8d-0e1f2a3b4c5d";

/// The demotion is a FIELD REDUCTION, and only a fixture richer than any real attachment
/// can prove it from the outside. `Kept.spine` alone suppresses EMISSION, so a demotion
/// that kept the whole record would still emit nothing and every other test here would
/// stay green - but the per-file indexes `turns_match` builds walk the record slice
/// WITHOUT consulting `spine` (`PlanIndex::from_records`, `build_tool_name_index`,
/// `tool_pair_ids`, `SummarizeIndex::from_records`, `resume_prompt_uuids`), so a record
/// that kept its payload would still feed them.
///
/// A real `plan_mode` attachment carries no `message`, so none of those indexes can see
/// it whether it is reduced or not; this fixture therefore hangs an `ExitPlanMode`
/// `tool_use` on the attachment record BESIDE the `plan_mode` payload. Claude Code writes
/// no such line - that is the point: the assertion holds for a record carrying fields it
/// should not, which is what "reduced to the chain fields and nothing else" means.
///
/// L1 the prompt, L2 that attachment, L3 the rejection naming the plan's tool_use id
/// (which renders a `[plan: <path>]` pointer when `PlanIndex` resolves it), L4 the reply,
/// L5 the leaf marker.
fn plan_payload_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{PENC}/{PSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"plan the harbor survey"}}"#, "\n",
            r#"{"type":"attachment","uuid":"at1","parentUuid":"u1","timestamp":"2026-06-07T05:01:00.000Z","attachment":{"type":"plan_mode","planFilePath":"/Users/dev/plans/harbor.md","note":"entered plan mode before compact_boundary metadata was written"},"message":{"role":"assistant","id":"m1","content":[{"type":"tool_use","id":"toolu_PLANX","name":"ExitPlanMode","input":{"plan":"survey the harbor","planFilePath":"/Users/dev/plans/harbor.md"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"rej","parentUuid":"at1","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_PLANX","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected. To tell you how to proceed, the user said:\nrun the smoke tests once before calling it done"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"rej","timestamp":"2026-06-07T05:03:00.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"adding the smoke-test check"}]}}"#, "\n",
            r#"{"type":"last-prompt","leafUuid":"a2"}"#, "\n",
        ),
    );
    h
}

#[test]
fn a_demoted_record_feeds_no_per_file_index_a_flagless_scan_builds() {
    let h = plan_payload_home();
    const POINTER: &str = "[plan: /Users/dev/plans/harbor.md]";

    // Flagless: the attachment is reduced to its chain fields, so the plan index never
    // learns the tool_use id and the rejection renders with no pointer.
    let bare = h.run(&["search", "smoke tests", &at(PSESS)]);
    assert!(bare.success, "stderr: {}", bare.stderr);
    assert!(
        bare.stdout.contains("user.rejection") && bare.stdout.contains("smoke tests"),
        "the rejection itself still surfaces:\n{}",
        bare.stdout
    );
    assert!(
        !bare.stdout.contains(POINTER),
        "a demoted record binds no plan - it kept structure and nothing else:\n{}",
        bare.stdout
    );

    // The flag keeps the same record WHOLE, and the pointer comes back. Same file, same
    // pattern, same rejection: the only variable is whether the record was reduced.
    let flagged = h.run(&["search", "smoke tests", &at(PSESS), "--attachments"]);
    assert!(flagged.success, "stderr: {}", flagged.stderr);
    assert!(
        flagged.stdout.contains(POINTER),
        "--attachments admits the record whole, so the plan resolves:\n{}",
        flagged.stdout
    );

    // And the demotion still does not cost the chain: every conversation record is live.
    let json = h.run(&["search", "", &at(PSESS), "--format", "json"]);
    assert!(json.success, "stderr: {}", json.stderr);
    assert!(
        survival_by_line(&json.stdout)
            .iter()
            .all(|(_, s)| s == "live"),
        "the chain reads through the demoted attachment:\n{}",
        json.stdout
    );
}
