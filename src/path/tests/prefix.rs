//! The `@<uuid-prefix>` rule: a token means its hex DIGITS, compared against every candidate
//! id with the dashes dropped on both sides, and it must name exactly one id.
//!
//! The candidates are real files in a scratch project directory, resolved through the same
//! scan and uniqueness check the resolver runs, because a dash-less token past the first
//! segment was accepted by the grammar and then matched nothing: a literal comparison held
//! `0a1b2c3d4` against `0a1b2c3d-4e5f-...`, which no uuid satisfies.

use super::*;

const A: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
/// Shares A's whole first segment and nothing after it.
const D: &str = "0a1b2c3d-5e5f-4a6b-8c7d-9e0f1a2b3c4d";
const C: &str = "7f001234-aaaa-4bbb-8ccc-dddddddddddd";
const ENC: &str = "-Users-dev-prefix";

/// One project directory holding the three sessions, each a one-record transcript.
fn sessions(tree: &Tree) -> Vec<ProjectDir> {
    for id in [A, D, C] {
        tree.write(
            &format!("{ENC}/{id}.jsonl"),
            "{\"type\":\"user\",\"uuid\":\"u1\",\"message\":{\"role\":\"user\",\"content\":\"x\"}}\n",
        );
    }
    vec![ProjectDir {
        dir: tree.path(ENC),
        target_cwd: None,
    }]
}

/// What ONE prefix token resolves to: the session basenames it kept, or the resolver's error.
fn resolve(dirs: &[ProjectDir], token: &str) -> std::result::Result<Vec<String>, String> {
    assert!(
        is_uuid_prefix(token),
        "`{token}` is not a prefix by the grammar"
    );
    let prefixes = vec![token.to_string()];
    let (top, hits, agent_hits) = scan_top_level(dirs, &[], &prefixes);
    let mut agents = Vec::new();
    resolve_prefix_uniqueness(&prefixes, &hits, &agent_hits, &mut agents)
        .map_err(|e| e.to_string())?;
    Ok(top
        .iter()
        .map(|p| crate::subagent::session_id_from_path(p))
        .collect())
}

#[test]
fn a_dashless_prefix_of_4_8_9_and_11_digits_resolves_its_one_session() {
    let tree = Tree::new();
    let dirs = sessions(&tree);
    assert_eq!(resolve(&dirs, "7f00").unwrap(), vec![C.to_string()], "4");
    assert_eq!(
        resolve(&dirs, "7f001234").unwrap(),
        vec![C.to_string()],
        "8"
    );
    // Past the first segment: the digits after the dash are what tell A from D.
    assert_eq!(
        resolve(&dirs, "0a1b2c3d4").unwrap(),
        vec![A.to_string()],
        "9"
    );
    assert_eq!(
        resolve(&dirs, "0a1b2c3d5").unwrap(),
        vec![D.to_string()],
        "9"
    );
    assert_eq!(
        resolve(&dirs, "0a1b2c3d4e5").unwrap(),
        vec![A.to_string()],
        "11"
    );
}

#[test]
fn a_prefix_two_sessions_share_is_an_error_naming_both() {
    let tree = Tree::new();
    let dirs = sessions(&tree);
    let err = resolve(&dirs, "0a1b2c3d").unwrap_err();
    assert!(err.contains("AMBIGUOUS"), "{err}");
    assert!(err.contains(A) && err.contains(D), "{err}");
    let err = resolve(&dirs, "0a1b").unwrap_err();
    assert!(err.contains("AMBIGUOUS"), "{err}");
    // A prefix no session carries is the honest empty, never a silent miss.
    let err = resolve(&dirs, "0a1b2c3d6").unwrap_err();
    assert!(err.contains("no session or agent id starts with"), "{err}");
}

/// A DASHED prefix is its dash-less equivalent: a literal prefix of the uuid layout, from the
/// first dash on, names exactly the sessions its digits name - including the short forms
/// below 12 characters, which the grammar refused.
#[test]
fn a_dashed_prefix_names_what_its_digits_name() {
    let tree = Tree::new();
    let dirs = sessions(&tree);
    for (dashed, dashless) in [
        ("0a1b2c3d-4", "0a1b2c3d4"),
        ("0a1b2c3d-4e5", "0a1b2c3d4e5"),
        ("0a1b2c3d-5e5f", "0a1b2c3d5e5f"),
    ] {
        let by_dashed = resolve(&dirs, dashed).unwrap();
        assert_eq!(by_dashed.len(), 1, "{dashed}");
        assert!(id_has_prefix(&by_dashed[0], dashless), "{dashed}");
    }
    // Ending ON the first dash names the first segment and nothing more.
    assert!(resolve(&dirs, "0a1b2c3d-")
        .unwrap_err()
        .contains("AMBIGUOUS"));
}

#[test]
fn the_grammar_admits_the_dashed_layout_only_at_its_template_positions() {
    assert!(is_uuid_prefix("1a2b3c4d-")); // 9: reaches the first dash
    assert!(is_uuid_prefix("1a2b3c4d-5")); // 10
    assert!(is_uuid_prefix("1a2b3c4d-5e")); // 11
    assert!(!is_uuid_prefix("1a2b-3c4d")); // a dash off the template
    assert!(!is_uuid_prefix("1a2b3c4-d")); // a dash one position early
    assert!(!is_uuid_prefix("1a2b3c4d5e6f")); // 12 dash-less is an agent id
    assert!(!is_uuid_prefix("1a2b3c4d5e6-")); // a dash where a digit belongs
}

#[test]
fn the_digit_comparison_drops_dashes_on_both_sides_and_nothing_else() {
    assert!(id_has_prefix(A, "0a1b2c3d4e5f"));
    assert!(id_has_prefix(A, "0a1b2c3d-4e5f"));
    assert!(id_has_prefix(A, A));
    assert!(!id_has_prefix(A, "0a1b2c3d5"));
    // Longer than the id itself: no id is its own prefix plus one more digit.
    assert!(!id_has_prefix("0a1b", "0a1b2"));
    // A bare-hex agent id has no dash, so the rule is the plain one there.
    assert!(id_has_prefix("abcdef0123456789", "abcdef01"));
    assert!(!id_has_prefix("abcdef0123456789", "abcdef02"));
    // Case is compared as written: the harness mints lowercase.
    assert!(!id_has_prefix(A, "0A1B"));
}
