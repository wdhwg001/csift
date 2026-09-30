//! The path spelling rules' two shortcuts, held to the rule they shorten: `normalize_path`
//! returns a path unchanged exactly when the rule leaves it so, and `path_matches_as`
//! refuses without building a candidate only where the candidate could not have matched.

use super::*;

#[test]
fn a_path_the_rule_leaves_unchanged_comes_back_borrowed() {
    for (raw, want) in [
        ("/w/a/lib.rs", "/w/a/lib.rs"),
        ("a/lib.rs", "a/lib.rs"),
        ("lib.rs", "lib.rs"),
        ("a/", "a/"),
        ("/", "/"),
        ("//", "//"),
        ("//srv/q", "//srv/q"),
        (r"\\srv\share\x", r"\\srv\share\x"),
        (r"C:\a\b", r"C:\a\b"),
        ("../lib.rs", "../lib.rs"),
        ("/p/x/../q", "/p/x/../q"),
        ("", ""),
        ("///p/q", "/p/q"),
        ("a//lib.rs", "a/lib.rs"),
        ("a//", "a/"),
        (r"a\/b", "a\\b"),
        ("./a", "a"),
        ("a/.", "a/"),
        ("a/./b", "a/b"),
        (".", "."),
        ("./", "."),
    ] {
        let got = normalize_path(raw);
        assert_eq!(got, want, "{raw}");
        // `.` alone is the one unchanged path built anew: the rule drops the `.` segment
        // and then spells an empty result as `.`, which the borrow test does not model.
        assert_eq!(
            matches!(got, std::borrow::Cow::Borrowed(_)),
            raw == want && raw != ".",
            "{raw}: borrowed exactly when unchanged"
        );
    }
}

#[test]
fn the_last_component_is_the_text_after_the_last_separator() {
    for (p, want) in [
        ("/w/a/lib.rs", "lib.rs"),
        (r"C:\x\lib.rs", "lib.rs"),
        (r"a/b\c", "c"),
        ("lib.rs", "lib.rs"),
        ("a/", ""),
        ("", ""),
    ] {
        assert_eq!(last_component(p), want, "{p}");
    }
}

/// Every verdict and every recorded candidate `path_matches_as` gives equals the one the
/// rule gives when the candidate is always built, over targets, record paths and cwds that
/// cover the shortcut's three escapes: a key ending in `.`, `..` or a separator.
#[test]
fn the_cheap_refusal_never_changes_a_verdict() {
    let targets = [
        "lib.rs",
        "a/lib.rs",
        "/w/a/lib.rs",
        "./a//lib.rs",
        "a/",
        "a",
        ".",
        "..",
        "w",
        "/w",
        r"C:\x\lib.rs",
        r"x\lib.rs",
        "~/lib.rs",
        "b",
    ];
    let paths = [
        "/w/a/lib.rs",
        "a/lib.rs",
        "lib.rs",
        "a/b/..",
        "a/.",
        "a/",
        ".",
        "..",
        "",
        "~/lib.rs",
        "$HOME/lib.rs",
        r"C:\x\lib.rs",
        r"x\lib.rs",
        "a//lib.rs",
        "./lib.rs",
        "sub/../lib.rs",
        "a/b",
        "/w",
    ];
    let cwds = [
        None,
        Some("/w"),
        Some("/w/a"),
        Some(r"C:\x"),
        Some("relative"),
    ];
    let mut checked = 0;
    for t in targets {
        let norm = normalize_path(t).into_owned();
        for exact in [false, true] {
            for (path, key) in paths
                .iter()
                .flat_map(|p| [(*p, *p), (*p, "/w/a/lib.rs"), ("lib.rs", *p)])
            {
                for cwd in cwds {
                    let target = if exact {
                        Target::new(Some(t)).exact()
                    } else {
                        Target::new(Some(t))
                    };
                    let file = candidate(key, cwd);
                    let want = !path.is_empty()
                        && if exact {
                            file == norm
                        } else {
                            names_file(&norm, &normalize_path(path)) || names_file(&norm, &file)
                        };
                    let got = path_matches_as(&target, path, key, cwd);
                    assert_eq!(
                        got, want,
                        "{t:?} exact={exact} {path:?} as {key:?} in {cwd:?}"
                    );
                    let recorded: Vec<String> = target.into_matched().into_iter().collect();
                    let want_recorded: Vec<String> = if want { vec![file] } else { Vec::new() };
                    assert_eq!(
                        recorded, want_recorded,
                        "{t:?} {path:?} as {key:?} in {cwd:?}"
                    );
                    checked += usize::from(want);
                }
            }
        }
    }
    assert!(
        checked > 40,
        "the matrix reaches matches, not only refusals: {checked}"
    );
}
