//! search zero-match self-diagnosis: definitive absence, label probe, skipped-line caveat.

use crate::harness::*;

#[test]
fn search_empty_diagnosis_names_the_excluding_label() {
    let h = populated_home();
    // "low-edge" occurs ONLY under agent.tool.result (record c0). Searching it under
    // `-t user.message` yields zero - the exact L74681 trap. The zero-result diagnosis must
    // NAME the excluding label so a model self-corrects instead of assuming a syntax error.
    let out = h.run(&["search", "low-edge", "--no-subagents", "-t", "user.message"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("no matching exchanges"));
    assert!(
        out.stderr.contains("DEFINITIVE absence"),
        "stderr: {}",
        out.stderr
    );
    assert!(out.stderr.contains("DOES occur"), "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("agent.tool.result"),
        "stderr: {}",
        out.stderr
    );
    // JSON summary carries the machine-legible diagnosis.
    let out = h.run(&[
        "search",
        "low-edge",
        "--no-subagents",
        "-t",
        "user.message",
        "--format",
        "json",
    ]);
    let summary = json_summary(&out.stdout);
    assert_eq!(summary["definitive_absence"], serde_json::json!(true));
    assert_eq!(
        summary["active_filters"],
        serde_json::json!("-t user.message")
    );
    assert_eq!(
        summary["excluded_by_label"]["by_label"]["agent.tool.result"],
        serde_json::json!(1)
    );
}

#[test]
fn search_empty_diagnosis_reports_genuine_absence() {
    let h = populated_home();
    // A token absent even WITHOUT the label filter → say so plainly (not a label mistake).
    let out = h.run(&[
        "search",
        "zzz-absent-zzz",
        "--no-subagents",
        "-t",
        "agent.message",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("DEFINITIVE absence"),
        "stderr: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("genuinely absent"),
        "stderr: {}",
        out.stderr
    );
    let out = h.run(&[
        "search",
        "zzz-absent-zzz",
        "--no-subagents",
        "-t",
        "agent.message",
        "--format",
        "json",
    ]);
    let summary = json_summary(&out.stdout);
    assert_eq!(summary["definitive_absence"], serde_json::json!(true));
    assert_eq!(summary["excluded_by_label"], serde_json::Value::Null);
}

#[test]
fn search_zero_match_diagnosis_discloses_skipped_lines() {
    // An absence claim over a corpus with malformed lines must disclose them: the stderr
    // zero-match diagnosis carries the skipped count (the fixture home has malformed lines).
    let h = populated_home();
    let out = h.run(&["search", "ZZNOSUCHPATTERNZZ"]);
    assert!(out.success, "a zero-match search exits 0: {}", out.stderr);
    assert!(
        out.stderr.contains("0 matches"),
        "diagnosis frames the absence: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains("malformed line(s) skipped")
            && out.stderr.contains("parseable lines only"),
        "diagnosis disclosed the skipped lines: {}",
        out.stderr
    );
}

#[test]
fn zero_match_diagnosis_on_a_clean_corpus_has_no_malformed_caveat() {
    // Mutation pin (the dual of the skipped-lines disclosure): on a corpus with ZERO
    // malformed lines, the zero-match diagnosis must NOT print the parseable-lines caveat.
    let h = Home::new();
    let _ = header_collision_scenario(&h); // clean fixtures, no malformed lines
    let out = h.run(&["search", "ZZABSENTZZ"]);
    assert!(out.success, "zero-match exits 0: {}", out.stderr);
    assert!(
        out.stderr.contains("0 matches"),
        "diagnosis present: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("malformed"),
        "no malformed caveat on a clean corpus: {}",
        out.stderr
    );
}
