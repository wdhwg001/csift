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

    let lines = for_closure(
        &rows,
        &ForAddr::Line(1),
        &crate::model::CallIndex::default(),
    );
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
        for_closure(
            &rows,
            &ForAddr::Uuid("q1".to_string()),
            &crate::model::CallIndex::default()
        )
        .into_iter()
        .collect::<Vec<_>>(),
        vec![7]
    );
    assert!(
        for_closure(
            &rows,
            &ForAddr::Uuid("nosuch".to_string()),
            &crate::model::CallIndex::default()
        )
        .is_empty(),
        "an address that names nothing closes over nothing"
    );
}

/// Whether a transcript HOLDS the addressed record is read off its bytes with `show`'s
/// test: a line number names a line the file has that is a JSON object, a uuid names a
/// line carrying it as its own `uuid`. Every branch of that test, pinned.
#[test]
fn an_address_names_a_record_by_the_bytes_the_file_holds() {
    let file = concat!(
        r#"{"type":"user","uuid":"q1","message":{"role":"user","content":"x"}}"#,
        "\n",
        "\n",
        "a torn line with no braces\n",
        "[1, 2, 3]\n",
        r#"{"type":"mode","mode":"default"}"#,
        "\n",
    )
    .as_bytes();
    let line = |n: usize| names_a_record(file, &ForAddr::Line(n));
    assert!(line(1), "a record line");
    assert!(!line(2), "a blank line is no record");
    assert!(!line(3), "a torn line is no JSON object");
    assert!(!line(4), "a JSON array is no record");
    assert!(
        line(5),
        "a bookkeeping line is addressable, as `show` addresses it"
    );
    assert!(!line(6), "past the last line");
    assert!(!line(99), "far past the last line");
    let uuid = |u: &str| names_a_record(file, &ForAddr::Uuid(u.to_string()));
    assert!(uuid("q1"));
    assert!(
        !uuid("q"),
        "a uuid names a whole value, not a prefix of one"
    );
    assert!(!uuid("nosuch"));
    // A final line with no trailing newline is still a line.
    assert!(names_a_record(br#"{"a":1}"#, &ForAddr::Line(1)));
    // Invalid UTF-8 inside a string is decoded the way the scan decodes it, never counted
    // against the line.
    let mut bad = br#"{"type":"user","uuid":"b1","message":{"role":"user","content":"x"#.to_vec();
    bad.push(0xff);
    bad.extend_from_slice(br#""}}"#);
    assert!(names_a_record(&bad, &ForAddr::Line(1)));
    // ...and a line that decodes to no object stays no record however it decodes.
    assert!(!names_a_record(b"a torn \xff line", &ForAddr::Line(1)));
    // An empty file holds nothing.
    assert!(!names_a_record(b"", &ForAddr::Line(1)));
}

/// The miss wording is one builder, so `show` and `search --for` cannot drift apart.
#[test]
fn the_address_miss_names_the_address_and_the_rule() {
    let m = address_miss("L42");
    assert!(
        m.starts_with("no such record(s): L42 \u{2014} an address renders EVERY"),
        "{m}"
    );
    assert!(m.ends_with("`--raw` reads a torn line"), "{m}");
}

/// A `--for` run fails only when NO transcript names the address; one that does anywhere in
/// the scope is enough, and a run with no address is never asked.
#[test]
fn a_for_run_fails_only_when_no_transcript_names_the_address() {
    let addr = ForAddr::Line(3);
    assert!(require_addressed(Some(&addr), [false, true, false].into_iter()).is_ok());
    let err = require_addressed(Some(&addr), [false, false].into_iter())
        .expect_err("no transcript names L3")
        .to_string();
    assert!(err.starts_with("no such record(s): L3 "), "{err}");
    assert!(
        require_addressed(Some(&addr), std::iter::empty()).is_err(),
        "an empty scope names nothing"
    );
    assert!(require_addressed(None, std::iter::empty()).is_ok());
}

/// A LINE address needs a scope of exactly one transcript; a uuid address takes any scope,
/// and no address takes any scope too.
#[test]
fn a_line_address_needs_one_transcript_and_a_uuid_does_not() {
    let line = ForAddr::parse("L42").unwrap();
    let uuid = ForAddr::parse("0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d").unwrap();
    assert!(require_one_transcript_for_a_line(Some(&line), 1).is_ok());
    for many in [0, 2, 9] {
        let err = require_one_transcript_for_a_line(Some(&line), many)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(&format!("resolves to {many} transcripts"))
                && err.contains("--for <uuid>"),
            "{err}"
        );
        assert!(require_one_transcript_for_a_line(Some(&uuid), many).is_ok());
        assert!(require_one_transcript_for_a_line(None, many).is_ok());
    }
}
