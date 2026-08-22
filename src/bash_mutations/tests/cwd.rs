//! Shell-cwd tracking and operand resolution: checkpoint-at-segment semantics,
//! the cd state machine, both separator families, and the no-guess contract.

use super::*;

fn one(cmd: &str) -> BashMutation {
    let v = parse_bash_mutations(cmd);
    assert_eq!(v.len(), 1, "expected one mutation from {cmd:?}: {v:?}");
    v.into_iter().next().unwrap()
}

fn resolved(cmd: &str, cwd: &str) -> (String, Resolution) {
    one(cmd).resolve(Some(cwd))
}

#[test]
fn operand_before_any_cd_is_cwd_joined() {
    let (p, r) = resolved("sed -i '' 's/a/b/' notes.md", "/Users/testuser/proj");
    assert_eq!(p, "/Users/testuser/proj/notes.md");
    assert_eq!(r, Resolution::CwdJoined);
}

#[test]
fn absolute_operand_is_absolute_regardless_of_cd() {
    let (p, r) = resolved("cd /elsewhere && rm /tmp/x.log", "/Users/testuser/proj");
    assert_eq!(p, "/tmp/x.log");
    assert_eq!(r, Resolution::Absolute);
}

#[test]
fn operand_after_absolute_cd_is_cd_tracked() {
    let (p, r) = resolved("cd /work/app && touch out.txt", "/Users/testuser/proj");
    assert_eq!(p, "/work/app/out.txt");
    assert_eq!(r, Resolution::CdTracked);
}

#[test]
fn relative_cd_chain_joins_through_the_record_cwd() {
    let (p, r) = resolved("cd sub && cd deeper && touch x.md", "/base");
    assert_eq!(p, "/base/sub/deeper/x.md");
    assert_eq!(r, Resolution::CdTracked);
}

#[test]
fn dotdot_normalizes_lexically_and_clamps_at_root() {
    let (p, _) = resolved("cd a/../b && touch f", "/base");
    assert_eq!(p, "/base/b/f");
    let (p2, _) = resolved("cd ../../.. && touch f", "/one");
    assert_eq!(p2, "/f");
}

#[test]
fn cd_takes_effect_only_for_later_segments() {
    // The mutation sits BEFORE the cd, so it resolves at the spawn cwd.
    let v = parse_bash_mutations("touch early.txt && cd /work && touch late.txt");
    assert_eq!(v.len(), 2);
    let (pe, re_) = v[0].resolve(Some("/spawn"));
    assert_eq!(
        (pe.as_str(), re_),
        ("/spawn/early.txt", Resolution::CwdJoined)
    );
    let (pl, rl) = v[1].resolve(Some("/spawn"));
    assert_eq!((pl.as_str(), rl), ("/work/late.txt", Resolution::CdTracked));
}

#[test]
fn unknowable_cd_targets_leave_operands_verbatim_unresolved() {
    for cmd in [
        "cd \"$DIR\" && touch f.txt",
        "cd ~/somewhere && touch f.txt",
        "cd $(mktemp -d) && touch f.txt",
        "cd && touch f.txt",
        "popd && touch f.txt",
    ] {
        let (p, r) = resolved(cmd, "/base");
        assert_eq!(p, "f.txt", "verbatim for {cmd:?}");
        assert_eq!(r, Resolution::Unresolved, "unresolved for {cmd:?}");
    }
}

#[test]
fn cd_dash_swaps_back_to_the_previous_directory() {
    let (p, r) = resolved("cd /work && cd - && touch back.txt", "/spawn");
    // prev of the second cd is the spawn state, so the operand is cwd-joined data
    // reached through tracked cds: still the inference class.
    assert_eq!(p, "/spawn/back.txt");
    assert_eq!(r, Resolution::CdTracked);
}

#[test]
fn quoted_literal_cd_target_is_tracked() {
    let (p, r) = resolved("cd \"my dir\" && touch f", "/base");
    assert_eq!(p, "/base/my dir/f");
    assert_eq!(r, Resolution::CdTracked);
}

#[test]
fn cd_inside_a_subshell_does_not_leak_to_the_parent() {
    let v = parse_bash_mutations("(cd /tmp && touch in.txt); touch out.txt");
    // Both mutations exist; the parent-shell one must NOT inherit /tmp.
    let out = v
        .iter()
        .find(|m| m.path == "out.txt")
        .expect("parent-shell mutation");
    let (p, r) = out.resolve(Some("/spawn"));
    assert_eq!(p, "/spawn/out.txt");
    assert_eq!(r, Resolution::CwdJoined);
}

#[test]
fn cd_flags_are_skipped_to_the_real_target() {
    let (p, r) = resolved("cd -P /work && touch f", "/spawn");
    assert_eq!(p, "/work/f");
    assert_eq!(r, Resolution::CdTracked);
}

#[test]
fn windows_family_join_uses_backslashes() {
    let (p, r) = resolved("sed -i '' 's/a/b/' tests/x.rs", r"C:\Users\w\proj");
    assert_eq!(p, r"C:\Users\w\proj\tests\x.rs");
    assert_eq!(r, Resolution::CwdJoined);
}

#[test]
fn unc_join_keeps_the_server_share_root() {
    let (p, _) = resolved("touch a/../b.txt", r"\\server\share\dir");
    assert_eq!(p, r"\\server\share\dir\b.txt");
}

#[test]
fn missing_or_relative_record_cwd_stays_verbatim() {
    let m = one("touch f.txt");
    assert_eq!(
        m.resolve(None),
        ("f.txt".to_string(), Resolution::Unresolved)
    );
    // A non-absolute record cwd is unusable as a join base: same verbatim contract.
    assert_eq!(
        m.resolve(Some("relative/cwd")),
        ("f.txt".to_string(), Resolution::Unresolved)
    );
}

#[test]
fn class_markers_are_never_resolved() {
    let m = one("git checkout -- .");
    assert!(is_class_marker(&m.path));
    let (p, r) = m.resolve(Some("/base"));
    assert_eq!(p, "git:checkout");
    assert_eq!(r, Resolution::Unresolved);
}

#[test]
fn glob_operand_joins_like_any_relative() {
    let (p, r) = resolved("rm logs/*.tmp", "/base");
    assert_eq!(p, "/base/logs/*.tmp");
    assert_eq!(r, Resolution::CwdJoined);
}

#[test]
fn heredoc_body_cd_never_moves_the_tracker() {
    let cmd = "cat > run.sh <<'EOF'\ncd /inside/body\nEOF\ntouch after.txt";
    let v = parse_bash_mutations(cmd);
    let after = v.iter().find(|m| m.path == "after.txt").expect("after");
    assert_eq!(
        after.resolve(Some("/spawn")),
        ("/spawn/after.txt".to_string(), Resolution::CwdJoined)
    );
}

#[test]
fn resolution_wire_spellings_are_pinned() {
    assert_eq!(Resolution::Absolute.as_str(), "absolute");
    assert_eq!(Resolution::CwdJoined.as_str(), "cwd-joined");
    assert_eq!(Resolution::CdTracked.as_str(), "cd-tracked");
    assert_eq!(Resolution::Unresolved.as_str(), "unresolved");
}

#[test]
fn is_absolute_shell_path_covers_all_three_families() {
    assert!(is_absolute_shell_path("/a/b"));
    assert!(is_absolute_shell_path(r"C:\a"));
    assert!(is_absolute_shell_path("C:/a"));
    assert!(is_absolute_shell_path(r"\\srv\share"));
    assert!(!is_absolute_shell_path("rel/a"));
    assert!(!is_absolute_shell_path("~x"));
    assert!(!is_absolute_shell_path("C:relative"));
}

#[test]
fn rsync_remote_destination_is_never_a_local_path() {
    assert!(parse_bash_mutations("rsync -a ./src builder@relay:~/dir/").is_empty());
    assert!(parse_bash_mutations("rsync -a ./src relay:backups/").is_empty());
    let local = parse_bash_mutations("rsync -a ./src /srv/backups/");
    assert_eq!(local.len(), 1);
    assert_eq!(local[0].path, "/srv/backups/");
    assert!(is_remote_spec("host:path"));
    assert!(is_remote_spec("user@host:dir/x"));
    assert!(!is_remote_spec(r"C:\work\x"));
    assert!(!is_remote_spec("/plain/path"));
}

#[test]
fn git_dry_runs_emit_no_mutation_row() {
    assert!(parse_bash_mutations("git apply --check fix.patch").is_empty());
    assert!(parse_bash_mutations("git clean -n").is_empty());
    assert!(parse_bash_mutations("git clean --dry-run").is_empty());
    let real = parse_bash_mutations("git apply fix.patch");
    assert_eq!(real.len(), 1);
    assert_eq!(real[0].path, "git:apply");
}
