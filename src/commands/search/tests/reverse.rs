//! The reverse closure's LINE SET, pinned directly.
//!
//! Its e2e suite reads the closure through the hits it produces, which cannot reach one
//! rule: a record with NO physical line contributes no line to the set. The only such
//! record is a merged elicitation-sidecar marker, and a line of `0` is not an address - it
//! fetches nothing - so admitting it would put a number in the set that no reader can use.

use super::*;

/// A `Kept` at the given line, from one fixture line. `line_no` 0 is the sidecar-merged
/// record: it has no physical line at all.
fn kept_at(line: &str, line_no: usize, from_sidecar: bool) -> Kept {
    Kept {
        rec: serde_json::from_str(line).expect("valid fixture record"),
        can_hit: true,
        line_no,
        from_sidecar,
        gated_raw: None,
    }
}

/// A record with NO physical line contributes NO line to the closure, and the records that
/// hang off it still do.
///
/// The sidecar marker is a real member - its uuid goes into the frontier, so the walk does
/// not stop at it - and what it must not contribute is the line `0`, which is what a
/// `>= 0` bound would put there. The set is an address list; `L0` addresses nothing.
#[test]
fn a_record_with_no_physical_line_contributes_no_line() {
    let opener = kept_at(
        r#"{"type":"user","uuid":"q1","timestamp":"2026-06-08T01:00:00.000Z","message":{"role":"user","content":"zzq the question"}}"#,
        1,
        false,
    );
    // The sidecar's pending marker: a real record, merged, with no line of its own.
    let merged = kept_at(
        r#"{"type":"assistant","uuid":"q2","parentUuid":"q1","timestamp":"2026-06-08T01:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_01q","name":"AskUserQuestion","input":{}}]}}"#,
        0,
        true,
    );
    let below = kept_at(
        r#"{"type":"user","uuid":"q3","parentUuid":"q2","timestamp":"2026-06-08T01:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01q","content":"zzq answered"}]}}"#,
        3,
        false,
    );
    let kept = vec![opener, merged, below];
    let rows: Vec<Row<'_>> = kept.iter().map(Row::Rec).collect();

    let lines = for_closure(&rows, &ForAddr::Line(1));
    let got: Vec<usize> = lines.into_iter().collect();
    assert_eq!(
        got,
        vec![1, 3],
        "the merged record is a member and contributes no line; the record under it does"
    );
}

/// The ADDRESS itself is in the set, and a uuid address finds it wherever it sits.
#[test]
fn the_addressed_record_is_in_the_set() {
    let kept = vec![kept_at(
        r#"{"type":"user","uuid":"q1","timestamp":"2026-06-08T01:00:00.000Z","message":{"role":"user","content":"zzq alone"}}"#,
        7,
        false,
    )];
    let rows: Vec<Row<'_>> = kept.iter().map(Row::Rec).collect();
    assert_eq!(
        for_closure(&rows, &ForAddr::Uuid("q1".to_string()))
            .into_iter()
            .collect::<Vec<_>>(),
        vec![7]
    );
    assert!(
        for_closure(&rows, &ForAddr::Uuid("nosuch".to_string())).is_empty(),
        "an address that names nothing closes over nothing"
    );
}
