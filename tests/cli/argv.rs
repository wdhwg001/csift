//! Argv surface: flag ordering freedom, clean unknown-flag errors, exit conventions.

use crate::harness::*;

#[test]
fn pre_subcommand_global_flag_with_trailing_flags() {
    // REGRESSION (≤v0.4.1): normalize_argv assumed argv[1] was the subcommand, so a
    // GLOBAL flag placed BEFORE the subcommand disabled normalization entirely and the
    // allow_hyphen_values PATH positional swallowed every flag that followed a
    // positional - `csift --claude-home DIR list <ENC> --max-count 1` died with a
    // misleading "not a project target". The subcommand is now located by SCANNING over
    // declared root flags (+ their values), so "flag order is free" and "--claude-home
    // any position" hold in combination.
    let h = populated_home();
    let claude_home = h.projects().parent().unwrap().to_path_buf();
    let home_s = claude_home.to_str().unwrap().to_string();

    // (1) The dead quadrant: global flag BEFORE subcommand + value flag AFTER a positional.
    let out = h.run(&[
        "--claude-home",
        home_s.as_str(),
        "list",
        ENC,
        "--max-count",
        "1",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS), "row listed:\n{}", out.stdout);

    // (2) Same shape through `search` (PATTERN-first positional) with trailing --format json.
    let out = h.run(&[
        "--claude-home",
        home_s.as_str(),
        "search",
        "xyzzy-no-such-text",
        ENC,
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(r#""kind":"header""#),
        "envelope present even on zero matches:\n{}",
        out.stdout
    );

    // (3) The inline `--claude-home=DIR` form spans one token and is scanned over too.
    let eq = format!("--claude-home={home_s}");
    let out = h.run(&[eq.as_str(), "list", ENC, "--max-count", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS));

    // (4) Guard: an UNKNOWN pre-subcommand flag aborts the scan and reaches clap's
    // standard unexpected-argument error (argv passes through untouched).
    let out = h.run(&["--bogus", "list"]);
    assert!(!out.success);
    assert!(out.stderr.contains("--bogus"), "stderr: {}", out.stderr);
}

#[test]
fn unknown_flag_reports_clean_error_not_project_dir_error() {
    // A typo'd / unknown flag on any scope-operating subcommand must surface as an
    // "unexpected argument" message (NOT the misleading "no Claude Code project dir named
    // --xxx"). The `--`-leading value-parser reject makes this uniform tool-wide.
    let h = populated_home();
    let at_sess = at(SESS);
    for args in [
        vec!["files", at_sess.as_str(), "--by-fil"],
        vec!["verbatim", at_sess.as_str(), "--budgett", "5000"],
        vec!["recover", "--bogus-flag"],
        vec!["agents", ENC, "--bogus"],
        vec!["list", "--by-fil"],
    ] {
        let out = h.run(&args);
        assert!(!out.success, "a bad flag must exit nonzero: {args:?}");
        assert!(
            out.stderr.contains("unexpected argument"),
            "expected an 'unexpected argument' message for {args:?}; got: {}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("no Claude Code project dir named"),
            "the misleading project-dir error must NOT appear for {args:?}; got: {}",
            out.stderr
        );
    }
}

#[test]
fn help_exits_zero() {
    let h = Home::new();
    let out = h.run(&["--help"]);
    assert!(out.success);
    assert!(out.stdout.contains("ripgrep for Claude Code"));
}

#[test]
fn version_exits_zero() {
    let h = Home::new();
    let out = h.run(&["--version"]);
    assert!(out.success);
    assert!(out.stdout.contains("csift"));
}

#[test]
fn no_subcommand_errors() {
    let h = Home::new();
    let out = h.run(&[]);
    assert!(!out.success, "missing subcommand must exit nonzero");
}

#[test]
fn short_value_flag_pairs_beyond_position_zero() {
    // Mutation pin (normalize_argv reorder arithmetic): a short value flag at an ODD tail
    // index still consumes exactly its next token (i+2, never i*2) with flags around it.
    let h = Home::new();
    h.write(
        "-Users-testuser-Projects-argv/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jsonl",
        "{\"type\":\"user\",\"uuid\":\"u0\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"zzargv payload\"}}\n",
    );
    let o = h.run(&[
        "search",
        "zzargv",
        "-Users-testuser-Projects-argv",
        "--no-truncate",
        "--format",
        "json",
        "-t",
        "user",
    ]);
    assert!(o.success, "stderr: {}", o.stderr);
    assert!(
        o.stdout.contains("\"label\":\"user.message\""),
        "-t user filter survived the reorder:\n{}",
        o.stdout
    );
}

#[test]
fn dangling_short_value_flag_is_not_paired_with_the_next_flag() {
    // Mutation pin: a value-taking short flag followed by another DECLARED flag stays
    // UNPAIRED (clap reports the missing value), never silently swallows the next flag.
    let h = Home::new();
    let o = h.run(&["search", "zz", ".", "-t", "--format"]);
    assert!(!o.success);
    assert!(
        o.stderr.contains("a value is required") && o.stderr.contains("<SELECTOR>"),
        "clap names the missing SELECTOR value (the -t flag stayed unpaired): {}",
        o.stderr
    );
}
