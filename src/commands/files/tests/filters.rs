//! Path filtering: regex, glob, and their conjunction.

use super::*;
// ── scan_one_file branch coverage (mmap-None, skipped-line counting) ──

#[test]
fn path_filter_none_keeps_everything() {
    let f = PathFilter::from_args(None, None).unwrap();
    assert!(f.keeps("/anything/at/all.rs"));
    assert!(f.keeps(""));
}

#[test]
fn path_filter_regex_matches_anywhere_in_full_path() {
    let f = PathFilter::from_args(Some(r"\.rs$"), None).unwrap();
    assert!(f.keeps("/Users/x/src/lib.rs"));
    assert!(!f.keeps("/Users/x/docs/readme.md"));
    // "anywhere" semantics: a mid-path match is enough.
    let mid = PathFilter::from_args(Some("src"), None).unwrap();
    assert!(mid.keeps("/Users/x/src/lib.rs"));
    assert!(!mid.keeps("/Users/x/docs/readme.md"));
}

#[test]
fn path_filter_glob_crosses_slash_with_double_star() {
    let f = PathFilter::from_args(None, Some("**/src/**")).unwrap();
    assert!(f.keeps("/Users/x/src/lib.rs"));
    assert!(!f.keeps("/Users/x/docs/readme.md"));
    let md = PathFilter::from_args(None, Some("**/*.md")).unwrap();
    assert!(md.keeps("/Users/x/docs/readme.md"));
    assert!(!md.keeps("/Users/x/src/lib.rs"));
}

#[test]
fn path_filter_regex_and_glob_are_anded() {
    let f = PathFilter::from_args(Some(r"\.rs$"), Some("**/src/**")).unwrap();
    assert!(f.keeps("/Users/x/src/lib.rs")); // both match
    assert!(!f.keeps("/Users/x/src/readme.md")); // glob yes, regex no
    assert!(!f.keeps("/Users/x/other/lib.rs")); // regex yes, glob no
}

#[test]
fn path_filter_invalid_patterns_error() {
    assert!(PathFilter::from_args(Some("("), None).is_err());
    assert!(PathFilter::from_args(None, Some("[abc")).is_err());
}
