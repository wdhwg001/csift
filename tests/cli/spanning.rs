//! Cross-command uniformity sweeps: span switches, scope banners, caps.

use crate::harness::*;

/// `--no-subagents` is the only span flag on the default-ON commands and suppresses the
/// fan-out the user asked to drop. The former no-op `--include-subagents` is GONE there, so the
/// only way to restrict span is `--no-subagents` — and it always restricts.
#[test]
fn no_subagents_restricts_span_end_to_end() {
    let h = populated_home();
    let span = |out: &Output| out.stdout.contains("sessions in scope");
    // `--no-subagents` suppresses the banner (top-level only) on every default-on command.
    assert!(!span(&h.run(&[
        "list",
        at(SESS).as_str(),
        "--no-subagents"
    ])));
    assert!(!span(&h.run(&[
        "files",
        at(SESS).as_str(),
        "--by",
        "file",
        "--no-subagents"
    ])));
    assert!(!span(&h.run(&[
        "search",
        "carry",
        at(SESS).as_str(),
        "--no-subagents"
    ])));
    // The removed `--include-subagents` is now an unknown argument on a default-on command.
    let gone = h.run(&["list", at(SESS).as_str(), "--include-subagents"]);
    assert!(
        !gone.success,
        "list --include-subagents must be rejected: {}",
        gone.stdout
    );
}

/// `--subagents-only` is GONE crate-wide (no user-facing flag, no hidden migration no-op). On
/// every span-aware subcommand it now falls through to the generic clap "unexpected argument"
/// rejection — the acceptable outcome once the pointed-migration machinery was removed.
#[test]
fn subagents_only_is_an_unknown_argument_everywhere() {
    let h = populated_home();
    for sub in ["verbatim", "recover", "list"] {
        let out = h.run(&[sub, at(SESS).as_str(), "--subagents-only"]);
        assert!(!out.success, "{sub} --subagents-only should fail");
        assert!(
            out.stderr.contains("unexpected argument"),
            "{sub}: expected the generic unknown-argument error, got: {}",
            out.stderr
        );
    }
    // search too (pattern positional first).
    let out = h.run(&["search", "x", at(SESS).as_str(), "--subagents-only"]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("unexpected argument"),
        "search: {}",
        out.stderr
    );
    // files itself rejects it as an unknown argument (the user-facing flag was removed earlier).
    let gone = h.run(&[
        "files",
        at(SESS).as_str(),
        "--subagents-only",
        "--by",
        "file",
    ]);
    assert!(
        !gone.success,
        "files --subagents-only must now be rejected: {}",
        gone.stdout
    );
    assert!(
        gone.stderr.contains("unexpected argument"),
        "files --subagents-only should be an unknown argument: {}",
        gone.stderr
    );
}

#[test]
fn list_and_stats_max_count_cap_and_report() {
    let h = populated_home(); // 1 top-level + 2 subagent = 3 rows
                              // list: cap to 2, drop 1 — reported in the JSON summary AND the text footer (never silent).
    let lj = h.run(&["list", "--max-count", "2", "--format", "json"]);
    assert!(lj.success, "stderr: {}", lj.stderr);
    assert_eq!(
        json_rows(&lj.stdout, "session").len(),
        2,
        "list capped to 2"
    );
    assert_eq!(
        json_summary(&lj.stdout)["dropped_by_cap"],
        serde_json::json!(1),
        "list drop reported"
    );
    let lt = h.run(&["list", "--max-count", "1"]);
    assert!(
        lt.stdout.contains("more session(s) not shown"),
        "list drop footer: {}",
        lt.stdout
    );
    assert!(
        lt.stdout.contains("--max-count"),
        "the guidance names the override"
    );
    // stats: cap to 2, drop 1.
    let sj = h.run(&["stats", "--max-count", "2", "--format", "json"]);
    assert!(sj.success, "stderr: {}", sj.stderr);
    assert_eq!(
        json_summary(&sj.stdout)["dropped_by_cap"],
        serde_json::json!(1),
        "stats drop reported: {}",
        sj.stdout
    );
}

/// The shared `scope  N sessions in scope (X top-level + Y subagent)` banner is now emitted
/// by EVERY subagent-spanning text surface (list/files/search/recover/turns), not just
/// list/turns. populated_home spans 2 subagents under 1 top-level session.
#[test]
fn scope_banner_uniform_across_spanning_subcommands() {
    let h = populated_home();
    let f = h.run(&["files", at(SESS).as_str(), "--by", "file"]);
    assert!(
        f.stdout.contains("sessions in scope"),
        "files banner:\n{}",
        f.stdout
    );
    let s = h.run(&["search", "carry", at(SESS).as_str()]);
    assert!(
        s.stdout.contains("sessions in scope"),
        "search banner:\n{}",
        s.stdout
    );
    let r = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--coverage",
        "--file",
        "/tmp/x",
    ]);
    assert!(
        r.stdout.contains("sessions in scope"),
        "recover banner:\n{}",
        r.stdout
    );
    let l = h.run(&["list", at(SESS).as_str()]);
    assert!(
        l.stdout.contains("sessions in scope"),
        "list banner:\n{}",
        l.stdout
    );
    // The banner is SUPPRESSED under --no-subagents (single top-level transcript).
    let f2 = h.run(&["files", at(SESS).as_str(), "--by", "file", "--no-subagents"]);
    assert!(
        !f2.stdout.contains("sessions in scope"),
        "files --no-subagents banner leaked:\n{}",
        f2.stdout
    );
}

/// The leading `{kind:"header", …}` JSON scope record is emitted by every spanning
/// subcommand's JSON, reusing turns' three span field names.
#[test]
fn scope_json_header_uniform_across_spanning_subcommands() {
    let h = populated_home();
    // Bind the `@<uuid>` target once so the vecs below can borrow it (a temporary `at(SESS)`
    // inside the array literal would be dropped before `h.run` borrows it).
    let at_sess = at(SESS);
    for args in [
        vec!["list", at_sess.as_str(), "--format", "json"],
        vec![
            "files",
            at_sess.as_str(),
            "--by",
            "file",
            "--format",
            "json",
        ],
        vec!["search", "carry", at_sess.as_str(), "--format", "json"],
        vec![
            "recover",
            at_sess.as_str(),
            "--coverage",
            "--file",
            "/tmp/x",
            "--format",
            "json",
        ],
    ] {
        let out = h.run(&args);
        assert!(out.success, "{:?} stderr: {}", args, out.stderr);
        let first = out.stdout.lines().find(|l| !l.trim().is_empty()).unwrap();
        let v: serde_json::Value = serde_json::from_str(first).unwrap();
        assert_eq!(
            v.get("kind").and_then(|k| k.as_str()),
            Some("header"),
            "{:?} first JSON line is not a session_header:\n{}",
            args,
            out.stdout
        );
        assert!(
            v.get("sessions_in_scope").is_some(),
            "{:?} header span",
            args
        );
        assert!(
            v.get("top_level_sessions").is_some(),
            "{:?} header span",
            args
        );
        assert!(
            v.get("subagent_sessions").is_some(),
            "{:?} header span",
            args
        );
    }
}
