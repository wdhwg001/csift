use crate::harness::*;

#[test]
fn turns_command_renamed_to_verbatim() {
    let h = populated_home();
    let t = at(SESS);
    // Zero-BC: the old `turns` verb is GONE — it hits the wall (unknown subcommand), which
    // sends a stale model back to re-read SKILL rather than silently mis-running.
    let old = h.run(&["turns", t.as_str()]);
    assert!(!old.success, "the old `turns` command must never run");
    // v0.6.4: the wall is still a wall, but a POINTED one — the hidden tombstone names the
    // successor (the `-t thinking` treatment) instead of clap's generic unrecognized error.
    assert!(
        old.stderr.contains("RENAMED to `csift verbatim`"),
        "error names the successor: {}",
        old.stderr
    );
    // The new `verbatim` verb is the compaction-fidelity reconstructor.
    let new = h.run(&["verbatim", t.as_str()]);
    assert!(new.success, "verbatim runs: {}", new.stderr);
}

#[test]
fn turns_requires_a_target() {
    // `--budget` multiplies per session, so bare `csift turns` (= every project) is an
    // output flood by construction — a target is REQUIRED (the `show` precedent).
    let h = populated_home();
    let bare = h.run(&["verbatim"]);
    assert!(!bare.success, "bare turns must error: {}", bare.stdout);
    assert!(
        bare.stderr.contains("name a target"),
        "the error teaches the target forms: {}",
        bare.stderr
    );
    // `--sessions-from` satisfies the requirement.
    let ids = h.root.join("ids.txt");
    std::fs::write(&ids, format!("{SESS}\n")).unwrap();
    let ok = h.run(&["verbatim", "--sessions-from", ids.to_str().unwrap()]);
    assert!(ok.success, "stderr: {}", ok.stderr);
}

#[test]
fn turns_bare_uuid_positional_routes_to_session() {
    let h = populated_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--budget",
        "2000",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS), "got: {}", out.stdout);
}

#[test]
fn turns_automation_notification_does_not_consume_human_round_trip_floor() {
    // The round-trip HARD FLOOR is reserved for HUMAN exchanges. A session whose RECENT
    // turns are machine automation pulses (each with an agent ack) plus ONE older human
    // round-trip, at a small budget, must still recover the human turn — the pulses must NOT
    // crowd it out of the protected floor (the prior `is_round_trip` ignored is_automation).
    let h = Home::new();
    let sess = "22222222-3333-4444-5555-666666666666";
    let mut lines = vec![
        // The OLDER human round-trip (the one the floor must protect).
        r#"{"type":"user","uuid":"u0","cwd":"/Users/x/r","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"HUMAN-QUESTION-MARKER please explain the carry-propagation bug in detail"}}"#.to_string(),
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"The carry is the partial line held across a chunk boundary; here is the full explanation of the propagation path and the fix."}]}}"#.to_string(),
    ];
    // SEVEN newer automation pulses (each a round-trip pulse→ack) — recency-first, these
    // would be picked before the human turn and (under the bug) consume the floor.
    for i in 0..7 {
        lines.push(format!(
            r#"{{"type":"user","uuid":"n{i}","timestamp":"2026-06-07T06:0{i}:00.000Z","message":{{"role":"user","content":"<task-notification>\n<task-id>auto{i}</task-id>\n<status>completed</status>\n<summary>Background command \"job {i}\" completed (exit code 0)</summary>\n</task-notification>"}}}}"#
        ));
        lines.push(format!(
            r#"{{"type":"assistant","uuid":"m{i}","parentUuid":"n{i}","timestamp":"2026-06-07T06:0{i}:05.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"Acknowledged pulse {i}."}}]}}}}"#
        ));
    }
    h.write(
        &format!("-Users-x-r/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );
    // A budget small enough that, if the floor were spent on pulses, the human turn would be
    // crowded out — but large enough to fit the human round-trip in its protected lane.
    let t = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--budget",
        "1200",
        "--no-subagents",
    ]);
    assert!(t.success, "stderr: {}", t.stderr);
    assert!(
        t.stdout.contains("HUMAN-QUESTION-MARKER"),
        "the human round-trip must survive the floor despite newer automation pulses; got: {}",
        t.stdout
    );
}

#[test]
fn turns_old_subcommand_name_gets_the_rename_error() {
    // R8: the v0.5 `turns`→`verbatim` rename used to surface as clap's teach-nothing
    // "unrecognized subcommand" — the one error below the tool's water line. The hidden
    // tombstone now bails with the successor (and swallows any flags, so the message
    // never loses to a flag-parse error).
    let h = populated_home();
    let out = h.run(&["turns", &at(SESS), "--slices", "4", "--turn", "-3.."]);
    assert!(!out.success, "the tombstone must never run: {}", out.stdout);
    assert!(
        out.stderr.contains("RENAMED to `csift verbatim`")
            && out.stderr.contains("show <target> --turn"),
        "pointed rename error expected, got: {}",
        out.stderr
    );
    // Hidden: no COMMAND ROW for it in the root help (a clap row is `  turns` alone or
    // `turns` + 2+ spaces + about; wrapped PROSE lines like "turns a compaction summary…"
    // are not rows and must not trip this).
    let help = h.run(&["--help"]);
    assert!(
        !help.stdout.lines().any(|l| {
            let t = l.trim_start();
            t == "turns" || t.starts_with("turns  ")
        }),
        "turns must stay hidden from the subcommand list: {}",
        help.stdout
    );
}

#[test]
fn turns_json_units_carry_id_domain_discriminators() {
    // turns per-unit JSON gains is_subagent + parent_session_id (top-level run here).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"ask a real question"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"a substantive reply"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["verbatim", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let unit = objs
        .iter()
        .find(|o| o["role"] == "user" || o["role"] == "assistant")
        .expect("a per-unit record present");
    assert_eq!(unit["is_subagent"], serde_json::json!(false));
    assert_eq!(unit["session_id"], serde_json::json!(SESS));
    assert_eq!(unit["parent_session_id"], serde_json::json!(SESS));
}

#[test]
fn turns_slice_reassembles_out_document_within_window() {
    // --slice paginates the SAME verbatim document `--out` writes into ≤window-CHAR chunks with
    // NO chrome. Assert: every chunk ≤ window, concatenating slices 1..K reproduces the `--out`
    // document byte-for-byte (the zero-drift contract between build_document_body and out_blob),
    // and an out-of-range slice is empty (exit 0).
    let h = turns_home();
    let window = 500usize;

    let out_path = h.root.join("turns_doc.md");
    let r = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--budget",
        "20000",
        "--no-subagents",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(r.success, "stderr: {}", r.stderr);
    let document = std::fs::read_to_string(&out_path).expect("out document written");
    assert!(!document.is_empty(), "fixture yields a non-empty document");
    assert!(
        document.chars().count() > window,
        "document must exceed one window to exercise multi-slice ({} chars)",
        document.chars().count()
    );

    let win = window.to_string();
    let mut reassembled = String::new();
    let mut n = 1usize;
    loop {
        let ns = n.to_string();
        let s = h.run(&[
            "verbatim",
            at(SESS).as_str(),
            "--budget",
            "20000",
            "--no-subagents",
            "--window",
            &win,
            "--slice",
            &ns,
        ]);
        assert!(s.success, "slice {n} stderr: {}", s.stderr);
        if s.stdout.is_empty() {
            break; // out-of-range slice → empty → done
        }
        assert!(
            s.stdout.chars().count() <= window,
            "slice {n} exceeds the {window}-char window ({} chars)",
            s.stdout.chars().count()
        );
        reassembled.push_str(&s.stdout);
        n += 1;
        assert!(n < 1000, "runaway slice loop");
    }
    assert!(
        n > 2,
        "fixture should span at least two slices, got {}",
        n - 1
    );
    assert_eq!(
        reassembled, document,
        "concatenated slices must reproduce the --out document byte-for-byte"
    );
}

#[test]
fn turns_slice_rejects_out_json_and_zero() {
    // --slice writes the selected chunk to stdout and is verbatim-text only, so it refuses
    // --out, --format json, and the 1-based 0 index — each with a pointed error.
    let h = turns_home();

    let bad_out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--slice",
        "1",
        "--out",
        h.root.join("x.md").to_str().unwrap(),
    ]);
    assert!(!bad_out.success);
    assert!(
        bad_out.stderr.contains("mutually exclusive"),
        "stderr: {}",
        bad_out.stderr
    );

    let bad_json = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--slice",
        "1",
        "--format",
        "json",
    ]);
    assert!(!bad_json.success);
    assert!(
        bad_json.stderr.contains("text format"),
        "stderr: {}",
        bad_json.stderr
    );

    let bad_zero = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--slice",
        "0",
    ]);
    assert!(!bad_zero.success);
    assert!(
        bad_zero.stderr.contains("1-based"),
        "stderr: {}",
        bad_zero.stderr
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
fn turns_ellipsis_role_asymmetry_and_counts() {
    // The huge live round-trip: user > 600 → head 360 / tail 240; assistant > 900 →
    // head 594 / tail 306. The assistant head is strictly larger. The text output shows
    // the head + the elision marker + the tail; JSON carries the exact elided counts.
    let h = turns_home();
    let text = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(text.success, "stderr: {}", text.stderr);
    // The user head begins with HEADuser then 'u's; the marker carries the elided count.
    assert!(
        text.stdout.contains("HEADuser"),
        "user head present: {}",
        text.stdout
    );
    assert!(
        text.stdout.contains("TAILuser"),
        "user tail kept: {}",
        text.stdout
    );
    assert!(text.stdout.contains("HEADasst"), "asst head present");
    assert!(text.stdout.contains("TAILasst"), "asst tail kept");
    assert!(
        text.stdout.contains("chars elided") || text.stdout.contains("chars]"),
        "elision marker present: {}",
        text.stdout
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
    let objs = json_lines(&json.stdout);
    // Find the huge user + huge assistant units (full_chars over the cap).
    let huge_user = objs
        .iter()
        .find(|o| o["role"] == "user" && o["full_chars"].as_u64().unwrap_or(0) > 600)
        .expect("huge user unit present");
    let huge_asst = objs
        .iter()
        .find(|o| o["role"] == "assistant" && o["full_chars"].as_u64().unwrap_or(0) > 900)
        .expect("huge assistant unit present");
    assert!(huge_user["truncated"].as_bool().unwrap());
    assert!(huge_asst["truncated"].as_bool().unwrap());
    // The assistant rendered_chars (900) is strictly larger than the user's (600) — the
    // role-asymmetric caps drive a larger assistant head.
    assert_eq!(huge_user["rendered_chars"].as_u64().unwrap(), 600);
    assert_eq!(huge_asst["rendered_chars"].as_u64().unwrap(), 900);
    assert!(
        huge_asst["rendered_chars"].as_u64().unwrap()
            > huge_user["rendered_chars"].as_u64().unwrap()
    );
    // elided_chars == full_chars - cap.
    assert_eq!(
        huge_user["elided_chars"].as_u64().unwrap(),
        huge_user["full_chars"].as_u64().unwrap() - 600
    );
    assert_eq!(
        huge_asst["elided_chars"].as_u64().unwrap(),
        huge_asst["full_chars"].as_u64().unwrap() - 900
    );
    // The JSON `text` field is the FULL verbatim message (un-truncated) — longer than
    // the rendered cap.
    assert!(huge_user["text"].as_str().unwrap().chars().count() > 600);
}
