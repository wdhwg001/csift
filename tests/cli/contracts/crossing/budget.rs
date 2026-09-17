//! What the BUDGET decides: how far the walk climbs on one transcript, what that does to
//! a record's survival, and the flag's own grammar.

use super::*;

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
    assert_eq!(
        one["boundary_cut_line"], one["floor_line"],
        "the named cut is the boundary the walk STOPPED at, never the one it stepped \
         over - a crossed cut is a cut csift got past: {one}"
    );

    // Budget 2: both crossed, the walk reaches the head, nothing left above the floor.
    let two = at_budget("2");
    assert_eq!(two["boundaries_crossed"], 2, "{two}");
    assert_eq!(two["budget_exhausted"], false, "{two}");
    assert_eq!(two["boundaries_above_floor"], 0, "{two}");
    assert_eq!(two["floor_cause"], "head", "{two}");
    assert_eq!(two["floor_line"], 1, "{two}");
    assert!(
        two["boundary_cut_line"].is_null(),
        "a walk that reached the head is cut at no boundary: {two}"
    );

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
fn a_bad_budget_token_names_both_accepted_forms() {
    let h = crossing_home();
    let out = h.run(&["stats", at(XSESS).as_str(), "--cross-compactions", "lots"]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(out.stderr.contains("or `all`"), "{}", out.stderr);
    assert!(out.stderr.contains("lots"), "{}", out.stderr);
}
