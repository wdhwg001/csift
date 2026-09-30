//! The `--file` target: which record paths it names and the files it records as matched.

use super::*;

#[test]
fn path_matches_exact_and_basename_suffix() {
    assert!(names_file("/p/a.rs", "/p/a.rs"), "exact");
    assert!(names_file("a.rs", "/p/a.rs"), "basename suffix");
    assert!(names_file("p/a.rs", "/x/p/a.rs"), "multi-segment suffix");
    assert!(
        !names_file("b.rs", "/p/ab.rs"),
        "not a component-aligned suffix"
    );
    assert!(
        !path_matches(&Target::new(None), "/p/a.rs", None),
        "no target matches nothing"
    );
}

#[test]
fn a_target_records_every_file_it_matched_and_nothing_else() {
    let t = Target::new(Some("lib.rs"));
    assert!(path_matches(&t, "/p/a/lib.rs", None));
    assert!(
        path_matches(&t, "/p/a/lib.rs", Some("/elsewhere")),
        "a second match of one path, whatever cwd carried it"
    );
    assert!(path_matches(&t, r"C:\p\b\lib.rs", None));
    // A relative path is a candidate too, resolved against its record's cwd: here it is
    // the SAME file as the first absolute one, so it adds nothing.
    assert!(path_matches(&t, "./lib.rs", Some("/p/a")));
    // Resolved against another cwd it is another file.
    assert!(path_matches(&t, "src/lib.rs", Some("/p/c")));
    // With no cwd to resolve against it stays the raw string.
    assert!(path_matches(&t, "other/lib.rs", None));
    // A `$`/`~` operand's expansion is not on record: raw, never joined.
    assert!(path_matches(&t, "$D/lib.rs", Some("/p/a")));
    // The belt: an operand matched verbatim records the file its resolution names.
    assert!(path_matches_as(
        &t,
        "lib.rs",
        "/p/c/src/lib.rs",
        Some("/p/c/src")
    ));
    assert!(!path_matches(&t, "/p/a/mylib.rs", None));
    let got: Vec<String> = t.into_matched().into_iter().collect();
    assert_eq!(
        got,
        [
            "$D/lib.rs",
            "/p/a/lib.rs",
            "/p/c/src/lib.rs",
            r"C:\p\b\lib.rs",
            "other/lib.rs",
        ]
    );
    assert_eq!(candidate(r"x\lib.rs", Some(r"C:\w")), r"C:\w\x\lib.rs");
    // An absolute target names a relative record path through the path's own cwd - the
    // candidate a refusal prints is passed back and finds the same events.
    let abs = Target::new(Some("/p/c/src/lib.rs"));
    assert!(path_matches(&abs, "src/lib.rs", Some("/p/c")));
    assert!(!path_matches(&abs, "src/lib.rs", Some("/p/d")));
    assert!(!path_matches(&abs, "src/lib.rs", None));
    // An empty path names nothing, even where its resolution would be a cwd the target ends.
    assert!(!path_matches(
        &Target::new(Some("w_app")),
        "",
        Some("/p/w_app")
    ));
    assert_eq!(candidate("../lib.rs", Some("/p/a/b")), "/p/a/lib.rs");
    for full in ["/x", r"\\srv\x", "C:/x", r"c:\x"] {
        assert!(is_full_path(full), "{full}");
    }
    for not_full in ["x/y", "C:", "1:/x", "", "lib.rs"] {
        assert!(!is_full_path(not_full), "{not_full}");
    }
}

#[test]
fn path_matches_multi_segment_suffix_requires_slash_boundary() {
    // The basename-suffix fallback strips `target` off the tail and accepts ONLY when the
    // remaining prefix is empty OR ends in '/'. This drives the `prefix.ends_with('/')`
    // operand: a deep multi-segment suffix whose prefix is non-empty but slash-aligned.
    assert!(
        names_file("turn_engine/engine.py", "/a/b/turn_engine/engine.py"),
        "multi-segment suffix accepted at a '/' boundary (prefix '/a/b/' ends in '/')"
    );
    // A suffix that lands mid-component (prefix does NOT end in '/') is rejected.
    assert!(
        !names_file("engine.py", "/a/bxengine.py"),
        "mid-component match rejected (prefix 'bx' is non-empty and not slash-aligned)"
    );
}

#[test]
fn path_matches_deep_suffix_after_slash_boundary() {
    // Directly drives the `prefix.ends_with('/')` operand: a target that is a trailing
    // multi-segment slice whose stripped prefix is non-empty and slash-terminated.
    assert!(
        names_file("src/relay/engine.py", "/root/app/src/relay/engine.py"),
        "deep suffix accepted (prefix '/root/app/' is non-empty and ends with '/')"
    );
    assert!(
        names_file("b/c.rs", "/a/b/c.rs"),
        "two-segment suffix at a '/' boundary"
    );
}

#[test]
fn path_matches_basename_suffix_accepts_windows_separator() {
    assert!(names_file("x.md", r"C:\plans\x.md"));
    assert!(names_file("x.md", "/plans/x.md"));
    assert!(!names_file("x.md", r"C:\plans\ax.md"));
    assert!(!names_file("x.md", "/plans/ax.md"));
}

#[test]
fn a_cwd_less_record_path_takes_the_transcripts_original_cwd() {
    let t = Target::new(Some("lib.rs")).with_origin_cwd(Some("/work/proj"));
    // A file-history key carries no cwd: it is the file under the original cwd, the same
    // candidate as the absolute path the tools wrote.
    assert!(path_matches(&t, "src/lib.rs", None));
    assert!(path_matches(
        &t,
        "/work/proj/src/lib.rs",
        Some("/work/proj")
    ));
    // A record with a cwd of its own keeps it, and a non-absolute one does not count as one.
    assert!(path_matches(&t, "lib.rs", Some("/elsewhere")));
    assert!(path_matches(&t, "b/lib.rs", Some("")));
    let got: Vec<String> = t.into_matched().into_iter().collect();
    assert_eq!(
        got,
        [
            "/elsewhere/lib.rs",
            "/work/proj/b/lib.rs",
            "/work/proj/src/lib.rs"
        ]
    );
    let records = numbered(&[
        r#"{"type":"file-history-snapshot","snapshot":{"trackedFileBackups":{}}}"#,
        r#"{"type":"user","cwd":"","message":{"role":"user","content":"a"}}"#,
        r#"{"type":"user","cwd":"/work/proj","message":{"role":"user","content":"b"}}"#,
        r#"{"type":"user","cwd":"/work/proj/sub","message":{"role":"user","content":"c"}}"#,
    ]);
    assert_eq!(original_cwd(&records), Some("/work/proj"));
    assert_eq!(original_cwd(&records[..2]), None);
}

#[test]
fn an_exact_target_matches_only_the_file_it_spells() {
    let e = Target::new(Some("a/lib.rs")).exact();
    assert!(path_matches(&e, "a/lib.rs", None));
    assert!(
        !path_matches(&e, "/w/a/lib.rs", None),
        "a file ending with it"
    );
    assert!(!path_matches(&e, "b/a/lib.rs", None));
    assert!(
        !path_matches(&e, "a/lib.rs", Some("/w")),
        "resolved elsewhere"
    );
    let got: Vec<String> = e.into_matched().into_iter().collect();
    assert_eq!(got, ["a/lib.rs"]);
    let abs = Target::new(Some("/w/a/lib.rs")).exact();
    assert!(path_matches(&abs, "a/lib.rs", Some("/w")));
    assert!(!path_matches(&abs, "a/lib.rs", None));
}

#[test]
fn a_path_is_compared_in_one_lexical_spelling() {
    for (raw, want) in [
        ("./a/lib.rs", "a/lib.rs"),
        ("a//lib.rs", "a/lib.rs"),
        ("a/./b/./lib.rs", "a/b/lib.rs"),
        ("/p//q/lib.rs", "/p/q/lib.rs"),
        ("/p/./q/lib.rs", "/p/q/lib.rs"),
        ("///p/q", "/p/q"),
        // A leading pair stays: the implementation-defined `//`, the UNC `\\`.
        ("//srv/q", "//srv/q"),
        (r"\\srv\share\.\x", r"\\srv\share\x"),
        (r"C:\a\.\b\\c", r"C:\a\b\c"),
        // `..` needs the filesystem: kept as written.
        ("/p/x/../q", "/p/x/../q"),
        ("../lib.rs", "../lib.rs"),
        ("~/./x", "~/x"),
        (".", "."),
        ("lib.rs", "lib.rs"),
    ] {
        assert_eq!(normalize_path(raw), want, "{raw}");
    }
    // The target and every candidate take it, so any spelling names the one file.
    let t = Target::new(Some("./a//lib.rs"));
    assert!(path_matches(&t, "/w/a/lib.rs", None));
    assert!(path_matches(&t, "a/./lib.rs", Some("/w")));
    let got: Vec<String> = t.into_matched().into_iter().collect();
    assert_eq!(got, ["/w/a/lib.rs"]);
    assert_eq!(candidate("/p//q/./lib.rs", None), "/p/q/lib.rs");
}
