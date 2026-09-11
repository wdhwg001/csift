//! Unit pins for the dangerous-removal port.
//!
//! The table below is the tracer's twenty-two example commands, each with the
//! branch it takes, the arm that decides it and the byte offset of that arm in the
//! 2.1.258 build, evaluated at all three generations. A row whose three columns
//! differ is a generational fact, not a bug.
//!
//! This file is declared with an explicit `#[path]` because the module is a set of
//! sibling files rather than a directory (see the note in `main.rs`).

use super::*;

/// One example, with the arm and offset that decides it.
struct Example {
    command: &'static str,
    /// (arm, offset in the 2.1.258 build)
    arm: (&'static str, &'static str),
    gen1: Decision,
    gen2: Decision,
    gen3: Decision,
}

const A_CRITICAL: (&str, &str) = ("_9 critical path", "162771910");
const A_UNRESOLVABLE: (&str, &str) = ("_9 unresolvable target", "162771609");
const A_CD_GLOB: (&str, &str) = ("_9 cd + relative glob", "162770807");
const A_NEEDS_FS: (&str, &str) = ("_9 workspace/ancestor and glob arms", "162772295");
const A_LEXICAL: (&str, &str) = ("hnt / out target test", "162774254");
const A_CENSUS_BAIL: (&str, &str) = ("Bno >64 bail", "162863359");
const A_NONE: (&str, &str) = ("no removal reaches a checker", "162773256");

const EXAMPLES: &[Example] = &[
    // 1. A plain relative removal. The harness resolves it against the shell cwd
    //    and compares it with the workspace set; csift holds neither.
    Example {
        command: "rm file.txt",
        arm: A_NEEDS_FS,
        gen1: Decision::Unresolved,
        gen2: Decision::Unresolved,
        gen3: Decision::Unresolved,
    },
    // 2. An absolute removal two levels down: not critical, so the workspace arm
    //    decides and only the filesystem can answer it.
    Example {
        command: "rm -rf /tmp/build",
        arm: A_NEEDS_FS,
        gen1: Decision::Unresolved,
        gen2: Decision::Unresolved,
        gen3: Decision::Unresolved,
    },
    // 3. A top-level directory: its dirname is `/`, which `eDe` calls critical.
    Example {
        command: "rm -rf /tmp",
        arm: A_CRITICAL,
        gen1: Decision::Ask,
        gen2: Decision::Ask,
        gen3: Decision::Ask,
    },
    // 4. The canonical case. The decomposer turns `$TMP` into a tracked-variable
    //    sentinel, so the target cannot be statically resolved.
    Example {
        command: "rm -rf $TMP/*",
        arm: A_UNRESOLVABLE,
        gen1: Decision::Ask,
        gen2: Decision::Ask,
        gen3: Decision::Ask,
    },
    // 5. Same variable with no glob: the trailing-glob fixpoint changes nothing,
    //    so the unresolvable arm never opens.
    Example {
        command: "rm $TMP/build",
        arm: A_NEEDS_FS,
        gen1: Decision::Unresolved,
        gen2: Decision::Unresolved,
        gen3: Decision::Unresolved,
    },
    // 6. A trailing SLASH is not a glob run. The old lexical-only port called this
    //    dangerous; the checker that actually runs does not.
    Example {
        command: "rm $D/",
        arm: A_NEEDS_FS,
        gen1: Decision::Unresolved,
        gen2: Decision::Unresolved,
        gen3: Decision::Unresolved,
    },
    // 7. Quoted expansion, same sentinel, same arm.
    Example {
        command: "rm -rf \"$DIR\"/*",
        arm: A_UNRESOLVABLE,
        gen1: Decision::Ask,
        gen2: Decision::Ask,
        gen3: Decision::Ask,
    },
    // 8. A subshell is too-complex, so the lexical classifier decides.
    Example {
        command: "(rm -rf $TMP/*)",
        arm: A_LEXICAL,
        gen1: Decision::Ask,
        gen2: Decision::Ask,
        gen3: Decision::Ask,
    },
    // 9. A for-loop body. The generation 1 and 2 clause head cannot match `do rm`,
    //    which is exactly the false positive the old port's keyword strip created.
    //    The generation 3 token walk skips the keyword and reaches the target.
    Example {
        command: "for f in a b; do rm -rf $TMP/$f; done",
        arm: A_LEXICAL,
        gen1: Decision::Passthrough,
        gen2: Decision::Passthrough,
        gen3: Decision::Ask,
    },
    // 10. The same loop carrying a `&&` clause: the clause split reaches the rm
    //     without any keyword skipping, so every generation asks.
    Example {
        command: "for f in a; do [ -f \"$S/$f\" ] && rm -f \"$S/$f\"; done",
        arm: A_LEXICAL,
        gen1: Decision::Ask,
        gen2: Decision::Ask,
        gen3: Decision::Ask,
    },
    // 11. A literal path segment after the slash fails both target regexes.
    Example {
        command: "rm $TMP/literal",
        arm: A_NEEDS_FS,
        gen1: Decision::Unresolved,
        gen2: Decision::Unresolved,
        gen3: Decision::Unresolved,
    },
    // 12. The dangerous-looking token belongs to a redirection, not to the rm.
    Example {
        command: "rm -r $X > $TMP/",
        arm: A_NEEDS_FS,
        gen1: Decision::Unresolved,
        gen2: Decision::Unresolved,
        gen3: Decision::Unresolved,
    },
    // 13. A fused redirection is skipped whole and the real operand is reached.
    Example {
        command: "rm -f 2>/dev/null $TMP/*",
        arm: A_UNRESOLVABLE,
        gen1: Decision::Ask,
        gen2: Decision::Ask,
        gen3: Decision::Ask,
    },
    // 14. A command substitution does NOT force a too-complex parse: its inner
    //     commands are decomposed and the inner rm reaches the structured checker.
    Example {
        command: "echo $(rm $TMP/*)",
        arm: A_UNRESOLVABLE,
        gen1: Decision::Ask,
        gen2: Decision::Ask,
        gen3: Decision::Ask,
    },
    // 15. Two levels of nesting, same answer.
    Example {
        command: "echo $(echo $(rm -rf $TMP/*))",
        arm: A_UNRESOLVABLE,
        gen1: Decision::Ask,
        gen2: Decision::Ask,
        gen3: Decision::Ask,
    },
    // 16. Three levels of nesting, same answer.
    Example {
        command: "echo $(echo $(echo $(rm -rf $TMP/*)))",
        arm: A_UNRESOLVABLE,
        gen1: Decision::Ask,
        gen2: Decision::Ask,
        gen3: Decision::Ask,
    },
    // 19. A leading assignment is a separate child of the command node, so the rm
    //     is still the verb.
    Example {
        command: "X=1 rm -rf $D/*",
        arm: A_UNRESOLVABLE,
        gen1: Decision::Ask,
        gen2: Decision::Ask,
        gen3: Decision::Ask,
    },
    // 20. `xargs rm` on a cleanly parsed pipeline: the verb is `xargs`, and the
    //     seven-name prefix peel does not reach it at ANY generation, because the
    //     seventeen-name skip belongs to the lexical token walk.
    Example {
        command: "ls | xargs rm -rf \"$DIR\"/*",
        arm: A_NONE,
        gen1: Decision::Passthrough,
        gen2: Decision::Passthrough,
        gen3: Decision::Passthrough,
    },
    // 21. `find -exec rm` likewise: the find scan lives on the lexical arm, and a
    //     bare `find` command parses cleanly.
    Example {
        command: "find . -exec rm -rf $TMP/{} \\;",
        arm: A_NONE,
        gen1: Decision::Passthrough,
        gen2: Decision::Passthrough,
        gen3: Decision::Passthrough,
    },
    // 22. A braced expansion with no glob: the old port called it dangerous, the
    //     structured checker does not decide it either way.
    Example {
        command: "rm ${DIR}/$x",
        arm: A_NEEDS_FS,
        gen1: Decision::Unresolved,
        gen2: Decision::Unresolved,
        gen3: Decision::Unresolved,
    },
    // A cd anywhere in the command turns a relative glob into an ask by itself.
    Example {
        command: "cd /tmp && rm -rf build/*",
        arm: A_CD_GLOB,
        gen1: Decision::Ask,
        gen2: Decision::Ask,
        gen3: Decision::Ask,
    },
];

fn versions() -> [(&'static str, Generation); 3] {
    [
        ("2.1.193", Generation::Gen1),
        ("2.1.258", Generation::Gen2),
        ("2.1.268", Generation::Gen3),
    ]
}

#[test]
fn the_traced_examples_decide_the_same_way_per_generation() {
    for ex in EXAMPLES {
        for (version, gen) in versions() {
            let want = match gen {
                Generation::Gen1 => ex.gen1,
                Generation::Gen2 => ex.gen2,
                Generation::Gen3 => ex.gen3,
            };
            let got = classify(ex.command, Some(version));
            assert_eq!(
                got.decision,
                want,
                "{:?} at {version} ({}): arm {} @{} - {}",
                ex.command,
                gen.label(),
                ex.arm.0,
                ex.arm.1,
                got.disclosure()
            );
            assert_eq!(
                got.generation,
                gen,
                "{:?} must run under {}",
                ex.command,
                gen.label()
            );
        }
    }
}

// 17 and 18: the substitution census. The bail counts nodes, not characters, and
// fires only above 64 AND only when the text carries an rm word.
fn with_substitutions(n: usize, body: &str) -> String {
    let subs = "$(true)".repeat(n);
    format!("if true; then {body}; fi {subs}")
}

#[test]
fn the_census_bails_above_sixty_four_substitutions_with_an_rm_word() {
    // 17: 65 substitutions, a too-complex parse and an rm word.
    let cmd = with_substitutions(65, "rm x");
    assert_eq!(
        classify(&cmd, Some("2.1.193")).decision,
        Decision::Passthrough,
        "the census did not exist before 2.1.208"
    );
    let v = classify(&cmd, Some("2.1.258"));
    assert_eq!(
        v.decision,
        Decision::Ask,
        "arm {} @{} - {}",
        A_CENSUS_BAIL.0,
        A_CENSUS_BAIL.1,
        v.disclosure()
    );
    assert_eq!(v.checker, Checker::Census);
    assert!(
        v.reason
            .contains("too many command substitutions to analyze"),
        "reason: {}",
        v.reason
    );
    assert!(v.reason.contains("(65)"), "reason: {}", v.reason);
}

#[test]
fn the_census_does_not_bail_at_exactly_sixty_four() {
    // The census still RUNS at 64 - it just does not bail, so nothing is asked.
    let cmd = with_substitutions(64, "rm x");
    let v = classify(&cmd, Some("2.1.258"));
    assert_ne!(
        v.decision,
        Decision::Ask,
        "64 is not more than 64: {}",
        v.disclosure()
    );
    assert!(!v.reason.contains("too many"), "reason: {}", v.reason);
}

#[test]
fn the_census_without_an_rm_word_asks_nothing() {
    // 18: the scan is abandoned and no ask comes from this path.
    let cmd = with_substitutions(65, "echo x");
    let v = classify(&cmd, Some("2.1.258"));
    assert_eq!(v.decision, Decision::Passthrough, "{}", v.disclosure());
}

#[test]
fn the_generation_is_read_off_the_version_and_assumed_when_absent() {
    assert_eq!(generation_of(Some("2.1.207")), (Generation::Gen1, false));
    assert_eq!(generation_of(Some("2.1.208")), (Generation::Gen2, false));
    assert_eq!(generation_of(Some("2.1.260")), (Generation::Gen2, false));
    assert_eq!(generation_of(Some("2.1.261")), (Generation::Gen3, false));
    assert_eq!(generation_of(Some("2.2.0")), (Generation::Gen3, false));
    assert_eq!(generation_of(Some("1.0.60")), (Generation::Gen1, false));
    // A missing or unreadable version falls to the generation of the build the
    // ledger is verified against, and says it assumed.
    assert_eq!(generation_of(None), (Generation::Gen2, true));
    assert_eq!(generation_of(Some("nightly")), (Generation::Gen2, true));
    assert!(classify("rm -rf $TMP/*", None).generation_assumed);
}

#[test]
fn the_branch_disclosure_names_the_shape_that_forced_the_lexical_path() {
    let v = classify("for f in a; do rm -rf $TMP/$f; done", Some("2.1.258"));
    assert_eq!(v.branch, Branch::Lexical("for_statement"), "{v:?}");
    assert!(
        v.disclosure()
            .contains("lexical (too-complex: for_statement)"),
        "{}",
        v.disclosure()
    );
    let s = classify("rm -rf /tmp", Some("2.1.258"));
    assert_eq!(s.branch, Branch::Structured);
    assert!(
        s.disclosure().contains("path: structured"),
        "{}",
        s.disclosure()
    );
}

#[test]
fn the_reason_tails_are_the_harness_strings() {
    let critical = classify("rm -rf /tmp", Some("2.1.258"));
    assert_eq!(critical.reason, "on critical path: /tmp");
    let lexical = classify("(rm -rf $TMP/*)", Some("2.1.258"));
    assert_eq!(
        lexical.reason,
        "on possibly-empty variable path: $TMP/*",
        "{}",
        lexical.disclosure()
    );
    let unresolved = classify("rm file.txt", Some("2.1.258"));
    assert_eq!(unresolved.reason, NOTE_NEEDS_FS);
    assert!(classify("echo hello", Some("2.1.258")).reason.is_empty());
}

#[test]
fn every_ask_is_bypass_immune_and_nothing_else_is() {
    for ex in EXAMPLES {
        for (version, _) in versions() {
            let v = classify(ex.command, Some(version));
            assert_eq!(
                v.immune,
                v.decision == Decision::Ask,
                "{:?} at {version}",
                ex.command
            );
            assert_eq!(v.blocks(), v.decision == Decision::Ask);
        }
    }
}

#[test]
fn the_generation_three_token_walk_reaches_a_prefixed_removal() {
    // The contract's generation-3 lane: a prefix command in a too-complex body.
    let cmd = "if true; then sudo rm -rf $D/*; fi";
    assert_eq!(
        classify(cmd, Some("2.1.258")).decision,
        Decision::Passthrough,
        "the generation 2 clause head cannot match `then sudo rm`"
    );
    let v = classify(cmd, Some("2.1.268"));
    assert_eq!(v.decision, Decision::Ask, "{}", v.disclosure());
    assert_eq!(v.checker, Checker::Lexical);
}

#[test]
fn the_generation_three_find_scan_reaches_an_exec_removal() {
    let cmd = "if true; then find . -exec rm -rf $TMP/* \\; ; fi";
    assert_eq!(
        classify(cmd, Some("2.1.258")).decision,
        Decision::Passthrough
    );
    assert_eq!(classify(cmd, Some("2.1.268")).decision, Decision::Ask);
}

#[test]
fn the_generation_three_rm_guard_is_case_insensitive() {
    // `out`'s entry guard gained `/i`; the older guard is case-sensitive, and the
    // structured path's verb test is exact, so only generation 3 reaches this.
    let cmd = "if true; then RM -rf $D/*; fi";
    assert_eq!(
        classify(cmd, Some("2.1.258")).decision,
        Decision::Passthrough
    );
    assert_eq!(classify(cmd, Some("2.1.268")).decision, Decision::Ask);
}

#[test]
fn a_command_with_no_removal_word_never_reaches_a_checker_arm() {
    for cmd in [
        "echo $HOME/x",
        "git status",
        "ls -la /tmp",
        "cargo test --quiet",
    ] {
        let v = classify(cmd, Some("2.1.258"));
        assert_eq!(
            v.decision,
            Decision::Passthrough,
            "{cmd}: {}",
            v.disclosure()
        );
        assert!(!v.blocks(), "{cmd}");
    }
}

#[test]
fn the_over_length_command_takes_the_parse_abort_branch() {
    let cmd = format!("rm -rf $TMP/* {}", "x".repeat(11_000));
    let v = classify(&cmd, Some("2.1.258"));
    assert_eq!(v.branch, Branch::Lexical("PARSE_ABORT"));
    assert_eq!(v.decision, Decision::Ask);
}
