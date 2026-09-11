//! Boundary pins for the argv reductions.
//!
//! These run below `classify`, on the steps that turn a command string into the
//! argv the structured checker reads. Each case was derived to differentiate one
//! surviving operator flip from the shipped behavior: the verb normalisation that
//! applies to two names and no others, the prefix peel's per-command token budget,
//! the positional-operand rule's two stop conditions, and the cd flag.
//!
//! Declared with an explicit `#[path]` because the module is a set of sibling
//! files rather than a directory (see the note in `main.rs`).

use super::*;

fn v(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_string()).collect()
}

#[test]
fn the_verb_is_basename_normalised_for_two_names_and_no_others() {
    // A path prefix is stripped only when what is left is a removal verb.
    assert_eq!(normalise_verb("/bin/rm"), "rm");
    assert_eq!(normalise_verb("/usr/bin/rmdir"), "rmdir");
    assert_eq!(normalise_verb("rm"), "rm");
    assert_eq!(normalise_verb("rmdir"), "rmdir");
    // Anything else keeps its whole token, path and all.
    assert_eq!(normalise_verb("/bin/ls"), "/bin/ls");
    assert_eq!(normalise_verb("/bin/rmx"), "/bin/rmx");
    assert_eq!(normalise_verb("xargs"), "xargs");
    assert_eq!(normalise_verb(""), "");
}

#[test]
fn the_prefix_peel_consumes_each_prefix_commands_own_tokens() {
    // A bare prefix word peels to the command after it.
    assert_eq!(
        strip_prefix_commands(&v(&["nohup", "rm", "-rf", "x"])),
        v(&["rm", "-rf", "x"])
    );
    // Its own flags go with it, and so does an explicit `--`.
    assert_eq!(
        strip_prefix_commands(&v(&["nice", "-n", "rm", "y"])),
        v(&["rm", "y"])
    );
    assert_eq!(
        strip_prefix_commands(&v(&["env", "--", "rm", "y"])),
        v(&["rm", "y"])
    );
    // `env` also consumes its NAME=value settings; a plain prefix does not.
    assert_eq!(
        strip_prefix_commands(&v(&["env", "A=1", "rm", "y"])),
        v(&["rm", "y"])
    );
    assert_eq!(
        strip_prefix_commands(&v(&["time", "A=1", "rm"])),
        v(&["A=1", "rm"])
    );
    // `timeout` consumes a duration and nothing that is not one.
    assert_eq!(
        strip_prefix_commands(&v(&["timeout", "30s", "rm", "y"])),
        v(&["rm", "y"])
    );
    assert_eq!(
        strip_prefix_commands(&v(&["timeout", "later", "rm"])),
        v(&["later", "rm"])
    );
    // Two prefixes in a row peel in turn.
    assert_eq!(
        strip_prefix_commands(&v(&["nohup", "nice", "rm"])),
        v(&["rm"])
    );
    // A non-prefix head is returned untouched, and so is an empty argv.
    assert_eq!(
        strip_prefix_commands(&v(&["rm", "-rf", "x"])),
        v(&["rm", "-rf", "x"])
    );
    assert!(strip_prefix_commands(&[]).is_empty());
    // A prefix word with nothing after it leaves no command at all.
    assert!(strip_prefix_commands(&v(&["nohup"])).is_empty());
    assert!(strip_prefix_commands(&v(&["env", "A=1"])).is_empty());
}

#[test]
fn a_duration_is_digits_with_an_optional_unit_suffix() {
    assert!(is_duration("30"));
    assert!(is_duration("30s"));
    assert!(is_duration("1.5h"));
    assert!(is_duration("2d"));
    assert!(!is_duration(""));
    assert!(!is_duration("s"), "a bare unit has no number left");
    assert!(!is_duration("later"));
    assert!(!is_duration("30x"));
}

#[test]
fn the_positional_walk_stops_at_the_first_operand_or_a_double_dash() {
    // Leading flags are skipped; everything from the first operand is taken,
    // including a later `-`-led token.
    assert_eq!(
        positional_operands(&v(&["-rf", "a", "-b"])),
        v(&["a", "-b"])
    );
    // A lone `-` is an operand, not a flag.
    assert_eq!(positional_operands(&v(&["-", "a"])), v(&["-", "a"]));
    // After `--` everything is an operand and the `--` itself is dropped.
    assert_eq!(
        positional_operands(&v(&["--", "-rf", "a"])),
        v(&["-rf", "a"])
    );
    // Flags only means no operands.
    assert!(positional_operands(&v(&["-r", "-f"])).is_empty());
    assert!(positional_operands(&[]).is_empty());
}

#[test]
fn the_cd_flag_is_true_for_any_statement_that_is_a_cd() {
    assert!(any_statement_is_cd("cd /tmp"));
    assert!(any_statement_is_cd("echo hi; cd /tmp && rm -rf x"));
    assert!(
        any_statement_is_cd("nohup cd /tmp"),
        "through the prefix peel"
    );
    assert!(!any_statement_is_cd("rm -rf /tmp/build"));
    assert!(!any_statement_is_cd("cdx /tmp"), "cd is a whole word");
    assert!(!any_statement_is_cd("echo cd"), "only as the verb");
}

#[test]
fn a_statements_argv_drops_its_assignments_and_its_redirections() {
    assert_eq!(command_argv("A=1 B=2 rm -rf x"), v(&["rm", "-rf", "x"]));
    assert_eq!(command_argv("rm -rf x > out"), v(&["rm", "-rf", "x"]));
    assert_eq!(command_argv("rm -rf 2>/dev/null x"), v(&["rm", "-rf", "x"]));
    // An assignment AFTER the verb is an ordinary argument.
    assert_eq!(command_argv("rm A=1"), v(&["rm", "A=1"]));
}

#[test]
fn the_substitution_walk_extracts_each_body_exactly() {
    // The four delimiter forms, each yielding its inside and nothing else.
    assert_eq!(substitution_bodies("echo $(a b)"), v(&["a b"]));
    assert_eq!(substitution_bodies("x `a b` y"), v(&["a b"]));
    assert_eq!(substitution_bodies("diff <(a) >(b)"), v(&["a", "b"]));
    // A nested pair is passed over WHOLE here: the outer body is the unit, and
    // the inner one is reached by walking that body in turn.
    assert_eq!(substitution_bodies("$(a $(b) c)"), v(&["a $(b) c"]));
    // Two in a row: the walk resumes just past each closer, never inside it.
    assert_eq!(substitution_bodies("$(a)$(b)"), v(&["a", "b"]));
    assert_eq!(substitution_bodies("`a``b`"), v(&["a", "b"]));
    // An empty body is a body.
    assert_eq!(substitution_bodies("$()"), v(&[""]));
    // A backslash escapes the next character, so the escaped opener is not one.
    assert_eq!(substitution_bodies("\\$(a) $(b)"), v(&["b"]));
    // An opener with no closer yields nothing rather than running off the end.
    assert!(substitution_bodies("echo $(a").is_empty());
    assert!(substitution_bodies("echo `a").is_empty());
    // A redirection is not a process substitution.
    assert_eq!(substitution_bodies("a < b $(c)"), v(&["c"]));
    assert!(substitution_bodies("echo plain").is_empty());
}

#[test]
fn decomposition_reaches_a_removal_inside_a_substitution() {
    let out = simple_commands("echo $(rm x); ls");
    // The outer statements see the sentinel the decomposer leaves behind, and the
    // substitution's own command is decomposed beside them.
    assert!(out.iter().any(|s| s == "rm x"), "{out:?}");
    assert!(out.iter().any(|s| s == "ls"), "{out:?}");
    assert!(
        out.iter().any(|s| s.contains("__CMDSUB_OUTPUT__")),
        "{out:?}"
    );
    assert_eq!(out.len(), 3, "{out:?}");
}

#[test]
fn the_decomposition_queue_stops_at_its_own_bound() {
    // The walk is breadth-first over substitutions, so a command can enqueue more
    // work than it is worth doing. The bound is 256 iterations: the command itself
    // plus 255 of its bodies, which is exactly what a pathological input returns.
    let subs: String = (0..300).map(|i| format!("$(x{i})")).collect();
    let out = simple_commands(&format!("echo {subs}"));
    assert_eq!(out.len(), 256, "the bound is a count, not an approximation");
    // A command inside the bound loses nothing.
    let small: String = (0..10).map(|i| format!("$(y{i})")).collect();
    let ten = simple_commands(&format!("echo {small}"));
    assert_eq!(ten.len(), 11, "{ten:?}");
    for i in 0..10 {
        assert!(ten.iter().any(|s| s == &format!("y{i}")), "{ten:?}");
    }
}

#[test]
fn the_substitution_splice_replaces_each_body_with_the_sentinel() {
    // The twin of the walk above: one lifts the body OUT, this one puts the
    // decomposer's stand-in IN, and what the operand tests read is this string.
    assert_eq!(strip_substitutions("echo $(a b)"), "echo __CMDSUB_OUTPUT__");
    assert_eq!(strip_substitutions("x `a` y"), "x __CMDSUB_OUTPUT__ y");
    assert_eq!(
        strip_substitutions("diff <(a) >(b)"),
        "diff __CMDSUB_OUTPUT__ __CMDSUB_OUTPUT__"
    );
    // A nested pair collapses to ONE sentinel: the outer group is the unit.
    assert_eq!(strip_substitutions("$(a $(b) c)"), "__CMDSUB_OUTPUT__");
    // Two in a row produce two, with nothing between them.
    assert_eq!(
        strip_substitutions("$(a)$(b)"),
        "__CMDSUB_OUTPUT____CMDSUB_OUTPUT__"
    );
    // What is NOT a substitution survives byte for byte: an escaped opener, an
    // opener with no closer, a bare trailing delimiter, an ordinary redirection.
    for unchanged in [
        "\\$(a)",
        "echo $(a",
        "echo `a",
        "echo $",
        "a < b",
        "rm -rf /tmp",
    ] {
        assert_eq!(strip_substitutions(unchanged), unchanged, "{unchanged:?}");
    }
}

#[test]
fn a_trailing_delimiter_is_not_an_opener() {
    // A command ending in `$`, `<` or `>` has no character after it to test, and
    // reading one would index past the end.
    for tail in ["echo $", "a <", "a >", "$", "<", ">"] {
        assert!(substitution_bodies(tail).is_empty(), "{tail:?}");
        assert_eq!(strip_substitutions(tail), tail, "{tail:?}");
    }
    // An unterminated opener at the very START consumes its own two characters
    // rather than standing still.
    assert!(substitution_bodies("$(a").is_empty());
    assert_eq!(strip_substitutions("$(a"), "$(a");
}

#[test]
fn a_dollar_is_an_opener_only_when_a_paren_follows_it() {
    // `$x` beside an unrelated group: reading the paren pair as this dollar's
    // body would swallow the whole span into one sentinel.
    assert_eq!(strip_substitutions("$x (y)"), "$x (y)");
    assert!(substitution_bodies("$x (y)").is_empty());
}

#[test]
fn a_backslash_with_nothing_after_it_escapes_nothing() {
    // A command whose last character is a backslash (a line continuation typed
    // without its line) - the escape arms must not read the character after it.
    assert_eq!(strip_substitutions("a\\"), "a\\");
    assert_eq!(argv_words("rm a\\"), v(&["rm", "a\\"]));
    assert_eq!(argv_words("rm \"a\\"), v(&["rm", "a\\"]));
}

#[test]
fn the_substitution_walk_resumes_past_the_closing_paren() {
    // Resuming anywhere but one character past the `)` re-reads the end of the
    // body: here the backtick before the paren would pair with the one after it
    // and report a second, fabricated body.
    assert_eq!(substitution_bodies("$(a`)`"), v(&["a`"]));
}

#[test]
fn argv_words_strips_quotes_and_sentinels_every_expansion() {
    assert_eq!(argv_words("rm -rf x"), v(&["rm", "-rf", "x"]));
    // Quotes are removed, and what they contain stays one word.
    assert_eq!(argv_words("rm \"a b\""), v(&["rm", "a b"]));
    assert_eq!(argv_words("rm 'a b'"), v(&["rm", "a b"]));
    assert_eq!(argv_words("rm \"\""), v(&["rm", ""]));
    // A DOUBLE-quoted expansion is expanded; a SINGLE-quoted one is literal,
    // which is the shell's own rule and the reason the two arms differ.
    assert_eq!(argv_words("rm \"$D/x\""), v(&["rm", "__TRACKED_VAR__/x"]));
    assert_eq!(argv_words("rm '$D/x'"), v(&["rm", "$D/x"]));
    // The braced form, two expansions with no separator, and a bare dollar.
    assert_eq!(argv_words("rm ${D}/x"), v(&["rm", "__TRACKED_VAR__/x"]));
    assert_eq!(
        argv_words("rm $A$B"),
        v(&["rm", "__TRACKED_VAR____TRACKED_VAR__"])
    );
    assert_eq!(argv_words("rm $"), v(&["rm", "$"]));
    // A name the decomposer resolves from the environment gets csift's own
    // marker instead, which is what makes that operand answer NeedsFs.
    assert_eq!(
        argv_words("rm $HOME/x"),
        v(&["rm", "__CSIFT_ENV_VALUE__/x"])
    );
    // A backslash outside quotes joins the next character into the word.
    assert_eq!(argv_words("rm a\\ b"), v(&["rm", "a b"]));
    // And INSIDE double quotes it does the same, which is what keeps an escaped
    // quote from closing the word early. A word AFTER the quoted one is what
    // makes the closing quote observable: at end of input a quote that reopens
    // instead of closing returns the same argv.
    assert_eq!(argv_words("rm \"a\\\"b\" c"), v(&["rm", "a\"b", "c"]));
    assert_eq!(argv_words("rm \"a b\" c"), v(&["rm", "a b", "c"]));
    // Runs of whitespace separate words and never produce an empty one.
    assert_eq!(argv_words("rm   x  "), v(&["rm", "x"]));
    assert!(argv_words("   ").is_empty());
    assert!(argv_words("").is_empty());
}

#[test]
fn an_expansion_reports_the_characters_it_consumed() {
    // The second return value is what the caller advances by, so an off-by-one
    // here silently re-reads or skips a character of the operand.
    let at = |s: &str| {
        let chars: Vec<char> = s.chars().collect();
        expansion_at(&chars, 0)
    };
    assert_eq!(at("$D/x"), ("__TRACKED_VAR__".to_string(), 2));
    assert_eq!(at("${D}/x"), ("__TRACKED_VAR__".to_string(), 4));
    assert_eq!(at("$HOME/x"), ("__CSIFT_ENV_VALUE__".to_string(), 5));
    assert_eq!(at("$"), ("$".to_string(), 1));
    assert_eq!(at("$/x"), ("$".to_string(), 1));
    // An unterminated brace consumes the rest of the operand.
    assert_eq!(at("${D"), ("__TRACKED_VAR__".to_string(), 3));
    // A BRACED name is read the same way as a bare one, so a resolved-environment
    // name still answers NeedsFs, and the width covers both braces.
    assert_eq!(at("${HOME}/x"), ("__CSIFT_ENV_VALUE__".to_string(), 7));
    // An underscore is part of the name, not its end - stopping at the `_` would
    // read BASH_VERSION as the unresolved BASH.
    assert_eq!(
        at("${BASH_VERSION}"),
        ("__CSIFT_ENV_VALUE__".to_string(), 15)
    );
}
