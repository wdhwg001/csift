//! The BOUNDED CROSSING across surfaces: one fixture, read at three budgets, and what each
//! command that prints survival says about what it crossed.
//!
//! The fixture is the smallest shape that makes the budget matter: TWO compaction boundaries,
//! each naming a `logicalParentUuid` the file still carries, with a branch above the OLDER one
//! that the conversation was rewound past. At budget 0 the walk stops at the newer boundary and
//! everything above is `pre-cut`; at budget 1 it steps over one and stops at the older; at
//! budget 2 (and at `all`) it reaches the head and the rewound branch up there is resolved.

use crate::harness::*;

const XENC: &str = "-Users-dev-example-project";
const XSESS: &str = "5c4b3a29-1817-4665-8443-2211ffee0099";

fn rows(out: &str) -> Vec<serde_json::Value> {
    out.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn summary(out: &str) -> serde_json::Value {
    rows(out)
        .into_iter()
        .find(|v| v["kind"] == "summary")
        .expect("a summary row")
}

/// L1-L2 the first exchange, L3-L4 a branch off L1 the conversation left behind (L4 carries an
/// image, so `image` has a row whose survival marker moves with the budget), L5 the older
/// boundary (logical parent L2), L6 its summary, L7-L8 the middle exchange, L9 the newer
/// boundary (logical parent L8), L10 its summary, L11-L12 the newest exchange. L2 and L8 each
/// write the same file, so `files` and `recover` have rows on both sides of both cuts.
fn crossing_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{XENC}/{XSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/Users/dev/chart.md","content":"alpha\nbeta\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r0","parentUuid":"u0","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"user","content":"survey the shoal instead"}}"#, "\n",
            r#"{"type":"assistant","uuid":"b0","parentUuid":"r0","timestamp":"2026-06-07T05:00:25.000Z","message":{"role":"assistant","id":"mb","content":[{"type":"text","text":"surveying the shoal"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgoAAAANSUhEUg=="}}]}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","parentUuid":null,"logicalParentUuid":"a0","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preTokens":900}}"#, "\n",
            r#"{"type":"user","uuid":"s1","parentUuid":"cb1","isCompactSummary":true,"timestamp":"2026-06-07T05:10:01.000Z","message":{"role":"user","content":"summary one"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"s1","timestamp":"2026-06-07T05:11:00.000Z","message":{"role":"user","content":"dredge the channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:11:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/Users/dev/chart.md","old_string":"alpha","new_string":"delta"}}]}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"cb2","parentUuid":null,"logicalParentUuid":"a1","timestamp":"2026-06-07T05:20:00.000Z","compactMetadata":{"trigger":"auto","preTokens":800}}"#, "\n",
            r#"{"type":"user","uuid":"s2","parentUuid":"cb2","isCompactSummary":true,"timestamp":"2026-06-07T05:20:01.000Z","message":{"role":"user","content":"summary two"}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"s2","timestamp":"2026-06-07T05:21:00.000Z","message":{"role":"user","content":"buoy the entrance"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:21:05.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"buoying the entrance"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn the_budget_decides_how_far_the_chain_climbs_and_the_summary_states_it() {
    let h = crossing_home();
    let t = at(XSESS);
    let at_budget = |b: &str| -> serde_json::Value {
        let out = h.run(&[
            "search",
            "",
            t.as_str(),
            "--no-subagents",
            "--format",
            "json",
            "--cross-compactions",
            b,
        ]);
        assert!(out.success, "budget {b} stderr: {}", out.stderr);
        summary(&out.stdout)
    };

    // Budget 0: nothing crossed, and nothing said about crossing.
    let zero = at_budget("0");
    assert!(
        zero.get("boundaries_crossed").is_none(),
        "budget 0 is the reading csift gave before the flag existed: {zero}"
    );
    assert_eq!(
        zero["boundary_cut_line"], 9,
        "the newer boundary is the cut"
    );

    // Budget 1: one boundary stepped over, the older one is the floor, the budget ran out.
    let one = at_budget("1");
    assert_eq!(one["crossing_budget"], 1, "{one}");
    assert_eq!(one["boundaries_crossed"], 1, "{one}");
    assert_eq!(one["budget_exhausted"], true, "{one}");
    assert_eq!(one["boundaries_above_floor"], 1, "{one}");
    assert_eq!(one["floor_line"], 5, "the older boundary: {one}");
    assert_eq!(one["floor_cause"], "budget-exhausted", "{one}");

    // Budget 2: both crossed, the walk reaches the head, nothing left above the floor.
    let two = at_budget("2");
    assert_eq!(two["boundaries_crossed"], 2, "{two}");
    assert_eq!(two["budget_exhausted"], false, "{two}");
    assert_eq!(two["boundaries_above_floor"], 0, "{two}");
    assert_eq!(two["floor_cause"], "head", "{two}");
    assert_eq!(two["floor_line"], 1, "{two}");

    // `all` is budget 2 on a file with two boundaries, and says so in its own words.
    let every = at_budget("all");
    assert_eq!(every["crossing_budget"], "all", "{every}");
    assert_eq!(every["boundaries_crossed"], 2, "{every}");
    assert_eq!(every["boundaries_above_floor"], 0, "{every}");
}

#[test]
fn crossing_the_older_cut_resolves_the_branch_above_it() {
    // The POINT of the budget: the rewound branch at L3-L4 sits above BOTH cuts. Until the
    // walk reaches it, csift declines to call it abandoned and it reads `pre-cut` like the
    // rest of the blind region; once the walk gets there it is named for what it is.
    let h = crossing_home();
    let t = at(XSESS);
    let survival_of = |b: &str, line: u64| -> String {
        let out = h.run(&[
            "search",
            "",
            t.as_str(),
            "--no-subagents",
            "--format",
            "json",
            "--cross-compactions",
            b,
        ]);
        rows(&out.stdout)
            .into_iter()
            .filter(|v| v["kind"] == "exchange")
            .flat_map(|v| v["hits"].as_array().cloned().unwrap_or_default())
            .find(|hit| hit["line"] == line)
            .map(|hit| hit["survival"].as_str().unwrap_or_default().to_string())
            .unwrap_or_else(|| format!("no hit at L{line}"))
    };
    assert_eq!(
        survival_of("0", 3),
        "pre-cut",
        "below the floor at budget 0"
    );
    assert_eq!(survival_of("1", 3), "pre-cut", "still below the floor");
    assert_eq!(survival_of("2", 3), "abandoned", "the chain resolved it");
    assert_eq!(survival_of("all", 3), "abandoned");
    // The live conversation above the older cut is LIVE once the walk reaches it.
    assert_eq!(survival_of("0", 1), "pre-cut");
    assert_eq!(survival_of("2", 1), "live");
}

#[test]
fn every_survival_printing_surface_carries_the_same_crossing_facts() {
    // Six text surfaces print the same two footer lines, `stats` prints the same counts on its
    // own dense `chain` row, and every one of them projects the six JSON keys. The fixture is
    // read at budget 1 so the lines carry an exhausted budget and a remedy.
    let h = crossing_home();
    let t = at(XSESS);
    let b = ["--cross-compactions", "1"];

    let text_surfaces: Vec<(&str, Vec<&str>)> = vec![
        ("search", vec!["search", "", t.as_str(), "--no-subagents"]),
        ("show", vec!["show", t.as_str(), "--branch-points"]),
        ("verbatim", vec!["verbatim", t.as_str()]),
        ("files", vec!["files", t.as_str(), "--no-subagents"]),
        (
            "recover",
            vec![
                "recover",
                t.as_str(),
                "--no-subagents",
                "--file",
                "/Users/dev/chart.md",
                "--coverage",
            ],
        ),
        ("image", vec!["image", t.as_str(), "--no-subagents"]),
    ];
    for (name, mut argv) in text_surfaces {
        argv.extend_from_slice(&b);
        let out = h.run(&argv);
        assert!(out.success, "{name} stderr: {}", out.stderr);
        assert!(
            out.stdout.contains(
                "(compaction crossing: 1 boundary(s) stepped over under \
                           --cross-compactions 1, exhausted; 1 above the chain floor were not)"
            ),
            "{name} text:\n{}",
            out.stdout
        );
        assert!(
            out.stdout
                .contains("to reach them: --cross-compactions 2 (or all)"),
            "{name} names the value that would reach the rest:\n{}",
            out.stdout
        );
    }

    // `stats` has one dense line per fact, so it carries the same counts there.
    let st = h.run(&["stats", t.as_str(), "--no-subagents", b[0], b[1]]);
    assert!(st.success, "stderr: {}", st.stderr);
    assert!(
        st.stdout
            .contains("compaction boundaries crossed 1/2 (budget 1, exhausted)"),
        "{}",
        st.stdout
    );

    // JSON: the same six keys on every one of them.
    let json_surfaces: Vec<(&str, Vec<&str>)> = vec![
        (
            "search",
            vec![
                "search",
                "",
                t.as_str(),
                "--no-subagents",
                "--format",
                "json",
            ],
        ),
        (
            "show",
            vec!["show", t.as_str(), "--branch-points", "--format", "json"],
        ),
        (
            "stats",
            vec!["stats", t.as_str(), "--no-subagents", "--format", "json"],
        ),
        ("verbatim", vec!["verbatim", t.as_str(), "--format", "json"]),
        (
            "files",
            vec!["files", t.as_str(), "--no-subagents", "--format", "json"],
        ),
        (
            "recover",
            vec![
                "recover",
                t.as_str(),
                "--no-subagents",
                "--file",
                "/Users/dev/chart.md",
                "--coverage",
                "--format",
                "json",
            ],
        ),
        (
            "image",
            vec!["image", t.as_str(), "--no-subagents", "--format", "json"],
        ),
    ];
    for (name, mut argv) in json_surfaces {
        argv.extend_from_slice(&b);
        let out = h.run(&argv);
        assert!(out.success, "{name} stderr: {}", out.stderr);
        let s = summary(&out.stdout);
        assert_eq!(s["crossing_budget"], 1, "{name}: {s}");
        assert_eq!(s["boundaries_crossed"], 1, "{name}: {s}");
        assert_eq!(s["budget_exhausted"], true, "{name}: {s}");
        assert_eq!(s["boundaries_above_floor"], 1, "{name}: {s}");
        assert_eq!(s["floor_cause"], "budget-exhausted", "{name}: {s}");
        assert!(s["floor_line"].is_number(), "{name}: {s}");
    }
}

#[test]
fn budget_zero_says_nothing_about_crossing_on_any_surface() {
    // The silence rule, swept: at `--cross-compactions 0` csift crossed nothing on purpose,
    // so neither the footer line nor the JSON keys appear anywhere.
    let h = crossing_home();
    let t = at(XSESS);
    let argvs: Vec<Vec<&str>> = vec![
        vec!["search", "", t.as_str(), "--no-subagents"],
        vec!["show", t.as_str(), "--branch-points"],
        vec!["stats", t.as_str(), "--no-subagents"],
        vec!["verbatim", t.as_str()],
        vec!["files", t.as_str(), "--no-subagents"],
        vec!["image", t.as_str(), "--no-subagents"],
    ];
    for mut argv in argvs {
        let name = argv[0];
        argv.extend_from_slice(&["--cross-compactions", "0"]);
        let out = h.run(&argv);
        assert!(out.success, "{name} stderr: {}", out.stderr);
        assert!(
            !out.stdout.contains("compaction crossing")
                && !out.stdout.contains("compaction boundaries crossed"),
            "{name} text:\n{}",
            out.stdout
        );
        let mut jargv: Vec<&str> = argv.clone();
        jargv.extend_from_slice(&["--format", "json"]);
        let jout = h.run(&jargv);
        assert!(jout.success, "{name} json stderr: {}", jout.stderr);
        let s = summary(&jout.stdout);
        for k in [
            "crossing_budget",
            "boundaries_crossed",
            "budget_exhausted",
            "boundaries_above_floor",
            "floor_line",
            "floor_cause",
        ] {
            assert!(s.get(k).is_none(), "{name} still carries {k}: {s}");
        }
    }
}

#[test]
fn a_bad_budget_token_names_both_accepted_forms() {
    let h = crossing_home();
    let out = h.run(&["stats", at(XSESS).as_str(), "--cross-compactions", "lots"]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(out.stderr.contains("or `all`"), "{}", out.stderr);
    assert!(out.stderr.contains("lots"), "{}", out.stderr);
}

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

/// A second transcript in the same project carrying no compaction boundary at all, and (when
/// `mentions_pattern` is false) not even the word the scan looks for, so the section 7f
/// whole-file gate prunes it before any chain is built. Its id sorts AFTER the crossing
/// fixture's, which is what makes it the LAST report folded into the scope's total.
fn add_quiet_sibling(h: &Home, id: &str, mentions_pattern: bool) {
    let body = if mentions_pattern {
        concat!(
            r#"{"type":"user","uuid":"q0","parentUuid":null,"timestamp":"2026-06-07T07:00:00.000Z","message":{"role":"user","content":"dredge the channel again"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"q1","parentUuid":"q0","timestamp":"2026-06-07T07:00:05.000Z","message":{"role":"assistant","id":"qm","content":[{"type":"text","text":"dredging again"}]}}"#,
            "\n",
        )
    } else {
        concat!(
            r#"{"type":"user","uuid":"q0","parentUuid":null,"timestamp":"2026-06-07T07:00:00.000Z","message":{"role":"user","content":"unrelated errand"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"q1","parentUuid":"q0","timestamp":"2026-06-07T07:00:05.000Z","message":{"role":"assistant","id":"qm","content":[{"type":"text","text":"running the errand"}]}}"#,
            "\n",
        )
    };
    h.write(&format!("{XENC}/{id}.jsonl"), body);
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
    assert_eq!(one["boundary_cut_line"], 9, "{one}");
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
