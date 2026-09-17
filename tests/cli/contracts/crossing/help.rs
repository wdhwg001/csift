//! The `--help` half of the five-document contract: every command whose JSON summary
//! carries the crossing keys names them in its own manual.

use super::*;

#[test]
fn every_help_that_documents_a_json_summary_names_the_six_crossing_keys() {
    // The five-document contract makes `--help` the human manual, at information parity
    // with SKILL: a key a consumer will meet in the stream and cannot find in the manual is
    // a key they have to discover by running the command and reading what came back. Only
    // one of the seven named them.
    let h = Home::new();
    for cmd in [
        "search", "show", "stats", "files", "recover", "verbatim", "image",
    ] {
        let out = h.run(&[cmd, "--help"]);
        assert!(out.success, "{cmd} --help stderr: {}", out.stderr);
        for key in [
            "crossing_budget",
            "boundaries_crossed",
            "budget_exhausted",
            "boundaries_above_floor",
            "floor_line",
            "floor_cause",
        ] {
            assert!(out.stdout.contains(key), "{cmd} --help does not name {key}");
        }
        assert!(
            out.stdout.contains("--cross-compactions 0"),
            "{cmd} --help does not say the keys are absent at budget 0"
        );
    }
}
