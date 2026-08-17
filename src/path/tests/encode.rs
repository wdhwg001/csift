//! CC-exact cwd encoding and encoded-token shapes across platforms.

use super::*;

// ── Forward encoding: a table of real (cwd, encoded) ground-truth pairs ──
// Every pair below is taken from an actual `~/.claude/projects` dir name.

#[test]
fn encode_real_ground_truth_table() {
    let table: &[(&str, &str)] = &[
        (
            "/Users/testuser/Projects/widget_app_prototype",
            "-Users-testuser-Projects-widget-app-prototype",
        ),
        (
            "/Users/testuser/Projects/Acme/widget_factory-worktrees/main",
            "-Users-testuser-Projects-Acme-widget-factory-worktrees-main",
        ),
        // The `/.cache` segment emits a literal `--` (proves no collapse).
        (
            "/Users/testuser/Projects/Acme/widget_factory/.cache-worktrees/sunny-meadow",
            "-Users-testuser-Projects-Acme-widget-factory--cache-worktrees-sunny-meadow",
        ),
        ("/a/.claude/b", "-a--claude-b"),
        // Case is preserved; digits pass through.
        (
            "/Users/testuser/Projects/Demo3",
            "-Users-testuser-Projects-Demo3",
        ),
    ];
    for (cwd, encoded) in table {
        assert_eq!(
            encode_cwd(Path::new(cwd)),
            *encoded,
            "encoding mismatch for {cwd}"
        );
    }
}

#[test]
fn encode_replaces_slash_and_underscore_with_dash() {
    assert_eq!(
        encode_cwd(Path::new("/Users/testuser/Projects/widget_app_prototype")),
        "-Users-testuser-Projects-widget-app-prototype"
    );
}

#[test]
fn encode_does_not_collapse_consecutive_dashes() {
    // A `/.claude/` segment yields a literal `--` (the two `/` and the `.`).
    assert_eq!(encode_cwd(Path::new("/a/.claude/b")), "-a--claude-b");
}

#[test]
fn encode_handles_worktree_path() {
    assert_eq!(
        encode_cwd(Path::new(
            "/Users/testuser/Projects/Acme/widget_factory-worktrees/main"
        )),
        "-Users-testuser-Projects-Acme-widget-factory-worktrees-main"
    );
}

#[test]
fn encode_preserves_case_and_digits() {
    assert_eq!(encode_cwd(Path::new("/Foo/Bar9/Baz")), "-Foo-Bar9-Baz");
}

#[test]
fn encode_space_and_dot_become_dash() {
    assert_eq!(encode_cwd(Path::new("/a b/c.d")), "-a-b-c-d");
}

// ── Encoded-token detection (§2.3 step 1) ──

#[test]
fn encoded_token_detection() {
    assert!(looks_like_encoded_token("-Users-testuser-Projects-foo"));
    assert!(looks_like_encoded_token("-a--claude-b"));
    assert!(looks_like_encoded_token("-")); // degenerate but well-formed
                                            // A real absolute path has slashes → not a bare token.
    assert!(!looks_like_encoded_token("/Users/testuser/Projects/foo"));
    // Must start with `-` (a real absolute cwd encodes to a leading `-`).
    assert!(!looks_like_encoded_token("Users-foo"));
    // No other punctuation survives in a real encoded name.
    assert!(!looks_like_encoded_token("-a_b"));
    assert!(!looks_like_encoded_token("-a/b"));
}

#[test]
fn encode_matches_cc_exactly_on_non_ascii_and_windows_paths() {
    // Extracted-law parity (CC 2.1.228 `gT`/`upo` + `.normalize("NFC")`): the
    // replacement runs per UTF-16 code unit on the NFC form.
    // NFC and NFD spellings of the same accented path encode IDENTICALLY.
    assert_eq!(encode_cwd(Path::new("/tmp/caf\u{e9}")), "-tmp-caf-");
    assert_eq!(encode_cwd(Path::new("/tmp/cafe\u{301}")), "-tmp-caf-");
    // An astral char is TWO surrogate units → TWO dashes (the JS regex's view).
    assert_eq!(encode_cwd(Path::new("/tmp/x\u{1D11E}y")), "-tmp-x--y");
    // A Windows cwd: the drive colon and each backslash are one dash → `C--…`.
    assert_eq!(
        encode_cwd(Path::new(r"C:\Users\dev\proj")),
        "C--Users-dev-proj"
    );
}

#[test]
fn encoded_token_shapes_unix_windows_unc() {
    assert!(looks_like_encoded_token("-Users-dev-example-project"));
    assert!(looks_like_encoded_token("C--Users-dev-proj")); // Windows drive shape
    assert!(looks_like_encoded_token("--server-share-proj")); // UNC (leads with --)
    assert!(!looks_like_encoded_token("Users-dev")); // no encoded lead-in
    assert!(!looks_like_encoded_token("C--Users/dev")); // a separator disqualifies
    assert!(!looks_like_encoded_token("C-Users-dev")); // one dash after the drive ≠ `C:\`
}

#[test]
fn looks_like_encoded_token_empty_is_false() {
    // An empty string has no leading `-` → false (the `chars.next()` None arm).
    assert!(!looks_like_encoded_token(""));
}
