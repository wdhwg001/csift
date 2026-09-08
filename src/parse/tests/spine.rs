//! The spine walk: which occurrence of a key it reads, and what it refuses to read.

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
