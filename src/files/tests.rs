use super::*;

fn rec(line: &str) -> Record {
    serde_json::from_slice(line.as_bytes()).expect("valid fixture record")
}

fn extract(records: &[Record]) -> Vec<TaggedMutation> {
    let line_nos: Vec<usize> = (1..=records.len()).collect();
    extract_mutations("0a1b2c3d-sess", records, &line_nos)
}

/// A synthetic multi-turn session: turn 0 Writes two /tmp docs + Edits a gaps doc;
/// an isMeta pseudo-turn (never a turn delimiter); turn 1 runs a Bash `rm` and a
/// MultiEdit. Each structured tool_use is paired with its carrier so `is_create`
/// joins correctly.
fn fixture() -> Vec<Record> {
    vec![
        // ── turn 0 ──
        rec(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"set up the docs"}}"#,
        ),
        // Write /tmp/a.md (a create) + its carrier.
        rec(
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/tmp/a.md","content":"x"}}]}}"#,
        ),
        rec(
            r#"{"type":"user","uuid":"c0","toolUseResult":{"type":"create","filePath":"/tmp/a.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#,
        ),
        // Write /tmp/b.md (a create) + carrier.
        rec(
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w2","name":"Write","input":{"file_path":"/tmp/b.md","content":"y"}}]}}"#,
        ),
        rec(
            r#"{"type":"user","uuid":"c1","toolUseResult":{"type":"create","filePath":"/tmp/b.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w2","content":"ok"}]}}"#,
        ),
        // Edit /p/spec/gaps.md (an update, not a create) + carrier.
        rec(
            r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/p/spec/gaps.md","old_string":"a","new_string":"b"}}]}}"#,
        ),
        rec(
            r#"{"type":"user","uuid":"c2","toolUseResult":{"type":"update","filePath":"/p/spec/gaps.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#,
        ),
        // isMeta pseudo-turn - NOT a delimiter.
        rec(
            r#"{"type":"user","uuid":"meta","isMeta":true,"timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"Continue from where you left off."}}"#,
        ),
        // ── turn 1 ──
        rec(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"clean up and refactor"}}"#,
        ),
        // Bash rm (heuristic).
        rec(
            r#"{"type":"assistant","uuid":"a3","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"rm /tmp/a.md"}}]}}"#,
        ),
        // MultiEdit /p/src/lib.rs (an update) + carrier.
        rec(
            r#"{"type":"assistant","uuid":"a4","timestamp":"2026-06-07T06:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"m1","name":"MultiEdit","input":{"file_path":"/p/src/lib.rs","edits":[]}}]}}"#,
        ),
        rec(
            r#"{"type":"user","uuid":"c3","toolUseResult":{"type":"update","filePath":"/p/src/lib.rs"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"m1","content":"ok"}]}}"#,
        ),
    ]
}

mod buckets;
mod filters;
mod mutations;
mod render;

// ── Rendering branches (called directly so coverage does not depend on the
//    integration-binary merge; output goes to test stdout, harmless). ──

fn outcome(detail: FilesDetail, muts: Vec<TaggedMutation>) -> Outcome {
    // Derive the scope span from the mutations' distinct transcripts (test-only proxy for
    // the real `resolve_session_files` span) so a subagent fixture still drives the banner.
    // Compute the owned counts BEFORE moving `muts` into the struct.
    let mut subs = std::collections::BTreeSet::new();
    let mut tops = std::collections::BTreeSet::new();
    for m in &muts {
        if m.is_subagent {
            subs.insert(m.session_id.clone());
        } else {
            tops.insert(m.session_id.clone());
        }
    }
    let (scope_top, scope_sub) = (tops.len(), subs.len());
    Outcome {
        detail,
        mutations: muts,
        boundaries: Vec::new(),
        skipped_lines: 0,
        turn_range: None,
        time_window_bounded: false,
        scope_top,
        scope_sub,
    }
}
