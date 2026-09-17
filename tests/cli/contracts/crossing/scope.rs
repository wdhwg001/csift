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
