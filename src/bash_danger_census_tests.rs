//! Boundary pins for the substitution census `Bno`.
//!
//! The census runs on the too-complex arm after the lexical classifier returned
//! nothing: it collects every substitution node, bails above its threshold, re-runs
//! the lexical classifier on each substitution's own text, and hands the tokenised
//! remainder to the structured checker. These cases pin each of those steps and the
//! two bounds it carries. `classify`'s own tests pin what the verdicts look like
//! once they reach the disclosure.
//!
//! Declared with an explicit `#[path]` because the module is a set of sibling
//! files rather than a directory (see the note in `main.rs`).

use super::*;

#[test]
fn the_census_answers_with_the_step_that_asked() {
    // A substitution's own text runs through the lexical classifier of whichever
    // generation is asking, since the census outlived the rewrite.
    assert!(matches!(
        census("echo $(rm -rf $D/*)", false),
        CensusVerdict::Lexical { .. }
    ));
    assert!(matches!(
        census("echo $(rm -rf $D/*)", true),
        CensusVerdict::Lexical { .. }
    ));
    // The tokenised remainder goes to the structured checker, whose ask carries
    // the harness's own tail and the target it names.
    assert_eq!(
        census("x && rm -rf /tmp", false),
        CensusVerdict::Structured {
            tail: bash_danger_removal::TAIL_CRITICAL,
            target: "/tmp".to_string(),
        }
    );
    // Its filesystem-dependent arms are remembered and reported only at the end,
    // so an ask from a later statement is never hidden behind one.
    assert_eq!(
        census("x && rm -rf /tmp/build", false),
        CensusVerdict::NeedsFs
    );
    assert_eq!(
        census("x && rm -rf /tmp/build && rm -rf /tmp", false),
        CensusVerdict::Structured {
            tail: bash_danger_removal::TAIL_CRITICAL,
            target: "/tmp".to_string(),
        }
    );
    assert_eq!(census("echo hi", false), CensusVerdict::Clear);
}

#[test]
fn the_census_group_peel_takes_exactly_one_wrapper() {
    assert_eq!(unwrap_group("{ rm -rf $D/*; }"), "rm -rf $D/*");
    assert_eq!(unwrap_group("(rm -rf $D/*)"), "rm -rf $D/*");
    // A wrapper that does not close is not a wrapper.
    assert_eq!(unwrap_group("{ rm -rf $D/*"), "{ rm -rf $D/*");
    assert_eq!(unwrap_group("(rm -rf $D/*"), "(rm -rf $D/*");
    // An unwrapped statement is returned unchanged.
    assert_eq!(unwrap_group("rm -rf $D/*"), "rm -rf $D/*");
    // The split yields each statement, not the whole string.
    let parts = statements_of("a; b; c");
    assert_eq!(parts.len(), 3, "{parts:?}");
    assert_eq!(parts[1].trim(), "b");
}

#[test]
fn a_text_the_statement_split_cannot_balance_is_one_statement() {
    // `ep`'s documented fallback: an unbalanced text is handed to the classifier
    // whole rather than dropped, because dropping it would hide a removal.
    assert_eq!(statements_of("echo \"a"), vec!["echo \"a".to_string()]);
    assert_eq!(statements_of("echo )"), vec!["echo )".to_string()]);
}

#[test]
fn the_bounded_cmdsub_fixpoint_stops_at_sixteen_iterations() {
    // Backticks go first and unconditionally.
    assert_eq!(cmdsub_fixpoint("a `b` c"), "a __CMDSUB__ c");
    // Each iteration removes ONE nesting level, innermost first.
    assert_eq!(cmdsub_fixpoint("$(a $(b))"), "__CMDSUB__");
    assert_eq!(cmdsub_fixpoint("x $(a) y"), "x __CMDSUB__ y");
    // Sixteen levels resolve; forty do not, which is the cap being real.
    let deep = |n: usize| "$(".repeat(n) + "x" + &")".repeat(n);
    assert_eq!(cmdsub_fixpoint(&deep(15)), "__CMDSUB__");
    assert!(
        cmdsub_fixpoint(&deep(40)).contains("$("),
        "the cap must leave the deepest nesting untouched"
    );
    // The cap sits exactly ON sixteen: one level per iteration, so sixteen levels are
    // the deepest nesting the fixpoint resolves and the seventeenth keeps its
    // outermost substitution.
    assert_eq!(cmdsub_fixpoint(&deep(16)), "__CMDSUB__");
    assert!(
        cmdsub_fixpoint(&deep(17)).contains("$("),
        "seventeen levels need a seventeenth iteration the cap does not give"
    );
    // A command with no substitution is returned unchanged.
    assert_eq!(cmdsub_fixpoint("rm -rf $D/*"), "rm -rf $D/*");
}

#[test]
fn the_census_counts_nested_substitutions_and_the_brace_command_form() {
    // A nested pair counts twice: the walk pushes every node it passes.
    assert_eq!(collect_substitutions("echo $(a $(b))").len(), 2);
    assert_eq!(collect_substitutions("echo `x` $(y)").len(), 2);
    assert!(collect_substitutions("echo plain").is_empty());
    // The `${ cmd}` form is a substitution too, with its pipe and semicolon
    // trimmed the way `u7e` trims them.
    assert_eq!(
        brace_command_body("x ${ |rm -rf $D/*;}", 2).as_deref(),
        Some("rm -rf $D/*")
    );
    assert_eq!(collect_substitutions("x ${ |echo hi;}").len(), 1);
}

#[test]
fn the_substitution_walk_stops_at_its_own_bound() {
    // One substitution per nesting level, so the walk yields exactly one body per
    // pop and the guard is observable as a count: 4096 bodies from 4100 levels.
    let deep = "$(".repeat(4100) + "x" + &")".repeat(4100);
    assert_eq!(collect_substitutions(&deep).len(), 4096);
}

// `resolve_dquote_escapes`, one transform of the shared text layer the fixpoint above
// sits beside: it lives in `bash_danger_lexical`, which this census is a caller of.
// Its cases are hosted here because `src/` sits at the 20-file structure cap, so that
// layer has no `*_tests.rs` file of its own to take them and the two files that do
// cover it are within a dozen lines of the 600-line cap.

/// A `\$` resolves by what FOLLOWS the dollar: a name keeps a real `$`, and anything
/// else leaves the stand-in, so a literal `$1` cannot read as a positional parameter.
/// The braced form looks one character further on.
#[test]
fn an_escaped_dollar_resolves_by_what_names_a_variable() {
    use crate::bash_danger_lexical::{resolve_dquote_escapes, KCT};
    assert_eq!(resolve_dquote_escapes("\\$D"), "$D");
    assert_eq!(resolve_dquote_escapes("\\${D}"), "${D}");
    assert_eq!(resolve_dquote_escapes("\\$0X"), format!("{KCT}0X"));
}

/// The other three replaces drop the backslash they carry.
#[test]
fn a_double_quoted_escape_drops_the_backslash_it_carries() {
    use crate::bash_danger_lexical::resolve_dquote_escapes;
    assert_eq!(resolve_dquote_escapes("a\\\"b"), "a\"b");
    assert_eq!(resolve_dquote_escapes("a\\\\b"), "a\\b");
    assert_eq!(resolve_dquote_escapes("a\\`b"), "a`b");
}
