//! parse_project_target and positional routing: encoded tokens, real paths, @-ids, flag typos.

#[allow(unused_imports)]
use super::*;

#[test]
fn bare_encoded_token_still_accepted_as_target() {
    // The leading-`-` tolerance must remain for genuine encoded tokens.
    let cli = parse(&["csift", "list", "-a--claude-b"]).expect("encoded token parses");
    match cli.command {
        Command::List(a) => assert_eq!(a.paths[0].to_string_lossy(), "-a--claude-b"),
        _ => panic!("expected list"),
    }
}

#[test]
fn real_path_target_still_accepted() {
    let cli = parse(&["csift", "list", "/Users/testuser/Projects/foo"]).expect("real path parses");
    match cli.command {
        Command::List(a) => {
            assert_eq!(a.paths[0].to_string_lossy(), "/Users/testuser/Projects/foo")
        }
        _ => panic!("expected list"),
    }
}

// ── Value parser accepts real targets, REJECTS a `--`-leading unknown flag ──

#[test]
fn parse_project_target_accepts_real_targets_rejects_double_dash() {
    // Genuine targets (single-dash encoded token, real path, `.`) parse.
    assert!(parse_project_target("-Users-testuser-Projects-foo").is_ok());
    assert!(parse_project_target("/Users/testuser/Projects/foo").is_ok());
    assert!(parse_project_target(".").is_ok());
    assert!(parse_project_target("-a--claude-b").is_ok());
    assert!(parse_project_target("C--Users-dev-proj").is_ok()); // Windows drive shape
                                                                // A bare `--`-leading token is rejected as a probable mistyped flag → clap reports
                                                                // "unexpected argument" instead of the misleading "no project dir named --xxx"
                                                                // (the one real `--`-led target, a UNC-encoded dir, goes through the `@` form -
                                                                // the error says so). A single `-` token still parses (encoded target).
    let err = parse_project_target("--by-fil").unwrap_err();
    assert!(err.contains("unexpected argument"), "got: {err}");
    assert!(err.contains("'@--by-fil'"), "routes the UNC escape: {err}");
    assert!(parse_project_target("-singledash").is_ok());
}

#[test]
fn unknown_flag_is_rejected_not_swallowed_as_target() {
    // The unknown-flag-masking fix: a `--`-prefixed unknown/typo token reaching the
    // positional is rejected so clap surfaces it, on EVERY scope-operating subcommand.
    for argv in [
        ["csift", "files", "--by-fil"].as_slice(),
        ["csift", "verbatim", "--budgett"].as_slice(),
        ["csift", "list", "--by-fil"].as_slice(),
        ["csift", "agents", "--bogus"].as_slice(),
    ] {
        let err = parse(argv).expect_err("unknown flag must error");
        let msg = err.to_string();
        assert!(
            !msg.contains("no Claude Code project dir named"),
            "must not mask as project-dir error for {argv:?}: {msg}"
        );
    }
}

#[test]
fn list_routes_at_uuid_and_bare_uuid_as_positional() {
    // A session is an `@<uuid>` POSITIONAL token, which the parser lands in `paths`
    // (the resolver does the routing, not the parser).
    let at = format!("@{SESS_UUID}");
    let cli = parse(&["csift", "list", at.as_str()]).unwrap();
    match cli.command {
        Command::List(a) => {
            assert_eq!(a.paths.len(), 1, "the @<uuid> token is a positional target");
            assert_eq!(a.paths[0].to_string_lossy(), at);
        }
        _ => panic!("expected list"),
    }
    // A BARE uuid (no `@`) is NOT special - it is just a positional (the
    // resolver later fails it as "no project dir named <uuid>", by design).
    let cli = parse(&["csift", "list", SESS_UUID]).unwrap();
    match cli.command {
        Command::List(a) => {
            assert_eq!(a.paths.len(), 1, "the bare uuid is a positional target");
            assert_eq!(a.paths[0].to_string_lossy(), SESS_UUID);
        }
        _ => panic!("expected list"),
    }
}

#[test]
fn removed_session_flag_no_longer_parses() {
    // The old session-pin flag was hard-removed; clap must reject it as an unknown argument
    // (never silently accept it) on a session-operating subcommand. The flag spelling is
    // assembled at runtime (not a source literal) so it stays a NEGATIVE regression guard,
    // not a lingering reference to the dead flag.
    let dead_flag = concat!("--", "session");
    assert!(
        parse(&["csift", "list", dead_flag, SESS_UUID]).is_err(),
        "a bare --session token must not parse"
    );
}
