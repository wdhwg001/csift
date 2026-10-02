//! The `harness.bookkeeping` family's own unit tests: the key=value head, the provenance
//! edges, and the `[not delivered]` tag every unit of the family renders.

use super::*;

// `use super::*` pulls in the test module list, where this file's own name shadows the
// family module - so the family is named once, explicitly, and read as `family::` below.
use crate::commands::search::record_text::bookkeeping as family;

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
    let line = r#"{"type":"cost-state","sessionId":"0a1b2c3d-0000-0000-0000-000000000000","totalCostUSD":1.5,"totalDuration":3600000,"startTime":1767225600000}"#;
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

/// The head of a line the family names keys for: the line's own `type` first, then the
/// keys in the WRITER's order, and only the ones the line carries.
#[test]
fn a_head_prints_the_writer_s_key_order_and_skips_an_absent_key() {
    let line = r#"{"type":"bridge-session","sessionId":"s","bridgeSessionId":"b","lastSequenceNum":7,"ownerAccountUuid":"cccccccc-0000-4000-8000-000000000003"}"#;
    assert_eq!(
        family::head(Some(line)).as_deref(),
        Some("[bridge-session bridgeSessionId=b lastSequenceNum=7 ownerAccountUuid=cccccccc]"),
    );
}

/// The TWO value rules and nothing else: an account, an organisation or a record uuid is
/// cut to eight characters; every other value is cut at eighty with the shared marker, an
/// OBJECT printing as its sorted key list. A session-shaped value falls under the second
/// rule, so it prints whole.
#[test]
fn the_two_value_rules_decide_every_head_value() {
    // A record uuid and an account uuid at eight; a session id whole.
    let fork = r#"{"type":"fork-context-ref","agentId":"a1","parentSessionId":"99999999-8888-4777-8666-555555555555","parentLastUuid":"aaaaaaaa-0000-4000-8000-000000000001","contextLength":12}"#;
    assert_eq!(
        family::head(Some(fork)).as_deref(),
        Some(
            "[fork-context-ref agentId=a1 \
             parentSessionId=99999999-8888-4777-8666-555555555555 \
             parentLastUuid=aaaaaaaa contextLength=12]"
        ),
    );
    // An object renders its SORTED key list, whatever order the line spells it in.
    let ledger = r#"{"type":"artifact-comment-monitor","v":1,"artifacts":{"zulu":{},"alpha":{}}}"#;
    assert_eq!(
        family::head(Some(ledger)).as_deref(),
        Some("[artifact-comment-monitor v=1 artifacts=alpha, zulu]"),
    );
    // Eighty-one characters of prose: eighty kept, the shared marker naming the one cut.
    let long = "x".repeat(81);
    let line = format!(r#"{{"type":"ai-title","aiTitle":"{long}"}}"#);
    assert_eq!(
        family::head(Some(&line)).as_deref(),
        Some(format!("[ai-title aiTitle={}… (+1 char)]", "x".repeat(80)).as_str()),
    );
}

/// The instant rule: a string value that is an RFC 3339 instant prints as the canonical
/// local text every other text instant takes, never the raw UTC string - which stays in the
/// verbatim body. A NUMBER is not an instant string, so `cost-state`'s epoch-millisecond
/// `startTime` prints as the line wrote it.
#[test]
fn an_instant_value_prints_as_local_text() {
    let raw = "2026-01-01T00:00:01.000Z";
    let line = format!(r#"{{"type":"history-suppression","cause":"c","ts":"{raw}"}}"#);
    let want = format!(
        "[history-suppression cause=c ts={}]",
        crate::timez::format_local_compact(Some(raw))
    );
    assert_eq!(family::head(Some(&line)).as_deref(), Some(want.as_str()));
    assert!(!want.contains(raw), "the UTC string is not the text form");
    let cost =
        r#"{"type":"cost-state","totalCostUSD":1.5,"totalDuration":42,"startTime":1767225600000}"#;
    assert_eq!(
        family::head(Some(cost)).as_deref(),
        Some("[cost-state totalCostUSD=1.5 totalDuration=42 startTime=1767225600000]"),
    );
}

/// The head's three refusals, each of which leaves the render falling back to the body or
/// to the record-modeled head rather than printing an empty bracket: no source line, a
/// line that is not a JSON object, and a line type the family names no keys for.
#[test]
fn a_head_refuses_what_it_cannot_summarise() {
    assert_eq!(family::head(None), None);
    assert_eq!(family::head(Some("not json")), None);
    assert_eq!(family::head(Some("[1,2]")), None);
    assert_eq!(
        family::head(Some(r#"{"sessionId":"s"}"#)),
        None,
        "a line with no `type` names no keys"
    );
    assert_eq!(
        family::head(Some(r#"{"type":"worktree-state","worktreeSession":"w"}"#)),
        None,
        "a type registered in Claude Code's tables that csift names no keys for"
    );
}

/// A line type the family DOES name keys for, carrying none of them: the bracket holds the
/// type alone rather than a trailing space or an empty `key=`.
#[test]
fn a_head_with_no_present_key_prints_the_type_alone() {
    assert_eq!(
        family::head(Some(r#"{"type":"mode","sessionId":"s"}"#)).as_deref(),
        Some("[mode]"),
    );
}

/// The family's facts, read straight off `facts()`: the marker on every unit, and the
/// relation each line type carries. A caller with no row space resolves no uuid, so the
/// edges print their ids - which is what a unit test, and any file-oriented surface, gets.
#[test]
fn facts_carry_the_line_type_s_relation_and_no_marker_of_their_own() {
    let ctx = ClassifyCtx::top_level();
    let zones = |line: &str| {
        let record = rec(line);
        let (facts, markers) = family::facts(
            &record,
            Some(line),
            &Section::record(record.promoted_class()),
            &ctx,
        );
        (
            facts
                .iter()
                .filter_map(HitFact::label_zone)
                .collect::<Vec<_>>(),
            markers.iter().map(Marker::render).collect::<Vec<_>>(),
        )
    };
    let (edges, markers) = zones(
        r#"{"type":"last-prompt","leafUuid":"aaaaaaaa-0000-4000-8000-000000000001","lastPrompt":"go"}"#,
    );
    // The `[not delivered]` tag is the ENGINE's, printed from `Hit::delivered` like every
    // other leaf's, so the family pushes no copy of it (a copy would print it twice).
    assert!(markers.is_empty(), "{markers:?}");
    assert_eq!(
        edges,
        vec!["[for: leaf aaaaaaaa (not in this file)]".to_string()]
    );
    // A line type with no relation carries nothing at all.
    let (edges, markers) = zones(r#"{"type":"mode","mode":"default"}"#);
    assert!(markers.is_empty(), "{markers:?}");
    assert!(edges.is_empty(), "{edges:?}");
    // The checkpoint is the SUM of two NUMBERS - `startTime` in epoch milliseconds, as the
    // harness writes it - one hour after the start instant, in local text.
    let (edges, _) =
        zones(r#"{"type":"cost-state","startTime":1767225600000,"totalDuration":3600000}"#);
    assert_eq!(
        edges,
        vec![format!(
            "[checkpoint closes {}]",
            crate::timez::format_local_compact(Some("2026-01-01T01:00:00Z"))
        )]
    );
    // A `startTime` STRING is not the harness's shape, so it is not read as an instant.
    let (edges, _) = zones(
        r#"{"type":"cost-state","startTime":"2026-01-01T00:00:00.000Z","totalDuration":3600000}"#,
    );
    assert_eq!(
        edges,
        vec![
            "[checkpoint closes unknown (needs startTime and totalDuration as numbers)]"
                .to_string()
        ]
    );
}

/// The JSON half of the checkpoint keeps the UTC instant, whatever the local zone.
#[test]
fn the_checkpoint_json_carries_the_utc_instant() {
    let line = r#"{"type":"cost-state","startTime":1767225600000,"totalDuration":3600000}"#;
    let record = rec(line);
    let (facts, _) = family::facts(
        &record,
        Some(line),
        &Section::record(record.promoted_class()),
        &ClassifyCtx::top_level(),
    );
    let json: Vec<(&str, serde_json::Value)> = facts.iter().flat_map(HitFact::json).collect();
    assert_eq!(
        json[0],
        (
            "checkpoint_closes_at_utc",
            serde_json::json!("2026-01-01T01:00:00Z")
        )
    );
    assert_eq!(
        json[1],
        (
            "checkpoint_closes_at_local",
            serde_json::json!(crate::timez::local_iso("2026-01-01T01:00:00Z"))
        )
    );
}

/// The fork edge names the FILE its target lives in - and when the line names no record
/// at all, what is left is still an answer: the parent SESSION. The builder's session-only
/// arm renders it, because `@<session>` is a target a reader feeds back to csift and
/// `unknown` would say the line named nothing when it named the file.
#[test]
fn the_fork_edge_names_its_session_with_or_without_a_record() {
    let ctx = ClassifyCtx::top_level();
    let zone = |line: &str| {
        let record = rec(line);
        let (facts, _) = family::facts(
            &record,
            Some(line),
            &Section::record(record.promoted_class()),
            &ctx,
        );
        facts
            .first()
            .and_then(HitFact::label_zone)
            .expect("the fork line carries a relation")
    };
    assert_eq!(
        zone(
            r#"{"type":"fork-context-ref","parentSessionId":"99999999-8888-4777-8666-555555555555","parentLastUuid":"aaaaaaaa-0000-4000-8000-000000000001"}"#
        ),
        "[for: forked_from @99999999-8888-4777-8666-555555555555 aaaaaaaa (not in this file)]"
    );
    assert_eq!(
        zone(
            r#"{"type":"fork-context-ref","parentSessionId":"99999999-8888-4777-8666-555555555555"}"#
        ),
        "[for: forked_from @99999999-8888-4777-8666-555555555555]",
        "with no record named, the parent session is the whole answer and prints alone"
    );
}

/// A `queue-operation` carrying the person's own text is `user.queued`, not this family's,
/// so `facts()` refuses it: no marker, no relation. A rider on the same line type is the
/// family's and carries the marker.
#[test]
fn the_person_s_queued_text_is_refused_by_the_family() {
    let ctx = ClassifyCtx::top_level();
    let rendered = |line: &str| {
        let record = rec(line);
        family::render(&record, Some(line), Class::BookkeepingQueue, &ctx)
    };
    assert_eq!(
        rendered(r#"{"type":"queue-operation","operation":"enqueue","content":"please continue"}"#),
        None,
        "the human's own queued text is the user family's"
    );
    let own = rendered(r#"{"type":"queue-operation","operation":"dequeue"}"#);
    assert!(
        own.as_deref()
            .is_some_and(|t| t.starts_with("[queue-operation")),
        "a content-less dequeue is the harness's own row: {own:?}"
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
