//! verbatim budget accounting: caps, floors, monotonicity.

use crate::harness::*;

#[test]
fn verbatim_header_carries_budget_accounting_in_json_and_spanned_of_total_in_text() {
    // R10: `spanned N compaction boundaries` read as a TRANSCRIPT property when it is a
    // QUERY property (budget-window-relative) — the text now prints `spanned K of N … in
    // scope`, and the JSON header carries the full budget accounting the text header
    // shows (the machine format must never be thinner than the human one).
    let h = populated_home();
    let out = h.run(&["verbatim", &at(SESS), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let header: serde_json::Value =
        serde_json::from_str(out.stdout.lines().next().unwrap()).unwrap();
    for key in [
        "budget_chars",
        "round_trip_fraction",
        "chars_used",
        "boundaries_spanned",
        "boundaries_total",
        "selected_user",
        "selected_assistant",
    ] {
        assert!(!header[key].is_null(), "header must carry {key}: {header}");
    }
}

#[test]
fn turns_token_budget_unit_scales_by_four() {
    // --budget-unit tokens multiplies by ~4 chars/token. A 3000-token budget should
    // select more than a 3000-char budget (4x the room).
    let h = turns_home();
    let tok = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "3000",
        "--budget-unit",
        "tokens",
        "--format",
        "json",
    ]);
    let chr = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "3000",
        "--budget-unit",
        "chars",
        "--format",
        "json",
    ]);
    let units = |s: &str| {
        json_lines(s)
            .iter()
            .filter(|o| o.get("role").is_some())
            .count()
    };
    assert!(
        units(&tok.stdout) >= units(&chr.stdout),
        "a token budget (4x chars) must not select fewer units"
    );
}

#[test]
fn turns_round_trip_floor_recovers_a_user_turn() {
    // The fixture's live tail is assistant-heavy (a huge assistant EOT). The 50% floor
    // must still recover at least one USER turn even at a modest budget — the pulse
    // regression. Without the floor a naive recency walk would recover zero users.
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "6000",
        "--round-trip-fraction",
        "0.5",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let users = objs.iter().filter(|o| o["role"] == "user").count();
    assert!(
        users >= 1,
        "the 50% floor must recover >=1 user turn, got {users}"
    );
}

#[test]
fn turns_budget_respected_real_emitted_chars() {
    // HONEST budget test: drive the compiled binary in default TEXT form AND with `--out`,
    // read the ACTUAL emitted bytes, count the WHOLE document with `.chars().count()`, and
    // assert it is <= budget at three real budgets on the multi-compaction fixture. This
    // replaces the old circular checks (the reported "chars used" number, and the JSON sum
    // re-derived with a hardcoded `+ 24`) — neither of which measured the real document.
    //
    // The contract binds the default TEXT form (SPEC §6.8 — budget allocation + text output).
    // We bound BOTH the stdout document (doc-header-block + banners + units, minus the
    // operational trailers) AND the `--out` file (the documented verbatim reconstruction,
    // which omits the stdout-only header block) — so every component the contract lists is
    // measured against budget.
    let h = turns_home();
    for budget in [40000usize, 15000, 8000] {
        let out_path = h.root.join(format!("turns-budget-{budget}.md"));
        let bs = budget.to_string();
        // Default text form (stdout is the document + operational chrome).
        let text = h.run(&[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            &bs,
        ]);
        assert!(text.success, "stderr: {}", text.stderr);

        let doc = turns_document_text(&text.stdout);
        let doc_chars = doc.chars().count();
        assert!(
            doc_chars <= budget,
            "REAL emitted text document is {doc_chars} chars, exceeds budget {budget}\n--- document ---\n{doc}"
        );

        // The `--out` file: the verbatim reconstruction document (no operational chrome).
        let outrun = h.run(&[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            &bs,
            "--out",
            out_path.to_str().unwrap(),
        ]);
        assert!(outrun.success, "stderr: {}", outrun.stderr);
        let body = std::fs::read_to_string(&out_path).expect("out file written");
        let out_chars = body.chars().count();
        assert!(
            out_chars <= budget,
            "REAL --out file is {out_chars} chars, exceeds budget {budget}"
        );

        // The reported "chars used" header line is itself within budget (it is now a real
        // upper bound on the emitted length, not a self-fulfilling cost() echo).
        let reported: usize = text
            .stdout
            .lines()
            .find_map(|l| {
                let l = l.trim();
                let idx = l.find(&format!(" / {budget} chars used"))?;
                l[..idx].rsplit(' ').next()?.parse().ok()
            })
            .expect("chars-used line present");
        assert!(
            reported <= budget,
            "reported chars-used {reported} must be <= budget {budget}"
        );
        // The reported figure must NOT under-state the truth: the real document is <= the
        // header's claim (the fix made the accounting an honest upper bound, never an
        // under-count — that was the original overshoot bug).
        assert!(
            doc_chars <= reported,
            "header claims {reported} chars but the real document is {doc_chars} — the \
             accounting under-states the cost (the overshoot bug)"
        );
    }

    // The skipped malformed line is still surfaced, never hidden (it just is not counted
    // against the reconstruction budget — it is operational chrome).
    let any = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "8000",
    ]);
    assert!(
        any.stdout.contains("1 malformed line(s) skipped"),
        "{}",
        any.stdout
    );
}

#[test]
fn turns_smaller_budget_emits_strictly_less() {
    // The emitted document shrinks monotonically with the budget (real measured chars),
    // and a bigger budget's selected line_no set is a superset of a smaller one's.
    let h = turns_home();
    let doc_len = |budget: &str| -> usize {
        let t = h.run(&[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            budget,
        ]);
        turns_document_text(&t.stdout).chars().count()
    };
    let big = doc_len("40000");
    let small = doc_len("8000");
    assert!(
        small < big,
        "smaller budget must emit fewer chars: 8000→{small} vs 40000→{big}"
    );
    assert!(small <= 8000 && big <= 40000, "both within budget");
}

#[test]
fn turns_smaller_budget_selects_fewer() {
    let h = turns_home();
    let big = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    let small = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "3000",
        "--format",
        "json",
    ]);
    let count_units = |s: &str| {
        json_lines(s)
            .iter()
            .filter(|o| o.get("role").is_some())
            .count()
    };
    assert!(
        count_units(&small.stdout) < count_units(&big.stdout),
        "small budget must select strictly fewer units"
    );
}

#[test]
fn turns_max_compactions_caps_the_reach() {
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--max-compactions",
        "1",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let boundaries = objs
        .iter()
        .filter(|o| o["kind"] == "compaction_boundary")
        .count();
    assert!(
        boundaries <= 1,
        "--max-compactions 1 caps boundaries to <=1, got {boundaries}"
    );
    // No selected unit may have compactions_before > 1.
    for o in objs.iter().filter(|o| o.get("role").is_some()) {
        assert!(
            o["compactions_before"].as_u64().unwrap() <= 1,
            "cap leaked: {o}"
        );
    }
}

#[test]
fn turns_json_single_side_units_present_under_tight_budget() {
    // A tight budget forces some single-side (user-only / assistant-only) selections in
    // the JSON output — exercise the single-side JSON emit path.
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "2500",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    // Some units selected, budget respected.
    let units: usize = objs.iter().filter(|o| o.get("role").is_some()).count();
    assert!(units >= 1, "at least one unit under a tight budget");
    let sum: usize = objs
        .iter()
        .filter(|o| o.get("role").is_some())
        .map(|o| o["rendered_chars"].as_u64().unwrap() as usize + 24)
        .sum();
    assert!(sum <= 2500, "tight budget respected: {sum}");
}

#[test]
fn turns_budget_is_chars_only() {
    // `--budget` is CHARS, period (the token-unit mode and its silent-4x default trap
    // are gone; ≈4 chars/token is a documented sizing rule of thumb, not a flag).
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "8000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("budget 8000 chars"), "{}", out.stdout);
}

#[test]
fn turns_budget_respected_under_rich_and_all_modes() {
    // The summed-cost == summed-emitted invariant holds with placeholders + multi-agent
    // lanes: the REAL emitted document stays <= budget under rich AND all, across budgets.
    let h = turns_home();
    for mode in ["rich", "all"] {
        for budget in [40000usize, 15000, 8000] {
            let bs = budget.to_string();
            let out = h.run(&[
                "verbatim",
                at(SESS).as_str(),
                "--no-subagents",
                "--budget",
                &bs,
                "--agent-msgs",
                mode,
            ]);
            assert!(out.success, "stderr: {}", out.stderr);
            let doc = turns_document_text(&out.stdout);
            assert!(
                doc.chars().count() <= budget,
                "mode {mode} budget {budget}: real document is {} chars (over budget)",
                doc.chars().count()
            );
        }
    }
}
