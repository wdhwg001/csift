//! The BOUNDED CROSSING across surfaces: one fixture, read at several budgets, and what each
//! command that prints survival says about what it crossed.
//!
//! The fixture is the smallest shape that makes the budget matter: TWO compaction boundaries,
//! each naming a `logicalParentUuid` the file still carries, with a branch above the OLDER one
//! that the conversation was rewound past. At budget 0 the walk stops at the newer boundary and
//! everything above is `pre-cut`; at budget 1 it steps over one and stops at the older; at
//! budget 2 (and at `all`) it reaches the head and the rewound branch up there is resolved.
//!
//! The root holds the fixtures and the two readers both halves use; `budget` pins what the
//! BUDGET decides and `disclosure` what the surfaces SAY about it.

use crate::harness::*;

mod budget;
mod disclosure;
mod help;
mod scope;

const XENC: &str = "-Users-dev-example-project";

const XSESS: &str = "5c4b3a29-1817-4665-8443-2211ffee0099";

fn rows(out: &str) -> Vec<serde_json::Value> {
    out.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn summary(out: &str) -> serde_json::Value {
    rows(out)
        .into_iter()
        .find(|v| v["kind"] == "summary")
        .expect("a summary row")
}

/// L1-L2 the first exchange, L3-L4 a branch off L1 the conversation left behind (L4 carries an
/// image, so `image` has a row whose survival marker moves with the budget), L5 the older
/// boundary (logical parent L2), L6 its summary, L7-L8 the middle exchange, L9 the newer
/// boundary (logical parent L8), L10 its summary, L11-L12 the newest exchange. L2 and L8 each
/// write the same file, so `files` and `recover` have rows on both sides of both cuts.
fn crossing_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{XENC}/{XSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/Users/dev/chart.md","content":"alpha\nbeta\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r0","parentUuid":"u0","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"user","content":"survey the shoal instead"}}"#, "\n",
            r#"{"type":"assistant","uuid":"b0","parentUuid":"r0","timestamp":"2026-06-07T05:00:25.000Z","message":{"role":"assistant","id":"mb","content":[{"type":"text","text":"surveying the shoal"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgoAAAANSUhEUg=="}}]}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","parentUuid":null,"logicalParentUuid":"a0","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preTokens":900}}"#, "\n",
            r#"{"type":"user","uuid":"s1","parentUuid":"cb1","isCompactSummary":true,"timestamp":"2026-06-07T05:10:01.000Z","message":{"role":"user","content":"summary one"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"s1","timestamp":"2026-06-07T05:11:00.000Z","message":{"role":"user","content":"dredge the channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:11:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/Users/dev/chart.md","old_string":"alpha","new_string":"delta"}}]}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"cb2","parentUuid":null,"logicalParentUuid":"a1","timestamp":"2026-06-07T05:20:00.000Z","compactMetadata":{"trigger":"auto","preTokens":800}}"#, "\n",
            r#"{"type":"user","uuid":"s2","parentUuid":"cb2","isCompactSummary":true,"timestamp":"2026-06-07T05:20:01.000Z","message":{"role":"user","content":"summary two"}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"s2","timestamp":"2026-06-07T05:21:00.000Z","message":{"role":"user","content":"buoy the entrance"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:21:05.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"buoying the entrance"}]}}"#, "\n",
        ),
    );
    h
}

/// A second transcript in the same project carrying no compaction boundary at all, and (when
/// `mentions_pattern` is false) not even the word the scan looks for, so the section 7f
/// whole-file gate prunes it before any chain is built. Its id sorts AFTER the crossing
/// fixture's, which is what makes it the LAST report folded into the scope's total.
fn add_quiet_sibling(h: &Home, id: &str, mentions_pattern: bool) {
    let body = if mentions_pattern {
        concat!(
            r#"{"type":"user","uuid":"q0","parentUuid":null,"timestamp":"2026-06-07T07:00:00.000Z","message":{"role":"user","content":"dredge the channel again"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"q1","parentUuid":"q0","timestamp":"2026-06-07T07:00:05.000Z","message":{"role":"assistant","id":"qm","content":[{"type":"text","text":"dredging again"}]}}"#,
            "\n",
        )
    } else {
        concat!(
            r#"{"type":"user","uuid":"q0","parentUuid":null,"timestamp":"2026-06-07T07:00:00.000Z","message":{"role":"user","content":"unrelated errand"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"q1","parentUuid":"q0","timestamp":"2026-06-07T07:00:05.000Z","message":{"role":"assistant","id":"qm","content":[{"type":"text","text":"running the errand"}]}}"#,
            "\n",
        )
    };
    h.write(&format!("{XENC}/{id}.jsonl"), body);
}
