//! The `harness.bookkeeping` family's own unit tests: the key=value head, the provenance
//! edges and the marker every unit of the family carries.

use super::*;

/// The needle set a query reaching every bookkeeping leaf arms.
fn book_args(pattern: &str) -> SearchArgs {
    let mut a = args(pattern);
    a.labels = vec!["harness.bookkeeping.*".to_string()];
    a
}

/// A FABRICATED head is not a byte substring of its line, so the literal prefilter cannot
/// see it and the whole-file gate would prune the file that holds it. The head's own raw
/// marker is what keeps the file: registered, the gate reaches stage 2, re-renders the
/// line and finds the head.
///
/// `cost-state` is the instrument because its head joins three values the line keeps in
/// three separate fields, so the pattern below exists in no run of the raw bytes.
#[test]
fn a_fabricated_bookkeeping_head_survives_the_whole_file_gate() {
    let line = r#"{"type":"cost-state","sessionId":"0a1b2c3d-0000-0000-0000-000000000000","totalCostUSD":1.5,"totalDuration":3600000,"startTime":"2026-01-01T00:00:00.000Z"}"#;
    let pattern = "totalCostUSD=1.5 totalDuration=3600000";
    assert!(
        !line.contains(pattern),
        "the pattern must exist only in the fabricated head"
    );
    let m = build_matcher(&book_args(pattern)).expect("a valid matcher");
    assert!(
        m.file_may_match(line.as_bytes()),
        "the registered marker is what keeps a file whose only hit is in a head"
    );
    let record: Record = serde_json::from_str(line).expect("valid fixture record");
    assert!(
        m.synth_texts_match(&record, Some(line)),
        "stage 2 re-renders the head from the source line and finds the pattern"
    );
    assert!(
        !m.synth_texts_match(&record, None),
        "without the line there are no fields to build the head from - the line is what \
         makes the marker verifiable"
    );
}

/// A DEFAULT scan arms none of the bookkeeping markers, so the same line costs it nothing
/// beyond the literal prefilter. The gate for each needle is the one that admits its line
/// as a scan candidate, so a query that never parses the line never scans for its head.
#[test]
fn a_default_scan_arms_no_bookkeeping_marker() {
    let (verifiable, _) = synth_marker_finders(&args("zz"));
    let registered: Vec<String> = verifiable
        .iter()
        .map(|f| String::from_utf8_lossy(f.needle()).into_owned())
        .collect();
    for (needle, _) in crate::commands::search::BOOKKEEPING_NEEDLES {
        assert!(
            !registered.contains(&needle.to_string()),
            "a default scan armed {needle} - the marker set must follow the candidate gate"
        );
    }
    assert!(!registered.contains(&r#""queue-operation""#.to_string()));
}
