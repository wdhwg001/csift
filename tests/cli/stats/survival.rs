//! The SURVIVAL AXIS in `stats`: live turn counts that match `search`, and the three
//! whole-file chain totals.

use crate::harness::*;

/// The SURVIVAL AXIS in `stats`: a rewind and a recalled draft on one transcript, plus
/// the hook attachment the chain walks through. L1 prompt, L2 reply, L3 hook attachment,
/// L4 the REWOUND prompt, L5 its reply, L6 the resend, L7 its reply, L8 a recalled DRAFT,
/// L9 the message that replaced it, L10 its reply.
fn chain_stats_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"attachment","uuid":"x0","parentUuid":"a0","timestamp":"2026-06-07T05:00:50.000Z","attachment":{"type":"hook_additional_context","content":["session context"]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"x0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"x0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"survey the southern shoal"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"surveying"}]}}"#, "\n",
            r#"{"type":"user","uuid":"d0","parentUuid":"a2","timestamp":"2026-06-07T05:03:00.000Z","message":{"role":"user","content":"sound the reef"}}"#, "\n",
            r#"{"type":"user","uuid":"u3","parentUuid":"a2","timestamp":"2026-06-07T05:03:30.000Z","message":{"role":"user","content":"sound the reef margin"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a3","parentUuid":"u3","timestamp":"2026-06-07T05:03:35.000Z","message":{"role":"assistant","id":"m3","content":[{"type":"text","text":"sounding"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn every_session_row_key_is_named_in_the_help() {
    // help IS surface, and the failure it guards against is silent: a new JSON key ships,
    // the help keeps describing the old set, and the manual is wrong with nothing failing.
    // So the help text is asserted against the keys the command actually emits.
    let h = chain_stats_home();
    let j = h.run(&["stats", &at(SESS), "--format", "json"]);
    assert!(j.success, "stderr: {}", j.stderr);
    let row: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("session row");
    let help = h.run(&["stats", "--help"]);
    assert!(help.success, "stderr: {}", help.stderr);
    let missing: Vec<&str> = row
        .as_object()
        .expect("an object row")
        .keys()
        // `kind` belongs to the envelope, which the JSON SCHEMA paragraph describes as a
        // shape rather than as a row field.
        .filter(|k| k.as_str() != "kind")
        .filter(|k| !help.stdout.contains(k.as_str()))
        .map(String::as_str)
        .collect();
    assert!(
        missing.is_empty(),
        "session-row keys absent from `stats --help`: {missing:?}\n{}",
        help.stdout
    );
}

#[test]
fn the_chain_walk_passes_through_a_non_conversation_record() {
    // The spine splice, pinned where its ABSENCE is visible. The live opener L6 is parented
    // to the hook ATTACHMENT L5, so the walk from the tail reaches L2 only THROUGH a record
    // the role prefilter drops. Building the chain over role rows alone stops the walk at
    // L6, leaves the rewound prompt L3 in the numbering (it has no later same-parent sibling
    // to fall back on) and reports 3 turns instead of 2 - which is what this asserts away.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging"}]}}"#, "\n",
            r#"{"type":"attachment","uuid":"x0","parentUuid":"a0","timestamp":"2026-06-07T05:02:00.000Z","attachment":{"type":"hook_additional_context","content":["session context"]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"x0","timestamp":"2026-06-07T05:02:10.000Z","message":{"role":"user","content":"survey the southern shoal"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:02:15.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"surveying"}]}}"#, "\n",
        ),
    );
    let j = h.run(&["stats", &at(SESS), "--format", "json"]);
    assert!(j.success, "stderr: {}", j.stderr);
    let row: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("session row");
    assert_eq!(
        row["turns"], 2,
        "the opener and the resend; the rewound prompt is not a turn: {}",
        j.stdout
    );
    assert_eq!(row["abandoned_turns"], 1, "{}", j.stdout);
    assert_eq!(row["rewound_turns"], 1, "{}", j.stdout);
    // And `search`, which has spliced the spine since the axis landed, agrees.
    let census = h.run(&[
        "search",
        "",
        &at(SESS),
        "--count-by",
        "turn",
        "--format",
        "json",
    ]);
    // The census keys an ABANDONED unit `null` - it belongs to no turn - so the parity is
    // against the rows that carry a real `tN` key, which is what `turns` counts.
    let turn_rows = census
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "census" && !v["key"].is_null())
        .count();
    assert_eq!(turn_rows, 2, "{}", census.stdout);
}

#[test]
fn stats_turns_are_live_turns_and_match_search() {
    // The parity that makes `stats` and `search` speak ONE numbering: `turns` counts the
    // LIVE turns, exactly the `tN` keys `search --count-by turn` reports.
    let h = chain_stats_home();
    let st = h.run(&["stats", &at(SESS), "--format", "json"]);
    assert!(st.success, "stderr: {}", st.stderr);
    let row: serde_json::Value = st
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("session row");
    let census = h.run(&[
        "search",
        "",
        &at(SESS),
        "--count-by",
        "turn",
        "--format",
        "json",
    ]);
    // The census keys an ABANDONED unit `null` - it belongs to no turn - so the parity is
    // against the rows that carry a real `tN` key, which is what `turns` counts.
    let turn_rows = census
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "census" && !v["key"].is_null())
        .count();
    assert_eq!(
        row["turns"], 3,
        "the opener, the resend and its own resend: {}",
        st.stdout
    );
    assert_eq!(
        row["turns"].as_u64().unwrap() as usize,
        turn_rows,
        "stats turns == search --count-by turn rows:\nstats {}\ncensus {}",
        st.stdout,
        census.stdout
    );
}

#[test]
fn the_chain_line_is_printed_only_when_there_is_something_on_it() {
    // An ordinary transcript's block has to stay what it was before the axis existed - a
    // `chain abandoned turns 0 ... replay copy lines 0` row on every session in the corpus
    // is a whole line of noise per session. The line's two triggers are independent, so
    // each is pinned on its own: a replay copy with nothing abandoned still earns it.
    let plain = Home::new();
    plain.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
        ),
    );
    let quiet = plain.run(&["stats", &at(SESS)]);
    assert!(quiet.success, "stderr: {}", quiet.stderr);
    assert!(
        !quiet.stdout.contains("chain "),
        "nothing off the chain, nothing said:\n{}",
        quiet.stdout
    );

    // The same two records written twice: the later pair is the survivor and the earlier
    // pair is a replay copy, with nothing abandoned at all.
    let replayed = Home::new();
    replayed.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
        ),
    );
    let loud = replayed.run(&["stats", &at(SESS)]);
    assert!(
        loud.stdout
            .contains("chain  abandoned turns 0 (0 rewound) · replay copy lines 2"),
        "a replay copy alone earns the line:\n{}",
        loud.stdout
    );
}

#[test]
fn stats_counts_the_survival_totals_and_sums_them() {
    let h = chain_stats_home();
    let out = h.run(&["stats", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("chain  abandoned turns 2 (1 rewound) · replay copy lines 0"),
        "the rewound prompt and the recalled draft, disclosed:\n{}",
        out.stdout
    );
    let j = h.run(&["stats", &at(SESS), "--format", "json"]);
    let rows: Vec<serde_json::Value> = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let row = rows
        .iter()
        .find(|v| v["kind"] == "session")
        .expect("session row");
    assert_eq!(row["abandoned_turns"], 2, "{}", j.stdout);
    assert_eq!(row["rewound_turns"], 1, "{}", j.stdout);
    assert_eq!(row["replay_copies"], 0, "{}", j.stdout);
    let summary = rows.last().expect("summary");
    assert_eq!(summary["abandoned_turns"], 2, "{}", j.stdout);
    assert_eq!(summary["rewound_turns"], 1, "{}", j.stdout);

    // A second transcript with no fork at all reads zeros, and the TOTAL sums both.
    h.write(
        &format!("{ENC}/1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"v0","parentUuid":null,"timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"chart the bay"}}"#, "\n",
            r#"{"type":"assistant","uuid":"b0","parentUuid":"v0","timestamp":"2026-06-07T06:00:05.000Z","message":{"role":"assistant","id":"n0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
        ),
    );
    let both = h.run(&["stats", ENC, "--format", "json"]);
    let brows: Vec<serde_json::Value> = both
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let clean = brows
        .iter()
        .find(|v| v["session_id"] == "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e")
        .expect("the fork-free session row");
    assert_eq!(clean["abandoned_turns"], 0, "{}", both.stdout);
    assert_eq!(clean["rewound_turns"], 0, "{}", both.stdout);
    assert_eq!(clean["replay_copies"], 0, "{}", both.stdout);
    assert_eq!(
        brows.last().expect("summary")["abandoned_turns"],
        2,
        "the TOTAL sums the rows: {}",
        both.stdout
    );
    let text = h.run(&["stats", ENC]);
    assert!(
        text.stdout.contains("TOTAL") && text.stdout.contains("abandoned turns 2 (1 rewound)"),
        "the TOTAL block carries the same line:\n{}",
        text.stdout
    );
}

#[test]
fn stats_turn_window_resolves_against_live_numbering() {
    // `--turn -1..` is the LAST live turn: the rewound prompt and the recalled draft sit
    // outside numbering, so the window lands on the final exchange, never on a rewound one.
    let h = chain_stats_home();
    let out = h.run(&["stats", &at(SESS), "--turn", "-1..", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let row: serde_json::Value = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "session")
        .expect("session row");
    assert_eq!(row["turns"], 1, "{}", out.stdout);
    assert_eq!(
        row["user_records"], 1,
        "only the surviving prompt of the last turn: {}",
        out.stdout
    );
    assert_eq!(row["assistant_records"], 1, "{}", out.stdout);
    // The chain totals are whole-file facts: a window never shrinks them.
    assert_eq!(row["abandoned_turns"], 2, "{}", out.stdout);
}
