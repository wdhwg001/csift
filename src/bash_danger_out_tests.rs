//! Boundary pins for the generation-3 lexical classifier `out`.
//!
//! `classify`'s example table drives `out` a whole command at a time. These cases
//! run one layer below it, on the transforms the walk is built from: the two masks
//! and their private-use stand-ins, the token walk that replaced the clause head,
//! the `find -exec` scan and its per-clause cap, the operand walk, and the
//! nested-shell recursion. Each case is read off the ported semantics in
//! `bash_danger_out.rs`, whose doc comment names the arm it belongs to.
//!
//! Declared with an explicit `#[path]` because the module is a set of sibling
//! files rather than a directory (see the note in `main.rs`).

use super::*;

/// The verb `token_walk` reached, if it reached one.
fn walk_verb(clause: &str) -> Option<&'static str> {
    token_walk(clause).map(|inv| inv.command)
}

/// The operand tokens `token_walk` left for the operand walk.
fn walk_tokens(clause: &str) -> Vec<String> {
    token_walk(clause).map(|inv| inv.tokens).unwrap_or_default()
}

/// The target the operand walk reads out of one clause, positional form allowed.
fn walk_target(clause: &str) -> Option<String> {
    let inv = token_walk(clause)?;
    operand_walk(&inv, true, false).map(|h| h.target)
}

/// The `-exec` argument run after the find terminator rule has cut it.
fn exec_tokens(clause: &str) -> Vec<String> {
    token_walk(clause)
        .map(|inv| truncate_at_find_predicate(inv).tokens)
        .unwrap_or_default()
}

#[test]
fn the_entry_guard_wants_a_dollar_and_a_removal_word() {
    assert!(out("rm -rf /tmp/build").is_none(), "no dollar");
    assert!(out("echo $HOME/x").is_none(), "no removal word");
    assert!(out("rm -rf $D/*").is_some(), "both present");
    // The guard gained `/i` at this generation, so a shouted verb reaches the walk.
    assert!(out("RM -rf $D/*").is_some(), "case-insensitive");
    // A private-use character is stripped from the input before anything reads it,
    // so a command carrying one cannot smuggle a sentinel of its own into the walk.
    assert_eq!(out("rm -rf \u{e020}$D/*").map(|h| h.target), tgt("$D/*"));
}

/// The target string a hit carries, as an owned option.
fn tgt(s: &str) -> Option<String> {
    Some(s.to_string())
}

#[test]
fn the_named_target_form_is_the_widened_one() {
    // The post-slash set gained `?`, `[` and `{`; a `${VAR:-default}` form is
    // accepted; and an optional backslash may precede the slash.
    for target in [
        "$D/*",
        "$D/?",
        "$D/[",
        "$D/{",
        "$D/$x",
        "$D//",
        "$D/",
        "${D}/*",
        "${D:-}/*",
        "${D:-$Y}/*",
        "${D:-''}/*",
        "\"$D\"/*",
        "$D\\/*",
    ] {
        assert!(
            walk_target(&format!("rm -rf {target}")).is_some(),
            "{target}"
        );
    }
    // A literal path segment after the slash is not a target.
    assert!(walk_target("rm -rf $D/literal").is_none());
    assert!(walk_target("rm -rf $D").is_none());
}

#[test]
fn the_positional_target_form_needs_no_function_and_no_set_dashdash() {
    // `$1 $@ $* $!` are targets in their own right, and only at this generation.
    assert_eq!(out("rm -rf $1/*").map(|h| h.target), tgt("$1/*"));
    assert_eq!(out("rm -rf $@/*").map(|h| h.target), tgt("$@/*"));
    assert_eq!(out("rm -rf ${2}/*").map(|h| h.target), tgt("${2}/*"));
    // A function definition anywhere in the command retires the form: inside one
    // the parameters are the function's own, not the script's.
    assert!(out("f() { rm -rf $1/*; }; f").is_none());
    assert!(out("function g { rm -rf $1/*; }").is_none());
    // So does a `set --` that assigns real positional parameters.
    assert!(out("set -- a; rm -rf $1/*").is_none());
    // Neither touches the named form.
    assert_eq!(out("set -- a; rm -rf $D/*").map(|h| h.target), tgt("$D/*"));
}

#[test]
fn a_set_dashdash_counts_only_when_it_assigns_a_real_parameter() {
    assert!(has_set_dashdash("set -- a b"));
    assert!(has_set_dashdash("echo hi; set -- a"));
    // An empty quoted first argument assigns nothing, at the end of the command
    // and before a separator alike.
    assert!(!has_set_dashdash("set -- ''"));
    assert!(!has_set_dashdash("set -- \"\" ; echo x"));
    // The empty pair has to END the word: `''x` is one real argument.
    assert!(has_set_dashdash("set -- ''x"));
    // Nor does a `$`-led word, whose value the classifier cannot see.
    assert!(!has_set_dashdash("set -- $x"));
    assert!(!has_set_dashdash("set -- '$x'"));
    assert!(!has_set_dashdash("set -- \"$x\""));
    // `set` has to open a statement; a `set` in the middle of one is an argument.
    assert!(!has_set_dashdash("echo set -- a"));
}

#[test]
fn the_comment_mask_runs_before_the_function_scan() {
    // The function scan decides whether the positional form applies, and it reads
    // the command with every comment blanked to the end of its line - so a
    // definition that only appears inside a comment retires nothing.
    assert_eq!(out("# f() {}\nrm -rf $1/*").map(|h| h.target), tgt("$1/*"));
    // The same definition outside a comment does retire it.
    assert!(out("f() {}\nrm -rf $1/*").is_none());
}

#[test]
fn the_scan_mask_blanks_a_quoted_run_and_a_comment() {
    // `blank` chooses whether a quoted run is erased or kept: the function scan
    // erases it, the `set --` scan keeps it.
    assert_eq!(mask("a 'b c' d", true), "a '   ' d");
    assert_eq!(mask("a 'b c' d", false), "a 'b c' d");
    // A `#` that opens a word starts a comment, blanked to the end of its line and
    // no further.
    assert_eq!(mask("a # b\nc", true), "a    \nc");
    // A `#` inside a word is an ordinary character.
    assert_eq!(mask("a#b", true), "a#b");
    // A backslash carries its partner past every test above.
    assert_eq!(mask("a \\' b", true), "a \\' b");
}

#[test]
fn the_special_mask_shields_a_special_only_inside_a_quote() {
    // A command carrying neither quote takes the fast path unchanged.
    assert_eq!(mask_specials("rm -rf $D/*"), "rm -rf $D/*");
    // Inside a quote each special becomes a private-use stand-in, in the order
    // `ANe` lists them, so the clause split cannot cut a quoted string apart;
    // unmask puts them back.
    let masked = mask_specials("echo 'a;b|c&d e'");
    assert_eq!(masked, "echo 'a\u{e001}b\u{e002}c\u{e003}d\u{e009}e'");
    assert_eq!(unmask(&masked), "echo 'a;b|c&d e'");
    // Outside the quotes the same characters are untouched.
    assert_eq!(mask_specials("a;b 'x'"), "a;b 'x'");
    // A backslash escape is copied whole, so an escaped quote opens no span.
    assert_eq!(mask_specials("echo \\' a;b '"), "echo \\' a;b '");
    // Only the shield range is mapped back; another private-use character is not.
    assert_eq!(shield(0), '\u{e001}');
    assert_eq!(unmask("x\u{e020}"), "x\u{e020}");
}

#[test]
fn the_backtick_strip_substitutes_the_sentinel() {
    assert_eq!(strip_backticks("a `b` c"), format!("a {VCT} c"));
    // A backslash carries its partner, so an escaped backtick opens nothing.
    assert_eq!(strip_backticks("a \\`b\\` c"), "a \\`b\\` c");
    // An opener with no closer is kept verbatim.
    assert_eq!(strip_backticks("a `b"), "a `b");
    assert_eq!(strip_backticks("plain"), "plain");
}

#[test]
fn the_paren_fixpoint_substitutes_a_group_and_keeps_an_unclosed_one() {
    assert_eq!(paren_fixpoint("a (b) c"), format!("a {VCT} c"));
    assert_eq!(paren_fixpoint("a $(b) c"), format!("a {VCT} c"));
    // Nesting needs more than one round, which is why it is a fixpoint.
    assert_eq!(paren_fixpoint("x $(a $(b) c) y"), format!("x {VCT} y"));
    // A `$`-led or backslash-led paren is not a plain group: the other pass owns
    // the first and the escape disarms the second.
    assert_eq!(replace_plain_parens("a $(b) c"), "a $(b) c");
    assert_eq!(replace_plain_parens("a \\(b) c"), "a \\(b) c");
    // Neither is a group whose closer is escaped, or missing before the next one.
    assert_eq!(replace_plain_parens("a (b\\) c"), "a (b\\) c");
    assert_eq!(replace_plain_parens("a (b (c) d"), format!("a (b {VCT} d"));
}

#[test]
fn the_verb_and_the_prefix_are_read_off_the_last_path_segment() {
    assert_eq!(verb_of("/bin/rm"), Some("rm"));
    assert_eq!(verb_of("/usr/bin/rmdir"), Some("rmdir"));
    assert_eq!(verb_of("\\rm"), Some("rm"));
    assert_eq!(verb_of("RM"), Some("rm"));
    assert_eq!(verb_of("rm>out"), Some("rm"), "a fused redirection tail");
    assert_eq!(verb_of("/bin/ls"), None);
    assert_eq!(verb_of("rmx"), None);
    // A `/` whose left side carries whitespace is not a path separator: the token
    // would have been two words, so the basename rule does not apply to it.
    assert_eq!(verb_of("a b/rm"), None);
    assert!(is_prefix_command("/usr/bin/sudo"));
    assert!(is_prefix_command("\\xargs"));
    assert!(!is_prefix_command("a b/sudo"));
    assert!(!is_prefix_command("/bin/rm"));
}

#[test]
fn the_token_walk_skips_the_shapes_that_lead_a_clause() {
    // A keyword, an assignment and a `$`-led word all precede the verb - which is
    // the whole difference from the generation-1 clause head.
    assert_eq!(walk_verb("then rm x"), Some("rm"));
    assert_eq!(walk_verb("do rmdir x"), Some("rmdir"));
    assert_eq!(walk_verb("A=1 rm x"), Some("rm"));
    assert_eq!(walk_verb("$CMD rm x"), Some("rm"));
    assert_eq!(walk_verb(&format!("{VCT} rm x")), Some("rm"));
    // A FUSED redirection consumes one token, a BARE operator two.
    assert_eq!(walk_tokens(">out rm -rf $D/*"), ["-rf", "$D/*"]);
    assert_eq!(walk_tokens("> out rm -rf $D/*"), ["-rf", "$D/*"]);
    // An ordinary word stops the walk: the clause is not a removal.
    assert!(token_walk("echo rm x").is_none());
    assert!(token_walk("").is_none());
}

#[test]
fn the_prefix_run_consumes_its_own_flags_redirections_and_settings() {
    // This is where `sudo rm` and `xargs rm` enter, the gap the old port patched
    // by hand.
    assert_eq!(walk_verb("sudo rm x"), Some("rm"));
    assert_eq!(walk_verb("xargs rm x"), Some("rm"));
    assert_eq!(walk_verb("sudo doas rm x"), Some("rm"), "two in a row");
    // A flag's own value goes with it, unless the next word is itself a flag, an
    // assignment, a verb or another prefix command.
    assert_eq!(walk_tokens("xargs -I {} rm -rf $D/*"), ["-rf", "$D/*"]);
    assert_eq!(walk_tokens("sudo -n rm -rf $D/*"), ["-rf", "$D/*"]);
    // A redirection, an assignment, a digit and a substitution stand-in all sit
    // between a prefix command and the verb.
    assert_eq!(walk_tokens("env >out rm -rf $D/*"), ["-rf", "$D/*"]);
    assert_eq!(walk_tokens("env > out rm -rf $D/*"), ["-rf", "$D/*"]);
    assert_eq!(walk_tokens("env A=1 rm -rf $D/*"), ["-rf", "$D/*"]);
    assert_eq!(walk_tokens("timeout 30 rm -rf $D/*"), ["-rf", "$D/*"]);
    assert_eq!(walk_tokens("sudo $FLAG rm -rf $D/*"), ["-rf", "$D/*"]);
    assert_eq!(
        walk_tokens(&format!("sudo {VCT} rm -rf $D/*")),
        ["-rf", "$D/*"]
    );
    // An ordinary word after a prefix command stops the walk there, and a flag
    // with nothing after it leaves no verb to read.
    assert!(token_walk("sudo foo rm x").is_none());
    assert!(token_walk("sudo -n").is_none());
}

#[test]
fn the_operand_walk_skips_flags_a_quote_opener_and_redirections() {
    assert_eq!(walk_target("rm -rf $D/*"), tgt("$D/*"));
    // A BARE redirect operator takes the next token with it, so a target-shaped
    // word belonging to the redirection is never read as the removal's operand.
    assert!(walk_target("rm $X > $D/*").is_none());
    // A FUSED one takes only itself, and the operand after it is read.
    assert_eq!(walk_target("rm $X 2>/dev/null $D/*"), tgt("$D/*"));
    // The walk advances past an ordinary word rather than stopping at it.
    assert_eq!(walk_target("rm plain $D/*"), tgt("$D/*"));
    assert!(walk_target("rm plain").is_none());
    // Trailing brackets are trimmed before the target test, and the target is
    // reported without them.
    assert_eq!(walk_target("rm $D/*)"), tgt("$D/*"));
}

#[test]
fn the_raw_second_pass_reads_what_the_masked_first_pass_hid() {
    // `out` runs a quote-aware pass and then a raw one. Here the first pass
    // shields the space inside the unclosed quote, so `rm` gets one unmatched
    // word; the raw pass splits them, and the operand walk then skips the word
    // that opens a single quote and ends on `$`.
    assert_eq!(out("rm '$ $D/*").map(|h| h.target), tgt("$D/*"));
    // A command neither pass can resolve returns nothing from both.
    assert!(out("rm $X").is_none());
    // A leading group wrapper is peeled before the clause split, in both passes.
    assert_eq!(out("(rm -rf $D/*)").map(|h| h.target), tgt("$D/*"));
    assert_eq!(out("{ rm -rf $D/*; }").map(|h| h.target), tgt("$D/*"));
}

#[test]
fn the_find_exec_run_ends_at_its_terminator_or_the_first_predicate() {
    assert_eq!(exec_tokens("rm -rf $D/* \\;"), ["-rf", "$D/*"]);
    assert_eq!(exec_tokens("rm -rf $D/* ;"), ["-rf", "$D/*"]);
    assert_eq!(exec_tokens("rm -rf $D/* {} +"), ["-rf", "$D/*", "{}"]);
    assert_eq!(exec_tokens("rm -rf $D/* -name x"), ["-rf", "$D/*"]);
    // A `+` that does NOT follow `{}` is an ordinary argument.
    assert_eq!(exec_tokens("rm -rf $D/* +"), ["-rf", "$D/*", "+"]);
    // After a `--` the predicate names are operands, so the run keeps them.
    assert_eq!(exec_tokens("rm -- -name $D/*"), ["--", "-name", "$D/*"]);
}

#[test]
fn the_find_scan_stops_after_eight_exec_occurrences_in_one_clause() {
    // The ninth `-exec` is past the per-clause cap, so nothing in it is read.
    let past = format!("find . {}-exec rm -rf $D/*", "-exec rm a ".repeat(8));
    assert!(out(&past).is_none());
    // The eighth is inside it.
    let inside = format!("find . {}-exec rm -rf $D/*", "-exec rm a ".repeat(7));
    assert_eq!(out(&inside).map(|h| h.target), tgt("$D/*"));
}

#[test]
fn a_nested_shell_script_is_read_off_the_masked_clause() {
    // `ZNo` matches the `sh -c '` head and the script is what follows up to the
    // matching quote. An EMPTY script is skipped rather than walked.
    assert!(out("sh -c ''rm -rf $D/*'").is_none());
    // The clause the recursion reads has already been through the special mask,
    // which turns every space inside the quotes into a stand-in - so a
    // space-separated script arrives at the token walk as ONE word and no target
    // is found in it, though the same command outside a nested shell is a hit.
    assert!(out("sh -c 'rm -rf $D/*'").is_none());
    assert!(out("rm -rf $D/*").is_some());
}
