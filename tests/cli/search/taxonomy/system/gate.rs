//! The whole-file gate against this family's fabricated heads and key words.

use super::*;

/// The whole-file gate must not prune a file whose only hit sits in a FABRICATED head or
/// key word. None of these patterns is a byte substring of the source line.
#[test]
fn a_pattern_matching_only_a_fabricated_word_survives_the_whole_file_gate() {
    let h = system_home();
    for (pattern, selector) in [
        (r"originalModel=model-alpha", "harness.system.refusal"),
        (r"fallbackModel=none", "harness.system.refusal"),
        (r"\[api-error status=529", "harness.system.api-error"),
        (r"retry 3/20", "harness.system.api-error"),
        (r"\[notice warning\]", "harness.system.notice"),
        (r"\[kill\]", "harness.system.kill"),
        (r"\[scheduled Jun 7 5:28am\]", "harness.schedule.event"),
        (r"task=task-zz9", "harness.schedule.event"),
        (r"prompt=zzarmed check the queue", "harness.schedule.event"),
    ] {
        let out = h.run(&["search", pattern, &at(SESS), "-t", selector, "-c"]);
        assert!(out.success, "{pattern} stderr: {}", out.stderr);
        assert_eq!(
            out.stdout.trim(),
            "1",
            "{pattern} lost to the whole-file gate:\n{}\n{}",
            out.stdout,
            out.stderr
        );
    }
}
