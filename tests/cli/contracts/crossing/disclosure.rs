//! What every SURFACE says about a crossing: the sweep over each one, the silence at
//! budget 0, and the four modes whose answer is a datum rather than a report.

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
        // The two surfaces that MOVE with the budget and are easiest to leave silent: an
        // ADDRESS (the record it names reads live at one budget and pre-cut at another) and a
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
        vec![
            "recover",
            t.as_str(),
            "--no-subagents",
            "--file",
            "/Users/dev/chart.md",
        ],
    ];
    for mut argv in argvs {
        let name = argv[0];
        argv.extend_from_slice(&["--cross-compactions", "0"]);
        let out = h.run(&argv);
        assert!(out.success, "{name} stderr: {}", out.stderr);
        let both = format!("{}{}", out.stdout, out.stderr);
        assert!(
            !both.contains("compaction crossing")
                && !both.contains("compaction boundaries crossed"),
            "{name} text:\n{both}"
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

    // And the summary tells those two runs apart: without the cause the two read alike.
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
fn a_raw_turn_window_discloses_on_stderr_and_a_raw_address_has_nothing_to_say() {
    // `--raw` prints the file's own bytes and nothing else, so its disclosure rides stderr.
    // A `--turn` window is resolved through the chain and moves with the budget, so it says
    // what the walk crossed - on stderr, never among the bytes. A `--line` or `--uuid`
    // address names its lines outright: the raw fetch builds no chain to find them, the bytes
    // it prints are the same under every budget, and it says nothing on either stream.
    // Asserted per STREAM, beside the recover sweep below, because which stream a line lands
    // on IS the contract.
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
        turns.stdout.starts_with('{') && !turns.stdout.contains("compaction crossing"),
        "stdout is the file's own jsonl and nothing else:\n{}",
        turns.stdout
    );

    for address in [
        vec!["--line", "1..4"],
        vec!["--line", "11"],
        vec!["--uuid", "u2"],
    ] {
        let mut argv = vec!["show", t.as_str()];
        argv.extend_from_slice(&address);
        argv.extend_from_slice(&["--raw", "--cross-compactions", "1"]);
        let out = h.run(&argv);
        assert!(out.success, "{address:?} stderr: {}", out.stderr);
        assert!(
            out.stdout.starts_with('{'),
            "{address:?} prints the addressed bytes:\n{}",
            out.stdout
        );
        for (stream, text) in [("stdout", &out.stdout), ("stderr", &out.stderr)] {
            assert!(
                !text.contains("compaction crossing"),
                "{address:?} crossed nothing to find its lines, yet {stream} says so:\n{text}"
            );
        }
    }
}

#[test]
fn a_raw_scan_discloses_on_stderr_and_says_nothing_at_budget_zero() {
    // `search --raw` is a SCAN: the survival walk decides which records are hits, so what it
    // prints moves with the budget, and it says what the walk crossed. Its stdout is the
    // matched records' own jsonl lines, so the lines ride stderr beside its accounting notes,
    // the `--count-by` precedent. At budget 0 it crossed nothing on purpose and says nothing.
    let h = crossing_home();
    let t = at(XSESS);
    let raw = |b: &str| {
        let out = h.run(&[
            "search",
            "",
            t.as_str(),
            "--no-subagents",
            "--raw",
            "--cross-compactions",
            b,
        ]);
        assert!(out.success, "budget {b} stderr: {}", out.stderr);
        assert!(
            out.stdout.starts_with('{') && out.stdout.lines().all(|l| l.starts_with('{')),
            "budget {b}: stdout is the matched lines and nothing else:\n{}",
            out.stdout
        );
        out
    };
    let one = raw("1");
    assert!(
        one.stderr.contains(
            "(compaction crossing: 1 boundary(s) stepped over under \
             --cross-compactions 1, exhausted; 1 above the chain floor were not)"
        ),
        "{}",
        one.stderr
    );
    assert!(
        one.stderr
            .contains("to try them: --cross-compactions 2 (or all)"),
        "{}",
        one.stderr
    );
    let zero = raw("0");
    for (stream, text) in [("stdout", &zero.stdout), ("stderr", &zero.stderr)] {
        assert!(
            !text.contains("compaction crossing") && !text.contains("chain floor"),
            "budget 0 crossed nothing, yet {stream} says so:\n{text}"
        );
    }
}

#[test]
fn every_recover_mode_discloses_what_the_walk_crossed() {
    // `recover` is five modes over one scan and EVERY one discloses, the DEFAULT one -
    // restore - most of all: it is the mode most likely to be piped straight into a file, so
    // a reader who never sees the other four has to be told there which budget produced the
    // replay. Its stdout is the restored FILE, so the lines ride stderr with the rest of its
    // status report; the other four print to stdout with their own reports.
    let h = crossing_home();
    let t = at(XSESS);
    let modes: Vec<(&str, Vec<&str>)> = vec![
        ("restore", vec![]),
        ("at", vec!["--at", "@turn:2"]),
        ("salvage", vec!["--salvage"]),
        ("patches", vec!["--patches"]),
        ("coverage", vec!["--coverage"]),
    ];
    for (name, extra) in modes {
        let mut argv = vec![
            "recover",
            t.as_str(),
            "--no-subagents",
            "--file",
            "/Users/dev/chart.md",
        ];
        argv.extend_from_slice(&extra);
        argv.extend_from_slice(&["--cross-compactions", "1"]);
        let out = h.run(&argv);
        assert!(out.success, "{name} stderr: {}", out.stderr);
        // Asserted per STREAM, not over the two joined: which stream a line lands on IS the
        // contract here - restore's stdout is the restored file and a status line in it would
        // corrupt the artifact - and a check over `stdout + stderr` passes either way round.
        let (spoken, silent) = if name == "restore" {
            (&out.stderr, &out.stdout)
        } else {
            (&out.stdout, &out.stderr)
        };
        assert!(
            spoken.contains("(compaction crossing: 1 boundary(s) stepped over"),
            "{name}:\n{spoken}"
        );
        assert!(
            !silent.contains("compaction crossing"),
            "{name} put the disclosure on the other stream too:\n{silent}"
        );

        let mut jargv = argv.clone();
        jargv.extend_from_slice(&["--format", "json"]);
        let jout = h.run(&jargv);
        assert!(jout.success, "{name} json stderr: {}", jout.stderr);
        let s = summary(&jout.stdout);
        assert_eq!(s["crossing_budget"], 1, "{name}: {s}");
        assert_eq!(s["boundaries_crossed"], 1, "{name}: {s}");
        assert_eq!(s["floor_line"], 5, "{name}: {s}");
    }
}
