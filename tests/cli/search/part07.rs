use crate::harness::*;

#[test]
fn count_by_tool_reports_exact_record_counts() {
    // Mutation pin: the per-axis counters must actually COUNT (a `+=` degraded to a
    // no-op leaves every tally at zero and the excluded total frozen) — pin exact
    // numbers on a fixed fixture: parent Write (tool_use + result carrier = 2 records)
    // + subagent Write (tool_use only = 1 record).
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["search", "", at(SESS).as_str(), "--count-by", "tool"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("3  Write"),
        "Write must tally 3 records: {}",
        out.stdout
    );
    assert!(
        out.stderr.contains("excluded") || out.stderr.contains("no tool"),
        "records outside the tool axis are reported: {}",
        out.stderr
    );
}

#[test]
fn zero_match_diagnosis_on_a_clean_corpus_has_no_malformed_caveat() {
    // Mutation pin (the dual of the skipped-lines disclosure): on a corpus with ZERO
    // malformed lines, the zero-match diagnosis must NOT print the parseable-lines caveat.
    let h = Home::new();
    let _ = header_collision_scenario(&h); // clean fixtures, no malformed lines
    let out = h.run(&["search", "ZZABSENTZZ"]);
    assert!(out.success, "zero-match exits 0: {}", out.stderr);
    assert!(
        out.stderr.contains("0 matches"),
        "diagnosis present: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("malformed"),
        "no malformed caveat on a clean corpus: {}",
        out.stderr
    );
}

#[test]
fn scope_banner_splits_top_level_and_subagent_exactly() {
    // Mutation pin: the banner's top-level/subagent split arithmetic (scope_top is the
    // resolved-set remainder after counting subagent paths).
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["search", "", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("2 sessions in scope (1 top-level + 1 subagent)"),
        "exact scope split: {}",
        out.stdout
    );
}

// An UNRECOGNIZED `@`-shape must fail loud naming the @-grammar — never strip the `@` and
// fall through to cwd-relative path resolution (the old behavior sent an ID typo down a
// misleading "no Claude Code project dir" filesystem trail).
#[test]
fn at_token_unrecognized_shapes_fail_loud_never_path_fallthrough() {
    let h = populated_home();
    // 1-3 dashless hex chars: below the 4-char uuid-prefix minimum → the dedicated message.
    for tok in ["@a", "@22", "@224"] {
        let out = h.run(&["list", tok]);
        assert!(!out.success, "{tok} must hard-error: {}", out.stdout);
        assert!(
            out.stderr.contains("too short") && out.stderr.contains("4-11"),
            "{tok} names the prefix minimum: {}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("no Claude Code project dir"),
            "{tok} must not fall through to path resolution: {}",
            out.stderr
        );
    }
    // Non-hex / dashed / empty tokens → the general @-grammar error.
    for tok in ["@notanid", "@1234-ab", "@"] {
        let out = h.run(&["list", tok]);
        assert!(!out.success, "{tok} must hard-error: {}", out.stdout);
        assert!(
            out.stderr.contains("not a recognized @-target"),
            "{tok} names the grammar: {}",
            out.stderr
        );
    }
    // The `@-Users-…` encoded-dir spelling STILL resolves (encoded cwds lead with `-`).
    let enc = h.run(&["list", &format!("@{ENC}")]);
    assert!(enc.success, "stderr: {}", enc.stderr);
    assert!(
        enc.stdout.contains(SESS),
        "lists the fixture session: {}",
        enc.stdout
    );
}

#[test]
fn agents_groups_multiple_sessions_with_separator() {
    // Two sessions each with a subagent → the render groups rows under per-session
    // headers separated by a blank line (the `last_session.is_some()` separator arm).
    let h = Home::new();
    let sess_a = "aaaaaaaa-0000-0000-0000-000000000001";
    let sess_b = "bbbbbbbb-0000-0000-0000-000000000002";
    for s in [sess_a, sess_b] {
        h.write(
            &format!("{ENC}/{s}.jsonl"),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        );
        h.write(
            &format!("{ENC}/{s}/subagents/agent-x{}.jsonl", &s[0..3]),
            &format!(
                "{{\"type\":\"user\",\"isSidechain\":true,\"agentId\":\"x{}\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"s\"}}}}\n{{\"type\":\"assistant\",\"timestamp\":\"2026-06-07T05:00:10.000Z\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"done\"}}]}}}}\n",
                &s[0..3]
            ),
        );
    }
    let out = h.run(&["agents", ENC]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.matches("SESSION").count() >= 2,
        "two session headers: {}",
        out.stdout
    );
}

#[test]
fn files_timeline_is_chronological_with_heuristic_label() {
    let h = files_scenario_home();
    let out = h.run(&["files", at(SESS).as_str(), "--by", "timeline"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("detail=timeline"));
    // The bash rm is the newest mutation (06:00) and carries the heuristic label.
    let lines: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| l.contains("/tmp/beacon-a.md") || l.contains("/p/spec/gaps"))
        .collect();
    // The first /tmp/beacon-a.md mention (the Write at 05:00) precedes the bash rm.
    let write_pos = out.stdout.find("write  /tmp/beacon-a.md");
    let bash_pos = out.stdout.find("bash (heuristic)  /tmp/beacon-a.md");
    assert!(write_pos.is_some() && bash_pos.is_some(), "{}", out.stdout);
    assert!(
        write_pos < bash_pos,
        "the Write precedes the bash rm chronologically: {}",
        out.stdout
    );
    assert!(!lines.is_empty());
}

#[test]
fn list_json_and_text_discriminate_subagent_id_domain_with_scope_banner() {
    // `list` spans subagents by default: a bare `csift list <uuid>` returns the top-level row
    // + each subagent row. JSON carries is_subagent + the re-feedable parent_session_id; text
    // leads with a scope banner and brands subagent rows SUBAGENT … · parent SESSION ….
    let h = Home::new();
    subagents_only_scenario(&h);

    let j = h.run(&["list", at(SESS).as_str(), "--format", "json"]);
    assert!(j.success, "stderr: {}", j.stderr);
    let objs = json_lines(&j.stdout);
    let top = objs
        .iter()
        .find(|o| o["session_id"] == serde_json::json!(SESS))
        .expect("top-level row present");
    assert_eq!(top["is_subagent"], serde_json::json!(false));
    assert_eq!(top["parent_session_id"], serde_json::json!(SESS));
    let sub = objs
        .iter()
        .find(|o| o["session_id"] == serde_json::json!("sub111"))
        .expect("subagent row present");
    assert_eq!(sub["is_subagent"], serde_json::json!(true));
    assert_eq!(sub["parent_session_id"], serde_json::json!(SESS));

    let t = h.run(&["list", at(SESS).as_str()]);
    assert!(t.success, "stderr: {}", t.stderr);
    assert!(
        t.stdout
            .contains("scope  2 sessions in scope (1 top-level + 1 subagent)"),
        "missing scope banner: {}",
        t.stdout
    );
    assert!(
        t.stdout
            .contains(&format!("SUBAGENT  sub111  ·  parent SESSION {SESS}")),
        "subagent row not branded: {}",
        t.stdout
    );

    // --no-subagents drops the banner + the subagent row entirely.
    let top_only = h.run(&["list", at(SESS).as_str(), "--no-subagents"]);
    assert!(top_only.success, "stderr: {}", top_only.stderr);
    assert!(
        !top_only.stdout.contains("scope  "),
        "no banner when no subagents in scope: {}",
        top_only.stdout
    );
    assert!(
        !top_only.stdout.contains("SUBAGENT"),
        "no subagent row under --no-subagents: {}",
        top_only.stdout
    );
}

#[test]
fn search_subagent_hit_json_marks_refeedable_parent() {
    // A search hit inside a subagent transcript: JSON carries is_subagent + the re-feedable
    // parent uuid (the bare-hex session_id is not a --session target).
    let h = Home::new();
    subagents_only_scenario(&h);
    // The subagent's user seed contains "write a file".
    let out = h.run(&[
        "search",
        "write a file",
        at(SESS).as_str(),
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let sub = objs
        .iter()
        .find(|o| o["is_subagent"] == serde_json::json!(true))
        .expect("a subagent hit present");
    assert_eq!(sub["session_id"], serde_json::json!("sub111"));
    assert_eq!(sub["parent_session_id"], serde_json::json!(SESS));
}

#[test]
fn search_at_uuid_path_scopes_to_session() {
    // `search "" @<uuid>` scopes to that session via the `@<uuid>` PATH positional (the grammar
    // that replaced the removed bare-uuid-pattern routing). An empty pattern = pure filter over
    // scope, so the session's own exchanges come back.
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["search", "", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("{}·t", &SESS[..8])),
        "scoped search should return the session's exchanges: {}",
        out.stdout
    );
}

#[test]
fn search_bare_uuid_is_a_literal_pattern_not_a_scope() {
    // A BARE uuid (no `@`) as the sole positional is now a LITERAL pattern, NOT a session scope.
    // It is searched verbatim across the corpus and emits no scope-routing note.
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["search", SESS]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("is a session id, not a pattern"),
        "a bare uuid must NOT be routed to a scope anymore; stderr: {}",
        out.stderr
    );
}

#[test]
fn search_help_mentions_regex_dialect_boundaries() {
    let h = Home::new();
    let out = h.run(&["search", "--help"]);
    assert!(out.success);
    assert!(
        out.stdout.contains("linear-time"),
        "dialect block: {}",
        out.stdout
    );
    assert!(out.stdout.contains("backreference"));
    assert!(out.stdout.contains("lookahead") || out.stdout.contains("lookbehind"));
}

#[test]
fn recover_restore_default_returns_raw_full_content() {
    // Default mode (no --salvage/--patches/--at/--coverage) RESTOREs the file's final content
    // as RAW bytes — no SESSION banner, no line numbers, no mode footer — because this session
    // saw the whole file (the post-drift full Read re-establishes all 6 lines).
    let h = recover_scenario_home();
    let out = h.run(&["recover", at(SESS).as_str(), "--file", RFILE]);
    assert!(out.success, "stderr: {}", out.stderr);
    let expected =
        "import os\nwith open(src) as fh:\n    raw = fh.read()\nuse(raw)\nprint(café🛠)\nEOF\n";
    assert_eq!(out.stdout, expected, "raw restored content");
    // No decoration leaks into the restorable bytes.
    for banned in ["SESSION", "mode=", "  1  "] {
        assert!(
            !out.stdout.contains(banned),
            "no {banned} in raw restore: {}",
            out.stdout
        );
    }
}

#[test]
fn recover_restore_out_writes_raw_file_no_stdout() {
    let h = recover_scenario_home();
    let out_path = h.root.join("restored.py");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        RFILE,
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.is_empty(),
        "restore --out keeps stdout empty (note goes to stderr): {:?}",
        out.stdout
    );
    assert!(
        out.stderr.contains("recovered"),
        "stderr note: {}",
        out.stderr
    );
    let written = std::fs::read_to_string(&out_path).unwrap();
    assert_eq!(
        written,
        "import os\nwith open(src) as fh:\n    raw = fh.read()\nuse(raw)\nprint(café🛠)\nEOF\n"
    );
}

#[test]
fn recover_real_reconstruction_matches_disk_on_contiguous_prefix() {
    let Some((enc, sess, _)) = real_fixture() else {
        eprintln!("SKIP recover_real_reconstruction_matches_disk: real fixture absent");
        return;
    };
    // Reconstruct the plan file from its Read/Edit stream (NOT the whole-plan anchor) and
    // assert the contiguous-from-line-1 KNOWN prefix matches the live on-disk file
    // byte-for-byte. Gaps + post-drift islands are allowed (partial by design); the
    // trustworthy contiguous prefix must never disagree with disk.
    let disk_plan = PathBuf::from(std::env::var_os("HOME").unwrap())
        .join(".claude")
        .join("plans")
        .join("goofy-finding-kettle.md");
    if !disk_plan.is_file() {
        eprintln!("SKIP: on-disk plan file absent");
        return;
    }
    let out = run_real(&[
        "recover",
        &enc,
        at(sess).as_str(),
        "--file",
        disk_plan.to_str().unwrap(),
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // A leading {kind:"header"} scope record may precede the snapshot when the scope
    // spans subagents — find the first snapshot object (the one carrying `lines`), not just
    // the first non-empty line.
    let snap: serde_json::Value = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| v.get("lines").is_some())
        .expect("a snapshot object with a `lines` array");
    let mut known: std::collections::BTreeMap<usize, String> = std::collections::BTreeMap::new();
    for l in snap.get("lines").and_then(|v| v.as_array()).unwrap() {
        let n = l.get("n").and_then(|v| v.as_u64()).unwrap() as usize;
        let t = l.get("text").and_then(|v| v.as_str()).unwrap().to_string();
        known.insert(n, t);
    }
    let disk = std::fs::read_to_string(&disk_plan).unwrap();
    let disk_lines: Vec<&str> = {
        let mut v: Vec<&str> = disk.split('\n').collect();
        if v.last() == Some(&"") {
            v.pop();
        }
        v
    };
    // Walk the contiguous prefix from line 1 and assert each known line matches disk.
    let mut n = 1usize;
    let mut prefix_len = 0usize;
    while let Some(text) = known.get(&n) {
        assert!(
            n <= disk_lines.len(),
            "reconstructed beyond disk length at {n}"
        );
        assert_eq!(
            text,
            disk_lines[n - 1],
            "contiguous-prefix line {n} must match disk"
        );
        prefix_len = n;
        n += 1;
    }
    assert!(
        prefix_len > 50,
        "expected a substantial clean prefix, got {prefix_len}"
    );
}

#[test]
fn recover_coverage_groups_multiple_sessions_with_separator() {
    // Two sessions BOTH touching the same file via a positional project-path target (no
    // --session) → the coverage renderer prints two SESSION headers separated by a blank
    // line (the `if !*first` separator arm).
    let h = Home::new();
    let sess_a = "aaaaaaaa-1111-1111-1111-111111111111";
    let sess_b = "bbbbbbbb-2222-2222-2222-222222222222";
    for s in [sess_a, sess_b] {
        h.write(
            &format!("{ENC}/{s}.jsonl"),
            concat!(
                r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
                r#"{"type":"user","uuid":"r0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"file":{"filePath":"/p/shared.rs","content":"a\nb","startLine":1,"numLines":2,"totalLines":2}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"rd0","content":"ok"}]}}"#, "\n",
            ),
        );
    }
    let out = h.run(&["recover", ENC, "--file", "/p/shared.rs", "--coverage"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.matches("SESSION").count() >= 2,
        "two session headers with a separator: {}",
        out.stdout
    );
}

#[test]
fn turns_dedup_demotes_summary_match_never_drops() {
    // Turn 0's user ("the very first ask...") is quoted verbatim by SUMMARY #1's §6.
    // BUT turn 0 sits BEFORE older boundaries (compactions_before > 0), so it is NOT
    // deduped (older summary content is gone from context). To exercise live-region
    // dedup we check the NEWEST summary's quotes against live turns; the fixture's live
    // turns are unique, so dedup count may be 0 here — assert the mechanism via the
    // header only when it fires, and always assert nothing is dropped.
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
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    // Every selected unit has a boolean also_in_summary field (mechanism wired).
    for o in objs.iter().filter(|o| o.get("role").is_some()) {
        assert!(o["also_in_summary"].is_boolean());
    }
    // Turn 0's verbatim user text is still present (not dropped) even though SUMMARY #1
    // quotes it — pre-boundary turns are pure restoration.
    assert!(
        objs.iter().any(|o| o["role"] == "user"
            && o["text"]
                .as_str()
                .unwrap()
                .contains("the very first ask about the café")),
        "the pre-boundary verbatim user turn is restored, never dropped"
    );
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
fn turns_include_subagents_opts_into_span_with_scope_banner() {
    // `--include-subagents` is the explicit opt-in for the rare cross-fan-out reconstruction;
    // it spans the subagents AND prints a scope banner that reports the TRUE top-level/subagent
    // split (never `0 top-level`, even though the budget applies per session).
    let h = populated_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--subagents",
        "--budget",
        "40000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("(subagent transcript)"),
        "--include-subagents must span subagents: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("scope  ") && out.stdout.contains("1 top-level"),
        "scope banner must report the targeted top-level (never 0 top-level): {}",
        out.stdout
    );
}
