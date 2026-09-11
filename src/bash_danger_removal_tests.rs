//! Boundary pins for the structured removal checker `_9`.
//!
//! `classify`'s example table reaches these arms a whole command at a time. These
//! cases run one layer below it: each disjunct of the unresolvable arm on its own
//! operand, the critical-path test and the three node path helpers it is built on.
//! Every case is read off the ported semantics in `bash_danger_removal.rs`, whose
//! doc comment names the arm and the harness offset it belongs to.
//!
//! Declared with an explicit `#[path]` because the module is a set of sibling
//! files rather than a directory (see the note in `main.rs`).

use super::*;

/// The ask one command produced, as (tail, target).
fn ask(command: &str) -> Option<(&'static str, String)> {
    match structured(command) {
        OperandVerdict::Ask { tail, target } => Some((tail, target)),
        _ => None,
    }
}

/// The expected ask, spelled once.
fn unresolvable(target: &str) -> Option<(&'static str, String)> {
    Some((TAIL_UNRESOLVABLE, target.to_string()))
}

fn critical(target: &str) -> Option<(&'static str, String)> {
    Some((TAIL_CRITICAL, target.to_string()))
}

#[test]
fn the_unresolvable_arm_reads_each_of_its_shapes_in_turn() {
    // The arm is a disjunction and the operands below take it one branch at a
    // time, in the harness's own order.
    assert_eq!(ask("rm -rf a/../*"), unresolvable("a/../*"));
    assert_eq!(ask("rm -rf $D/*"), unresolvable("__TRACKED_VAR__/*"));
    assert_eq!(
        ask("rm -rf $(id -u)/*"),
        unresolvable("__CMDSUB_OUTPUT__/*")
    );
    assert_eq!(ask("rm -rf ~/x/*"), unresolvable("~/x/*"));
    assert_eq!(
        ask("rm -rf //server/share/*"),
        unresolvable("//server/share/*")
    );
    assert_eq!(ask("rm -rf ../*"), unresolvable("../*"));
    assert_eq!(ask("rmdir -p a/*"), unresolvable("a/*"));
    // A relative glob carrying none of those shapes falls through the whole
    // chain and lands on the arm that reads the filesystem.
    assert_eq!(structured("rm -rf build/*"), OperandVerdict::NeedsFs);
    // Without the trailing glob run the arm never opens at all.
    assert_eq!(structured("rm -rf $D/build"), OperandVerdict::NeedsFs);
}

#[test]
fn a_cd_anywhere_turns_a_relative_glob_into_an_ask_by_itself() {
    assert_eq!(ask("cd /tmp && rm -rf build/*"), unresolvable("build/*"));
    // An ABSOLUTE glob is not that shape, so the cd does not reach it.
    assert_eq!(
        structured("cd /tmp && rm -rf /tmp/build/*"),
        OperandVerdict::NeedsFs
    );
}

#[test]
fn the_critical_path_arm_answers_for_a_root_a_drive_and_a_top_level_name() {
    assert_eq!(ask("rm -rf /tmp"), critical("/tmp"));
    assert_eq!(
        ask("rm -rf /*"),
        critical("/*"),
        "the glob resolves to the root"
    );
    assert_eq!(ask("rm -rf C:"), critical("C:"));
    assert_eq!(ask("rm -rf C:/x"), critical("C:/x"));
    // A bare `~` is critical whatever the home directory turns out to be.
    assert_eq!(ask("rm ~"), critical("~"));
    assert_eq!(ask("rm ~/"), critical("~/"));
    // Two levels down is not critical, and what follows needs the filesystem.
    assert_eq!(structured("rm -rf /tmp/build"), OperandVerdict::NeedsFs);
    // A drive-relative name carries no separator, so its dirname is `.`.
    assert_eq!(structured("rm -rf C:x"), OperandVerdict::NeedsFs);
}

#[test]
fn an_environment_resolved_expansion_answers_needs_fs() {
    // The decomposer resolves `To`'s twenty names from the real environment, so
    // the operand's text is not in the transcript at all and csift declines to
    // substitute a sentinel for it.
    assert_eq!(structured("rm -rf $HOME/*"), OperandVerdict::NeedsFs);
    assert_eq!(structured("rm -rf ${TMPDIR}/x"), OperandVerdict::NeedsFs);
    // A name outside that set keeps the tracked-variable stand-in, which the
    // unresolvable arm does read.
    assert_eq!(ask("rm -rf $TMP/*"), unresolvable("__TRACKED_VAR__/*"));
}

#[test]
fn an_interior_glob_reaches_the_arm_that_counts_what_it_would_enumerate() {
    // The fixpoint strips only a TRAILING glob run, so an interior `*` survives
    // into `z` and the last arm takes the operand - and that arm counts against a
    // realpath-resolved target, which is not in the transcript.
    assert_eq!(structured("rm -rf a/*/b/*"), OperandVerdict::NeedsFs);
}

#[test]
fn an_ask_from_any_operand_outranks_a_needs_fs_from_another() {
    // The walk remembers NeedsFs and keeps going; the first ask ends it.
    assert_eq!(
        ask("rm -rf /tmp/build $D/*"),
        unresolvable("__TRACKED_VAR__/*")
    );
    // A command with no removal verb and one with no operand both answer Clear.
    assert_eq!(structured("ls -la /"), OperandVerdict::Clear);
    assert_eq!(structured("rm -rf"), OperandVerdict::Clear);
    assert_eq!(structured(""), OperandVerdict::Clear);
}

#[test]
fn the_trailing_glob_fixpoint_strips_only_a_trailing_run() {
    assert_eq!(trailing_glob_fixpoint("/a/b/*"), "/a/b");
    assert_eq!(trailing_glob_fixpoint("/a/*/b/*"), "/a/*/b");
    assert_eq!(trailing_glob_fixpoint("/a/**/"), "/a");
    assert_eq!(
        trailing_glob_fixpoint("/*"),
        "/",
        "an empty strip becomes the root"
    );
    assert_eq!(trailing_glob_fixpoint("/a/b"), "/a/b");
    // A stripped remainder carrying no separator is kept as it is rather than
    // normalized, which is how the symbolic cwd survives the walk.
    assert_eq!(trailing_glob_fixpoint(&format!("{CWD}/*")), CWD);
    // `z` rebases a relative target off that symbolic head; an absolute one is
    // already its own.
    assert_eq!(relative_z(&format!("{CWD}/a/*/b")), "a/*/b");
    assert_eq!(relative_z("/a/b"), "/a/b");
}

#[test]
fn the_critical_path_test_answers_for_a_glob_a_root_and_a_top_level_name() {
    // The first arm takes a bare glob or a path ending in one. `_9` reaches this
    // test only after its own fixpoint has stripped exactly that shape, so the arm
    // is here because `eDe` is shared with call sites that have not.
    assert!(is_critical_path("*"));
    assert!(is_critical_path("/x/*"));
    // A root, in either separator, and a drive root.
    assert!(is_critical_path("/"));
    assert!(is_critical_path("//"));
    assert!(is_critical_path("\\"));
    assert!(is_critical_path("C:"));
    assert!(is_critical_path("C:/"));
    // A top-level name, and a drive's top level.
    assert!(is_critical_path("/tmp"));
    assert!(is_critical_path("/tmp/"));
    assert!(is_critical_path("C:/x"));
    // Two levels down is not.
    assert!(!is_critical_path("/tmp/build"));
    assert!(!is_critical_path("C:x"));
    assert!(!is_critical_path("C:/x/y"));
}

#[test]
fn normalize_is_the_node_posix_path_normalize() {
    assert_eq!(normalize("/a//b/./c"), "/a/b/c");
    assert_eq!(normalize("a/b/../c"), "a/c");
    // A `..` with nothing to pop is KEPT on a relative path and DROPPED on an
    // absolute one, because an absolute path cannot rise above its root.
    assert_eq!(normalize("../a"), "../a");
    assert_eq!(normalize("../../a"), "../../a");
    assert_eq!(normalize("/../a"), "/a");
    // An empty result is `.` for a relative path and `/` for an absolute one.
    assert_eq!(normalize("a/.."), ".");
    assert_eq!(normalize("/a/.."), "/");
    assert_eq!(normalize(""), ".");
}

#[test]
fn the_path_helpers_are_the_node_ones_they_are_named_for() {
    assert_eq!(dirname("/a/b"), "/a");
    assert_eq!(dirname("/a"), "/");
    assert_eq!(dirname("a"), ".", "no separator at all");
    assert_eq!(collapse_slashes("a\\\\b//c"), "a/b/c");
    assert_eq!(collapse_slashes("a"), "a");
    // A `..` counts for the first arm only when a real segment precedes it.
    assert!(has_dotdot_after_segment("a/../b"));
    assert!(has_dotdot_after_segment("a/./../b"));
    assert!(!has_dotdot_after_segment("../b"));
    assert!(!has_dotdot_after_segment("../../b"));
    assert!(!has_dotdot_after_segment("a/b"));
    // The other two shape tests the arm reads.
    assert!(has_dotdot_segment("../b"));
    assert!(!has_dotdot_segment("a/..b"));
    assert!(ends_with_star_slash("a/*/"));
    assert!(!ends_with_star_slash("a/*"));
    assert!(!ends_with_star_slash("a/b/"));
}
