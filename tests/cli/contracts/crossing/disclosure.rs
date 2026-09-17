//! What every surface SAYS about a crossing: the sweep over each one, the silence at
//! budget 0, and the rules a scope of several transcripts folds by.

use super::*;

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
        ("show --line", vec!["show", t.as_str(), "--line", "1..12"]),
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
                .contains("to try them: --cross-compactions 2 (or all)"),
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
        // The two surfaces that MOVE with the budget and used to say nothing: an ADDRESS
        // (the record it names reads live at one budget and pre-cut at another) and a
        // CENSUS (crossing a cut resolves records above it, so every axis counts differently).
        (
            "show --line",
            vec!["show", t.as_str(), "--line", "1..12", "--format", "json"],
        ),
        (
            "search --count-by",
            vec![
                "search",
                "",
                t.as_str(),
                "--no-subagents",
                "--count-by",
                "label",
                "--format",
                "json",
            ],
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
        vec!["show", t.as_str(), "--line", "1..12"],
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

#[test]
fn an_address_and_a_census_both_move_with_the_budget_and_both_say_so() {
    // Neither surface is a scan over survival, and both ANSWER differently under the flag:
    // `show` renders one record's `survival`, and a census counts records the crossing
    // resolved. Silent, each was a reading with no way to tell which budget produced it.
    let h = crossing_home();
    let t = at(XSESS);

    // The rewound branch at L3 is `pre-cut` until the walk reaches it, then `abandoned`.
    let survival_at = |b: &str| -> String {
        let out = h.run(&[
            "show",
            t.as_str(),
            "--line",
            "3",
            "--format",
            "json",
            "--cross-compactions",
            b,
        ]);
        assert!(out.success, "stderr: {}", out.stderr);
        rows(&out.stdout)
            .into_iter()
            .find(|v| v["kind"] == "record")
            .and_then(|v| v["survival"].as_str().map(str::to_string))
            .unwrap_or_default()
    };
    assert_eq!(survival_at("0"), "pre-cut");
    assert_eq!(survival_at("2"), "abandoned", "the address moved");

    // And the summary that carried no way to tell those two runs apart now does.
    let out = h.run(&[
        "show",
        t.as_str(),
        "--line",
        "3",
        "--format",
        "json",
        "--cross-compactions",
        "2",
    ]);
    let s = summary(&out.stdout);
    assert_eq!(s["crossing_budget"], 2, "{s}");
    assert_eq!(s["boundaries_crossed"], 2, "{s}");

    // The census counts the same records under a label; the keys ride its summary and the
    // two lines ride its stderr, beside the census's own accounting note.
    let census = h.run(&[
        "search",
        "",
        t.as_str(),
        "--no-subagents",
        "--count-by",
        "label",
        "--cross-compactions",
        "1",
    ]);
    assert!(census.success, "stderr: {}", census.stderr);
    assert!(
        census
            .stderr
            .contains("(compaction crossing: 1 boundary(s) stepped over"),
        "the census discloses on stderr, keeping its table parseable:\n{}",
        census.stderr
    );
    assert!(
        !census.stdout.contains("compaction crossing"),
        "stdout stays the table:\n{}",
        census.stdout
    );
}

#[test]
fn a_raw_turn_window_discloses_on_stderr_and_a_line_address_has_nothing_to_say() {
    // `--raw` prints the file's own bytes and nothing else, so its disclosure rides stderr.
    // A `--turn` window is resolved through the chain and moves with the budget; a `--line`
    // address names its lines outright, crosses nothing to find them, and says nothing.
    let h = crossing_home();
    let t = at(XSESS);
    let turns = h.run(&[
        "show",
        t.as_str(),
        "--turn",
        "..",
        "--raw",
        "--cross-compactions",
        "1",
    ]);
    assert!(turns.success, "stderr: {}", turns.stderr);
    assert!(
        turns
            .stderr
            .contains("(compaction crossing: 1 boundary(s) stepped over"),
        "{}",
        turns.stderr
    );
    assert!(
        turns.stdout.starts_with('{'),
        "stdout is the file's own jsonl:\n{}",
        turns.stdout
    );

    let lines = h.run(&[
        "show",
        t.as_str(),
        "--line",
        "1..4",
        "--raw",
        "--cross-compactions",
        "1",
    ]);
    assert!(lines.success, "stderr: {}", lines.stderr);
    assert!(
        !lines.stderr.contains("compaction crossing"),
        "a line address crossed nothing to find its lines:\n{}",
        lines.stderr
    );
}
