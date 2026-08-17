use crate::harness::*;

#[test]
fn pinned_id_matching_nothing_bails_never_silent_empty() {
    // AGENTS §4 fail-closed wall (T0.3): a PINNED id that resolves to no file must BAIL loud —
    // never a silent empty, never a widening to every project (the L56255 `--subagent` →
    // whole-corpus class). Both the full-uuid and the prefix forms are locked here so a future
    // resolver change cannot quietly reintroduce scope-widening.
    let h = populated_home();
    // A nonexistent FULL uuid pinned as a target (search's pattern is the 1st positional).
    let a = h.run(&["search", "carry", "@99999999-8888-4777-8666-555555555555"]);
    assert!(
        !a.success,
        "a nonexistent @uuid must error, not widen: {}",
        a.stdout
    );
    // A PREFIX that matches no session must bail, naming the prefix.
    let b = h.run(&["list", "@deadbeef"]);
    assert!(
        !b.success,
        "a no-match @prefix must error, not widen: {}",
        b.stdout
    );
    assert!(
        b.stderr.contains("deadbeef"),
        "the error names the unresolved prefix: {}",
        b.stderr
    );
}

#[test]
fn stats_turn_range_windows_the_aggregates() {
    let h = Home::new();
    let enc = "-Users-testuser-Projects-statturn";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            // Turn 0: one Read tool call.
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"first ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t0","name":"Read","input":{}}]}}"#, "\n",
            // Turn 1: one Edit tool call.
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"second ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{}}]}}"#, "\n",
        ),
    );
    // Bare-N shorthand: turn 1 only — Edit counted, Read not, turns == 1.
    let out = h.run(&["stats", enc, "--turn", "1", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows = json_rows(&out.stdout, "session");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["turns"], 1, "one turn in window: {}", out.stdout);
    assert!(
        rows[0]["tools"].get("Edit").is_some() && rows[0]["tools"].get("Read").is_none(),
        "only turn 1's tool calls count: {}",
        out.stdout
    );
}

#[test]
fn sessions_with_matches_pipes_into_sessions_from_and_refetch_round_trips() {
    let h = populated_home();
    // `-l`: bare ids, one per line — WHICH sessions matched.
    let l = h.run(&["search", "", "-l"]);
    assert!(l.success, "stderr: {}", l.stderr);
    assert!(
        l.stdout.lines().any(|s| s.trim() == SESS),
        "lists the matching session: {}",
        l.stdout
    );
    // The id stream pipes STRAIGHT into `--sessions-from -` (the composition loop closes
    // inside csift — no jq/sed re-quoting).
    let piped = h.run_with_stdin(
        &["stats", "--sessions-from", "-", "--format", "json"],
        &l.stdout,
    );
    assert!(piped.success, "stderr: {}", piped.stderr);
    assert!(
        piped.stdout.contains(SESS),
        "piped scope reached stats: {}",
        piped.stdout
    );
    // `-l --format json` is a pointed error (JSON readers use the summary's transcript_ids).
    let j = h.run(&["search", "", "-l", "--format", "json"]);
    assert!(!j.success);
    // Every JSON hit carries `refetch` — a ready-to-run `csift show` addressed at the hit's
    // OWN transcript — and the command actually round-trips.
    let js = h.run(&["search", "", &format!("@{SESS}"), "--format", "json"]);
    assert!(js.success, "stderr: {}", js.stderr);
    let ex_rows = json_rows(&js.stdout, "exchange");
    let refetch = ex_rows[0]["hits"][0]["refetch"]
        .as_str()
        .expect("refetch is a string");
    assert!(refetch.starts_with("csift show @"), "got: {refetch}");
    let parts: Vec<&str> = refetch.split_whitespace().skip(1).collect();
    let rf = h.run(&parts);
    assert!(rf.success, "the refetch command round-trips: {}", rf.stderr);
}

#[test]
fn search_raw_emits_verbatim_lines_on_a_pure_stdout() {
    // `--raw` = show's escape hatch on search's filter surface: the matched records'
    // VERBATIM jsonl lines, byte-identical to the file, stdout pure (notes → stderr).
    let (h, sess, _hex) = show_subagent_home();
    let out = h.run(&[
        "search",
        "go",
        &format!("@{sess}"),
        "--no-subagents",
        "--raw",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Every stdout line parses as JSON AND is byte-identical to a line of the transcript.
    let disk = std::fs::read_to_string(
        h.projects()
            .join("-Users-testuser-Projects-linehex")
            .join(format!("{sess}.jsonl")),
    )
    .unwrap();
    let disk_lines: Vec<&str> = disk.lines().collect();
    let mut n = 0;
    for line in out.stdout.lines().filter(|l| !l.trim().is_empty()) {
        assert!(
            serde_json::from_str::<serde_json::Value>(line).is_ok(),
            "stdout is a pure jsonl stream: {line}"
        );
        assert!(
            disk_lines.contains(&line),
            "verbatim (byte-identical to the file): {line}"
        );
        n += 1;
    }
    assert!(n >= 1, "at least the matching record emits: {}", out.stdout);
    // The filter surface still applies: an excluding -T yields an empty (exit 0) stream.
    let none = h.run(&[
        "search",
        "go",
        &format!("@{sess}"),
        "--no-subagents",
        "--raw",
        "-T",
        "user",
    ]);
    assert!(none.success, "stderr: {}", none.stderr);
    assert!(
        none.stdout.trim().is_empty(),
        "user-record hit excluded: {}",
        none.stdout
    );
    // Conflicts: --raw excludes the rendered-surface modes.
    for extra in [["--siblings"], ["-c"], ["-l"]] {
        let mut args = vec!["search", "go", "--raw"];
        args.extend_from_slice(&extra);
        let bad = h.run(&args);
        assert!(!bad.success, "--raw + {extra:?} must conflict");
    }
    let badjson = h.run(&["search", "go", "--raw", "--format", "json"]);
    assert!(!badjson.success, "--raw + --format json must error");
}

#[test]
fn label_not_flag_surface_and_empty_set_guard() {
    // `-T` mirrors `-t` (rg's -t/-T duality): same selector grammar, exclusion semantics.
    let (h, sess, _hex) = show_subagent_home();
    // The main transcript's L2 is an Agent tool_use — `-T agent.tool` must drop it while a
    // plain filter still finds it.
    let plain = h.run(&[
        "search",
        "",
        &format!("@{sess}"),
        "--no-subagents",
        "-t",
        "agent.tool.use",
    ]);
    assert!(plain.success, "stderr: {}", plain.stderr);
    assert!(
        plain.stdout.contains("Agent"),
        "premise: the tool_use hits: {}",
        plain.stdout
    );
    let excl = h.run(&[
        "search",
        "",
        &format!("@{sess}"),
        "--no-subagents",
        "-T",
        "agent.tool",
    ]);
    assert!(excl.success, "stderr: {}", excl.stderr);
    assert!(
        !excl.stdout.contains("agent.tool.use"),
        "-T agent.tool drops the tool_use hit: {}",
        excl.stdout
    );
    // An invalid -T selector gets the same teaching error as -t.
    let bad = h.run(&["search", "x", "-T", "thinking"]);
    assert!(!bad.success);
    // A statically-empty include-minus-exclude combination is a hard error, never an
    // honest-looking empty result.
    let contradictory = h.run(&[
        "search",
        "x",
        "-t",
        "agent.thinking",
        "-T",
        "agent.thinking",
    ]);
    assert!(!contradictory.success);
    assert!(
        contradictory.stderr.contains("can never match"),
        "the error names the contradiction: {}",
        contradictory.stderr
    );
}

#[test]
fn search_tool_response_names_the_tool_it_answers() {
    let h = populated_home();
    // Fixture L4 = a tool_result for tool_use_id `call0`, whose tool_use (L3) is `Read`.
    let out = h.run(&[
        "search",
        "carry",
        at(SESS).as_str(),
        "--no-subagents",
        "-t",
        "agent.tool.result",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Its tool_use (L3) is in scope, so the label renders the `▹` pair; the `Read` tool name still
    // trails it (so the `agent.tool.result Read` substring holds).
    assert!(
        out.stdout.contains("agent.tool.result Read"),
        "the response names the tool it answers: {}",
        out.stdout
    );
}

#[test]
fn search_text_output_is_token_lean() {
    let h = populated_home();
    let out = h.run(&["search", "carry", at(SESS).as_str(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // Every exchange header opens with the STABLE id-prefix token (`<first-8>·t<n>`) — no
    // per-invocation `sN` ordinal, no `sN = <uuid>` legend block anywhere in the output.
    assert!(
        out.stdout.contains(&format!("{}·t", &SESS[..8])),
        "id-prefix header token: {}",
        out.stdout
    );
    assert!(
        !out.stdout.lines().any(|l| l.starts_with("s1 = ")),
        "no legend line: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("s1·t"),
        "no ordinal header: {}",
        out.stdout
    );
    // The old heavyweight header is gone: no `═══` rule, no uppercase `SESSION `/`TURN `.
    assert!(
        !out.stdout.contains("═══"),
        "no rule glyphs: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("TURN "),
        "no uppercase TURN: {}",
        out.stdout
    );
    // The FULL uuid never repeats per exchange — the 8-char prefix token carries each header.
    assert_eq!(
        out.stdout.matches(SESS).count(),
        0,
        "the full uuid is never printed; the prefix token references it: {}",
        out.stdout
    );
    // Timestamps are single local+offset (no `(<UTC>)` second copy on the turn header).
    assert!(
        !out.stdout.contains(" (2026-"),
        "no parenthesised UTC copy: {}",
        out.stdout
    );
}

#[test]
fn search_header_tokens_are_stable_across_invocations() {
    // The header token derives from the transcript id (its leading chars), never from
    // enumeration order — two identical invocations emit byte-identical output, so a token
    // pasted from an earlier run still names the same transcript.
    let h = populated_home();
    let a = h.run(&["search", "carry"]);
    let b = h.run(&["search", "carry"]);
    assert!(
        a.success && b.success,
        "stderr: {} / {}",
        a.stderr,
        b.stderr
    );
    assert_eq!(
        a.stdout, b.stdout,
        "byte-identical output across identical invocations"
    );
}

#[test]
fn search_header_token_collision_lengthens_the_group_only() {
    // Two DISTINCT ids sharing their first 8 chars lengthen TOGETHER to their first 12 raw
    // chars (for a uuid that spans the first dash — still a valid `@` target); the
    // non-colliding third id stays at 8. The bare collided 8-prefix never appears as a token.
    let h = Home::new();
    let _ = header_collision_scenario(&h);
    let out = h.run(&["search", "SEEDWORD"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("aaaabbbb-111\u{b7}t"),
        "colliding id 1 lengthens to 12: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("aaaabbbb-222\u{b7}t"),
        "colliding id 2 lengthens to 12: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("ccccdddd\u{b7}t"),
        "non-collider stays at 8: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("aaaabbbb\u{b7}t"),
        "the collided bare 8-prefix must not appear as a token: {}",
        out.stdout
    );
}

#[test]
fn search_lengthened_uuid_token_resolves_and_short_prefix_fails_loud() {
    // A collision-lengthened header token (12 chars, spanning the uuid's first dash) is a
    // valid `@` target; the ambiguous bare 8-prefix fails loud naming the candidates.
    let h = Home::new();
    let (c1, c2, _) = header_collision_scenario(&h);
    let one = h.run(&["search", "SEEDWORD", "@aaaabbbb-111"]);
    assert!(one.success, "stderr: {}", one.stderr);
    assert!(one.stdout.contains("COLLIDEONE"), "got: {}", one.stdout);
    assert!(
        !one.stdout.contains("COLLIDETWO"),
        "the sibling session must be out of scope: {}",
        one.stdout
    );
    let ambi = h.run(&["search", "SEEDWORD", "@aaaabbbb"]);
    assert!(!ambi.success, "an ambiguous prefix must error");
    assert!(
        ambi.stderr.contains("AMBIGUOUS") && ambi.stderr.contains(c1) && ambi.stderr.contains(c2),
        "the error names both candidates: {}",
        ambi.stderr
    );
}

#[test]
fn search_subagent_header_carries_parent_token_on_every_exchange() {
    // EVERY subagent exchange header carries `(parent <first-8-of-owning-uuid>)` — a
    // tail-truncated read must still resolve ownership; top-level headers carry no parent.
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["search", "", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains(&format!("sub111\u{b7}t0 (parent {})", &SESS[..8])),
        "subagent header carries the parent token: {}",
        out.stdout
    );
    for line in out.stdout.lines() {
        if line.starts_with(&format!("{}\u{b7}t", &SESS[..8])) {
            assert!(
                !line.contains("(parent"),
                "a top-level header must not carry a parent: {line}"
            );
        }
    }
}

#[test]
fn search_and_show_resolve_an_agent_id_prefix_token() {
    // An 8-char prefix of a subagent's bare-hex id — the exact header token `search`
    // emits — resolves as an `@` target on every target-taking surface.
    let h = Home::new();
    let (_, _, gamma) = agent_prefix_scenario(&h);
    let out = h.run(&["search", "AGENTGAMMA", &format!("@{}", &gamma[..8])]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("AGENTGAMMA"), "got: {}", out.stdout);
    let s = h.run(&["show", &format!("@{}", &gamma[..8]), "--line", "1"]);
    assert!(s.success, "show by agent prefix: {}", s.stderr);
    assert!(s.stdout.contains("AGENTGAMMA"), "got: {}", s.stdout);
}

#[test]
fn search_agent_twelve_hex_token_falls_back_to_unique_prefix() {
    // Two agents sharing their first 8 hex lengthen their header tokens to 12; a 12-hex
    // token routes as an exact agent id, misses, and falls back to a UNIQUE literal-prefix
    // match. The ambiguous 8-hex form fails loud naming both ids.
    let h = Home::new();
    let (alpha, beta, _) = agent_prefix_scenario(&h);
    let out = h.run(&["search", "seed"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("{}\u{b7}t", &alpha[..12]))
            && out.stdout.contains(&format!("{}\u{b7}t", &beta[..12])),
        "colliding agent tokens lengthen to 12: {}",
        out.stdout
    );
    let one = h.run(&["search", "AGENT", &format!("@{}", &alpha[..12])]);
    assert!(one.success, "stderr: {}", one.stderr);
    assert!(one.stdout.contains("AGENTALPHA"), "got: {}", one.stdout);
    assert!(
        !one.stdout.contains("AGENTBETA"),
        "the sibling agent must be out of scope: {}",
        one.stdout
    );
    let ambi = h.run(&["search", "AGENT", &format!("@{}", &alpha[..8])]);
    assert!(!ambi.success, "an ambiguous agent prefix must error");
    assert!(
        ambi.stderr.contains("AMBIGUOUS")
            && ambi.stderr.contains(alpha)
            && ambi.stderr.contains(beta),
        "the error names both agent ids: {}",
        ambi.stderr
    );
}

#[test]
fn search_match_banner_at_head_mirrors_footer_and_json() {
    // The head banner carries the TRUE totals + direction before the first exchange; the
    // footer repeats the same numbers; the JSON summary's post-cap `matched` +
    // `dropped_by_cap` reconcile to the banner total.
    let h = Home::new();
    let _ = header_collision_scenario(&h);
    let out = h.run(&["search", "SEEDWORD"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // No subagents in scope → the scope banner is suppressed, so the banner is line 1.
    assert_eq!(
        out.stdout.lines().next(),
        Some("matches  3 exchanges · 3 sessions · oldest first"),
        "head banner: {}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("matched 3 exchanges · 3 sessions · label=all"),
        "footer repeats the totals: {}",
        out.stdout
    );
    // Clean-corpus duals for the footer's `> 0` note gates: no malformed note, no
    // sidecar note, no zero-count drop note in the plain text mode.
    assert!(
        !out.stdout.contains("malformed") && !out.stdout.contains("sidecar"),
        "no zero-count footer notes on a clean run: {}",
        out.stdout
    );
    let js = h.run(&["search", "SEEDWORD", "--format", "json"]);
    assert!(js.success, "stderr: {}", js.stderr);
    assert!(
        !js.stdout.contains("matches  "),
        "no banner in JSON mode: {}",
        js.stdout
    );
    let last: serde_json::Value =
        serde_json::from_str(js.stdout.lines().next_back().unwrap()).unwrap();
    assert_eq!(last["matched"], serde_json::json!(3));
    assert_eq!(last["sessions"], serde_json::json!(3));

    // Capped: the banner keeps the TRUE total and discloses the window; JSON reconciles.
    let cap = h.run(&["search", "SEEDWORD", "--max-count", "2"]);
    assert!(cap.success, "stderr: {}", cap.stderr);
    assert_eq!(
        cap.stdout.lines().next(),
        Some("matches  3 exchanges · 3 sessions · oldest first · showing earliest 2"),
        "capped head banner: {}",
        cap.stdout
    );
    assert!(
        cap.stdout.contains("matched 3 exchanges · 3 sessions")
            && cap.stdout.contains("1 later dropped by --max-count"),
        "capped footer: {}",
        cap.stdout
    );
    let capjs = h.run(&["search", "SEEDWORD", "--max-count", "2", "--format", "json"]);
    let last: serde_json::Value =
        serde_json::from_str(capjs.stdout.lines().next_back().unwrap()).unwrap();
    assert_eq!(
        last["matched"].as_u64().unwrap() + last["dropped_by_cap"].as_u64().unwrap(),
        3,
        "JSON post-cap matched + dropped reconcile to the banner total"
    );

    // Single-purpose modes carry no banner.
    let c = h.run(&["search", "SEEDWORD", "-c"]);
    assert!(
        !c.stdout.contains("matches  "),
        "no banner under -c: {}",
        c.stdout
    );
    let l = h.run(&["search", "SEEDWORD", "-l"]);
    assert!(
        !l.stdout.contains("matches  "),
        "no banner under -l: {}",
        l.stdout
    );
    let cb = h.run(&["search", "SEEDWORD", "--count-by", "label"]);
    assert!(
        !cb.stdout.contains("matches  "),
        "no banner under --count-by: {}",
        cb.stdout
    );
}
