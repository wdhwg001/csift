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

// ── Bare-uuid positional routing (so `csift files <uuid>` works as documented) ──

#[test]
fn is_uuid_recognizes_canonical_form() {
    assert!(is_uuid("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"));
    assert!(is_uuid("00000000-0000-0000-0000-000000000000"));
    // Wrong group lengths, non-hex, missing dashes, real paths → not a uuid.
    assert!(!is_uuid("0a1b2c3d-4e5f-4a6b-8c7d"));
    assert!(!is_uuid("zzzzzzzz-4e5f-4a6b-8c7d-9e0f1a2b3c4d"));
    assert!(!is_uuid("-Users-testuser-Projects-foo"));
    assert!(!is_uuid("/Users/testuser/Projects/foo"));
    assert!(!is_uuid("."));
}

#[test]
fn is_bare_subagent_hex_recognizes_agent_ids() {
    assert!(is_bare_subagent_hex("ae24045bd6d4bdaff"));
    assert!(is_bare_subagent_hex("a585e25a580c59e7a"));
    // Too short, dashed (uuid), or a word → not a bare subagent hex.
    assert!(!is_bare_subagent_hex("abc123")); // < 12
    assert!(!is_bare_subagent_hex(
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
    ));
    assert!(!is_bare_subagent_hex("plain-token"));
}

#[test]
fn is_teammate_agent_id_recognizes_name_embedded_ids() {
    // The new `in_process_teammate` shape: a<Name>-<hex>. These are the canonical ids
    // `csift agents` prints for a teammate, and must round-trip as `@<id>` targets.
    assert!(is_teammate_agent_id("aVSRepro-68a2a1661c9390c1"));
    assert!(is_teammate_agent_id("aVSSpeedField-d5dab904cc98a239"));
    assert!(is_teammate_agent_id("aVSMultiRegion-06fb13dd400b53a5"));
    // A teammate NAME may itself carry dashes (real data: teammate "P1-engine") — the
    // head is dash-tolerant so the id `csift agents` prints still round-trips.
    assert!(is_teammate_agent_id("aP1-engine-9cf2f06d6235ca64"));
    // A bare hex (built-in/workflow) has no dash → NOT teammate-shaped (it routes via
    // is_bare_subagent_hex instead).
    assert!(!is_teammate_agent_id("ae24045bd6d4bdaff"));
    // A uuid is rejected: the explicit is_uuid guard (an `a`-led uuid would otherwise
    // pass the dash-tolerant head with its exactly-12-hex final segment).
    assert!(!is_teammate_agent_id(
        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
    ));
    assert!(!is_teammate_agent_id(
        "a93b39f8-1681-4535-88eb-5b8ecce0abcd"
    ));
    // An encoded project dir starts with `-` (leading-slash sanitisation) → head not `a`.
    assert!(!is_teammate_agent_id("-Users-testuser-Projects-foo"));
    // Hex tail too short, or a non-hex tail → rejected.
    assert!(!is_teammate_agent_id("aVSRepro-68a2a1")); // tail < 12
    assert!(!is_teammate_agent_id("aVSRepro-zzzzzzzzzzzz")); // non-hex tail
}

#[test]
fn is_subagent_id_accepts_both_bare_hex_and_teammate() {
    // The unified gate the @-grammar routes through.
    assert!(is_subagent_id("ae24045bd6d4bdaff")); // built-in/workflow bare hex
    assert!(is_subagent_id("aVSRepro-68a2a1661c9390c1")); // teammate
    assert!(is_subagent_id("aP1-engine-9cf2f06d6235ca64")); // teammate with dashed name
    assert!(!is_subagent_id("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d")); // uuid
    assert!(!is_subagent_id("a93b39f8-1681-4535-88eb-5b8ecce0abcd")); // a-led uuid
    assert!(!is_subagent_id("-Users-testuser-Projects-foo")); // encoded dir
    assert!(!is_subagent_id("abc123")); // too short
}

#[test]
fn pins_single_session_covers_at_tokens_and_jsonl() {
    assert!(pins_single_session("@main"));
    assert!(pins_single_session("@trap:CrimsonWillowFen5180"));
    assert!(pins_single_session("@0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"));
    assert!(pins_single_session("@ae24045bd6d4bdaff"));
    assert!(pins_single_session("@aVSRepro-68a2a1661c9390c1")); // teammate id pins one
    assert!(pins_single_session("@aP1-engine-9cf2f06d6235ca64")); // dashed-name teammate
    assert!(pins_single_session("@13d9645a")); // uuid-prefix
    assert!(pins_single_session("/a/b/0a1b2c3d.jsonl"));
    // A bare uuid (no `@`), an encoded token, a plain path, `.` → NOT a session pin.
    assert!(!pins_single_session("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"));
    assert!(!pins_single_session("-Users-testuser-Projects-foo"));
    assert!(!pins_single_session("."));
}

#[test]
fn is_uuid_prefix_covers_first_segment_not_full_or_agent() {
    assert!(is_uuid_prefix("13d9")); // 4 hex (minimum)
    assert!(is_uuid_prefix("13d9645a")); // 8 hex (the uuid first segment)
    assert!(is_uuid_prefix("13d9645a3a5")); // 11 hex (max dash-less)
                                            // Too short (<4), dash-less ≥12 (agent-hex territory), non-hex, off-template dash → NOT a prefix.
    assert!(!is_uuid_prefix("13d")); // 3
    assert!(!is_uuid_prefix("13d9645a3a5b")); // 12 dash-less → agent hex
    assert!(!is_uuid_prefix("13d9645g")); // non-hex g
    assert!(!is_uuid_prefix("13d9-645a")); // dash off the 8-4-4-4-12 template
                                           // LITERAL layout prefixes (collision-lengthened header tokens) ARE prefixes.
    assert!(is_uuid_prefix("13d9645a-3a5")); // 12 chars, dash at template position 8
    assert!(is_uuid_prefix("13d9645a-3a5b-4a92")); // deeper into the layout
    assert!(is_uuid_prefix("13d9645a-3a5b-4a92-b83d-e0f94c5a9b9")); // 35 (max — one short of full)
    assert!(!is_uuid_prefix("13d9645a-3a5b-4a92-b83d-e0f94c5a9b90")); // 36 = a FULL uuid, not a prefix
    assert!(!is_uuid_prefix("13d9645a-3a5g")); // non-hex inside the layout
}

#[test]
fn trivial_digit_runs_pinned() {
    // Mutation pin: constant-step runs in -2..=2 are trivial; anything else is not —
    // including runs where only the FIRST step matches (the conjunction must hold
    // across all three steps).
    for t in ["0000", "1234", "9876", "1357", "2468", "4321"] {
        assert!(is_trivial_4_digits(t), "{t} is a trivial run");
    }
    for ok in ["4283", "1233", "1122", "5180", "7391"] {
        assert!(!is_trivial_4_digits(ok), "{ok} is not a trivial run");
    }
}
