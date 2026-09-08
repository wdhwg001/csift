//! show --branch-points forks + the compaction-boundary logicalParent excerpt.

use crate::harness::*;

fn rows_of(out: &str) -> Vec<serde_json::Value> {
    out.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// The ONE child line for `L<n>`, trimmed. Asserting against this rather than against two
/// independent `contains` calls is the point: `contains("L4") && contains("rewound")` also
/// passes when the verdicts are swapped onto the wrong children.
fn child_line(out: &str, line: usize) -> String {
    let want = format!("L{line}  ");
    let mut hits = out
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with(&want) && !l.contains("csift show"));
    let found = hits
        .next()
        .unwrap_or_else(|| panic!("no child line for L{line} in:\n{out}"))
        .to_string();
    assert!(
        hits.next().is_none(),
        "L{line} appears on more than one child line in:\n{out}"
    );
    found
}

/// A rewind whose fork parent is an `attachment` line (a prompt submitted after a
/// SessionStart hook is parented to that hook's record, not to the assistant before it),
/// plus a recalled draft on a second fork: L1 prompt, L2 reply, L3 hook attachment,
/// L4 the REWOUND prompt, L5 its reply, L6 the resend, L7 its reply, L8 a recalled DRAFT
/// under L7, L9 the message that replaced it, L10 its reply.
fn forked_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting the lagoon"}]}}"#, "\n",
            r#"{"type":"attachment","uuid":"x0","parentUuid":"a0","timestamp":"2026-06-07T05:00:50.000Z","attachment":{"type":"hook_additional_context","content":["session context"]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"x0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging the northern channel"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"x0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"survey the southern shoal instead"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"surveying the southern shoal"}]}}"#, "\n",
            r#"{"type":"user","uuid":"d0","parentUuid":"a2","timestamp":"2026-06-07T05:03:00.000Z","message":{"role":"user","content":"sound the reef"}}"#, "\n",
            r#"{"type":"user","uuid":"u3","parentUuid":"a2","timestamp":"2026-06-07T05:03:30.000Z","message":{"role":"user","content":"sound the reef margin"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a3","parentUuid":"u3","timestamp":"2026-06-07T05:03:35.000Z","message":{"role":"assistant","id":"m3","content":[{"type":"text","text":"sounding the reef margin"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn a_fork_names_its_live_child_and_the_verdict_of_every_other() {
    let h = forked_home();
    let out = h.run(&["show", at(SESS).as_str(), "--branch-points"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The x0 fork's parent is an ATTACHMENT line: located, with its type printed.
    assert!(
        out.stdout.contains("uuid x0  L3  attachment"),
        "a non-conversation fork parent is located and typed:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("live child: L6") && out.stdout.contains("live child: L9"),
        "each fork names the child the conversation continued from:\n{}",
        out.stdout
    );
    // L4 was answered before the rewind; L8 never was. Each verdict is asserted on ITS OWN
    // child line, so a swap between children fails instead of passing.
    assert!(
        child_line(&out.stdout, 4).ends_with("user  rewound"),
        "the answered branch reads rewound:\n{}",
        out.stdout
    );
    assert!(
        child_line(&out.stdout, 6).ends_with("user  live"),
        "its resend reads live:\n{}",
        out.stdout
    );
    assert!(
        child_line(&out.stdout, 8).ends_with("user  draft"),
        "the recalled prompt reads draft:\n{}",
        out.stdout
    );
    assert!(
        child_line(&out.stdout, 9).ends_with("user  live"),
        "the message that replaced it reads live:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("does not guess"),
        "csift still guesses nothing past the chain:\n{}",
        out.stdout
    );

    let j = h.run(&[
        "show",
        at(SESS).as_str(),
        "--branch-points",
        "--format",
        "json",
    ]);
    let points: Vec<serde_json::Value> = rows_of(&j.stdout)
        .into_iter()
        .filter(|v| v["kind"] == "branch-point")
        .collect();
    let att = points
        .iter()
        .find(|p| p["uuid"] == "x0")
        .expect("the attachment-parented fork");
    assert_eq!(att["parent_line"], 3, "{}", j.stdout);
    assert_eq!(att["parent_type"], "attachment", "{}", j.stdout);
    assert_eq!(att["line"], 3, "{}", j.stdout);
    assert_eq!(att["live_child_line"], 6, "{}", j.stdout);
    assert_eq!(att["children"][0]["line"], 4, "{}", j.stdout);
    assert_eq!(att["children"][0]["survival"], "abandoned", "{}", j.stdout);
    assert_eq!(att["children"][0]["verdict"], "rewound", "{}", j.stdout);
    assert_eq!(att["children"][1]["verdict"], "live", "{}", j.stdout);
    let draft = points
        .iter()
        .find(|p| p["uuid"] == "a2")
        .expect("the recalled-draft fork");
    assert_eq!(draft["live_child_line"], 9, "{}", j.stdout);
    assert_eq!(draft["children"][0]["verdict"], "draft", "{}", j.stdout);
}

#[test]
fn two_same_message_id_siblings_are_both_live_and_the_single_line_field_says_null() {
    // The chain's membership rules keep every assistant record sharing a chain record's
    // `message.id`, so a fork CAN have two live children. The text names both; the JSON
    // field is singular, so it goes null rather than picking one.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0b","parentUuid":"u0","timestamp":"2026-06-07T05:00:07.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting on"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"a0b","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["show", at(SESS).as_str(), "--branch-points"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("live children: L2 L3"),
        "both siblings are named, neither is picked:\n{}",
        out.stdout
    );
    assert!(
        child_line(&out.stdout, 2).ends_with("assistant  live")
            && child_line(&out.stdout, 3).ends_with("assistant  live"),
        "{}",
        out.stdout
    );
    let j = h.run(&[
        "show",
        at(SESS).as_str(),
        "--branch-points",
        "--format",
        "json",
    ]);
    let bp = rows_of(&j.stdout)
        .into_iter()
        .find(|v| v["kind"] == "branch-point")
        .expect("branch-point row");
    assert!(
        bp["live_child_line"].is_null(),
        "the singular field never picks one of two:\n{}",
        j.stdout
    );
    assert_eq!(bp["children"][0]["survival"], "live", "{}", j.stdout);
    assert_eq!(bp["children"][1]["survival"], "live", "{}", j.stdout);
}

#[test]
fn an_off_chain_child_that_opens_no_turn_reads_plain_abandoned() {
    // The fifth verdict. A RETRIED assistant record - a distinct `message.id`, so the
    // membership rules do not rescue it - is off the chain and is no turn opener, so it has
    // no draft/rewound kind to report and reads `abandoned` on its own.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0b","parentUuid":"u0","timestamp":"2026-06-07T05:00:40.000Z","message":{"role":"assistant","id":"mX","content":[{"type":"text","text":"a retried opening"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"a0b","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["show", at(SESS).as_str(), "--branch-points"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        child_line(&out.stdout, 2).ends_with("assistant  abandoned"),
        "the superseded opening reads plain abandoned:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("live child: L3"),
        "the retry is the conversation:\n{}",
        out.stdout
    );
    let j = h.run(&[
        "show",
        at(SESS).as_str(),
        "--branch-points",
        "--format",
        "json",
    ]);
    let bp = rows_of(&j.stdout)
        .into_iter()
        .find(|v| v["kind"] == "branch-point")
        .expect("branch-point row");
    assert_eq!(bp["children"][0]["survival"], "abandoned", "{}", j.stdout);
    assert_eq!(bp["children"][0]["verdict"], "abandoned", "{}", j.stdout);
    assert_eq!(bp["live_child_line"], 3, "{}", j.stdout);
}

#[test]
fn a_parent_uuid_absent_from_the_file_says_exactly_that() {
    // Two children re-attach to a uuid no line in this transcript carries (a clipped
    // head, or a fork copied away): the fork is still a fact, the parent line is not.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":"gone-with-the-head","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"gone-with-the-head","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"survey the shoal"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"surveying"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["show", at(SESS).as_str(), "--branch-points"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("parent uuid not in this file"),
        "the miss is named, not fudged as a missing line:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("parent line not located"),
        "the old wording is gone:\n{}",
        out.stdout
    );
    let j = h.run(&[
        "show",
        at(SESS).as_str(),
        "--branch-points",
        "--format",
        "json",
    ]);
    let bp = rows_of(&j.stdout)
        .into_iter()
        .find(|v| v["kind"] == "branch-point")
        .expect("branch-point row");
    assert!(bp["parent_line"].is_null(), "{}", j.stdout);
    assert!(bp["parent_type"].is_null(), "{}", j.stdout);
    assert!(bp["line"].is_null(), "{}", j.stdout);
}

#[test]
fn a_fork_above_a_compaction_cut_reads_pre_cut() {
    // Claude Code's loader stops at the boundary; csift keeps reading above it and flags
    // what it finds. A fork up there has no live child at all.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"u0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the channel"}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"u0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preTokens":900}}"#, "\n",
            r#"{"type":"user","uuid":"u3","parentUuid":"b1","timestamp":"2026-06-07T05:11:00.000Z","message":{"role":"user","content":"survey the southern shoal"}}"#, "\n",
        ),
    );
    let j = h.run(&[
        "show",
        at(SESS).as_str(),
        "--branch-points",
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let bp = rows_of(&j.stdout)
        .into_iter()
        .find(|v| v["kind"] == "branch-point")
        .expect("branch-point row");
    // L3 sits in the region the chain could not resolve: pre-cut, never "abandoned" -
    // csift declines to call a record abandoned where it cannot see. L2 still reads as a
    // draft, because the measured same-parent opener rule holds even in that blind region.
    assert_eq!(bp["children"][1]["survival"], "pre-cut", "{}", j.stdout);
    assert_eq!(bp["children"][1]["verdict"], "pre-cut", "{}", j.stdout);
    assert_eq!(bp["children"][0]["verdict"], "draft", "{}", j.stdout);
    assert!(bp["live_child_line"].is_null(), "{}", j.stdout);
    let out = h.run(&["show", at(SESS).as_str(), "--branch-points"]);
    assert!(
        out.stdout.contains("live child: none"),
        "an honest none, never a guessed winner:\n{}",
        out.stdout
    );
}

#[test]
fn branch_points_reports_forks_and_excludes_tool_result_carriers() {
    // a1 has FOUR children by parentUuid: two parallel tool_result carriers (must not
    // count), one prompt 10s later, and one rewind re-attach 2h later. The one branch
    // point is a1 with 2 conversation children, widest gap 1h59m50s.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"working"},{"type":"tool_use","id":"t1","name":"Read","input":{}},{"type":"tool_use","id":"t2","name":"Read","input":{}}]}}"#, "\n",
            r#"{"type":"user","uuid":"tr1","parentUuid":"a1","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"one"}]}}"#, "\n",
            r#"{"type":"user","uuid":"tr2","parentUuid":"a1","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"two"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"a1","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"first path"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:00:12.000Z","message":{"role":"assistant","content":[{"type":"text","text":"down the first path"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u3","parentUuid":"a1","timestamp":"2026-06-07T07:00:00.000Z","message":{"role":"user","content":"rewound here"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1b","parentUuid":"u1","timestamp":"2026-06-07T05:00:50.000Z","message":{"role":"assistant","content":[{"type":"text","text":"a retried opening"}]}}"#, "\n",
            "not json here", "\n",
        ),
    );
    let out = h.run(&["show", at(SESS).as_str(), "--branch-points"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("2 branch point(s)") && out.stdout.contains("uuid a1"),
        "the a1 rewind + the u1 assistant-retry fork:\n{}",
        out.stdout
    );
    // The retry fork's children are ASSISTANT records (a1 L2 + a1b L8, 45s apart):
    // assistant children count, and a sub-minute gap renders bare seconds.
    assert!(
        out.stdout.contains("uuid u1") && out.stdout.contains("widest gap 45s"),
        "assistant-retry fork with a seconds-scale gap:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("(1 malformed line(s) skipped)"),
        "the torn tail line is booked:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("children 2"),
        "tool_result carriers excluded (else 4):\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("widest gap 1h59m50s"),
        "gap between L5 and L7 children:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("--line 7"),
        "refetch points at the latest child:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("does not guess"),
        "never classifies live/abandoned:\n{}",
        out.stdout
    );

    let outj = h.run(&[
        "show",
        at(SESS).as_str(),
        "--branch-points",
        "--format",
        "json",
    ]);
    let rows: Vec<serde_json::Value> = outj
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows[0]["kind"], "header");
    assert_eq!(rows[0]["mode"], "branch-points");
    let bp = rows
        .iter()
        .find(|r| r["kind"] == "branch-point")
        .expect("branch-point row");
    assert_eq!(bp["uuid"], "a1");
    assert_eq!(bp["line"], 2);
    assert_eq!(bp["widest_gap_seconds"], 7190);
    assert_eq!(bp["children"].as_array().unwrap().len(), 2);
    assert_eq!(bp["children"][0]["line"], 5);
    assert_eq!(bp["children"][1]["line"], 7);
    let summary = rows.last().unwrap();
    assert_eq!(summary["branch_points"], 2);
    assert_eq!(
        summary["conversation_records"], 6,
        "u1 a1 u2 a2 u3 a1b; carriers excluded: {}",
        outj.stdout
    );

    // A linear session reports an honest zero.
    let h2 = Home::new();
    h2.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"only"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#, "\n",
        ),
    );
    let lin = h2.run(&["show", at(SESS).as_str(), "--branch-points"]);
    assert!(lin.success);
    assert!(
        lin.stdout.contains("0 branch point(s)") && lin.stdout.contains("no forks"),
        "{}",
        lin.stdout
    );
    assert!(
        !lin.stdout.contains("malformed"),
        "a clean file prints no zero note: {}",
        lin.stdout
    );
}

#[test]
fn compaction_boundary_surfaces_logical_parent_uuid() {
    // The boundary record's parentUuid is null; logicalParentUuid names the record the
    // compaction re-links to. It rides the boundary's rendered excerpt (search + show).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"before"}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","parentUuid":null,"logicalParentUuid":"u1","timestamp":"2026-06-07T05:10:00.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"auto","preTokens":900,"postTokens":100,"durationMs":40}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "-t",
        "harness.compaction.boundary",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("[logicalParent=u1]"),
        "the boundary excerpt names the re-link target:\n{}",
        out.stdout
    );
    let shown = h.run(&["show", at(SESS).as_str(), "--line", "2"]);
    assert!(
        shown.stdout.contains("[logicalParent=u1]"),
        "show renders it too:\n{}",
        shown.stdout
    );
}
