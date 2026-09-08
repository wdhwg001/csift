//! The spine walk: which occurrence of a key it reads, what it refuses to read, and the
//! agreement between its two entries.
//!
//! `spine_record` is the byte walk the search path takes (no validating pass to ride on);
//! `line_type_and_spine` lifts the same fields out of the validating parse `stats` already
//! runs. They must agree field for field, or one surface would number turns differently
//! from another - so the agreement is pinned here rather than assumed.

use super::*;

/// An attachment record whose payload can be made to carry whatever the caller wants to
/// hide inside it, at a length that is representative of a real one.
fn attachment_line(inner: &str) -> String {
    let filler = "x".repeat(2000);
    format!(
        concat!(
            r#"{{"parentUuid":"p-1","isSidechain":false,"attachment":{{"type":"edited_text_file",{}"#,
            r#""snippet":"{}"}},"type":"attachment","uuid":"u-1","#,
            r#""timestamp":"2026-06-07T05:00:00.000Z","cwd":"/w","version":"2.1.258"}}"#
        ),
        inner, filler
    )
}

#[test]
fn a_payload_uuid_never_shadows_the_record_own_uuid() {
    let line = attachment_line(r#""uuid":"NOT-THE-RECORD","#);
    let rec = spine_record(line.as_bytes()).expect("a spine row");
    assert_eq!(rec.uuid.as_deref(), Some("u-1"));
    assert_eq!(rec.parent_uuid.as_deref(), Some("p-1"));
    assert_eq!(rec.r#type.as_deref(), Some("attachment"));
    assert_eq!(rec.timestamp.as_deref(), Some("2026-06-07T05:00:00.000Z"));
    assert_eq!(rec.is_sidechain, Some(false));
}

#[test]
fn a_payload_type_written_after_the_record_own_type_never_wins() {
    // The real shape this guards, and the reason a reader must not take the LAST
    // occurrence of a key token on faith: five system records in a real corpus carry
    // `"type":"overloaded_error"` inside an api_error payload, later in the line than the
    // record's own top-level `type` (claim REC-104).
    let filler = "y".repeat(2000);
    let line = format!(
        concat!(
            r#"{{"parentUuid":"p-2","isSidechain":false,"type":"system","subtype":"api_error","#,
            r#""content":{{"pad":"{}","error":{{"type":"overloaded_error"}},"uuid":"NOPE"}},"#,
            r#""timestamp":"2026-06-07T05:01:00.000Z","uuid":"u-2"}}"#
        ),
        filler
    );
    let rec = spine_record(line.as_bytes()).expect("a spine row");
    assert_eq!(rec.r#type.as_deref(), Some("system"));
    assert_eq!(rec.subtype.as_deref(), Some("api_error"));
    assert_eq!(rec.uuid.as_deref(), Some("u-2"));
    assert_eq!(rec.timestamp.as_deref(), Some("2026-06-07T05:01:00.000Z"));
}

#[test]
fn a_line_of_an_unwanted_type_yields_no_spine_row() {
    let filler = "z".repeat(4000);
    let line = format!(
        r#"{{"parentUuid":null,"snapshot":{{"blob":"{filler}"}},"type":"file-history-snapshot","uuid":"s-1"}}"#
    );
    assert!(spine_record(line.as_bytes()).is_none());
}

#[test]
fn a_reserialized_line_reads_the_same_as_the_compact_one() {
    let compact = attachment_line("");
    let value: serde_json::Value = serde_json::from_str(&compact).expect("valid");
    let spaced = serde_json::to_string_pretty(&value).expect("re-serializable");
    let a = spine_record(compact.as_bytes()).expect("compact");
    let b = spine_record(spaced.as_bytes()).expect("spaced");
    assert_eq!(a.uuid, b.uuid);
    assert_eq!(a.parent_uuid, b.parent_uuid);
    assert_eq!(a.timestamp, b.timestamp);
    assert_eq!(a.r#type, b.r#type);
}

#[test]
fn a_torn_line_is_no_record() {
    let filler = "q".repeat(2000);
    let line = format!(r#"{{"parentUuid":"p","attachment":{{"snippet":"{filler}"#);
    assert!(spine_record(line.as_bytes()).is_none());
}

/// Every field the walk reads, in a shape a real transcript produces.
const FULL: &str = r#"{"parentUuid":"p-1","isSidechain":false,"type":"attachment","uuid":"u-1","timestamp":"2026-06-07T05:00:00.000Z","attachment":{"type":"hook_additional_context","content":["ctx"]}}"#;
const BOUNDARY: &str = r#"{"type":"system","subtype":"compact_boundary","uuid":"b-1","parentUuid":null,"logicalParentUuid":"a-9","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preTokens":900}}"#;
const LAST_PROMPT: &str =
    r#"{"type":"last-prompt","leafUuid":"a-9","explicit":true,"rewound":false,"lastPrompt":"go"}"#;
const UNWANTED: &str = r#"{"type":"file-history-snapshot","messageId":"m-1"}"#;
const TYPELESS: &str = r#"{"uuid":"u-2"}"#;
/// A reserialized line puts whitespace between a value and its delimiter. The byte walk's
/// span runs to that delimiter, so the bools have to be trimmed or the two entries disagree
/// on exactly the fields the leaf choice reads.
const SPACED_BOOL: &str = r#"{"type":"last-prompt","leafUuid":"a-9","explicit":true ,"rewound":false ,"lastPrompt":"go"}"#;
/// A duplicate top-level key. Unreachable on the harness's output - one writer, one key
/// each - so the two entries are allowed to differ here, and this pins WHICH way.
const DUPLICATE_KEY: &str = r#"{"type":"user","uuid":"u-1","uuid":"u-2","parentUuid":"p-1"}"#;

fn both(line: &str) -> (Option<Record>, String, Option<Record>) {
    let walked = spine_record(line.as_bytes());
    let (census, probed) = line_type_and_spine(line.as_bytes())
        .expect("valid json")
        .expect("non-blank");
    (walked, census, probed)
}

fn same(a: Option<&Record>, b: Option<&Record>) {
    match (a, b) {
        (None, None) => {}
        (Some(x), Some(y)) => {
            assert_eq!(x.r#type, y.r#type);
            assert_eq!(x.subtype, y.subtype);
            assert_eq!(x.uuid, y.uuid);
            assert_eq!(x.parent_uuid, y.parent_uuid);
            assert_eq!(x.logical_parent_uuid, y.logical_parent_uuid);
            assert_eq!(x.leaf_uuid, y.leaf_uuid);
            assert_eq!(x.timestamp, y.timestamp);
            assert_eq!(x.is_sidechain, y.is_sidechain);
            assert_eq!(x.explicit, y.explicit);
            assert_eq!(x.rewound, y.rewound);
            assert_eq!(x.compact_metadata, y.compact_metadata);
        }
        _ => panic!("one extractor produced a spine row and the other did not"),
    }
}

#[test]
fn the_two_extractors_agree_on_every_admitted_shape() {
    for line in [FULL, BOUNDARY, LAST_PROMPT, UNWANTED, TYPELESS, SPACED_BOOL] {
        let (walked, _, probed) = both(line);
        same(walked.as_ref(), probed.as_ref());
    }
}

#[test]
fn the_probe_reports_the_census_type_and_gates_the_spine_on_it() {
    let (_, census, spine) = both(FULL);
    assert_eq!(census, "attachment");
    let s = spine.expect("an attachment is on the chain");
    assert_eq!(s.uuid.as_deref(), Some("u-1"));
    assert_eq!(s.parent_uuid.as_deref(), Some("p-1"));
    assert_eq!(s.is_sidechain, Some(false));
    // The payload is never lifted: a spine row carries structure and nothing else.
    assert!(s.message.is_none());

    let (_, census, spine) = both(UNWANTED);
    assert_eq!(census, "file-history-snapshot", "censused all the same");
    assert!(
        spine.is_none(),
        "a line the loader never admits gets no row"
    );

    let (_, census, spine) = both(TYPELESS);
    assert_eq!(census, "(untyped)");
    assert!(spine.is_none());

    let (_, _, spine) = both(BOUNDARY);
    let s = spine.expect("a boundary is the chain's cut");
    assert_eq!(s.logical_parent_uuid.as_deref(), Some("a-9"));
    assert!(s.compact_metadata.is_some());
    assert!(s.parent_uuid.is_none(), "an explicit null reads as absent");
}

#[test]
fn a_value_of_an_unexpected_json_type_reads_as_absent_in_both() {
    // Tolerance is the point: a field carrying the wrong JSON type must not make the line
    // malformed, or the census would over-report corruption.
    let line = r#"{"type":"user","uuid":7,"parentUuid":"p-1","isSidechain":"no"}"#;
    let (walked, census, probed) = both(line);
    assert_eq!(census, "user");
    same(walked.as_ref(), probed.as_ref());
    let s = probed.expect("still a chain row");
    assert!(s.uuid.is_none());
    assert!(s.is_sidechain.is_none());
    assert_eq!(s.parent_uuid.as_deref(), Some("p-1"));
}

#[test]
fn a_torn_interior_is_malformed_for_the_probe_and_no_row_for_the_walk() {
    // The exact-census contract: `{…}`-framed but invalid inside still counts.
    let line = br#"{"type":"attachment","uuid":"u-1","attachment":{"conte"#;
    assert!(line_type_and_spine(line).is_err());
    assert!(spine_record(line).is_none());
    assert!(matches!(line_type_and_spine(b"   "), Ok(None)));
}

#[test]
fn a_bool_followed_by_whitespace_reads_the_same_in_both() {
    // The span the byte walk cuts ends at the delimiter, so an untrimmed `true ` would
    // read as absent on that side while serde reports `Some(true)` - and `explicit` /
    // `rewound` steer which leaf the chain walks from.
    let (walked, census, probed) = both(SPACED_BOOL);
    assert_eq!(census, "last-prompt");
    same(walked.as_ref(), probed.as_ref());
    let w = walked.expect("a last-prompt row");
    assert_eq!(w.explicit, Some(true));
    assert_eq!(w.rewound, Some(false));
    assert_eq!(w.leaf_uuid.as_deref(), Some("a-9"));
}

#[test]
fn a_duplicate_top_level_key_parts_the_two_entries_and_the_census_says_malformed() {
    // The ONE shape where they legitimately differ, asserted rather than assumed. serde's
    // derive rejects a repeated known field, so the census counts the line malformed and
    // builds no row; the byte walk has no such rule and keeps the LAST occurrence. Both
    // readings are defensible on a shape the harness never writes.
    assert!(line_type_and_spine(DUPLICATE_KEY.as_bytes()).is_err());
    let w = spine_record(DUPLICATE_KEY.as_bytes()).expect("the walk still builds a row");
    assert_eq!(w.uuid.as_deref(), Some("u-2"), "the last occurrence wins");
    assert_eq!(w.parent_uuid.as_deref(), Some("p-1"));
}
