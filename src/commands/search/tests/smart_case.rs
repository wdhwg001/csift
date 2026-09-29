//! Smart-case reads the letters a pattern SPELLS, never the letters an escape is written with.

use super::*;

use crate::commands::search::pattern_has_uppercase_literal;

/// The escapes for a class or an assertion are written with a capital letter and match no
/// letter of their own, so a lowercase pattern built on them stays case-insensitive.
#[test]
fn an_escape_is_not_an_uppercase_letter() {
    for pattern in [r"\Sbash", r"\Wbash", r"\Dbash", r"bash\B", r"\Sfoo\Wbar"] {
        assert!(
            !pattern_has_uppercase_literal(pattern),
            "{pattern} spells no uppercase letter"
        );
    }
    // End to end through the compiled matcher: the lowercase word matches its uppercase
    // spelling, which a case-sensitive compile would refuse.
    let m = build_matcher(&args(r"\Sbash")).unwrap();
    assert!(
        m.is_match("xBASH"),
        "the escape turned the search case-sensitive"
    );
}

/// A letter the pattern spells counts wherever it sits: bare, escaped as a literal, or inside
/// a bracket class, where a RANGE's ends are letters the reader wrote.
#[test]
fn a_spelled_letter_counts_bare_and_in_a_class() {
    for pattern in ["Carry", "[A-Z]x", "[aQz]", r"\x41b", "a(?:B)c"] {
        assert!(
            pattern_has_uppercase_literal(pattern),
            "{pattern} spells an uppercase letter"
        );
    }
    let m = build_matcher(&args("[A-Z]arry")).unwrap();
    assert!(
        m.is_match("Carry") && !m.is_match("carry"),
        "a class range is case"
    );
}

/// Everything lowercase stays insensitive, and a pattern the AST refuses falls back to the
/// character scan rather than guessing.
#[test]
fn lowercase_and_unparseable_patterns_take_the_character_scan_answer() {
    for pattern in ["bash", r"\d+", "[a-z]", r"\pL"] {
        assert!(!pattern_has_uppercase_literal(pattern), "{pattern}");
    }
    assert!(pattern_has_uppercase_literal("(Unclosed"));
    assert!(!pattern_has_uppercase_literal("(unclosed"));
}
