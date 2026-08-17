//! Data-root resolution and path normalization: claude-home precedence, absolutize.

use super::*;

#[test]
fn strip_prefix_recognizes_bare_token() {
    let root = Path::new("/home/u/.claude/projects");
    assert_eq!(
        strip_projects_root_prefix(Path::new("-Users-foo-bar"), root).as_deref(),
        Some("-Users-foo-bar")
    );
}

#[test]
fn strip_prefix_recognizes_under_root() {
    let root = Path::new("/home/u/.claude/projects");
    assert_eq!(
        strip_projects_root_prefix(Path::new("/home/u/.claude/projects/-Users-foo-bar"), root)
            .as_deref(),
        Some("-Users-foo-bar")
    );
}

#[test]
fn strip_prefix_rejects_real_path() {
    let root = Path::new("/home/u/.claude/projects");
    // A real cwd with slashes is NOT a bare token and is not under the root.
    assert!(strip_projects_root_prefix(Path::new("/Users/testuser/Projects/foo"), root).is_none());
    // Under-root but with an extra nested component (a session dir, not an
    // encoded project token) → not a single-component encoded token.
    assert!(
        strip_projects_root_prefix(Path::new("/home/u/.claude/projects/-Users-foo/sub"), root)
            .is_none()
    );
}

#[test]
fn lexical_normalize_resolves_dotdot() {
    assert_eq!(
        lexical_normalize(Path::new("/a/b/../c")),
        PathBuf::from("/a/c")
    );
    assert_eq!(
        lexical_normalize(Path::new("/a/./b")),
        PathBuf::from("/a/b")
    );
}

#[test]
fn projects_root_ends_with_claude_projects() {
    // We do NOT mutate $HOME here: env is process-global and cargo runs tests
    // as threads, so a set/restore would race sibling tests. Assert the shape
    // off the ambient $HOME instead.
    let root = projects_root().expect("projects_root");
    assert!(root.ends_with("projects"));
    assert!(root.to_string_lossy().contains(".claude"));
}

#[test]
fn resolve_claude_home_precedence_flag_then_env_then_home() {
    let home = Path::new("/home/u");
    // 1. The `--claude-home` flag wins over everything else.
    assert_eq!(
        resolve_claude_home(
            Some(Path::new("/flag/dir")),
            Some(OsStr::new("/env/dir")),
            home
        ),
        PathBuf::from("/flag/dir")
    );
    // 2. With no flag, $CLAUDE_CONFIG_DIR wins over the $HOME default.
    assert_eq!(
        resolve_claude_home(None, Some(OsStr::new("/env/dir")), home),
        PathBuf::from("/env/dir")
    );
    // 3. An EMPTY $CLAUDE_CONFIG_DIR is ignored → falls through to $HOME/.claude.
    assert_eq!(
        resolve_claude_home(None, Some(OsStr::new("")), home),
        PathBuf::from("/home/u/.claude")
    );
    // 4. Nothing set → $HOME/.claude (the historical default, unchanged).
    assert_eq!(
        resolve_claude_home(None, None, home),
        PathBuf::from("/home/u/.claude")
    );
}

// ── Branch-completeness for the pure helpers (env-touching arms are covered by
//    the tests/cli_integration.rs suite, which sets a child-scoped $HOME). ──

#[test]
fn absolutize_existing_path_canonicalizes() {
    // An existing dir → the `canonicalize` Ok arm.
    let tmp = std::env::temp_dir();
    let abs = absolutize(&tmp).expect("absolutize existing");
    assert!(abs.is_absolute());
}

#[test]
fn absolutize_nonexistent_absolute_path_normalizes_lexically() {
    // A non-existent ABSOLUTE path → canonicalize fails → the `p.is_absolute()`
    // true arm + lexical_normalize (resolving the `..`).
    let abs = absolutize(Path::new("/no/such/csift/a/../b")).expect("absolutize");
    assert_eq!(abs, PathBuf::from("/no/such/csift/b"));
}

#[test]
fn absolutize_nonexistent_relative_path_joins_cwd() {
    // A non-existent RELATIVE path → canonicalize fails → the `else` (join cwd)
    // arm. The result is absolute and ends with our unique segment.
    let rel = Path::new("csift-nonexistent-xyzzy-rel");
    let abs = absolutize(rel).expect("absolutize relative");
    assert!(abs.is_absolute());
    assert!(abs.ends_with("csift-nonexistent-xyzzy-rel"));
}

#[test]
fn lexical_normalize_handles_curdir_and_parentdir_and_plain() {
    assert_eq!(
        lexical_normalize(Path::new("/a/./b/../c")),
        PathBuf::from("/a/c")
    );
    // A leading `.` (CurDir) is dropped; a plain component is pushed.
    assert_eq!(lexical_normalize(Path::new("a/b")), PathBuf::from("a/b"));
}

#[test]
fn strip_prefix_under_root_with_token() {
    // The under-root branch where exactly one component remains AND it is a valid
    // encoded token.
    let root = Path::new("/home/u/.claude/projects");
    assert_eq!(
        strip_projects_root_prefix(Path::new("/home/u/.claude/projects/-Enc-Token"), root)
            .as_deref(),
        Some("-Enc-Token")
    );
    // Under root but the single component is NOT a valid encoded token (no leading
    // dash) → None (the `looks_like_encoded_token` false arm under-root).
    assert!(
        strip_projects_root_prefix(Path::new("/home/u/.claude/projects/plain"), root).is_none()
    );
}

#[test]
fn strip_prefix_under_root_empty_remainder_is_none() {
    // `target == root` exactly → zero components remain → not a single-token match.
    let root = Path::new("/home/u/.claude/projects");
    assert!(strip_projects_root_prefix(root, root).is_none());
}

#[test]
fn resolve_target_real_path_that_does_not_resolve_errors() {
    // A real absolute path whose encoded dir does not exist under the projects
    // root → the final `bail!` arm (step 4). Uses a path guaranteed absent.
    let err = resolve_target(Path::new(
        "/Users/csift-definitely-no-such-project-9999/zzz",
    ))
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("no Claude Code project dir"),
        "expected step-4 bail, got: {msg}"
    );
}

#[test]
fn resolve_target_encoded_token_not_a_dir_errors() {
    // A leading-`-` token that is NOT an existing dir under the root → the
    // token-branch bail (does not fall through to path-encoding).
    let err = resolve_target(Path::new("-csift-no-such-encoded-token-zzzz")).unwrap_err();
    assert!(
        err.to_string().contains("no Claude Code project dir named"),
        "expected token bail, got: {err}"
    );
}
