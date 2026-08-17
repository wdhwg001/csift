//! search filters: labels, windows, signed max-count, additional-context.

use crate::harness::*;

#[test]
fn legacy_flat_selector_error_names_the_successor() {
    let h = populated_home();
    for (legacy, successor) in [
        ("thinking", "agent.thinking"),
        ("tool", "agent.tool"),
        ("tool-response", "agent.tool.result"),
    ] {
        let out = h.run(&["search", "x", &at(SESS), "-t", legacy]);
        assert!(!out.success, "legacy selector must still hard-error");
        assert!(
            out.stderr.contains("pre-v0.5") && out.stderr.contains(successor),
            "'{legacy}' should point at '{successor}': {}",
            out.stderr
        );
    }
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
fn search_signed_max_count_selects_the_window_ends() {
    // `--max-count N` keeps the EARLIEST N of the chronological stream, `-N` the LATEST N,
    // `0` stays uncapped; the kept exchanges always emit oldest-first among themselves, and
    // both ends disclose the window (banner: showing earliest/latest; footer: later/earlier
    // dropped).
    let h = Home::new();
    let _ = header_collision_scenario(&h); // COLLIDEONE (05h) < COLLIDETWO (06h) < SOLOWORD (07h)

    let first = h.run(&["search", "SEEDWORD", "--max-count", "1"]);
    assert!(first.success, "stderr: {}", first.stderr);
    assert!(
        first.stdout.contains("COLLIDEONE")
            && !first.stdout.contains("COLLIDETWO")
            && !first.stdout.contains("SOLOWORD"),
        "--max-count 1 keeps the chronologically EARLIEST exchange: {}",
        first.stdout
    );
    assert!(
        first.stdout.contains("showing earliest 1")
            && first.stdout.contains("2 later dropped by --max-count"),
        "disclosures at both ends: {}",
        first.stdout
    );

    let last = h.run(&["search", "SEEDWORD", "--max-count", "-1"]);
    assert!(last.success, "stderr: {}", last.stderr);
    assert!(
        last.stdout.contains("SOLOWORD")
            && !last.stdout.contains("COLLIDEONE")
            && !last.stdout.contains("COLLIDETWO"),
        "--max-count -1 keeps the chronologically LATEST exchange: {}",
        last.stdout
    );
    assert!(
        last.stdout.contains("showing latest 1")
            && last.stdout.contains("2 earlier dropped by --max-count"),
        "disclosures at both ends: {}",
        last.stdout
    );

    // A latest-N window still emits oldest-first among the kept exchanges.
    let two = h.run(&["search", "SEEDWORD", "--max-count", "-2"]);
    assert!(two.success, "stderr: {}", two.stderr);
    assert!(
        !two.stdout.contains("COLLIDEONE"),
        "the earliest exchange is outside the latest-2 window: {}",
        two.stdout
    );
    let pos2 = two.stdout.find("COLLIDETWO").expect("second kept");
    let pos3 = two.stdout.find("SOLOWORD").expect("third kept");
    assert!(
        pos2 < pos3,
        "kept exchanges emit oldest-first among themselves: {}",
        two.stdout
    );

    // `0` = uncapped (the crate-wide convention), no window note.
    let all = h.run(&["search", "SEEDWORD", "--max-count", "0"]);
    assert!(all.success, "stderr: {}", all.stderr);
    assert!(
        all.stdout.contains("COLLIDEONE")
            && all.stdout.contains("COLLIDETWO")
            && all.stdout.contains("SOLOWORD"),
        "--max-count 0 is uncapped: {}",
        all.stdout
    );
    assert!(
        !all.stdout.contains("showing "),
        "no window note when uncapped: {}",
        all.stdout
    );
}

#[test]
fn search_session_filter_and_turn_range() {
    let h = populated_home();
    // --session selects the parent; --turn picks turn 1 only.
    let out = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "--turn",
        "1..1",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("·t1"), "turn 1 header: {}", out.stdout);
    assert!(
        !out.stdout.contains("·t0"),
        "turn 0 excluded: {}",
        out.stdout
    );
}

#[test]
fn search_turn_range_intersects_with_time_window() {
    // --turn ∧ --since/--until INTERSECT (both filters AND) — the old
    // mutual-exclusion interface law is gone. An impossible intersection (turns exist,
    // but none inside the window) is an honest empty result, exit 0.
    let h = populated_home();
    let ok = h.run(&[
        "search",
        "carry",
        &at(SESS),
        "--turn",
        "0..1",
        "--until",
        "2027-01-01",
    ]);
    assert!(ok.success, "stderr: {}", ok.stderr);
    assert!(
        ok.stdout.contains("carry"),
        "in-range ∧ in-window matches: {}",
        ok.stdout
    );
    let none = h.run(&[
        "search",
        "carry",
        &at(SESS),
        "--turn",
        "0..1",
        "--until",
        "2020-01-01",
    ]);
    assert!(none.success, "an empty intersection is not an error");
    assert!(
        none.stdout.contains("no matching exchanges"),
        "window excludes everything: {}",
        none.stdout
    );
}

#[test]
fn search_since_until_window() {
    let h = populated_home();
    // A window that starts at 06:00 drops turn 0 (05:00) and keeps turn 1.
    let out = h.run(&[
        "search",
        "",
        "--since",
        "2026-06-07T06:00:00Z",
        "--no-subagents",
        at(SESS).as_str(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("·t1"),
        "turn 1 surfaced: {}",
        out.stdout
    );
}

#[test]
fn search_rejects_old_flat_category_selector() {
    // 0 back-compat (GOLD §6): the old flat `-t tool-response` is a HARD clap error that lists the
    // valid selectors; the dotted form works.
    let h = populated_home();
    let bad = h.run(&["search", "carry", "-t", "tool-response"]);
    assert!(!bad.success, "old flat -t must error; got:\n{}", bad.stdout);
    assert!(
        bad.stderr.contains("agent.tool.result"),
        "the error lists the valid selectors; stderr: {}",
        bad.stderr
    );
}

#[test]
fn search_global_max_count_caps_across_files() {
    // Two sessions each matching once; --max-count 1 emits one and drops one GLOBALLY
    // (the cross-file cap merge arm). Use --no-subagents to keep the count exact.
    let h = Home::new();
    for i in 0..2 {
        let sid = format!("ssss{i}ss-0000-0000-0000-00000000000{i}");
        h.write(
            &format!("{ENC}/{sid}.jsonl"),
            &format!(
                "{{\"type\":\"user\",\"uuid\":\"u{i}\",\"timestamp\":\"2026-06-0{}T05:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"global cap token zzcap\"}}}}\n",
                i + 1
            ),
        );
    }
    let out = h.run(&["search", "zzcap", "--max-count", "1", "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // TRUE total at both ends; the emitted window + global drop are disclosed.
    assert!(out.stdout.contains("matched 2"), "{}", out.stdout);
    assert!(out.stdout.contains("showing earliest 1"), "{}", out.stdout);
    assert!(out.stdout.contains("1 later dropped"), "{}", out.stdout);
    assert!(out.stdout.contains("by --max-count"));
}

#[test]
fn additional_context_is_invisible_by_default() {
    // The default scan never parses attachment lines: a pattern that lives only in the
    // hook-injected context is a DEFINITIVE absence (exit 0), not a hit.
    let h = Home::new();
    hook_context_scenario(&h);
    let out = h.run(&["search", "quartzlantern", &at(HOOKCTX_SESS)]);
    assert!(out.success, "zero-match exits 0: {}", out.stderr);
    assert!(
        out.stdout.contains("no matching exchanges"),
        "default scan must not see hook context:\n{}",
        out.stdout
    );
}

#[test]
fn additional_context_flag_surfaces_hook_attachment_under_meta_hook() {
    let h = Home::new();
    hook_context_scenario(&h);
    // First array element matches; the hit is labeled harness.meta.hook at its real line.
    let out = h.run(&[
        "search",
        "quartzlantern",
        &at(HOOKCTX_SESS),
        "--additional-context",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.meta.hook")
            && out.stdout.contains("L2")
            && out.stdout.contains("quartzlantern"),
        "flag surfaces the attachment under harness.meta.hook:\n{}",
        out.stdout
    );
    // Second array element is part of the SAME joined text (the `\n` join seam).
    let out2 = h.run(&[
        "search",
        "harborlight",
        &at(HOOKCTX_SESS),
        "--additional-context",
    ]);
    assert!(
        out2.stdout.contains("harness.meta.hook"),
        "every content element is searchable:\n{}",
        out2.stdout
    );
    // The label filter still governs: -t user can never surface it.
    let out3 = h.run(&[
        "search",
        "quartzlantern",
        &at(HOOKCTX_SESS),
        "--additional-context",
        "-t",
        "user",
    ]);
    assert!(
        out3.stdout.contains("no matching exchanges"),
        "-t user excludes meta.hook even with the flag:\n{}",
        out3.stdout
    );
    // JSON: the hit carries the leaf as `label`, and the summary reconciles.
    let outj = h.run(&[
        "search",
        "quartzlantern",
        &at(HOOKCTX_SESS),
        "--additional-context",
        "--format",
        "json",
    ]);
    assert!(
        outj.stdout.contains(r#""label":"harness.meta.hook""#),
        "JSON hit label:\n{}",
        outj.stdout
    );
}
