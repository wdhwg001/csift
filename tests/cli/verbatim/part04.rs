use crate::harness::*;

#[test]
fn turns_eot_only_escape_is_byte_identical_to_pre_feature_baseline() {
    // The `eot-only` ESCAPE reproduces the pre-feature single-EOT document byte-for-byte
    // (the "force last-only" guarantee), asserted TWO ways:
    //   (1) `--agent-msgs eot-only` is byte-identical to a CAPTURED pre-feature baseline —
    //       catches a drift in the last-only path even if the default moved with it;
    //   (2) the IMPLICIT default now DIFFERS — it restores intermediate substance (the
    //       longest + rich members) the old single-EOT default silently dropped.
    // `TZ=UTC` pins the system-local timestamp render so the captured baseline is portable.
    let h = turns_home();
    let tz_utc = [("TZ", "UTC")];
    let eot_only = h.run_with_env(
        &[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            "40000",
            "--agent-msgs",
            "eot-only",
        ],
        &tz_utc,
    );
    assert!(eot_only.success, "stderr: {}", eot_only.stderr);
    assert_eq!(
        eot_only.stdout, TURNS_PRE_FEATURE_BASELINE,
        "`--agent-msgs eot-only` must be byte-identical to the captured pre-feature \
         (single-EOT) baseline; an INTENDED eot-only-output change requires re-capturing \
         tests/turns_pre_feature_baseline.txt under TZ=UTC --agent-msgs eot-only"
    );

    // The implicit default (Longest) is DIFFERENT — it restores the substance the
    // single-EOT default dropped (proving the default changed, not just a flag alias).
    let implicit = h.run_with_env(
        &[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            "40000",
        ],
        &tz_utc,
    );
    assert!(implicit.success, "stderr: {}", implicit.stderr);
    assert_ne!(
        implicit.stdout, eot_only.stdout,
        "the implicit default must NO LONGER equal eot-only — it keeps the longest + \
         rich members the single-EOT default silently dropped"
    );
}

#[test]
fn turns_profile_heavy_keeps_at_least_as_many_as_light() {
    // heavy (lower thresholds) selects >= as many KEPT agent messages as light, and both
    // are bounded by `all` and floored by `eot-only`.
    let h = turns_home();
    let at_sess = at(SESS);
    let kept_agents = |args: &[&str]| -> usize {
        let mut full = vec![
            "verbatim",
            at_sess.as_str(),
            "--no-subagents",
            "--budget",
            "40000",
            "--format",
            "json",
        ];
        full.extend_from_slice(args);
        let out = h.run(&full);
        assert!(out.success, "stderr: {}", out.stderr);
        json_lines(&out.stdout)
            .iter()
            .filter(|o| o["role"] == "assistant")
            .count()
    };
    let eot = kept_agents(&["--agent-msgs", "eot-only"]);
    let light = kept_agents(&["--profile", "light"]);
    let heavy = kept_agents(&["--profile", "heavy"]);
    let all = kept_agents(&["--agent-msgs", "all"]);
    assert!(heavy >= light, "heavy {heavy} >= light {light}");
    assert!(light >= eot, "light {light} >= eot-only {eot}");
    assert!(all >= heavy, "all {all} >= heavy {heavy}");
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

#[test]
fn turns_reconstructs_auq_exchange_and_plan_rejection_with_pointer() {
    let h = holes_home();
    let out = h.run(&["verbatim", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The AUQ exchange is reconstructed as a complete unit: marker + question + options
    // + the answer prose.
    assert!(
        out.stdout.contains("AskUserQuestion"),
        "AUQ unit label missing:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("option A (recommended)"),
        "AUQ options missing:\n{}",
        out.stdout
    );
    // Each option's DESCRIPTION (supplementary note) must survive — not just the label.
    assert!(
        out.stdout
            .contains("the conservative path that reuses existing state"),
        "AUQ option description missing:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("the full path that rebuilds from scratch"),
        "second AUQ option description missing:\n{}",
        out.stdout
    );
    // Free-text notes the user attached to the answer must surface verbatim.
    assert!(
        out.stdout
            .contains("it is more involved than a quick tweak"),
        "AUQ answer notes missing:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("option A is fine, the scope is broader than stated"),
        "AUQ answer missing:\n{}",
        out.stdout
    );
    // The plan rejection surfaces the user's typed instruction AND a pointer to the
    // plan file.
    assert!(
        out.stdout
            .contains("please run the smoke tests once before calling it done"),
        "plan-rejection user message missing:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("[plan: /Users/testuser/.claude/plans/elegant-scribbling-dream.md]"),
        "plan pointer missing:\n{}",
        out.stdout
    );
}
