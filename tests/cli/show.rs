//! show end to end: line/uuid/turn addressing, caps, hard misses, single-transcript rule.

use crate::harness::*;

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
    // L8 is the fixture's MALFORMED line - raw emits its exact bytes (that is the point).
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
fn show_bad_flag_error_names_the_flag_not_the_target() {
    // A mistyped/foreign flag on `show` must be blamed BY NAME in any position - never the
    // user's perfectly valid target (the misattribution sent a real consumer down a
    // targeting-grammar rabbit hole). Same error family as every sibling command.
    let h = populated_home();
    let sess = format!("@{SESS}");
    for argv in [
        vec!["show", &sess, "--line", "1", "--no-truncate"], // flag after target
        vec!["show", "--no-truncate", &sess, "--line", "1"], // flag before target
        vec!["show", &sess, "--bogus-flag"],                 // fully invented flag
    ] {
        let out = h.run(&argv);
        assert!(!out.success, "must fail: {argv:?}");
        let flag = if argv.contains(&"--bogus-flag") {
            "--bogus-flag"
        } else {
            "--no-truncate"
        };
        assert!(
            out.stderr.contains(flag) && out.stderr.contains("did you mistype a flag"),
            "error must name {flag}: {}",
            out.stderr
        );
        assert!(
            !out.stderr
                .contains(&format!("unexpected argument '{sess}'")),
            "must not blame the valid target: {}",
            out.stderr
        );
    }
    // Two targets: a pointed arity error (addresses are per-FILE), not a clap surplus.
    let out = h.run(&["show", &sess, "@1234abcd", "--line", "1"]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("exactly ONE transcript"),
        "arity error: {}",
        out.stderr
    );
}

#[test]
fn show_by_turn_fetches_the_whole_turn() {
    let h = populated_home();
    let t = at(SESS);
    // Turn 0 = the first genuine-user turn AND its whole back-and-forth (unified fetch - no
    // "pick the command by what address you hold"; `show` addresses by line, uuid, OR turn).
    let out = h.run(&["show", t.as_str(), "--turn", "0"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("why is the carry needed?"),
        "turn 0 user message: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("partial line"),
        "turn 0 agent reply: {}",
        out.stdout
    );
    // Turn 1 is a DIFFERENT turn - the numbering matches the `·tN` in `search`'s headers.
    let out1 = h.run(&["show", t.as_str(), "--turn", "1"]);
    assert!(
        out1.stdout.contains("now explain the panic path"),
        "turn 1: {}",
        out1.stdout
    );
    assert!(
        !out1.stdout.contains("why is the carry needed?"),
        "turn 1 must not bleed into turn 0: {}",
        out1.stdout
    );
    // `-1..` = the last turn (the tail-peek / monitoring intent → `show`, not a special mode).
    let last = h.run(&["show", t.as_str(), "--turn", "-1.."]);
    assert!(
        last.stdout.contains("now explain the panic path"),
        "last turn via -1..: {}",
        last.stdout
    );
    // Mutually exclusive with --line (one addressing mode at a time).
    let conflict = h.run(&["show", t.as_str(), "--turn", "0", "--line", "5"]);
    assert!(!conflict.success, "--turn + --line must conflict");
    // --raw emits the turn's records verbatim.
    let raw = h.run(&["show", t.as_str(), "--turn", "0", "--raw"]);
    assert!(raw.success, "stderr: {}", raw.stderr);
    assert!(
        raw.stdout.contains("why is the carry needed?"),
        "raw turn 0: {}",
        raw.stdout
    );
    // JSON records all carry turn_index 0.
    let j = h.run(&["show", t.as_str(), "--turn", "0", "--format", "json"]);
    let recs = json_rows(&j.stdout, "record");
    assert!(!recs.is_empty(), "json records: {}", j.stdout);
    assert!(
        recs.iter().all(|r| r["turn_index"] == serde_json::json!(0)),
        "every fetched record is in turn 0: {}",
        j.stdout
    );
}

#[test]
fn show_line_fetches_the_record_in_full() {
    let h = populated_home();
    // Fixture L1 = the opening user record. Addressing it returns it FULL.
    let out = h.run(&["show", at(SESS).as_str(), "--line", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("◂ user.message  L1  "),
        "the addressed user record, with its L1 address: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("why is the carry needed?"),
        "the full body: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains(&format!("SESSION {SESS}")),
        "the transcript banner: {}",
        out.stdout
    );
}

#[test]
fn show_line_renders_uncapped() {
    let h = populated_home();
    // L2 = the assistant thinking + agent-text record. Fetched → full (no excerpt cap).
    let out = h.run(&["show", at(SESS).as_str(), "--line", "2"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("The carry is the partial line at a chunk boundary."),
        "the agent block renders end-to-end: {}",
        out.stdout
    );
}

#[test]
fn show_multiple_lines_and_ranges() {
    let h = populated_home();
    // L1 (turn 0) + L6 (turn 1) - distinct turns, both fetched.
    let list = h.run(&["show", at(SESS).as_str(), "--line", "6,1"]);
    assert!(list.success, "stderr: {}", list.stderr);
    assert!(
        list.stdout.contains("why is the carry needed?")
            && list.stdout.contains("now explain the panic path"),
        "both addressed records: {}",
        list.stdout
    );
    // A range expands to every record in span (L1-L7 are records; L8 malformed is skipped).
    let range = h.run(&["show", at(SESS).as_str(), "--line", "1..7"]);
    assert!(range.success, "stderr: {}", range.stderr);
    assert!(
        range.stdout.contains("why is the carry needed?") && range.stdout.contains("No panic"),
        "the spanned records: {}",
        range.stdout
    );
}

#[test]
fn show_uuid_addresses_records() {
    let h = populated_home();
    let one = h.run(&["show", at(SESS).as_str(), "--uuid", "u0"]);
    assert!(one.success, "stderr: {}", one.stderr);
    assert!(
        one.stdout.contains("why is the carry needed?"),
        "by uuid u0: {}",
        one.stdout
    );
    let many = h.run(&["show", at(SESS).as_str(), "--uuid", "u0,u1"]);
    assert!(many.success, "stderr: {}", many.stderr);
    assert!(
        many.stdout.contains("why is the carry needed?")
            && many.stdout.contains("now explain the panic path"),
        "both uuids: {}",
        many.stdout
    );
}

#[test]
fn show_explicit_miss_is_a_hard_error() {
    let h = populated_home();
    // Address law: an explicitly named line that resolves to nothing is an ERROR.
    let out = h.run(&["show", at(SESS).as_str(), "--line", "999"]);
    assert!(!out.success, "an explicit miss must fail: {}", out.stdout);
    assert!(
        out.stderr.contains("no such record") && out.stderr.contains("L999"),
        "the error names the miss: {}",
        out.stderr
    );
    let uuid = h.run(&["show", at(SESS).as_str(), "--uuid", "no-such-uuid"]);
    assert!(!uuid.success);
    assert!(
        uuid.stderr.contains("no-such-uuid"),
        "the error names the uuid: {}",
        uuid.stderr
    );
}

#[test]
fn show_range_clamps_but_errors_when_empty() {
    let h = populated_home();
    // 6-1000: L6/L7 are records; the rest of the range clamps silently.
    let out = h.run(&["show", at(SESS).as_str(), "--line", "6..1000"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("now explain the panic path"),
        "{}",
        out.stdout
    );
    // A range yielding ZERO records errors (addressing nothing is a miss, not an empty ok).
    let empty = h.run(&["show", at(SESS).as_str(), "--line", "900..1000"]);
    assert!(
        !empty.success,
        "a zero-yield range must fail: {}",
        empty.stdout
    );
    assert!(empty.stderr.contains("no such record"), "{}", empty.stderr);
}

#[test]
fn show_requires_an_address() {
    let h = populated_home();
    // No --line/--uuid → a pointed error naming the file (never a whole-transcript dump).
    let out = h.run(&["show", at(SESS).as_str()]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("--line") && out.stderr.contains(".jsonl"),
        "guidance + the resolved path: {}",
        out.stderr
    );
}

#[test]
fn show_subagent_target_addresses_its_transcript() {
    // The TARGET names the transcript: `@<agent-id>` fetches from THAT subagent's file.
    let (h, _sess, hex) = show_subagent_home();
    let out = h.run(&["show", &format!("@{hex}"), "--line", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("sub: do the thing about carry"),
        "the subagent record body: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains(&format!("SUBAGENT {hex}")),
        "the subagent banner: {}",
        out.stdout
    );
}

#[test]
fn show_unknown_agent_id_fails_closed() {
    let (h, _sess, _hex) = show_subagent_home();
    let out = h.run(&["show", "@deadbeefdeadbeef0", "--line", "1"]);
    assert!(
        !out.success,
        "an unmatched agent id must fail, never widen scope; stdout: {}",
        out.stdout
    );
}

#[test]
fn show_turn_oob_and_flood_guard() {
    // v0.5.0: (a) an EXPLICIT `--turn N`/`A..B` is an ADDRESS - fully out of range is a
    // hard error naming the transcript's turn domain (it used to be a silent empty, the
    // one address-miss that violated law 1); (b) open ranges are capped (DEFAULT 200,
    // here forced low) with the exact continuation command reported; (c) non-record
    // lines inside an addressed range are counted, never silently absorbed.
    let h = Home::new();
    let sess = "9a9b9c9d-1111-4222-8333-944455566677";
    let mut lines = String::new();
    for i in 0..3 {
        lines.push_str(&format!(
            r#"{{"type":"user","uuid":"u{i}","sessionId":"{sess}","timestamp":"2026-06-07T05:0{i}:00.000Z","message":{{"role":"user","content":"question {i}"}}}}"#
        ));
        lines.push('\n');
        lines.push_str(&format!(
            r#"{{"type":"assistant","uuid":"a{i}","sessionId":"{sess}","timestamp":"2026-06-07T05:0{i}:05.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"answer {i}"}}]}}}}"#
        ));
        lines.push('\n');
    }
    // A metadata line (never renderable) - line 7.
    lines.push_str(&format!(
        r#"{{"type":"attachment","uuid":"m1","sessionId":"{sess}"}}"#
    ));
    lines.push('\n');
    h.write(
        &format!("-Users-testuser-Projects-oob/{sess}.jsonl"),
        &lines,
    );
    let at = format!("@{sess}");

    // (1) Explicit single turn out of range = hard error with the turn domain.
    let out = h.run(&["show", &at, "--turn", "99"]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("no such turn(s): t99"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("3 turn(s) (t0..t2)"),
        "stderr: {}",
        out.stderr
    );

    // (2) Explicit closed range fully out = same error; PARTIALLY out clamps.
    let out = h.run(&["show", &at, "--turn", "50..99"]);
    assert!(!out.success);
    let out = h.run(&["show", &at, "--turn", "1..99"]);
    assert!(out.success, "partially-out clamps: {}", out.stderr);

    // (3) From-end / open forms clamp - the tail-peek must stay robust.
    let out = h.run(&["show", &at, "--turn", "-9.."]);
    assert!(out.success, "stderr: {}", out.stderr);

    // (4) Flood guard: keep-first + the exact continuation command; the metadata line
    //     in the addressed range is counted, not silently absorbed.
    let out = h.run(&["show", &at, "--line", "..", "--max-count", "2"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("fetched 2 record unit(s)"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("+4 more record unit(s)"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains(&format!("continue: csift show @{sess} --line 3..6")),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("1 line(s) in the addressed range are not records"),
        "{}",
        out.stdout
    );

    // (5) `--max-count 0` = uncapped (the crate-wide convention).
    let out = h.run(&["show", &at, "--line", "..", "--max-count", "0"]);
    assert!(
        out.stdout.contains("fetched 6 record unit(s)"),
        "{}",
        out.stdout
    );

    // (6) JSON summary carries the machine echo of all three.
    let out = h.run(&[
        "show",
        &at,
        "--line",
        "..",
        "--max-count",
        "2",
        "--format",
        "json",
    ]);
    let summary = json_summary(&out.stdout);
    assert_eq!(summary["dropped_by_cap"].as_u64(), Some(4), "{summary}");
    assert_eq!(summary["non_record_lines"].as_u64(), Some(1), "{summary}");
    assert!(
        summary["refetch_remainder"]
            .as_str()
            .is_some_and(|s| s.contains("--line 3..6")),
        "{summary}"
    );

    // (7) The raw mode caps too (stderr note; stdout stays pure jsonl).
    let out = h.run(&["show", &at, "--line", "..", "--max-count", "2", "--raw"]);
    assert_eq!(out.stdout.lines().count(), 2, "{}", out.stdout);
    assert!(
        out.stderr.contains("+5 more line(s)"),
        "raw cap counts LINES (metadata included): {}",
        out.stderr
    );
}

#[test]
fn show_multi_transcript_target_errors() {
    // A project dir holding SEVERAL sessions is ambiguous - show needs exactly one.
    // (A dir that unambiguously holds ONE top-level session is accepted, like the resolver
    // everywhere else: unambiguous ⇒ resolved.)
    let (h, _sess, _hex) = show_subagent_home();
    h.write(
        "-Users-testuser-Projects-linehex/bbbbbbbb-cccc-4ddd-8eee-ffffffffffff.jsonl",
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"second session\"}}\n",
    );
    let out = h.run(&["show", "-Users-testuser-Projects-linehex", "--line", "1"]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("ONE transcript"),
        "the error explains the single-transcript law: {}",
        out.stderr
    );
}

#[test]
fn show_span_flag_is_rejected_with_the_pointed_rule() {
    // R7 §2.3: ten sibling commands take the span pair; `show` does not - the muscle-memory
    // slip gets the actual rule, not the generic "did you mistype a flag?" guess.
    let h = populated_home();
    for flag in ["--no-subagents", "--subagents"] {
        let out = h.run(&["show", &at(SESS), flag, "--line", "1"]);
        assert!(!out.success, "span flag must be rejected: {}", out.stdout);
        assert!(
            out.stderr.contains("no subagent-span flag")
                && out.stderr.contains("exactly ONE transcript"),
            "pointed rule expected, got: {}",
            out.stderr
        );
    }
}

#[test]
fn show_turn_explicit_miss_errors_and_open_forms_clamp() {
    // Mutation pin on the v0.5 turn-address law: an EXPLICIT single --turn miss is a hard
    // error naming the turn; an OPEN out-of-range form CLAMPS (never a hard error); and a
    // clean in-range fetch prints no cap/non-record notes.
    let h = Home::new();
    subagents_only_scenario(&h); // SESS has exactly one turn (t0)
    let miss = h.run(&["show", at(SESS).as_str(), "--turn", "5"]);
    assert!(!miss.success, "explicit single miss must hard-error");
    assert!(
        miss.stderr.contains("no such turn") && miss.stderr.contains("t5"),
        "the miss names the turn: {}",
        miss.stderr
    );
    let open = h.run(&["show", at(SESS).as_str(), "--turn", "99.."]);
    assert!(
        open.success,
        "an open out-of-range form clamps, never errors: {}",
        open.stderr
    );
    let ok = h.run(&["show", at(SESS).as_str(), "--turn", "0"]);
    assert!(ok.success, "stderr: {}", ok.stderr);
    assert!(
        !ok.stdout.contains("beyond the") && !ok.stdout.contains("not records"),
        "no cap/non-record notes on a clean in-range fetch: {}",
        ok.stdout
    );
}

#[test]
fn addressed_show_renders_hook_attachment_without_the_flag() {
    // The refetch a search hit prints (`csift show @<id> --line N`) carries no flag - an
    // explicit line/uuid address must render the attachment record regardless.
    let h = Home::new();
    hook_context_scenario(&h);
    let out = h.run(&["show", &at(HOOKCTX_SESS), "--line", "2"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("quartzlantern") && out.stdout.contains("harborlight"),
        "show --line renders the joined hook context flag-free:\n{}",
        out.stdout
    );
    let out2 = h.run(&["show", &at(HOOKCTX_SESS), "--uuid", "att1"]);
    assert!(
        out2.stdout.contains("quartzlantern"),
        "show --uuid renders it too:\n{}",
        out2.stdout
    );
}

mod branching;
