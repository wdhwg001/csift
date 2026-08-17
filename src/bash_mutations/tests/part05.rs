use super::*;

#[test]
fn test_double_bracket_comparison_is_not_a_redirect() {
    // `[[ a > b ]]` is a lexicographic comparison; the `>` is not a redirect and `y`
    // must not be fabricated. The masked test span emits nothing.
    assert!(
        just_paths("[[ x > y ]]").is_empty(),
        "[[ ]] comparison must not fabricate a file"
    );
    assert!(!just_paths("if [[ x > y ]]; then echo hi; fi").contains(&"y".to_string()));
}

// ── tar archive creation recall ──

#[test]
fn tar_create_emits_archive_dest() {
    // `-czf <archive>` (dashed bundle, spaced archive).
    assert_eq!(
        paths("tar -czf /tmp/arch.tar.gz src/"),
        vec![("/tmp/arch.tar.gz".to_string(), "tar")]
    );
    // `czf <archive>` (bundle without a leading dash).
    assert_eq!(
        paths("tar czf backup.tar.gz ."),
        vec![("backup.tar.gz".to_string(), "tar")]
    );
    // `-cf <archive>` (no compression flag).
    assert_eq!(
        paths("tar -cf out.tar a b c"),
        vec![("out.tar".to_string(), "tar")]
    );
    // Long-flag inline + spaced forms.
    assert_eq!(
        paths("tar --create --file=/tmp/x.tar dir/"),
        vec![("/tmp/x.tar".to_string(), "tar")]
    );
    assert_eq!(
        paths("tar --create --file /tmp/y.tar dir/"),
        vec![("/tmp/y.tar".to_string(), "tar")]
    );
    // A glued archive (`-czfARCHIVE`).
    assert_eq!(
        paths("tar -czf/tmp/glued.tgz src/"),
        vec![("/tmp/glued.tgz".to_string(), "tar")]
    );
}

#[test]
fn tar_extract_or_list_writes_nothing() {
    // No create flag → no archive is written, so nothing is emitted.
    assert!(just_paths("tar -xzf /tmp/arch.tar.gz").is_empty());
    assert!(just_paths("tar -tzf /tmp/arch.tar.gz").is_empty());
    assert!(just_paths("tar tf archive.tar").is_empty());
}

#[test]
fn tar_create_to_stdout_emits_nothing() {
    // A create bundle with NO `f` (archive → stdout, e.g. piped) writes no named file.
    assert!(just_paths("tar cz src/").is_empty());
    // `--create` long flag with no `--file` likewise has no destination to emit.
    assert!(just_paths("tar --create --gzip dir/").is_empty());
}

#[test]
fn tar_file_without_create_writes_nothing() {
    // `-f <archive>` but NO create flag (`-rf` append is not a create) → no emit, since
    // `has_create` is false.
    assert!(just_paths("tar -rf /tmp/x.tar extra").is_empty());
    // A spaced `--file <archive>` with no `--create` likewise emits nothing.
    assert!(just_paths("tar --list --file /tmp/x.tar").is_empty());
}
