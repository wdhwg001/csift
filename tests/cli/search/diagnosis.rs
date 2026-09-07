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
    assert!(
        !out.stdout.contains("malformed"),
        "and no zero note on stdout either: {}",
        out.stdout
    );
}

#[test]
fn an_unfiltered_zero_match_points_at_the_label_census_not_at_the_filter() {
    // Three branches share this line, and only one is right for a query that carried no
    // `-t`/`-T`: there was no filter to exonerate, so the next move is to ask the scope
    // what it holds. Saying "even without the filter" here names a filter that was never
    // there and sends the reader looking for a mistake they did not make.
    let h = populated_home();
    let out = h.run(&["search", "ZZABSENTZZ", &at(SESS), "--no-subagents"]);
    assert!(out.success, "zero-match exits 0: {}", out.stderr);
    assert!(
        out.stderr.contains("--count-by label"),
        "an unfiltered absence points at the census: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("even without the -t/-T filter"),
        "there was no filter to clear: {}",
        out.stderr
    );
}

#[test]
fn the_excluding_label_list_discloses_the_labels_it_could_not_show() {
    // The probe renders six labels and then DISCLOSES the remainder. Under six there is
    // no remainder, so no tail: a "(+0 more label(s))" would claim something was withheld,
    // and a missing tail past six would hide real labels from a reader already lost.
    let h = populated_home();
    // "carry" occurs under four leaves here, so the list is complete on its face.
    let few = h.run(&[
        "search",
        "carry",
        &at(SESS),
        "--no-subagents",
        "-t",
        "harness.interrupt",
    ]);
    assert!(few.success, "stderr: {}", few.stderr);
    assert!(
        few.stderr.contains("DOES occur"),
        "the probe ran: {}",
        few.stderr
    );
    assert!(
        !few.stderr.contains("more label(s)"),
        "nothing was withheld, so nothing is claimed: {}",
        few.stderr
    );

    // A token under NINE leaves: six are shown, the rest are disclosed as a count.
    let h = Home::new();
    let enc = "-Users-dev-example-project";
    let sess = "00000000-0000-4000-8000-000000000051";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"ZQTOKEN please"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"ZQTOKEN musing"},{"type":"text","text":"ZQTOKEN reply"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"a0","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"echo ZQTOKEN"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"c1","parentUuid":"a1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ZQTOKEN done"}]}}"#, "\n",
            r#"{"type":"user","uuid":"o1","parentUuid":"c1","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"user","content":"<local-command-stdout>ZQTOKEN out</local-command-stdout>"}}"#, "\n",
            r#"{"type":"user","uuid":"n1","parentUuid":"o1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>mon1</task-id>\n<event>tick</event>\n<summary>Monitor \"ZQTOKEN watch\" fired</summary>\n</task-notification>"}}"#, "\n",
            r#"{"type":"user","uuid":"tm1","parentUuid":"n1","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":"<teammate-message teammate_id=\"peer\">\nZQTOKEN hello\n</teammate-message>"}}"#, "\n",
            r#"{"type":"user","uuid":"sm1","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"timestamp":"2026-06-07T05:00:07.000Z","message":{"role":"user","content":"This session is being continued. ZQTOKEN prior context"}}"#, "\n",
        ),
    );
    let many = h.run(&["search", "ZQTOKEN", &at(sess), "-t", "harness.interrupt"]);
    assert!(many.success, "stderr: {}", many.stderr);
    let line = many
        .stderr
        .lines()
        .find(|l| l.contains("DOES occur"))
        .unwrap_or_else(|| panic!("no probe line in:\n{}", many.stderr));
    assert_eq!(
        line.matches('\u{b7}').count(),
        5,
        "exactly six labels are shown, joined by five separators: {line}"
    );
    assert!(
        line.contains("(+3 more label(s))"),
        "the remainder is disclosed with its count: {line}"
    );
}
