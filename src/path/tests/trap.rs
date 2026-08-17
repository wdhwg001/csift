//! The @trap marker grammar: CamelCase words, non-trivial digits, the reserved example.

use super::*;

#[test]
fn trivial_digit_runs_pinned() {
    // Mutation pin: constant-step runs in -2..=2 are trivial; anything else is not -
    // including runs where only the FIRST step matches (the conjunction must hold
    // across all three steps).
    for t in ["0000", "1234", "9876", "1357", "2468", "4321"] {
        assert!(is_trivial_4_digits(t), "{t} is a trivial run");
    }
    for ok in ["4283", "1233", "1122", "5180", "7391"] {
        assert!(!is_trivial_4_digits(ok), "{ok} is not a trivial run");
    }
}

#[test]
fn minimum_length_marker_is_accepted() {
    // Mutation pin: the 13-char minimum (3 words x 3 chars + 4 digits) is INCLUSIVE.
    assert!(validate_trap_marker("FoxBarBaz4283").is_ok());
}

#[test]
fn validate_trap_marker_enforces_the_strict_grammar() {
    // Accepted: EXACTLY 3 imaginative CamelCase words + 4 non-trivial digits.
    for ok in [
        "CrimsonWillowFen5180",
        "MossyLanternCove6024",
        "GildedHeronVale7391",
    ] {
        assert!(validate_trap_marker(ok).is_ok(), "should accept {ok}");
    }
    // Rejected - every lazy shortcut fails loudly.
    for bad in [
        "",                         // empty
        "foo",                      // too short / not the shape
        "CrimsonOwlPond",           // no trailing 4 digits
        "DeepRiverStone12",         // only 2 digits
        "OneTwo4283",               // 2 words (must be EXACTLY 3)
        "WistfulAmberGlenMoor8135", // 4 words (must be EXACTLY 3, not >=3)
        "GoFooBars4283",            // "Go" is a 2-letter word (need >=3 chars)
        "HTTPSPROXYGATE4827",       // ALLCAPS "word" — no lowercase tail
        "HTML0000",                 // the acronym + zeros loophole
        "DeepRiverStone1234",       // trivial: consecutive
        "DeepRiverStone0000",       // trivial: all-equal
        "DeepRiverStone9876",       // trivial: descending
        "DeepRiverStone1357",       // trivial: odd run (+2)
        "DeepRiverStone2468",       // trivial: even run (+2)
    ] {
        assert!(validate_trap_marker(bad).is_err(), "should reject {bad:?}");
    }
}

#[test]
fn validate_trap_marker_refuses_the_reserved_doc_example() {
    // The literal printed in the SKILL / --help / SPEC is reserved: the doc shows it next to
    // `csift` (so quoting the doc self-matches) and every copy-paste collides, so csift refuses
    // it even though it passes the grammar - forcing a fresh hand-invented marker.
    for reserved in RESERVED_EXAMPLE_MARKERS {
        let err = validate_trap_marker(reserved)
            .expect_err("the documented example must be refused")
            .to_string();
        assert!(
            err.contains("RESERVED") && err.to_lowercase().contains("example"),
            "reserved-marker error must explain it is the reserved doc example: {err}"
        );
    }
}

#[test]
fn is_trivial_4_digits_flags_arithmetic_runs_only() {
    for t in ["0000", "1234", "9876", "1357", "2468", "8642", "3210"] {
        assert!(is_trivial_4_digits(t), "{t} is trivial");
    }
    for ok in ["4283", "6024", "7391", "8135", "1212", "1122"] {
        assert!(!is_trivial_4_digits(ok), "{ok} is NOT trivial");
    }
}
