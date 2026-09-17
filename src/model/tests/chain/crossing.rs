//! The CROSSING BUDGET: what the walk does at a `compact_boundary` under each budget, and
//! what it then says about the boundaries it did not step over.
//!
//! Every fixture here carries `compactMetadata` WITHOUT a preserved set, so the pre-walk
//! relink aborts and the only thing under test is the walk itself. A boundary carrying a
//! preserved set is the relink's own subject and is pinned in `chain_relink.rs`.
//!
//! The root holds the fixtures the two halves share; `walk` pins what the walk DOES at each
//! budget and `report` what the disclosure SAYS about it.

use super::*;

mod report;
mod walk;

/// `u0 -> a0` under a boundary that names `a0` as its logical parent, with a second branch
/// (`u2 -> a2`) hanging off `u0` that the conversation was rewound past.
fn rewound_above_a_boundary() -> Vec<Record> {
    recs(&[
        &u("u0", "", "chart the lagoon", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "charting", "2026-06-07T05:00:05.000Z"),
        &u("u2", "u0", "survey the shoal", "2026-06-07T05:00:10.000Z"),
        &a("a2", "u2", "surveying", "2026-06-07T05:00:15.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"a0","timestamp":"2026-06-07T05:10:00.000Z","compactMetadata":{"trigger":"auto","preTokens":900}}"#,
        r#"{"type":"user","uuid":"s1","parentUuid":"b1","isCompactSummary":true,"timestamp":"2026-06-07T05:10:01.000Z","message":{"role":"user","content":"summary"}}"#,
        &u("u1", "s1", "dredge the channel", "2026-06-07T05:11:00.000Z"),
        &a("a1", "u1", "dredging", "2026-06-07T05:11:05.000Z"),
    ])
}

/// Five plain boundaries stacked, each naming the assistant record just above it.
fn five_boundaries() -> Vec<Record> {
    let mut lines: Vec<String> = vec![
        u("u0", "", "prompt 0", "2026-06-07T05:00:00.000Z"),
        a("a0", "u0", "reply 0", "2026-06-07T05:00:05.000Z"),
    ];
    for k in 1..=5 {
        lines.push(format!(
            r#"{{"type":"system","subtype":"compact_boundary","uuid":"b{k}","parentUuid":null,"logicalParentUuid":"a{}","timestamp":"2026-06-07T06:0{k}:00.000Z","compactMetadata":{{"trigger":"auto"}}}}"#,
            k - 1
        ));
        lines.push(format!(
            r#"{{"type":"user","uuid":"s{k}","parentUuid":"b{k}","isCompactSummary":true,"timestamp":"2026-06-07T06:0{k}:01.000Z","message":{{"role":"user","content":"summary {k}"}}}}"#
        ));
        lines.push(u(
            &format!("u{k}"),
            &format!("s{k}"),
            "prompt",
            &format!("2026-06-07T06:0{k}:02.000Z"),
        ));
        lines.push(a(
            &format!("a{k}"),
            &format!("u{k}"),
            "reply",
            &format!("2026-06-07T06:0{k}:03.000Z"),
        ));
    }
    recs(&lines.iter().map(String::as_str).collect::<Vec<_>>())
}

/// The landmark's own shape: two boundaries the walk steps over, then a third whose logical
/// parent is gone. `b3` is the newest and is crossed, `b2` likewise, `b1` is the floor.
fn two_crossed_then_a_dangling_floor() -> Vec<Record> {
    recs(&[
        &u("u0", "", "prompt 0", "2026-06-07T05:00:00.000Z"),
        &a("a0", "u0", "reply 0", "2026-06-07T05:00:05.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"logicalParentUuid":"gone-from-disk","timestamp":"2026-06-07T06:01:00.000Z","compactMetadata":{"trigger":"auto"}}"#,
        r#"{"type":"user","uuid":"s1","parentUuid":"b1","isCompactSummary":true,"timestamp":"2026-06-07T06:01:01.000Z","message":{"role":"user","content":"summary 1"}}"#,
        &u("u1", "s1", "prompt 1", "2026-06-07T06:01:02.000Z"),
        &a("a1", "u1", "reply 1", "2026-06-07T06:01:03.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b2","parentUuid":null,"logicalParentUuid":"a1","timestamp":"2026-06-07T06:02:00.000Z","compactMetadata":{"trigger":"auto"}}"#,
        r#"{"type":"user","uuid":"s2","parentUuid":"b2","isCompactSummary":true,"timestamp":"2026-06-07T06:02:01.000Z","message":{"role":"user","content":"summary 2"}}"#,
        &u("u2", "s2", "prompt 2", "2026-06-07T06:02:02.000Z"),
        &a("a2", "u2", "reply 2", "2026-06-07T06:02:03.000Z"),
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b3","parentUuid":null,"logicalParentUuid":"a2","timestamp":"2026-06-07T06:03:00.000Z","compactMetadata":{"trigger":"auto"}}"#,
        r#"{"type":"user","uuid":"s3","parentUuid":"b3","isCompactSummary":true,"timestamp":"2026-06-07T06:03:01.000Z","message":{"role":"user","content":"summary 3"}}"#,
        &u("u3", "s3", "prompt 3", "2026-06-07T06:03:02.000Z"),
        &a("a3", "u3", "reply 3", "2026-06-07T06:03:03.000Z"),
    ])
}
