//! The ONE tool walk, pinned on the answer no hit can show.
//!
//! Three of the four things `build_tool_index` produces are read by surfaces a test can
//! drive: the tool name on a `tool-response` row, the two pairing id sets behind the
//! use/result verdict. The fourth is the `tool_use` id-to-LINE map the `result_of` edge
//! resolves through, and it has one rule no e2e case can reach - a record with NO physical
//! line records none, because a merged elicitation-sidecar marker has no line at all and an
//! edge pointing at `L0` is an address that fetches nothing.

use super::*;

/// A `Kept` at the given line from one fixture line.
fn kept_at(line: &str, line_no: usize, from_sidecar: bool) -> Kept {
    Kept {
        rec: serde_json::from_str(line).expect("valid fixture record"),
        can_hit: true,
        line_no,
        from_sidecar,
        gated_raw: None,
    }
}

const NATIVE_CALL: &str = r#"{"type":"assistant","uuid":"p1","timestamp":"2026-06-08T02:00:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_01native","name":"Bash","input":{"command":"zzp make"}}]}}"#;
const MERGED_CALL: &str = r#"{"type":"assistant","uuid":"p2","timestamp":"2026-06-08T02:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_01merged","name":"AskUserQuestion","input":{}}]}}"#;
const RESULT: &str = r#"{"type":"user","uuid":"p3","parentUuid":"p1","timestamp":"2026-06-08T02:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01native","content":"zzp done"}]}}"#;

/// ONE walk answers all four questions, and the id-to-line map records a line only when
/// there IS one.
///
/// The merged call is a full member of the other three answers - its id is in the use set,
/// its name is in the name map - and contributes no line, which is the difference between
/// an edge that renders an address and one that renders the id it could not place.
#[test]
fn the_tool_walk_answers_four_questions_and_records_no_line_for_a_merged_record() {
    let kept = vec![
        kept_at(NATIVE_CALL, 4, false),
        kept_at(MERGED_CALL, 0, true),
        kept_at(RESULT, 6, false),
    ];
    let rows: Vec<Row<'_>> = kept.iter().map(Row::Rec).collect();
    let ix = build_tool_index(&rows);

    assert_eq!(
        ix.names.get("toolu_01native").map(String::as_str),
        Some("Bash")
    );
    assert_eq!(
        ix.names.get("toolu_01merged").map(String::as_str),
        Some("AskUserQuestion")
    );
    assert!(ix.uses.contains("toolu_01native") && ix.uses.contains("toolu_01merged"));
    assert!(ix.results.contains("toolu_01native"));
    assert!(!ix.results.contains("toolu_01merged"), "it never returned");

    assert_eq!(
        ix.prov.tool_use_at("toolu_01native"),
        Some(4),
        "the native call records the line that carries it"
    );
    assert_eq!(
        ix.prov.tool_use_at("toolu_01merged"),
        None,
        "a merged record has no physical line, and L0 is an address that fetches nothing"
    );
}
