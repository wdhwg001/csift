//! verbatim command surface: the rename tombstone, required target, flag validation.

use crate::harness::*;

#[test]
fn turns_help_lists_the_subcommand_and_flags() {
    let h = turns_home();
    let top = h.run(&["--help"]);
    assert!(
        top.stdout.contains("verbatim"),
        "top help lists the verbatim command: {}",
        top.stdout
    );
    let sub = h.run(&["verbatim", "--help"]);
    assert!(sub.stdout.contains("--budget"), "{}", sub.stdout);
    assert!(
        sub.stdout.contains("--round-trip-fraction"),
        "{}",
        sub.stdout
    );
    assert!(sub.stdout.contains("--max-compactions"), "{}", sub.stdout);
    assert!(
        !sub.stdout.contains("--budget-unit"),
        "budget is chars-only now: {}",
        sub.stdout
    );
}

#[test]
fn turns_command_renamed_to_verbatim() {
    let h = populated_home();
    let t = at(SESS);
    // Zero-BC: the old `turns` verb is GONE - it hits the wall (unknown subcommand), which
    // sends a stale model back to re-read SKILL rather than silently mis-running.
    let old = h.run(&["turns", t.as_str()]);
    assert!(!old.success, "the old `turns` command must never run");
    // v0.6.4: the wall is still a wall, but a POINTED one - the hidden tombstone names the
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
    // output flood by construction - a target is REQUIRED (the `show` precedent).
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
fn turns_old_subcommand_name_gets_the_rename_error() {
    // R8: the v0.5 `turns`→`verbatim` rename used to surface as clap's teach-nothing
    // "unrecognized subcommand" - the one error below the tool's water line. The hidden
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
fn turns_slice_rejects_out_json_and_zero() {
    // --slice writes the selected chunk to stdout and is verbatim-text only, so it refuses
    // --out, --format json, and the 1-based 0 index - each with a pointed error.
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
fn turns_slices_requires_a_slice_index() {
    // `--slices N` sets the fleet size; without `--slice i` there is no chunk to emit - a clear
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
fn turns_turn_range_alone_is_not_a_conflict() {
    // --turn WITHOUT --since/--until is valid (the L186 false arm: turn_range set
    // but since/until both None). Restrict to turns 0..2.
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--turn",
        "0..2",
        "--format",
        "json",
    ]);
    assert!(
        out.success,
        "a bare --turn must not conflict: {}",
        out.stderr
    );
    let objs = json_lines(&out.stdout);
    // No turn beyond index 2 selected.
    for o in objs.iter().filter(|o| o.get("role").is_some()) {
        assert!(o["turn_index"].as_u64().unwrap() <= 2, "turn cap: {o}");
    }
}

#[test]
fn turns_valid_round_trip_fraction_accepted() {
    // A fraction strictly inside (0,1) is accepted (the L189 false arm - valid input).
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--round-trip-fraction",
        "0.7",
    ]);
    assert!(out.success, "0.7 is a valid fraction: {}", out.stderr);
    assert!(
        out.stdout.contains("round-trip-fraction 0.70"),
        "{}",
        out.stdout
    );
}

#[test]
fn turns_nonzero_budget_accepted() {
    // A positive budget passes the L195 check (false arm).
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "1000",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("SESSION"), "{}", out.stdout);
}

#[test]
fn turns_agent_msg_surface_is_two_flags() {
    // The per-knob tuning flags are GONE (surface diet): `--agent-msgs` + `--profile`
    // are the whole agent-message policy surface.
    let h = turns_home();
    let help = h.run(&["verbatim", "--help"]);
    assert!(help.success);
    for flag in ["--agent-msgs", "--profile"] {
        assert!(
            help.stdout.contains(flag),
            "help must list {flag}: {}",
            help.stdout
        );
    }
    for gone in [
        "--agent-run-threshold",
        "--agent-rich-min-chars",
        "--agent-declaration-max-chars",
        "--keep-first",
        "--no-keep-first",
        "--budget-unit",
    ] {
        assert!(
            !help.stdout.contains(gone),
            "{gone} must be gone from help: {}",
            help.stdout
        );
        let run = h.run(&["verbatim", at(SESS).as_str(), gone]);
        assert!(!run.success, "{gone} must be an unknown argument now");
    }
    // Invalid enum values exit nonzero with a clap error.
    let bad_mode = h.run(&["verbatim", at(SESS).as_str(), "--agent-msgs", "bogus"]);
    assert!(!bad_mode.success, "invalid --agent-msgs must fail");
    let bad_profile = h.run(&["verbatim", at(SESS).as_str(), "--profile", "bogus"]);
    assert!(!bad_profile.success, "invalid --profile must fail");
}
