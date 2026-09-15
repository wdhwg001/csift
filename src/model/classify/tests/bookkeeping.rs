//! The bookkeeping family: the 18 non-transcript line types, and the one line type
//! whose leaf depends on its CONTENT rather than its type.

use super::*;
use crate::model::classify::bookkeeping;

#[test]
fn queue_operation_with_human_text_is_user_queued() {
    let l = labels(
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-06-07T05:00:00.000Z","sessionId":"s","content":"check the widget build"}"#,
    );
    assert_eq!(l, vec![Class::UserQueued]);
    // popAll (recalled to the input box) and remove keep the text too.
    for op in ["popAll", "remove"] {
        let l = labels(&format!(
            r#"{{"type":"queue-operation","operation":"{op}","timestamp":"t","sessionId":"s","content":"later"}}"#
        ));
        assert_eq!(l, vec![Class::UserQueued], "{op}");
    }
    // A slash command typed into the queue is still the human.
    let l = labels(
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"t","sessionId":"s","content":"/compact"}"#,
    );
    assert_eq!(l, vec![Class::UserQueued]);
}

#[test]
fn queue_riders_and_blanks_are_the_bookkeeping_leaf() {
    // A content-less dequeue has nothing to search, so it is the harness's own row.
    let l = labels(
        r#"{"type":"queue-operation","operation":"dequeue","timestamp":"t","sessionId":"s"}"#,
    );
    assert_eq!(l, vec![Class::BookkeepingQueue], "{l:?}");
    // Whitespace-only content is a blank, not a queued message.
    let l = labels(
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"t","sessionId":"s","content":"  \n"}"#,
    );
    assert_eq!(l, vec![Class::BookkeepingQueue], "{l:?}");
    // A harness rider (an automation pulse queued behind the human) is NOT the human.
    let l = labels(
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"t","sessionId":"s","content":"<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n<summary>done</summary>\n</task-notification>"}"#,
    );
    assert_eq!(l, vec![Class::BookkeepingQueue], "{l:?}");
    // Nor is a queued peer message.
    let l = labels(
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"t","sessionId":"s","content":"<teammate-message teammate_id=\"lead\">go</teammate-message>"}"#,
    );
    assert_eq!(l, vec![Class::BookkeepingQueue], "{l:?}");
    // Non-string content (odd shape) is tolerated as the bookkeeping row.
    let l = labels(
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"t","sessionId":"s","content":{"x":1}}"#,
    );
    assert_eq!(l, vec![Class::BookkeepingQueue], "{l:?}");
}

#[test]
fn every_bookkeeping_line_type_maps_to_its_leaf() {
    // The hand-kept table of all 18 non-transcript types, each with the leaf the map
    // assigns it. Typed out here rather than read from the code it guards.
    let table: &[(&str, Class)] = &[
        ("last-prompt", Class::BookkeepingLastPrompt),
        ("mode", Class::BookkeepingMode),
        ("permission-mode", Class::BookkeepingMode),
        ("ai-title", Class::BookkeepingName),
        ("agent-name", Class::BookkeepingName),
        ("atis-latch", Class::BookkeepingAtis),
        ("bridge-session", Class::BookkeepingBridge),
        ("history-suppression", Class::BookkeepingSuppression),
        ("pr-link", Class::BookkeepingPrLink),
        ("frame-link", Class::BookkeepingArtifact),
        ("artifact-comment-monitor", Class::BookkeepingArtifact),
        ("artifact-autoreact-ledger", Class::BookkeepingArtifact),
        ("file-history-snapshot", Class::BookkeepingFileHistory),
        ("file-history-delta", Class::BookkeepingFileHistory),
        ("cost-state", Class::BookkeepingCost),
        ("fork-context-ref", Class::BookkeepingFork),
        ("continued-in", Class::BookkeepingContinued),
        ("queue-operation", Class::BookkeepingQueue),
    ];
    assert_eq!(table.len(), 18, "the line-type table is not 18 rows");
    for &(ty, leaf) in table {
        assert_eq!(bookkeeping::classify(ty), Some(leaf), "{ty}");
        let line = format!(r#"{{"type":"{ty}","sessionId":"s"}}"#);
        assert_eq!(labels(&line), vec![leaf], "{line}");
    }
    // A type csift models no leaf for stays unlabeled - an address renders it as an
    // unmodeled unit rather than borrowing a neighbouring leaf's name.
    assert_eq!(bookkeeping::classify("some-future-line"), None);
    assert!(labels(r#"{"type":"some-future-line","sessionId":"s"}"#).is_empty());
}

#[test]
fn file_history_lines_carry_their_payload_shapes() {
    let snap = labels(
        r#"{"type":"file-history-snapshot","messageId":"m","snapshot":{"messageId":"m","trackedFileBackups":{"/x/f.txt":{"version":3,"backupFileName":"f.txt@v3","backupTime":"t"}},"timestamp":"t"},"isSnapshotUpdate":true}"#,
    );
    assert_eq!(snap, vec![Class::BookkeepingFileHistory]);
    let delta = labels(
        r#"{"type":"file-history-delta","messageId":"m","snapshotMessageId":"m","trackingPath":"/x/f.txt","backup":{"backupFileName":"f.txt@v4","version":4,"backupTime":"t"},"timestamp":"t"}"#,
    );
    assert_eq!(delta, vec![Class::BookkeepingFileHistory]);
}

#[test]
fn every_bookkeeping_leaf_is_invisible_and_harness_roled() {
    for &c in Class::ALL {
        if c.category() != "harness.bookkeeping" {
            continue;
        }
        assert!(!c.llm_visible(), "{} must be invisible", c.path());
        assert_eq!(c.role(), Role::Harness, "{}", c.path());
    }
    // `user.queued` sits in the USER role although its line is bookkeeping: the text
    // is the person's own, which is why the content decides the leaf.
    assert_eq!(Class::UserQueued.role(), Role::User);
    assert!(!Class::UserQueued.llm_visible());
}

#[test]
fn u64_field_is_tolerant() {
    let v = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
    assert_eq!(Record::u64_field(Some(&v("42"))), Some(42));
    assert_eq!(Record::u64_field(Some(&v("42.0"))), Some(42));
    assert_eq!(Record::u64_field(Some(&v("42.5"))), None);
    assert_eq!(Record::u64_field(Some(&v("-1"))), None);
    assert_eq!(Record::u64_field(Some(&v("\"42\""))), None);
    assert_eq!(Record::u64_field(None), None);
}
