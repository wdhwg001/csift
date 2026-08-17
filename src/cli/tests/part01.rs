use crate::cli::*;

#[allow(unused_imports)]
use super::*;

#[test]
fn image_out_format_ext_and_media_type_pinned() {
    // Mutation pin: the transcode surface's extension/media-type mapping, per variant.
    use crate::cli::ImageOutFormat as F;
    for (f, ext, mt) in [
        (F::Png, "png", "image/png"),
        (F::Jpeg, "jpg", "image/jpeg"),
        (F::Gif, "gif", "image/gif"),
        (F::Webp, "webp", "image/webp"),
    ] {
        assert_eq!(f.ext(), ext);
        assert_eq!(f.media_type(), mt);
    }
}

use clap::CommandFactory;

/// clap's own internal consistency check — catches duplicate flags, bad
/// `overrides_with` targets, malformed value parsers at build time.
#[test]
fn command_definition_is_valid() {
    Cli::command().debug_assert();
}

#[test]
fn whoami_accepts_trap_and_main_self_target_else_none() {
    // `@trap:<marker>` and `@main` land in whoami's optional positional; bare whoami = None.
    let cli =
        parse(&["csift", "whoami", "@trap:CrimsonWillowFen5180"]).expect("whoami @trap parses");
    match cli.command {
        Command::Whoami(a) => {
            assert_eq!(a.self_target.as_deref(), Some("@trap:CrimsonWillowFen5180"));
        }
        _ => panic!("expected whoami"),
    }
    let cli = parse(&["csift", "whoami", "@main"]).expect("whoami @main parses");
    match cli.command {
        Command::Whoami(a) => assert_eq!(a.self_target.as_deref(), Some("@main")),
        _ => panic!("expected whoami"),
    }
    let cli = parse(&["csift", "whoami"]).expect("bare whoami parses");
    match cli.command {
        Command::Whoami(a) => assert!(a.self_target.is_none()),
        _ => panic!("expected whoami"),
    }
}

// ── The reported flag-ordering bug (CONFIRMED against target/debug/csift) ──

#[test]
fn list_format_json_after_encoded_positional_now_works() {
    // The exact repro: `--format json` AFTER a leading-`-` encoded positional
    // used to be swallowed as two extra PATH values. It must now parse as the
    // format flag, with the positional intact.
    let cli = parse(&[
        "csift",
        "list",
        "-Users-testuser-Projects-widget-app-prototype",
        "--format",
        "json",
    ])
    .expect("flag after encoded positional must parse");
    match cli.command {
        Command::List(a) => {
            assert_eq!(a.format, OutputFormat::Json);
            assert_eq!(a.paths.len(), 1);
            assert_eq!(
                a.paths[0].to_string_lossy(),
                "-Users-testuser-Projects-widget-app-prototype"
            );
        }
        _ => panic!("expected list"),
    }
}

#[test]
fn list_format_json_before_positional_still_works() {
    // The old workaround ordering must keep working (backward-compatible).
    let cli = parse(&[
        "csift",
        "list",
        "--format",
        "json",
        "-Users-testuser-Projects-foo",
    ])
    .expect("flag before positional still parses");
    match cli.command {
        Command::List(a) => {
            assert_eq!(a.format, OutputFormat::Json);
            assert_eq!(a.paths.len(), 1);
        }
        _ => panic!("expected list"),
    }
}

#[test]
fn search_flag_after_encoded_target_parses() {
    // A `--format json` flag is hoisted ahead of a leading-`-` encoded POSITIONAL target
    // in any position (the allow_hyphen_values greedy-absorb fix).
    let cli = parse(&[
        "csift",
        "search",
        "carry",
        "-Users-testuser-Projects-foo",
        "--format",
        "json",
    ])
    .expect("search <encoded> then --format must parse");
    match cli.command {
        Command::Search(a) => {
            assert_eq!(a.format, OutputFormat::Json);
            assert_eq!(a.paths.len(), 1, "the encoded target is a positional");
            assert_eq!(a.targets().len(), 1);
            assert_eq!(a.pattern, "carry");
        }
        _ => panic!("expected search"),
    }
}

#[test]
fn search_positional_path_like_siblings() {
    // The fix: `csift search PATTERN .` — a POSITIONAL path, the SAME surface every
    // sibling subcommand uses.
    let cli = parse(&["csift", "search", "carry", "."]).expect("positional PATH must parse");
    match cli.command {
        Command::Search(a) => {
            assert_eq!(a.pattern, "carry");
            assert_eq!(a.paths.len(), 1);
            assert_eq!(a.paths[0].to_string_lossy(), ".");
            assert_eq!(a.targets().len(), 1);
        }
        _ => panic!("expected search"),
    }
}

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
                                                                // (the one real `--`-led target, a UNC-encoded dir, goes through the `@` form —
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
    // A BARE uuid (no `@`) is NOT special — it is just a positional (the
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

// ── argv normalizer (the actual fix mechanism) ──

#[test]
fn normalize_moves_long_flag_with_value_ahead_of_positional() {
    let out = normalize_argv(
        ["csift", "list", "-Users-foo", "--format", "json"]
            .map(String::from)
            .to_vec(),
    );
    // program + subcommand stay first; the flag+value are pulled ahead of the
    // leading-`-` positional.
    assert_eq!(out, vec!["csift", "list", "--format", "json", "-Users-foo"]);
}

#[test]
fn normalize_keeps_boolean_flag_without_grabbing_a_value() {
    let out = normalize_argv(
        ["csift", "list", "-Users-foo", "--no-subagents"]
            .map(String::from)
            .to_vec(),
    );
    // `--no-subagents` is a SetTrue flag → it must NOT consume the positional.
    assert_eq!(out, vec!["csift", "list", "--no-subagents", "-Users-foo"]);
}

#[test]
fn normalize_passes_through_after_double_dash() {
    let out = normalize_argv(
        ["csift", "list", "--", "-weird-token", "--not-a-flag"]
            .map(String::from)
            .to_vec(),
    );
    // After `--`, tokens are verbatim positionals (clap's escape) — untouched.
    assert_eq!(
        out,
        vec!["csift", "list", "--", "-weird-token", "--not-a-flag"]
    );
}

#[test]
fn normalize_leaves_unknown_subcommand_untouched() {
    let out = normalize_argv(["csift", "--help"].map(String::from).to_vec());
    assert_eq!(out, vec!["csift", "--help"]);
}

#[test]
fn normalize_handles_equals_form() {
    let out = normalize_argv(
        ["csift", "list", "-Users-foo", "--format=json"]
            .map(String::from)
            .to_vec(),
    );
    assert_eq!(out, vec!["csift", "list", "--format=json", "-Users-foo"]);
}

#[test]
fn normalize_value_flag_at_end_has_no_following_value() {
    // A value-taking flag with NOTHING after it → the `i+1 < rest.len()` FALSE arm
    // (no value consumed). clap will later report the missing value; normalize
    // just leaves the flag in place.
    let out = normalize_argv(
        ["csift", "search", "x", "--since"]
            .map(String::from)
            .to_vec(),
    );
    assert_eq!(out, vec!["csift", "search", "--since", "x"]);
}

#[test]
fn normalize_value_flag_followed_by_another_flag_does_not_consume_it() {
    // `--since --format json`: `--since` is value-taking but the NEXT token is a
    // declared long flag → it must NOT be consumed as the value (the
    // `!all_long.contains(next)` FALSE arm). Both flags are emitted; clap reports
    // the missing `--since` value.
    let out = normalize_argv(
        ["csift", "search", "x", "--since", "--format", "json"]
            .map(String::from)
            .to_vec(),
    );
    // --since stays unpaired; --format json stay paired; positional `x` last.
    assert_eq!(
        out,
        vec!["csift", "search", "--since", "--format", "json", "x"]
    );
}

#[test]
fn normalize_hoists_short_value_flag_after_positional() {
    // The reported critical bug: `search PATTERN <path> -t user` let the
    // `allow_hyphen_values` positional swallow `-t` ("no project dir named -t").
    // The pre-pass must now hoist the declared short flag `-t` AND pair its value.
    let out = normalize_argv(
        ["csift", "search", "spec", ".", "-t", "user"]
            .map(String::from)
            .to_vec(),
    );
    assert_eq!(out, vec!["csift", "search", "-t", "user", "spec", "."]);
}

#[test]
fn normalize_hoists_short_bool_flag_after_positional() {
    // `-i` is a boolean short flag → hoisted but NOT paired with a value.
    let out = normalize_argv(
        ["csift", "search", "spec", ".", "-i"]
            .map(String::from)
            .to_vec(),
    );
    assert_eq!(out, vec!["csift", "search", "-i", "spec", "."]);
}

#[test]
fn normalize_hoists_bundled_short_flag_after_positional() {
    // A bundled `-tuser` carries its value inline (len != 2) → emitted as one token,
    // never pairing the next positional.
    let out = normalize_argv(
        ["csift", "search", "spec", ".", "-tuser"]
            .map(String::from)
            .to_vec(),
    );
    assert_eq!(out, vec!["csift", "search", "-tuser", "spec", "."]);
}

#[test]
fn normalize_short_value_flag_at_end_has_no_following_value() {
    // `-t` with nothing after it → the no-value arm (clap reports the missing value).
    let out = normalize_argv(
        ["csift", "search", "spec", ".", "-t"]
            .map(String::from)
            .to_vec(),
    );
    assert_eq!(out, vec!["csift", "search", "-t", "spec", "."]);
}

#[test]
fn normalize_short_value_flag_followed_by_flag_does_not_consume_it() {
    // `-t -i`: `-t` is value-taking but the next token starts with `-` → not consumed.
    let out = normalize_argv(
        ["csift", "search", "spec", ".", "-t", "-i"]
            .map(String::from)
            .to_vec(),
    );
    assert_eq!(out, vec!["csift", "search", "-t", "-i", "spec", "."]);
}

#[test]
fn normalize_leaves_encoded_token_with_letter_short_collision_as_positional() {
    // An encoded `-Users-…` token's first char `U` is NOT a declared short flag, so it
    // stays a positional even though `-t`/`-i` exist. The short-flag set is per-char.
    let out = normalize_argv(
        [
            "csift",
            "search",
            "spec",
            "-Users-testuser-Projects-foo",
            "-i",
        ]
        .map(String::from)
        .to_vec(),
    );
    assert_eq!(
        out,
        vec![
            "csift",
            "search",
            "-i",
            "spec",
            "-Users-testuser-Projects-foo"
        ]
    );
}
