use crate::harness::*;

#[test]
fn turns_slices_pins_emitted_count_to_the_fleet() {
    // `--slices N` makes the slice COUNT the hard constraint: it emits AT MOST N chunks no matter
    // how many a char budget would have produced, and each chunk stays within the window. A 2-slice
    // fleet over this multi-block fixture: slices 1-2 are within window, and any index > 2 is empty
    // — the count can never drift to 3/4/5 as the turns grow.
    let h = turns_home();
    let win = 1500usize;
    for i in 1..=2 {
        let o = h.run(&[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--slices",
            "2",
            "--window",
            "1500",
            "--slice",
            &i.to_string(),
        ]);
        assert!(o.success, "stderr: {}", o.stderr);
        assert!(
            o.stdout.chars().count() <= win,
            "slice {i} exceeds the window: {}",
            o.stdout.chars().count()
        );
    }
    let s1 = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--slices",
        "2",
        "--window",
        "1500",
        "--slice",
        "1",
    ]);
    assert!(
        !s1.stdout.is_empty(),
        "slice 1 of a filled 2-fleet is non-empty"
    );
    let s3 = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--slices",
        "2",
        "--window",
        "1500",
        "--slice",
        "3",
    ]);
    assert!(
        s3.stdout.is_empty(),
        "an index beyond the fixed fleet must be empty, got: {}",
        s3.stdout
    );
}

#[test]
fn turns_slices_keeps_newest_discards_oldest() {
    // The fleet fills newest-first; the oldest turns that don't fit are DISCARDED (not truncated).
    // A tight 2-slice fleet keeps the live tail and drops the oldest round-trip ("the very first
    // ask … café").
    let h = turns_home();
    let mut doc = String::new();
    for i in 1..=2 {
        doc.push_str(
            &h.run(&[
                "verbatim",
                at(SESS).as_str(),
                "--no-subagents",
                "--slices",
                "2",
                "--window",
                "1500",
                "--slice",
                &i.to_string(),
            ])
            .stdout,
        );
    }
    assert!(
        !doc.contains("the very first ask"),
        "the oldest turn must be discarded by a small fleet: {doc}"
    );
    assert!(
        doc.contains("final committed answer")
            || doc.contains("TAILuser")
            || doc.contains("short live ask")
            || doc.contains("do the final thing"),
        "the newest turns must be kept: {doc}"
    );
}

#[test]
fn turns_slices_keeps_user_turns_whole_no_role_cap() {
    // The defect a peer session caught: budget mode middle-truncates a USER turn at the 600-char
    // role cap even with budget to spare. In `--slices` mode the only cap is the WINDOW, so a
    // multi-hundred-char user directive survives VERBATIM. The fixture's huge_user (≈817 chars)
    // appears whole (no mid-cut) when the window comfortably exceeds it.
    let h = turns_home();
    let mut doc = String::new();
    for i in 1..=8 {
        doc.push_str(
            &h.run(&[
                "verbatim",
                at(SESS).as_str(),
                "--no-subagents",
                "--slices",
                "8",
                "--window",
                "9000",
                "--slice",
                &i.to_string(),
            ])
            .stdout,
        );
    }
    let whole = format!("HEADuser {} TAILuser", "u".repeat(800));
    assert!(
        doc.contains(&whole),
        "a long user turn must be kept whole in --slices mode (it was gutted at the 600 cap?)"
    );
    // Contrast: the SAME fixture under budget mode STILL applies the 600 user cap (legacy behavior
    // is untouched) — so the verbatim user body is NOT present and the elision marker IS.
    let budgeted = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(
        !budgeted.stdout.contains(&whole),
        "budget mode must still apply the 600 user cap (legacy unchanged)"
    );
    assert!(
        budgeted.stdout.contains("chars elided") || budgeted.stdout.contains("chars]"),
        "budget mode shows the elision marker"
    );
}

#[test]
fn turns_slices_ellipsizes_only_a_turn_bigger_than_one_window() {
    // The ONLY content cut in --slices mode is a single turn that ALONE exceeds one window. With a
    // small window the big assistant turn (≈3000 chars) is middle-elided, while the shorter user
    // turn (≈817) in the same fleet is kept whole.
    let h = turns_home();
    let mut doc = String::new();
    for i in 1..=8 {
        doc.push_str(
            &h.run(&[
                "verbatim",
                at(SESS).as_str(),
                "--no-subagents",
                "--slices",
                "8",
                "--window",
                "1200",
                "--slice",
                &i.to_string(),
            ])
            .stdout,
        );
    }
    assert!(
        doc.contains("chars elided") || doc.contains("chars]"),
        "a turn larger than one window is ellipsized: {doc}"
    );
    assert!(
        doc.contains(&format!("HEADuser {} TAILuser", "u".repeat(800))),
        "a turn that fits within one window is kept whole alongside it: {doc}"
    );
}

#[test]
fn turns_slices_requires_a_slice_index() {
    // `--slices N` sets the fleet size; without `--slice i` there is no chunk to emit — a clear
    // error, not a silent full-document dump.
    let h = turns_home();
    let o = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--slices",
        "4",
    ]);
    assert!(!o.success, "must fail without --slice");
    assert!(
        o.stderr.contains("--slice"),
        "error names the missing flag: {}",
        o.stderr
    );
}

#[test]
fn turns_tool_call_markers_present_with_correct_counts() {
    // The fixture's huge live round-trip has 5 tool calls; turn "fifth ask" has 3.
    let h = turns_home();
    let text = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(
        text.stdout.contains("[5 tool calls]"),
        "5-tool marker: {}",
        text.stdout
    );
    assert!(
        text.stdout.contains("[3 tool calls]"),
        "3-tool marker present"
    );
    // A 0-tool turn omits the marker — "third reply" turn had 0 tools, so there is no
    // "[0 tool calls]" anywhere.
    assert!(
        !text.stdout.contains("[0 tool calls]"),
        "0-tool marker must be omitted"
    );
    // JSON carries the exact tool_calls count.
    let json = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    let objs = json_lines(&json.stdout);
    assert!(
        objs.iter()
            .any(|o| o["role"] == "user" && o["tool_calls"] == 5),
        "a unit with tool_calls==5 present"
    );
}

#[test]
fn turns_line_numbers_present_in_text_and_json() {
    let h = turns_home();
    let text = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    // Text lines carry L<number> markers (the jsonl line) for both roles.
    assert!(
        text.stdout.lines().any(|l| l.starts_with("▽ L")),
        "user lines carry L-numbers: {}",
        text.stdout
    );
    assert!(
        text.stdout.lines().any(|l| l.starts_with("△ L")),
        "assistant lines carry L-numbers"
    );
    let json = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    for o in json_lines(&json.stdout)
        .iter()
        .filter(|o| o.get("role").is_some())
    {
        assert!(
            o["line"].as_u64().unwrap() > 0,
            "every unit carries a positive line_no"
        );
        // full_chars == text.chars().count().
        assert_eq!(
            o["full_chars"].as_u64().unwrap() as usize,
            o["text"].as_str().unwrap().chars().count()
        );
    }
}

#[test]
fn turns_fidelity_beats_summary_verbatim_count() {
    // The summary holds ~1 verbatim assistant quote (§9) + a handful of clipped §6
    // bullets. `turns` restores MANY more verbatim units. Assert concrete counts:
    // >= 3 restored user units and >= 3 restored assistant units, far exceeding the
    // summary's single verbatim assistant quote.
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    let objs = json_lines(&out.stdout);
    let users = objs.iter().filter(|o| o["role"] == "user").count();
    let asst = objs.iter().filter(|o| o["role"] == "assistant").count();
    assert!(
        users >= 3,
        "restored user units {users} must exceed the summary's clipped bullets"
    );
    assert!(
        asst >= 3,
        "restored assistant units {asst} must exceed the summary's 1 verbatim quote"
    );
}

#[test]
fn turns_deterministic_byte_identical() {
    let h = turns_home();
    let a = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "10000",
    ]);
    let b = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "10000",
    ]);
    assert_eq!(
        a.stdout, b.stdout,
        "two identical invocations must be byte-identical"
    );
}

#[test]
fn turns_out_file_holds_full_reconstruction() {
    let h = turns_home();
    let out_path = h.root.join("turns-out.md");
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("wrote full reconstruction"),
        "{}",
        out.stdout
    );
    let body = std::fs::read_to_string(&out_path).expect("out file written");
    assert!(body.contains("▽ L"), "out file carries the rendered turns");
    assert!(
        body.contains("compaction boundary"),
        "out file carries banners"
    );
}

#[test]
fn turns_turn_range_and_since_intersect() {
    // Same rule as every sibling: the windows AND (the former bail was a leftover).
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--turn",
        "0..2",
        "--since",
        "2h",
    ]);
    assert!(
        out.success,
        "combined windows intersect, never error: {}",
        out.stderr
    );
}

#[test]
fn turns_invalid_round_trip_fraction_errors() {
    let h = turns_home();
    for f in ["0", "1", "1.5", "-0.1"] {
        let out = h.run(&[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--round-trip-fraction",
            f,
        ]);
        assert!(!out.success, "round-trip-fraction {f} must be rejected");
    }
}

#[test]
fn turns_zero_budget_errors() {
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "0",
    ]);
    assert!(!out.success, "a zero budget must error");
    assert!(
        out.stderr.contains("--budget must be > 0"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn turns_live_region_dedup_demotes_and_flags() {
    let h = turns_dedup_home();
    // Text: the dedup header line + the (also in summary) flag must appear.
    let text = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(
        text.stdout.contains("also present in summary"),
        "dedup header line: {}",
        text.stdout
    );
    assert!(
        text.stdout.contains("(also in summary)"),
        "demoted-unit flag: {}",
        text.stdout
    );
    // JSON: exactly the live duplicate unit carries also_in_summary:true, and it is still
    // PRESENT (demoted, never dropped).
    let json = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    let objs = json_lines(&json.stdout);
    let flagged: Vec<_> = objs
        .iter()
        .filter(|o| o.get("also_in_summary").and_then(|v| v.as_bool()) == Some(true))
        .collect();
    assert!(
        !flagged.is_empty(),
        "at least one unit flagged also_in_summary"
    );
    assert!(
        flagged.iter().any(|o| o["text"]
            .as_str()
            .unwrap()
            .contains("the live duplicate ask")),
        "the live duplicate unit is flagged AND present (not dropped)"
    );
}

#[test]
fn turns_json_out_file_is_verbatim() {
    let h = turns_home();
    let out_path = h.root.join("turns.json");
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let body = std::fs::read_to_string(&out_path).expect("json out file");
    // The huge user unit's full verbatim text (un-truncated) is in the file.
    assert!(
        body.contains("HEADuser"),
        "out json carries the unit objects"
    );
    // Every non-blank line is valid JSON.
    for line in body.lines().filter(|l| !l.trim().is_empty()) {
        serde_json::from_str::<serde_json::Value>(line).expect("each out line is JSON");
    }
}

#[test]
fn turns_no_genuine_turns_emits_honest_empty_message() {
    // A session with NO genuine user turns (only a summary + an isMeta pseudo-turn +
    // tool noise) → nothing selected, an honest "no turns selected" message (never a
    // fabricated turn). This is the only empty-selection path: the most-recent complete
    // turn is always force-included when one exists (load-bearing).
    let h = Home::new();
    let mut s = String::new();
    s.push_str(r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"6. All user messages:\n   - \"gone\""}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off."}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"a carrier, not a genuine turn"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{SESS}.jsonl"), &s);
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no turns selected"),
        "honest empty message: {}",
        out.stdout
    );
}
