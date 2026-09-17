//! What a SCOPE of several transcripts says: which facts add, which are one file's and
//! drop, and what a file with nothing to say may not do to the total.

use super::*;

#[test]
fn a_scope_of_two_transcripts_folds_the_counts_and_drops_the_one_floor_it_cannot_name() {
    // A scope has as many floors as it has files. The COUNTS add - the total says 2 of 4
    // boundaries crossed over two identical transcripts - while the floor's LINE and CAUSE
    // are single-transcript facts and go null, the rule `boundary_cut_line` already follows.
    // Naming one file's floor on a two-file total would name the wrong one.
    let h = crossing_home();
    let second = "5c4b3a29-1817-4665-8443-2211ffee00aa";
    let body = std::fs::read_to_string(
        h.root
            .join(".claude/projects")
            .join(XENC)
            .join(format!("{XSESS}.jsonl")),
    )
    .expect("the first transcript");
    h.write(&format!("{XENC}/{second}.jsonl"), &body);

    let text = h.run(&["stats", XENC, "--no-subagents", "--cross-compactions", "1"]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(
        text.stdout
            .contains("compaction boundaries crossed 2/4 (budget 1, exhausted)"),
        "the TOTAL folds both transcripts:\n{}",
        text.stdout
    );

    let out = h.run(&[
        "stats",
        XENC,
        "--no-subagents",
        "--format",
        "json",
        "--cross-compactions",
        "1",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let s = summary(&out.stdout);
    assert_eq!(s["boundaries_crossed"], 2, "{s}");
    assert_eq!(s["boundaries_above_floor"], 2, "{s}");
    assert_eq!(s["budget_exhausted"], true, "{s}");
    assert!(s["floor_line"].is_null(), "no line names two files: {s}");
    assert!(
        s["floor_cause"].is_null(),
        "and neither does one cause: {s}"
    );
    // Each session ROW still names its own floor - the drop is the TOTAL's rule alone.
    let session_rows: Vec<serde_json::Value> = rows(&out.stdout)
        .into_iter()
        .filter(|v| v["kind"] == "session")
        .collect();
    assert_eq!(session_rows.len(), 2, "{}", out.stdout);
    for r in session_rows {
        assert_eq!(r["floor_line"], 5, "{r}");
        assert_eq!(r["floor_cause"], "budget-exhausted", "{r}");
    }
}

#[test]
fn a_transcript_with_nothing_to_say_cannot_turn_the_scopes_disclosure_off() {
    // THE BUDGET IS THE RUN'S, never a file's. Every per-file report is BUILT carrying it, so a
    // file the whole-file gate pruned and a file with no records at all both fold in as
    // "crossed nothing under budget B" rather than as "crossing off". Read off the LAST member
    // instead, one such file deleted both footer lines and all six JSON keys for the whole
    // scope - and whether it did depended on where its name sorted.
    let h = crossing_home();
    add_quiet_sibling(&h, "6d5c4b3a-2928-4776-8554-3322ffee0099", false);
    h.write(
        &format!("{XENC}/7e6d5c4b-3a39-4887-8665-4433ffee0099.jsonl"),
        "",
    );

    let out = h.run(&[
        "search",
        "dredge",
        XENC,
        "--no-subagents",
        "--format",
        "json",
        "--cross-compactions",
        "1",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let s = summary(&out.stdout);
    assert_eq!(s["crossing_budget"], 1, "{s}");
    assert_eq!(
        s["boundaries_crossed"], 1,
        "the one compacted transcript still counts: {s}"
    );
    assert_eq!(s["boundaries_above_floor"], 1, "{s}");
    assert_eq!(s["budget_exhausted"], true, "{s}");

    let text = h.run(&[
        "search",
        "dredge",
        XENC,
        "--no-subagents",
        "--cross-compactions",
        "1",
    ]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(
        text.stdout
            .contains("(compaction crossing: 1 boundary(s) stepped over"),
        "the footer survives a pruned sibling:\n{}",
        text.stdout
    );
}

#[test]
fn every_spanning_surface_keeps_the_keys_with_an_empty_transcript_in_scope() {
    // The same rule on every surface that spans sessions, through the path that produces a
    // report for a file holding no records at all (each surface's own early return).
    let h = crossing_home();
    h.write(
        &format!("{XENC}/7e6d5c4b-3a39-4887-8665-4433ffee0099.jsonl"),
        "",
    );
    let b = ["--cross-compactions", "1"];
    let cases: Vec<(&str, Vec<&str>)> = vec![
        (
            "search",
            vec!["search", "", XENC, "--no-subagents", "--format", "json"],
        ),
        (
            "stats",
            vec!["stats", XENC, "--no-subagents", "--format", "json"],
        ),
        (
            "files",
            vec!["files", XENC, "--no-subagents", "--format", "json"],
        ),
        (
            "image",
            vec!["image", XENC, "--no-subagents", "--format", "json"],
        ),
        ("verbatim", vec!["verbatim", XENC, "--format", "json"]),
        (
            "recover",
            vec![
                "recover",
                XENC,
                "--no-subagents",
                "--file",
                "/Users/dev/chart.md",
                "--coverage",
                "--format",
                "json",
            ],
        ),
    ];
    for (name, mut argv) in cases {
        argv.extend_from_slice(&b);
        let out = h.run(&argv);
        assert!(out.success, "{name} stderr: {}", out.stderr);
        let s = summary(&out.stdout);
        assert_eq!(s["crossing_budget"], 1, "{name}: {s}");
        assert_eq!(s["boundaries_crossed"], 1, "{name}: {s}");
        assert_eq!(s["budget_exhausted"], true, "{name}: {s}");
    }
}

#[test]
fn the_scope_names_one_floor_only_when_it_holds_one_transcript() {
    // The fold's first-member test is an explicit COUNTER, not a predicate over the very fields
    // the fold clears. Read off those fields it answered "first" again on every OTHER member,
    // so an ODD-sized scope published one transcript's floor as the whole scope's while an
    // even one published none - the same scope, two different answers, decided by parity.
    let h = crossing_home();
    let body = std::fs::read_to_string(
        h.root
            .join(".claude/projects")
            .join(XENC)
            .join(format!("{XSESS}.jsonl")),
    )
    .expect("the first transcript");
    let ids = [
        "6d5c4b3a-2928-4776-8554-3322ffee0099",
        "7e6d5c4b-3a39-4887-8665-4433ffee0099",
        "8f7e6d5c-4b4a-4998-8776-5544ffee0099",
    ];

    // `search` folds through the counts that carry the pre-existing twins of this rule
    // (`boundary_cut_line` and `leaf_source`), so its summary is where parity showed.
    let scope = |h: &Home| -> serde_json::Value {
        let out = h.run(&[
            "search",
            "",
            XENC,
            "--no-subagents",
            "--format",
            "json",
            "--cross-compactions",
            "1",
        ]);
        assert!(out.success, "stderr: {}", out.stderr);
        summary(&out.stdout)
    };

    // One transcript: the scope IS that transcript, so its floor is the scope's.
    let one = scope(&h);
    assert_eq!(one["floor_line"], 5, "{one}");
    assert_eq!(one["floor_cause"], "budget-exhausted", "{one}");
    assert_eq!(one["boundary_cut_line"], one["floor_line"], "{one}");
    assert!(one["leaf_source"].is_string(), "{one}");

    for (n, id) in ids.iter().enumerate() {
        h.write(&format!("{XENC}/{id}.jsonl"), &body);
        let total = n + 2; // the crossing fixture plus the copies added so far
        let s = scope(&h);
        assert_eq!(
            s["boundaries_crossed"], total,
            "{total} transcripts each cross one: {s}"
        );
        assert!(
            s["floor_line"].is_null(),
            "{total} transcripts name no single floor: {s}"
        );
        assert!(s["floor_cause"].is_null(), "{total} transcripts: {s}");
        assert!(
            s["boundary_cut_line"].is_null(),
            "{total} transcripts, the same rule on the older twin: {s}"
        );
        assert!(s["leaf_source"].is_null(), "{total} transcripts: {s}");
    }
}

/// A transcript whose single compaction boundary the walk can never climb through: its
/// `logicalParentUuid` names nothing on any line. Nothing is crossed at any budget, the floor
/// is that boundary, and the ONE fact the scope has about it is that a boundary sits above
/// the floor uncrossed - which is exactly the report a fold reduces to, since the fold drops
/// the cause.
fn add_shut_door_transcript(h: &Home, id: &str) {
    h.write(
        &format!("{XENC}/{id}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"z0","parentUuid":null,"timestamp":"2026-06-07T08:00:00.000Z","message":{"role":"user","content":"sound the bar"}}"#, "\n",
            r#"{"type":"assistant","uuid":"z1","parentUuid":"z0","timestamp":"2026-06-07T08:00:05.000Z","message":{"role":"assistant","id":"zm","content":[{"type":"tool_use","id":"zw","name":"Write","input":{"file_path":"/Users/dev/chart.md","content":"one\ntwo\n"}}]}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"zb","parentUuid":null,"logicalParentUuid":"gone-from-disk","timestamp":"2026-06-07T08:10:00.000Z","compactMetadata":{"trigger":"auto","preTokens":700}}"#, "\n",
            r#"{"type":"user","uuid":"zs","parentUuid":"zb","isCompactSummary":true,"timestamp":"2026-06-07T08:10:01.000Z","message":{"role":"user","content":"summary z"}}"#, "\n",
            r#"{"type":"user","uuid":"z2","parentUuid":"zs","timestamp":"2026-06-07T08:11:00.000Z","message":{"role":"user","content":"dredge the bar"}}"#, "\n",
            r#"{"type":"assistant","uuid":"z3","parentUuid":"z2","timestamp":"2026-06-07T08:11:05.000Z","message":{"role":"assistant","id":"zn","content":[{"type":"text","text":"dredging the bar"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgoAAAANSUhEUg=="}}]}}"#, "\n",
        ),
    );
}

#[test]
fn a_scope_that_crossed_nothing_still_says_what_it_did_not_cross() {
    // The keys and the lines answer ONE question - is there anything to say about crossing -
    // and they used to answer it differently. A transcript whose walk stops at a shut door
    // crosses nothing, so the only fact it has is the boundary left above its floor; alone it
    // printed both lines, because the CAUSE was there to say so. Fold two such transcripts
    // and the fold drops the cause, which is a rule about not naming one file's floor for a
    // scope - and the text went silent while the JSON still read
    // `boundaries_above_floor: 2`. Silence and a non-zero count are not the same answer.
    let h = Home::new();
    add_shut_door_transcript(&h, "1a2b3c4d-5566-4777-8888-99aabbccdd01");
    add_shut_door_transcript(&h, "2b3c4d5e-6677-4888-8999-aabbccddee02");
    let b = ["--cross-compactions", "3"];

    // Each ALONE says both lines: this is the behaviour the fold had to preserve.
    for id in [
        "1a2b3c4d-5566-4777-8888-99aabbccdd01",
        "2b3c4d5e-6677-4888-8999-aabbccddee02",
    ] {
        let one = h.run(&["search", "", &at(id), "--no-subagents", b[0], b[1]]);
        assert!(one.success, "stderr: {}", one.stderr);
        assert!(
            one.stdout.contains("0 boundary(s) stepped over"),
            "one transcript:\n{}",
            one.stdout
        );
        assert!(
            one.stdout.contains("1 above the chain floor were not"),
            "one transcript:\n{}",
            one.stdout
        );
    }

    // TOGETHER - the fold - the text says the same thing the JSON does.
    let surfaces: Vec<(&str, Vec<&str>)> = vec![
        ("search", vec!["search", "", XENC, "--no-subagents"]),
        ("files", vec!["files", XENC, "--no-subagents"]),
        ("image", vec!["image", XENC, "--no-subagents"]),
        ("verbatim", vec!["verbatim", XENC]),
        (
            "recover",
            vec![
                "recover",
                XENC,
                "--no-subagents",
                "--file",
                "/Users/dev/chart.md",
                "--coverage",
            ],
        ),
    ];
    for (name, mut argv) in surfaces {
        argv.extend_from_slice(&b);
        let out = h.run(&argv);
        assert!(out.success, "{name} stderr: {}", out.stderr);
        let both = format!("{}{}", out.stdout, out.stderr);
        assert!(
            both.contains("0 boundary(s) stepped over"),
            "{name} went silent about a scope that crossed nothing:\n{both}"
        );
        // WHERE the text folds, the counts add and the floor is dropped. `verbatim` and
        // `recover` render one BLOCK per session and print each session's own footer inside
        // it, so their text carries the per-session count twice rather than the total (both
        // JSON summaries fold like every other). Either way the text is not silent, which is
        // the rule under test.
        if name == "verbatim" || name == "recover" {
            assert_eq!(
                both.matches("1 above the chain floor were not").count(),
                2,
                "{name} prints one footer per session block:\n{both}"
            );
        } else {
            assert!(
                both.contains("2 above the chain floor were not"),
                "{name}: the counts add over the fold:\n{both}"
            );
        }
        // Neither of these floors is one a budget could move, so no line offers a value.
        assert!(
            both.contains("raising the budget reaches none of them"),
            "{name}: {both}"
        );
        assert!(
            !both.contains("to try them"),
            "{name} offers a value no floor here could use:\n{both}"
        );

        let mut jargv = argv.clone();
        jargv.extend_from_slice(&["--format", "json"]);
        let jout = h.run(&jargv);
        assert!(jout.success, "{name} json stderr: {}", jout.stderr);
        let s = summary(&jout.stdout);
        assert_eq!(s["boundaries_crossed"], 0, "{name}: {s}");
        assert_eq!(s["boundaries_above_floor"], 2, "{name}: {s}");
        assert!(s["floor_cause"].is_null(), "{name}: {s}");
    }

    // `stats` keeps its own shape: a dense segment on each SESSION row and on the TOTAL.
    let st = h.run(&["stats", XENC, "--no-subagents", b[0], b[1]]);
    assert!(st.success, "stderr: {}", st.stderr);
    assert_eq!(
        st.stdout
            .matches("compaction boundaries crossed 0/1")
            .count(),
        2,
        "one segment per session row:\n{}",
        st.stdout
    );
    assert!(
        st.stdout.contains("compaction boundaries crossed 0/2"),
        "and the TOTAL row carries the fold:\n{}",
        st.stdout
    );
    let sj = h.run(&[
        "stats",
        XENC,
        "--no-subagents",
        "--format",
        "json",
        b[0],
        b[1],
    ]);
    assert!(sj.success, "stderr: {}", sj.stderr);
    let s = summary(&sj.stdout);
    assert_eq!(s["boundaries_above_floor"], 2, "{s}");
}
