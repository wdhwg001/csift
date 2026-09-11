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
