//! The SURVIVAL AXIS end to end: a turn the operator rewound past leaves the numbered
//! conversation under its own leaf, a bare ROLE selector stops seeing it, an explicit leaf
//! still reaches it, and every disclosure says so.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "3f2e1d0c-9b8a-4756-8342-1f0e9d8c7b6a";

/// L1 first prompt, L2 its reply, L3 a hook attachment, L4 the REWOUND prompt, L5 its
/// reply, L6+L7 two Edit records under it, L8 the resend (same parent as L4), L9 its reply.
fn rewind_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting the lagoon"}]}}"#, "\n",
            r#"{"type":"attachment","uuid":"x0","parentUuid":"a0","timestamp":"2026-06-07T05:00:50.000Z","attachment":{"type":"hook_additional_context","content":["session context"]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"x0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging the northern channel"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"a1","timestamp":"2026-06-07T05:01:06.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/Users/dev/chart.md"}}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a3","parentUuid":"a2","timestamp":"2026-06-07T05:01:07.000Z","message":{"role":"assistant","id":"m3","content":[{"type":"tool_use","id":"t2","name":"Edit","input":{"file_path":"/Users/dev/depths.md"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"x0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"survey the southern shoal instead"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a4","parentUuid":"u2","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","id":"m4","content":[{"type":"text","text":"surveying the southern shoal"}]}}"#, "\n",
        ),
    );
    h
}

fn rows(out: &str) -> Vec<serde_json::Value> {
    out.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn summary(out: &str) -> serde_json::Value {
    rows(out)
        .into_iter()
        .find(|v: &serde_json::Value| v["kind"] == "summary")
        .expect("summary row")
}

mod branches;
mod census;
mod leaf;
