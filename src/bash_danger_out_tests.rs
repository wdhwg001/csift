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
use crate::bash_danger::{classify, Checker, Decision, Generation};
use crate::bash_danger_lexical::{replace_plain_parens, shield};

/// The generation-3 decision for one command, with the checker that made it.
fn gen3(command: &str) -> (Decision, Checker) {
    let v = classify(command, Some("2.1.268"));
    assert_eq!(v.generation, Generation::Gen3, "{command:?}");
    (v.decision, v.checker)
}

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
    // A backslash carries its partner past every test above, wherever in the command
    // the pair sits, so the character it escapes is never blanked or re-read.
    assert_eq!(mask("a \\' b", true), "a \\' b");
    assert_eq!(mask("a\\xy", true), "a\\xy");
    assert_eq!(mask("a\"xy\"", true), "a\"  \"");
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
    assert_eq!(mask_specials("echo \\' a;b"), "echo \\' a;b");
    // Only the shield range is mapped back; another private-use character is not.
    assert_eq!(shield(0), '\u{e001}');
    assert_eq!(unmask("x\u{e020}"), "x\u{e020}");
}

#[test]
fn the_special_mask_shields_an_escaped_blank_and_bails_on_an_open_quote() {
    // The guard admits a command carrying no quote at all when it carries a
    // backslash-escaped blank, which is shielded OUTSIDE the quotes so the operand
    // stays one word. The backslash itself is kept.
    assert_eq!(mask_specials("rm -rf a\\ b"), "rm -rf a\\\u{e009}b");
    assert_eq!(mask_specials("rm -rf a\\\tb"), "rm -rf a\\\u{e00a}b");
    // Inside a double quote the same escape is not an escape at all: a backslash
    // there only escapes `"`, `\`, `$` and a backtick, so the blank is shielded as
    // an ordinary quoted special and the backslash stands on its own.
    assert_eq!(mask_specials("echo \"a\\ b\""), "echo \"a\\\u{e009}b\"");
    // Inside a SINGLE quote a backslash escapes nothing at all, so it is copied as
    // an ordinary character and the special beside it is still shielded.
    assert_eq!(mask_specials("echo 'a\\;b'"), "echo 'a\\\u{e001}b'");
    // A trailing backslash has nothing to carry and is copied alone.
    assert_eq!(mask_specials("echo 'x' \\"), "echo 'x' \\");
    // An UNBALANCED quote bails the whole pass: the shield cannot know where the
    // run ends, so nothing is shielded and the clause split reads the raw text.
    assert_eq!(mask_specials("echo 'a;b"), "echo 'a;b");
    assert_eq!(mask_specials("echo \\' a;b '"), "echo \\' a;b '");
}

#[test]
fn the_backtick_strip_substitutes_the_sentinel() {
    assert_eq!(strip_backticks("a `b` c"), format!("a {VCT} c"));
    // A backslash carries its partner, so an escaped backtick opens nothing.
    assert_eq!(strip_backticks("a \\`b\\` c"), "a \\`b\\` c");
    // An opener with no closer is kept verbatim, and so is a TRAILING lone backslash,
    // which has no partner to carry.
    assert_eq!(strip_backticks("a `b"), "a `b");
    assert_eq!(strip_backticks("a\\"), "a\\");
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
    // A group that OPENS the command, and one whose scan runs off the end of it:
    // the first has no preceding character to disarm it, the second no closer at all.
    assert_eq!(replace_plain_parens("(a)"), VCT.to_string());
    assert_eq!(replace_plain_parens("(abc"), "(abc");
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
    // shields the space after the slash, so the target token ends on a stand-in
    // the target regex does not accept there; the raw pass splits the word at the
    // space and the shorter token matches on end-of-string.
    assert_eq!(out("rm \"$D/ x\"").map(|h| h.target), tgt("\"$D/"));
    // An unclosed quote bails the shield, so the first pass already reads the raw
    // text; the operand walk then skips the word that opens a single quote and
    // ends on `$`.
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
fn a_nested_shell_script_is_rebuilt_and_unmasked_before_the_walk() {
    // The clause the recursion reads has been through the special mask, which
    // turned every space inside the quotes into a stand-in. The script is
    // REASSEMBLED from its runs and unmasked, so the token walk sees words again.
    assert_eq!(out("sh -c 'rm -rf $D/*'").map(|h| h.target), tgt("$D/*"));
    // Adjacent runs are ONE script: the walk ends at the first unquoted space, not
    // at the first closing quote, so the quotes the reassembly keeps ride along.
    assert_eq!(
        out("sh -c 'rm -rf '\"$D\"'/*'").map(|h| h.target),
        tgt("'$D'/*")
    );
    // An UNQUOTED run between two quoted ones is a run of its own.
    assert_eq!(
        out("sh -c 'rm -rf '$D'/*'").map(|h| h.target),
        tgt("'$D'/*")
    );
    // An empty leading run leaves a word the walk cannot read as a verb.
    assert!(out("sh -c ''rm -rf $D/*'").is_none());
    // A head sitting INSIDE a quoted run of a balanced clause is text, not an
    // invocation. A no-break space is whitespace to the head regex and to the
    // token split, but it is not one of the ten characters the shield maps, so it
    // is what lets such a head survive the mask at all.
    assert!(out("echo 'a\u{a0}sh\u{a0}-c\u{a0}\"rm -rf $D/*\"'").is_none());
}

#[test]
fn a_clause_is_trimmed_of_leading_whitespace_and_sentinels_before_the_walk() {
    // The paren fixpoint leaves a sentinel where the substitution was. Trimming it
    // off the clause head is what lets the verb fused to it be read: the token walk
    // skips a sentinel-led WORD, but `\u{e020}rm` is not the verb `rm`.
    assert_eq!(out("$(x)rm -rf $D/*").map(|h| h.target), tgt("$D/*"));
}

#[test]
fn the_nested_allowance_reads_the_depth_it_is_at_and_the_quote_it_came_from() {
    // At the TOP level the caller's allowance is granted whatever the command
    // says, so a `set --` that retires the positional form outside does not retire
    // it inside a double-quoted nested script whose own arguments are supplied.
    assert_eq!(
        out("set -- a; sh -c \"rm -rf $1/*\" _ /tmp").map(|h| h.target),
        tgt("$1/*")
    );
    // Below the top level it is NOT granted: the single-quoted arm here refuses it
    // (its own arguments are supplied), so the script one level down has only its
    // own empty tail to go on - which is enough.
    assert_eq!(
        out("sh -c 'sh -c \"rm -rf $1/*\"' _ /tmp").map(|h| h.target),
        tgt("$1/*")
    );
    // The operand walk's single-quote-opener skip is off inside a DOUBLE-quoted
    // script, where such a word is an ordinary operand and can be the target.
    assert_eq!(
        out("sh -c \"rm -rf '$D/*$\"").map(|h| h.target),
        tgt("'$D/*$")
    );
}

#[test]
fn the_nested_recursion_reaches_depth_two_and_stops() {
    // Generation 3 recurses into a nested script, and a script that is itself a
    // nested shell is recursed into once more.
    assert_eq!(
        out("sh -c \"sh -c 'rm -rf $D/*'\"").map(|h| h.target),
        tgt("$D/*")
    );
    // `JNo` is 2, so the third level is never entered.
    assert!(out("sh -c \"sh -c \\\"sh -c 'rm -rf $D/*'\\\"\"").is_none());
}

#[test]
fn a_double_quoted_nested_script_keeps_an_escaped_dollar_out_of_the_target_test() {
    // `\$NAME` is resolved by the INNER shell, so it reads as a variable and the
    // named target form applies to it exactly as an unescaped one would.
    assert_eq!(
        out("sh -c \"rm -rf \\$D/*\"").map(|h| h.target),
        tgt("$D/*")
    );
    // `\$1` names no variable, so it becomes the stand-in. With nothing supplying
    // the positional parameters the stand-in is put back and the positional form
    // applies; with an argument after the script it stays hidden and nothing
    // matches. Either way the reported target spells the escape as it was written.
    assert_eq!(
        out("sh -c \"rm -rf \\$1/*\"").map(|h| h.target),
        tgt("$1/*")
    );
    assert!(out("sh -c \"rm -rf \\$1/*\" _ /tmp").is_none());
    // A backslash run standing before a live `$` is dropped with the same rule.
    assert_eq!(
        out("sh -c \"rm -rf \\\\$D/*\"").map(|h| h.target),
        tgt("$D/*")
    );
    // A backslash before anything else escapes nothing there, so it and its
    // neighbour are both kept and the run is left where it stands.
    assert_eq!(
        out("sh -c \"rm -rf $D/* \\q\"").map(|h| h.target),
        tgt("$D/*")
    );
}

#[test]
fn the_nested_positional_allowance_is_derived_from_what_supplies_the_arguments() {
    // Nothing follows the script, so `$1` is unset and the positional form applies.
    assert_eq!(out("sh -c 'rm -rf $1/*'").map(|h| h.target), tgt("$1/*"));
    // Two words after it are `$0` and `$1`, so the form does not apply. One word
    // is only `$0` and the script is still dangerous.
    assert!(out("sh -c 'rm -rf $1/*' _ /tmp").is_none());
    assert_eq!(out("sh -c 'rm -rf $1/*' _").map(|h| h.target), tgt("$1/*"));
    // A redirection and its operand are not arguments, and neither is a trailing
    // find terminator, so both are taken out before the words are counted.
    assert_eq!(
        out("sh -c 'rm -rf $1/*' > log").map(|h| h.target),
        tgt("$1/*")
    );
    assert_eq!(
        out("find . -exec sh -c 'rm -rf $1/*' \\;").map(|h| h.target),
        tgt("$1/*")
    );
    // A backslash-escaped space joins two words into one operand.
    assert_eq!(
        out("sh -c 'rm -rf $1/*' a\\ b").map(|h| h.target),
        tgt("$1/*")
    );
    // An `xargs` ahead of the script appends the arguments instead.
    assert!(out("find . | xargs sh -c 'rm -rf $1/*'").is_none());
    // Unless it is told where to put them, in which case it appends nothing.
    assert_eq!(
        out("find . | xargs -I{} sh -c 'rm -rf $1/*'").map(|h| h.target),
        tgt("$1/*")
    );
    assert_eq!(
        out("find . | xargs -n 1 sh -c 'rm -rf $1/*'").map(|h| h.target),
        tgt("$1/*")
    );
    // `-n5` is one word, so the boundary the option needs is not there.
    assert!(out("find . | xargs -n5 sh -c 'rm -rf $1/*'").is_none());
    // Any other option leaves xargs appending, so the allowance is still off.
    assert!(out("find . | xargs -r sh -c 'rm -rf $1/*'").is_none());
    // The word has to BE `xargs`: neither a word ending in it nor one beginning
    // with it supplies anything.
    assert_eq!(
        out("find . | myxargs sh -c 'rm -rf $1/*'").map(|h| h.target),
        tgt("$1/*")
    );
    assert_eq!(
        out("find . | xargsfoo sh -c 'rm -rf $1/*'").map(|h| h.target),
        tgt("$1/*")
    );
}

#[test]
fn a_too_complex_head_carrying_a_nested_removal_asks_at_generation_three() {
    // The nested script is only reachable on the lexical arm, so the same removal
    // routes two ways: under a too-complex head the lexical classifier reads it
    // and asks, and alone it parses cleanly and the structured checker passes it.
    assert_eq!(
        gen3("if true; then sh -c 'rm -rf $D/*'; fi"),
        (Decision::Ask, Checker::Lexical)
    );
    assert_eq!(
        gen3("[[ -f x ]] && sh -c 'rm -rf $D/*'"),
        (Decision::Ask, Checker::Lexical)
    );
    assert_eq!(
        gen3("sh -c 'rm -rf $D/*'"),
        (Decision::Passthrough, Checker::Structured)
    );
    // Depth two under the same head.
    assert_eq!(
        gen3("if true; then sh -c \"sh -c 'rm -rf $D/*'\"; fi"),
        (Decision::Ask, Checker::Lexical)
    );
    // Generation 2's classifier has no recursion, so the same command passes the
    // lexical arm there and falls through to the census.
    let v2 = classify("if true; then sh -c 'rm -rf $D/*'; fi", Some("2.1.258"));
    assert_eq!(v2.generation, Generation::Gen2);
    assert_ne!(v2.checker, Checker::Lexical);
}

#[test]
fn the_entry_guard_reads_the_command_before_the_sentinel_strip() {
    // The guard runs on the text AS WRITTEN, and the sentinel strip that follows it
    // cannot put back a verb the guard already failed to find: a private-use
    // character splitting `rm` means there is no removal word to match.
    assert_eq!(out("r\u{E020}m -rf $D/*"), None);
    assert_eq!(out("rm -rf $D/*").map(|h| h.target), tgt("$D/*"));
}

#[test]
fn the_token_walk_stops_on_a_verb_whose_word_also_looks_skippable() {
    // A `$`-led path prefix makes the verb token match one of the walk's own skip
    // shapes. The verb test runs FIRST, so the walk stops there instead of stepping
    // over the removal it was looking for.
    let inv = token_walk("sudo $x/rm -rf $D/*").expect("the verb is found");
    assert_eq!(inv.command, "rm");
    assert_eq!(inv.tokens, vec!["-rf", "$D/*"]);
}

#[test]
fn the_operand_skip_needs_a_single_quote_that_stays_open() {
    // The skip drops a token that OPENS a single quote and ends on `$`, whose `$` is
    // therefore literal. A token that closes its quote again is an ordinary operand,
    // and this one is a target.
    let target = Invocation {
        command: "rm",
        tokens: vec!["'$D/'$".to_string()],
    };
    assert_eq!(
        operand_walk(&target, true, false).map(|h| h.target),
        tgt("'$D/'$")
    );
    let literal = Invocation {
        command: "rm",
        tokens: vec!["'$D/$".to_string()],
    };
    assert!(operand_walk(&literal, true, false).is_none());
}

#[test]
fn the_run_split_keeps_a_trailing_unquoted_run() {
    // `iLo` rebuilds the script from its runs, so the text after the closing quote
    // belongs to it: dropping that run would hand the walk a shorter script than the
    // shell would run.
    let chars: Vec<char> = "'ab'c".chars().collect();
    let (runs, end) = quoted_runs(&chars, 0);
    assert_eq!(
        runs,
        vec![(Some('\''), "ab".to_string()), (None, "c".to_string())]
    );
    assert_eq!(end, chars.len());
}

#[test]
fn the_wrapping_quote_strip_needs_a_quote_at_both_ends() {
    // The reassembly puts a single-quoted run's own quotes back, and this undoes
    // exactly that. A script quoted at one end only is not the shape, so nothing is
    // cut off it - cutting would drop a real character from the nested command.
    assert_eq!(strip_wrapping_quotes("'abc'"), "abc");
    assert_eq!(strip_wrapping_quotes("'abc"), "'abc");
    assert_eq!(strip_wrapping_quotes("abc'"), "abc'");
    assert_eq!(strip_wrapping_quotes(""), "");
}
