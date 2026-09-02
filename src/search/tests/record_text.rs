//! Record-text resolution for the v0.9.5 promoted leaves + the duration formatter.

use super::*;

fn rec(line: &str) -> Record {
    serde_json::from_str(line).unwrap()
}

#[test]
fn promoted_text_is_verbatim_for_queued_and_recap() {
    let q = rec(
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"t","sessionId":"s","content":"check the widget build"}"#,
    );
    // Both the promoted path and the shared raw-text entry return the verbatim content
    // (the promoted arm is pinned directly: the D7 system fallback would also read
    // `content`, so only a direct call proves the arm is live).
    assert_eq!(
        promoted_record_text(&q).as_deref(),
        Some("check the widget build")
    );
    assert_eq!(
        record_raw_text(&q).as_deref(),
        Some("check the widget build")
    );
    let a = rec(
        r#"{"type":"system","subtype":"away_summary","content":"Two checks finished.","uuid":"u","timestamp":"t"}"#,
    );
    assert_eq!(
        promoted_record_text(&a).as_deref(),
        Some("Two checks finished.")
    );
    assert_eq!(record_raw_text(&a).as_deref(), Some("Two checks finished."));
    // A non-promoted record is untouched by the promoted path.
    let u = rec(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
    assert_eq!(promoted_record_text(&u), None);
    assert_eq!(record_raw_text(&u).as_deref(), Some("hi"));
}

#[test]
fn turn_duration_excerpt_lists_present_fields_only() {
    let full = rec(
        r#"{"type":"system","subtype":"turn_duration","durationMs":64911,"messageCount":908,"pendingBackgroundAgentCount":2,"uuid":"u","timestamp":"t"}"#,
    );
    assert_eq!(
        promoted_record_text(&full).as_deref(),
        Some("[turn duration: 1m 5s · durationMs=64911 messageCount=908 pendingBackgroundAgentCount=2]")
    );
    let bare = rec(r#"{"type":"system","subtype":"turn_duration","durationMs":1400,"uuid":"u"}"#);
    assert_eq!(
        promoted_record_text(&bare).as_deref(),
        Some("[turn duration: 1s · durationMs=1400]")
    );
    // No numeric field at all: nothing fabricated.
    let empty = rec(r#"{"type":"system","subtype":"turn_duration","uuid":"u"}"#);
    assert_eq!(promoted_record_text(&empty), None);
    // An absurd span (a turn straddling a resume gap) reads as days, not seconds.
    let long =
        rec(r#"{"type":"system","subtype":"turn_duration","durationMs":926676611,"uuid":"u"}"#);
    assert!(
        promoted_record_text(&long)
            .unwrap()
            .starts_with("[turn duration: 10d 17h ·"),
        "{:?}",
        promoted_record_text(&long)
    );
}

#[test]
fn stop_hooks_excerpt_carries_the_ledger_and_commands() {
    let sh = rec(
        r#"{"type":"system","subtype":"stop_hook_summary","hookCount":2,"hookInfos":[{"command":"/x/a.sh","durationMs":9},{"command":"/x/b.sh"}],"hookErrors":[{"command":"/x/b.sh"}],"preventedContinuation":true,"uuid":"u","timestamp":"t"}"#,
    );
    assert_eq!(
        promoted_record_text(&sh).as_deref(),
        Some("[stop hooks: count=2 errors=1 prevented=true]\n/x/a.sh (9ms)\n/x/b.sh")
    );
    // Missing arrays degrade to zero, never a crash.
    let bare = rec(r#"{"type":"system","subtype":"stop_hook_summary","uuid":"u"}"#);
    assert_eq!(
        promoted_record_text(&bare).as_deref(),
        Some("[stop hooks: count=0 errors=0 prevented=false]")
    );
}

#[test]
fn snapshot_and_delta_excerpts_name_paths_and_versions() {
    let snap = rec(
        r#"{"type":"file-history-snapshot","messageId":"m","snapshot":{"messageId":"m","trackedFileBackups":{"/z/b.md":{"version":1},"/a/f.txt":{"version":3,"backupFileName":"f.txt@v3"}},"timestamp":"2026-06-07T05:00:00.000Z"},"isSnapshotUpdate":true}"#,
    );
    assert_eq!(
        promoted_record_text(&snap).as_deref(),
        Some("[file-history snapshot at 2026-06-07T05:00:00.000Z: /a/f.txt@v3, /z/b.md@v1]")
    );
    let delta = rec(
        r#"{"type":"file-history-delta","messageId":"m","trackingPath":"/a/f.txt","backup":{"backupFileName":"f.txt@v4","version":4},"timestamp":"2026-06-07T05:01:00.000Z"}"#,
    );
    assert_eq!(
        promoted_record_text(&delta).as_deref(),
        Some("[file-history delta at 2026-06-07T05:01:00.000Z: /a/f.txt@v4 backup=f.txt@v4]")
    );
    // A snapshot line with no snapshot object has nothing to render.
    let none = rec(r#"{"type":"file-history-snapshot","messageId":"m"}"#);
    assert_eq!(promoted_record_text(&none), None);
    // A delta with no tracking path likewise.
    let none = rec(r#"{"type":"file-history-delta","messageId":"m","timestamp":"t"}"#);
    assert_eq!(promoted_record_text(&none), None);
}

#[test]
fn fmt_ms_picks_the_top_two_units() {
    assert_eq!(fmt_ms(0), "0s");
    assert_eq!(fmt_ms(499), "0s");
    assert_eq!(fmt_ms(999), "1s");
    assert_eq!(fmt_ms(12_000), "12s");
    assert_eq!(fmt_ms(64_911), "1m 5s");
    assert_eq!(fmt_ms(3_600_000), "1h 0m");
    assert_eq!(fmt_ms(7_380_000), "2h 3m");
    assert_eq!(fmt_ms(926_676_611), "10d 17h");
}
