use crate::harness::*;

#[test]
fn search_short_match_has_no_truncation_caution() {
    // Every "carry" match in the canonical fixture fits the cap → nothing clipped → no caution.
    let h = populated_home();
    let out = h.run(&["search", "carry"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("TRUNCATED"),
        "no truncation expected for short matches:\n{}",
        out.stdout
    );
}

#[test]
fn search_category_filter_and_max_count() {
    let h = populated_home();
    // "carry" matches the top-level session AND both subagents (each is one exchange), so
    // --max-count 1 caps to one and DROPS the rest (the drop note appears only when something is
    // actually dropped — the footer no longer prints "0 dropped"). (No `-t`: the subagent "carry"
    // records are spawn-prompt openers, now `agent.communication.inbox`, not `user`.)
    let out = h.run(&["search", "carry", "--max-count", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The footer reports the TRUE match total (both-ends law) — the cap only windows the
    // emitted exchanges, and the drop is disclosed at BOTH ends.
    assert!(out.stdout.contains("matched 3"), "{}", out.stdout);
    assert!(
        out.stdout.contains("showing earliest 1"),
        "the head banner discloses the window: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("dropped by --max-count"),
        "{}",
        out.stdout
    );
}

#[test]
fn search_empty_pattern_warns_then_emits() {
    let h = populated_home();
    let out = h.run(&["search", ""]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The unbounded empty-pattern warning goes to stderr.
    assert!(
        out.stderr.contains("empty pattern with no category"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn search_empty_pattern_with_session_target_only_does_not_warn() {
    // Empty pattern + ONLY an `@<uuid>` session target (no category/time/turn filter) → the
    // warning's `has_session_filter` operand (a `pins_single_session` target) is TRUE → warning
    // suppressed.
    let h = populated_home();
    let out = h.run(&["search", "", at(SESS).as_str(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("empty pattern with no category"),
        "an @<uuid> session scope must suppress the warning; stderr: {}",
        out.stderr
    );
}

#[test]
fn search_empty_pattern_with_category_does_not_warn() {
    // An empty pattern but WITH a `-t` category → the warning's
    // `args.categories.is_empty()` operand is FALSE, so the warning is suppressed.
    let h = populated_home();
    let out = h.run(&[
        "search",
        "",
        "-t",
        "user",
        "--no-subagents",
        at(SESS).as_str(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("empty pattern with no category"),
        "category filter must suppress the warning; stderr: {}",
        out.stderr
    );
}

#[test]
fn search_empty_pattern_with_uuid_positional_does_not_warn() {
    // A bare-uuid POSITIONAL routes to the SAME session filter as `--session` (via
    // resolve_session_files), so the empty-pattern warning — which claims "no session
    // filter" — must be SUPPRESSED. Previously the gate only inspected `--session` and
    // printed the misleading warning here.
    let h = populated_home();
    let out = h.run(&["search", "", at(SESS).as_str(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stderr.contains("empty pattern with no category"),
        "a bare-uuid positional scopes to one session and must suppress the warning; \
         stderr: {}",
        out.stderr
    );
}

#[test]
fn search_short_t_after_positional_parses_and_filters() {
    // The reported critical bug: a trailing short flag after the positional path used to
    // be swallowed ("no project dir named -t"). End-to-end through the real binary, a
    // `-t user` after the path must now parse and filter to user turns.
    let h = populated_home();
    let out = h.run(&["search", "carry", ENC, "-t", "user", "--no-subagents"]);
    assert!(
        out.success,
        "short flag after positional must parse; stderr: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("no Claude Code project dir named"),
        "the short flag must not be misrouted as a project dir; stderr: {}",
        out.stderr
    );
}

#[test]
fn search_short_i_after_positional_parses() {
    // The trailing boolean short flag `-i` likewise must parse, not error.
    let h = populated_home();
    let out = h.run(&["search", "CARRY", ENC, "-i", "--no-subagents"]);
    assert!(
        out.success,
        "trailing -i must parse; stderr: {}",
        out.stderr
    );
    assert!(!out.stderr.contains("no Claude Code project dir named"));
}

#[test]
fn search_with_positional_path_target_like_siblings() {
    // `csift search PATTERN <encoded>` — a POSITIONAL path, exactly like
    // `files`/`recover`/`turns`; exercises the explicit-paths branch (`paths.is_empty()` FALSE).
    let h = populated_home();
    let out = h.run(&["search", "carry", ENC, "--no-subagents"]);
    assert!(
        out.success,
        "positional PATH must work; stderr: {}",
        out.stderr
    );
    assert!(out.stdout.contains("matched"), "got: {}", out.stdout);
}

#[test]
fn search_count_prints_only_the_match_total() {
    // `-c`/--count: just the integer, no headers — and it must equal the footer `matched`
    // (the ripgrep `-c` contract). Compare against the JSON summary so the assertion tracks
    // whatever the fixture actually yields.
    let h = populated_home();
    let full = h.run(&["search", "carry", "--no-subagents", "--format", "json"]);
    let footer: serde_json::Value = serde_json::from_str(
        full.stdout
            .lines()
            .filter(|l| !l.is_empty())
            .next_back()
            .unwrap(),
    )
    .unwrap();
    let expected = footer["matched"].as_u64().unwrap();

    let out = h.run(&["search", "carry", "--no-subagents", "-c"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert_eq!(
        out.stdout.trim().parse::<u64>().unwrap(),
        expected,
        "-c must print exactly the match total; got {:?}",
        out.stdout
    );
    // No per-exchange output leaked through.
    assert!(!out.stdout.contains("SESSION"), "got: {}", out.stdout);
    assert!(!out.stdout.contains("matched "), "got: {}", out.stdout);

    // JSON form is `{"matched":N}`.
    let j = h.run(&[
        "search",
        "carry",
        "--no-subagents",
        "-c",
        "--format",
        "json",
    ]);
    let v = json_summary(&j.stdout);
    assert_eq!(v["matched"].as_u64().unwrap(), expected);
}

#[test]
fn search_count_reports_true_total_despite_max_count() {
    // `-c` reports the TRUE total even when `--max-count` would cap the listing (the count
    // adds the capped-away remainder back), so the number is never silently shrunk.
    let h = populated_home();
    let capped = h.run(&["search", "carry", "-c", "--max-count", "1"]);
    let uncapped = h.run(&["search", "carry", "-c"]);
    assert_eq!(
        capped.stdout.trim(),
        uncapped.stdout.trim(),
        "--max-count must not change the -c total"
    );
}

#[test]
fn search_siblings_surface_the_rest_of_the_turn() {
    // The Q3 shape: a matched USER record should be able to surface WITH the agent reply.
    // "needed" lives ONLY in the opening user message, so the agent text can reach the
    // output only as a `--siblings` context record (default sibling set = all-but-`-t`).
    let h = populated_home();
    let base = h.run(&["search", "needed", "-t", "user", "--no-subagents"]);
    assert!(
        !base.stdout.contains("partial line at a chunk boundary"),
        "without --siblings the agent reply must NOT appear: {}",
        base.stdout
    );

    // `--siblings` (zero-arg): the fixed policy renders the turn's other side.
    let out = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--siblings",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("· agent"),
        "the agent sibling renders under the `·` marker: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("partial line at a chunk boundary"),
        "the agent reply text surfaces as a sibling: {}",
        out.stdout
    );
    // The matched user record opens the exchange (◂) and is never repeated as a sibling.
    assert_eq!(
        out.stdout.matches("carry needed").count(),
        1,
        "the matched user line appears once, not duplicated as a sibling: {}",
        out.stdout
    );
}

#[test]
fn search_siblings_fixed_policy_renders_turn_and_json_carries_array() {
    // `--siblings` (zero-arg, FIXED policy): message units always render; the fixture
    // turn's few tool units fall inside the per-leaf caps, so the whole back-and-forth
    // surfaces. JSON carries the `siblings` array; absent without the flag.
    let h = populated_home();
    let out = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--siblings",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("· agent.message"),
        "agent.message sibling present: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("· agent.tool.use"),
        "tool siblings render under the fixed caps: {}",
        out.stdout
    );

    let j = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--siblings",
        "--format",
        "json",
    ]);
    let env = json_rows(&j.stdout, "exchange").remove(0);
    let sibs = env["siblings"].as_array().expect("siblings array present");
    assert!(
        sibs.iter().any(|s| s["label"] == "agent.message"),
        "{sibs:?}"
    );
    assert_eq!(
        env["siblings_hidden"], 0,
        "nothing capped on this small turn"
    );

    let plain = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--format",
        "json",
    ]);
    let env2 = json_rows(&plain.stdout, "exchange").remove(0);
    assert!(
        env2.get("siblings").is_none(),
        "no siblings key without the flag: {env2}"
    );
}

#[test]
fn search_no_truncate_emits_the_untruncated_record() {
    // A message far longer than the ~400-char excerpt cap, with a token at the very TAIL.
    // The default excerpt truncates (explicit `… (+N chars)` marker) and hides the tail;
    // `--no-truncate` emits the whole record so the tail is readable — the gap that otherwise
    // forces a drop to the raw jsonl.
    let h = Home::new();
    let filler = "x".repeat(900);
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!(
            r#"{{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"needle {filler} taIlToken9z"}}}}"#
        ),
    );

    let def = h.run(&["search", "needle", "--no-subagents"]);
    assert!(def.success, "stderr: {}", def.stderr);
    assert!(
        def.stdout.contains("… (+"),
        "default excerpt must truncate with the explicit marker: {}",
        def.stdout
    );
    assert!(
        !def.stdout.contains("taIlToken9z"),
        "the tail token is hidden by the default cap: {}",
        def.stdout
    );

    let full = h.run(&["search", "needle", "--no-subagents", "--no-truncate"]);
    assert!(
        full.stdout.contains("taIlToken9z"),
        "--no-truncate must surface the tail: {}",
        full.stdout
    );
    assert!(
        !full.stdout.contains("… (+"),
        "--no-truncate removes the truncation marker: {}",
        full.stdout
    );

    // Zero back-compat: the old `--full` spelling is GONE — it must ERROR (unknown argument),
    // never silently work, so existing users are forced onto the unambiguous `--no-truncate`.
    let removed = h.run(&["search", "needle", "--no-subagents", "--full"]);
    assert!(
        !removed.success,
        "--full was removed and must be rejected, got success:\n{}",
        removed.stdout
    );
}

#[test]
fn search_hit_carries_line_and_uuid_address() {
    let h = populated_home();
    // "needed" lives only in the opening user record (fixture line 1).
    let out = h.run(&["search", "needed", "-t", "user", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("user.message  L1  "),
        "the hit header carries its `L<line>` address: {}",
        out.stdout
    );
    // JSON: per-hit `line` + `uuid` (the `csift get` address).
    let j = h.run(&[
        "search",
        "needed",
        "-t",
        "user",
        "--no-subagents",
        "--format",
        "json",
    ]);
    let env = json_rows(&j.stdout, "exchange").remove(0);
    assert_eq!(env["hits"][0]["line"], 1);
    assert_eq!(env["hits"][0]["uuid"], "u0");
}

#[test]
fn search_footer_always_reports_match_and_session_totals() {
    let h = populated_home();
    let out = h.run(&["search", "carry", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("· 1 session ·"),
        "the footer carries the distinct-session total: {}",
        out.stdout
    );
    // JSON footer gains `sessions` alongside `matched`.
    let j = h.run(&["search", "carry", "--no-subagents", "--format", "json"]);
    let footer: serde_json::Value = serde_json::from_str(
        j.stdout
            .lines()
            .filter(|l| !l.is_empty())
            .next_back()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(footer["sessions"], 1);
    assert!(footer["matched"].as_u64().unwrap() >= 1);
}

#[test]
fn show_json_is_header_record_summary() {
    let h = populated_home();
    let out = h.run(&["show", at(SESS).as_str(), "--line", "2", "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.first().unwrap()["kind"], "header");
    assert_eq!(rows.first().unwrap()["command"], "show");
    assert_eq!(rows.last().unwrap()["kind"], "summary");
    // One physical record yields one row PER rendered unit (thinking + message here).
    let rec = rows
        .iter()
        .find(|v| v["kind"] == "record" && v["line"] == 2 && v["label"] == "agent.message")
        .expect("the agent.message row for L2");
    assert_eq!(rec["uuid"], "a0");
    assert_eq!(rec["session_id"], SESS);
    assert!(
        rec["text"]
            .as_str()
            .unwrap()
            .contains("The carry is the partial line at a chunk boundary."),
        "full text on the row: {rec}"
    );
}

#[test]
fn show_raw_emits_the_verbatim_line() {
    let h = populated_home();
    // L8 is the fixture's MALFORMED line — raw emits its exact bytes (that is the point).
    let out = h.run(&["show", at(SESS).as_str(), "--line", "8", "--raw"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert_eq!(
        out.stdout, "{\"type\":\"user\",\"role\":\"user\" this is broken json after the marker}\n",
        "verbatim bytes, trailing newline"
    );
    // raw + --format json is a pointed clash (raw IS the file's own JSON).
    let clash = h.run(&[
        "show",
        at(SESS).as_str(),
        "--line",
        "8",
        "--raw",
        "--format",
        "json",
    ]);
    assert!(!clash.success);
    assert!(clash.stderr.contains("--raw"), "{}", clash.stderr);
}

#[test]
fn stats_aggregates_records_turns_tools_and_tokens() {
    let h = Home::new();
    let enc = "-Users-testuser-Projects-statsy";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"first ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","model":"claude-opus-4-8","usage":{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":100,"cache_creation_input_tokens":5},"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c0","parentUuid":"a0","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"second ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T06:00:09.000Z","message":{"role":"assistant","model":"claude-opus-4-8","usage":{"input_tokens":1,"output_tokens":2},"content":[{"type":"text","text":"done"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["stats", &format!("@{sess}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("turns 2"), "{}", out.stdout);
    assert!(out.stdout.contains("Bash×1"), "{}", out.stdout);
    assert!(
        out.stdout
            .contains("claude-opus-4-8: in 11 · out 22 · cache-read 100 · cache-write 5"),
        "token sums: {}",
        out.stdout
    );

    let j = h.run(&["stats", &format!("@{sess}"), "--format", "json"]);
    assert!(j.success, "stderr: {}", j.stderr);
    let row = json_rows(&j.stdout, "session").remove(0);
    assert_eq!(row["turns"], 2);
    assert_eq!(row["user_records"], 3); // 2 genuine + 1 tool_result carrier
    assert_eq!(row["assistant_records"], 2);
    assert_eq!(row["tools"]["Bash"], 1);
    assert_eq!(row["tokens"]["claude-opus-4-8"]["output"], 22);
    let sum = json_summary(&j.stdout);
    assert_eq!(sum["turns"], 2);

    // --since bounds the counted records (only the second turn's records remain).
    let win = h.run(&[
        "stats",
        &format!("@{sess}"),
        "--since",
        "2026-06-07T05:30:00Z",
        "--format",
        "json",
    ]);
    let row = json_rows(&win.stdout, "session").remove(0);
    assert_eq!(row["turns"], 1, "window admits only the later turn");
    assert_eq!(row["tokens"]["claude-opus-4-8"]["output"], 2);
}
