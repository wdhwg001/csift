//! normalize_argv: flag reordering ahead of hyphen-led positionals, short/long/equals forms.

#[allow(unused_imports)]
use super::*;

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

#[test]
fn parse_search_short_t_after_positional_sets_category() {
    // End-to-end through clap: the hoisted `-t user` lands as a selector, positional intact.
    let cli =
        parse(&["csift", "search", "spec", ".", "-t", "user"]).expect("short flag after path");
    match cli.command {
        Command::Search(a) => {
            assert_eq!(a.labels, vec!["user".to_string()]);
            assert_eq!(a.paths.len(), 1);
            assert_eq!(a.pattern, "spec");
        }
        _ => panic!("expected search"),
    }
}

#[test]
fn parse_search_short_i_after_positional_sets_ignore_case() {
    let cli = parse(&["csift", "search", "SPEC", ".", "-i"]).expect("short -i after path");
    match cli.command {
        Command::Search(a) => {
            assert!(a.ignore_case);
            assert_eq!(a.paths.len(), 1);
        }
        _ => panic!("expected search"),
    }
}

#[test]
fn normalize_value_flag_before_double_dash_does_not_consume_terminator() {
    // `--since -- rest`: the `--` terminator must NOT be eaten as --since's value
    // (the `rest[i+1] != "--"` FALSE arm).
    let out = normalize_argv(
        ["csift", "search", "--since", "--", "-weird"]
            .map(String::from)
            .to_vec(),
    );
    // --since left unpaired; everything after `--` passes through verbatim.
    assert_eq!(out, vec!["csift", "search", "--since", "--", "-weird"]);
}
