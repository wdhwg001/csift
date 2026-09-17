//! The `--help` half of the five-document contract, checked against the thing it documents:
//! every JSON SUMMARY a chain-walking command emits, and the manual that has to name it.
//!
//! Both halves are per SUMMARY, not per COMMAND. Two of the seven emit a second summary under
//! a flag - `search --count-by` swaps the exchange stream for a census, `show --branch-points`
//! swaps the record rows for forks - and a per-command check passes with the keys on one of
//! them and the other left silent, which is the reading the consumer of that flag gets.

use super::*;

/// The six keys the crossing set is, in the order the disclosure builds them.
const CROSSING_KEYS: [&str; 6] = [
    "crossing_budget",
    "boundaries_crossed",
    "budget_exhausted",
    "boundaries_above_floor",
    "floor_line",
    "floor_cause",
];

/// How many distinct SUMMARY SHAPES each command's JSON stream can carry, and what they are.
/// `recover` is five modes over ONE shape, so it counts once here and five times in the run
/// table below: the question here is how many summaries a manual has to describe.
const SUMMARY_SHAPES: [(&str, usize); 7] = [
    ("search", 2),   // the exchange footer, and the `--count-by` census summary
    ("show", 2),     // the record summary, and the `--branch-points` summary
    ("stats", 1),    // one summary (its session ROWS carry the set too, on the same terms)
    ("files", 1),    //
    ("recover", 1),  // one shape, emitted by each of the five modes
    ("verbatim", 1), //
    ("image", 1),    //
];

#[test]
fn every_json_summary_a_chain_walking_command_emits_carries_the_six_keys() {
    // The stream is the instrument the manual is checked against: a summary shape that omits
    // the set is a reader who cannot tell which budget produced the rows above it, and the
    // shape a flag swaps in is exactly the one a per-command check never reaches.
    let h = crossing_home();
    let t = at(XSESS);
    let f = "/Users/dev/chart.md";
    let modes: Vec<(&str, Vec<&str>)> = vec![
        ("search", vec!["search", "", t.as_str(), "--no-subagents"]),
        (
            "search --count-by",
            vec![
                "search",
                "",
                t.as_str(),
                "--no-subagents",
                "--count-by",
                "label",
            ],
        ),
        ("show --turn", vec!["show", t.as_str(), "--turn", ".."]),
        (
            "show --branch-points",
            vec!["show", t.as_str(), "--branch-points"],
        ),
        ("stats", vec!["stats", t.as_str(), "--no-subagents"]),
        ("files", vec!["files", t.as_str(), "--no-subagents"]),
        ("verbatim", vec!["verbatim", t.as_str()]),
        ("image", vec!["image", t.as_str(), "--no-subagents"]),
        (
            "recover restore",
            vec!["recover", t.as_str(), "--no-subagents", "--file", f],
        ),
        (
            "recover --at",
            vec![
                "recover",
                t.as_str(),
                "--no-subagents",
                "--file",
                f,
                "--at",
                "@turn:2",
            ],
        ),
        (
            "recover --salvage",
            vec![
                "recover",
                t.as_str(),
                "--no-subagents",
                "--file",
                f,
                "--salvage",
            ],
        ),
        (
            "recover --patches",
            vec![
                "recover",
                t.as_str(),
                "--no-subagents",
                "--file",
                f,
                "--patches",
            ],
        ),
        (
            "recover --coverage",
            vec![
                "recover",
                t.as_str(),
                "--no-subagents",
                "--file",
                f,
                "--coverage",
            ],
        ),
    ];
    // The two tables are the same claim from two sides - what the stream emits, and what the
    // manual has to describe - so neither may name a command the other does not.
    for (cmd, _) in SUMMARY_SHAPES {
        assert!(
            modes.iter().any(|(_, argv)| argv[0] == cmd),
            "{cmd} owes its manual a summary and this table never runs it"
        );
    }
    for (name, base) in modes {
        let mut argv = base.clone();
        argv.extend_from_slice(&["--cross-compactions", "3", "--format", "json"]);
        let out = h.run(&argv);
        assert!(out.success, "{name} stderr: {}", out.stderr);
        let s = rows(&out.stdout)
            .into_iter()
            .find(|v| v["kind"] == "summary")
            .unwrap_or_else(|| panic!("{name} emitted no summary row:\n{}", out.stdout));
        for key in CROSSING_KEYS {
            assert!(s.get(key).is_some(), "{name}: its summary omits {key}: {s}");
        }
        assert_eq!(s["crossing_budget"], 3, "{name}: {s}");
    }
}

#[test]
fn every_help_that_documents_a_json_summary_names_the_six_crossing_keys() {
    // The five-document contract makes `--help` the human manual, at information parity with
    // SKILL: a key a consumer will meet in the stream and cannot find in the manual is a key
    // they have to discover by running the command and reading what came back.
    //
    // Naming the keys ONCE is not parity for a command with two summaries, so the manual also
    // has to REFER to the set once per shape - either by spelling the keys again or by tying
    // the second summary back to the first ("plus the same crossing set", "carries the same
    // six"). Counting those references is what a per-command `contains` cannot do.
    let h = Home::new();
    for (cmd, shapes) in SUMMARY_SHAPES {
        let out = h.run(&[cmd, "--help"]);
        assert!(out.success, "{cmd} --help stderr: {}", out.stderr);
        for key in CROSSING_KEYS {
            assert!(out.stdout.contains(key), "{cmd} --help does not name {key}");
        }
        assert!(
            out.stdout.contains("--cross-compactions 0"),
            "{cmd} --help does not say the keys are absent at budget 0"
        );
        let refs =
            out.stdout.matches("crossing set").count() + out.stdout.matches("the same six").count();
        assert!(
            refs >= shapes,
            "{cmd} --help carries {refs} reference(s) to the crossing set for {shapes} summary \
             shape(s): a reader of the other one is never told the keys are there"
        );
    }
}
