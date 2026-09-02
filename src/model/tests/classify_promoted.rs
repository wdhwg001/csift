//! v0.10.0 promoted non-record lines: one invisible leaf each, riders and blanks excluded.

use super::*;

fn labels(line: &str) -> Vec<Class> {
    parse(line).classify(&ClassifyCtx::top_level())
}

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
fn queue_riders_and_blanks_carry_no_label() {
    // A content-less dequeue has nothing to search.
    let l = labels(
        r#"{"type":"queue-operation","operation":"dequeue","timestamp":"t","sessionId":"s"}"#,
    );
    assert!(l.is_empty(), "{l:?}");
    // Whitespace-only content is a blank, not a queued message.
    let l = labels(
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"t","sessionId":"s","content":"  \n"}"#,
    );
    assert!(l.is_empty(), "{l:?}");
    // A harness rider (an automation pulse queued behind the human) is NOT the human.
    let l = labels(
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"t","sessionId":"s","content":"<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n<summary>done</summary>\n</task-notification>"}"#,
    );
    assert!(l.is_empty(), "{l:?}");
    // Nor is a queued peer message.
    let l = labels(
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"t","sessionId":"s","content":"<teammate-message teammate_id=\"lead\">go</teammate-message>"}"#,
    );
    assert!(l.is_empty(), "{l:?}");
    // Non-string content (odd shape) is tolerated as no label.
    let l = labels(
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"t","sessionId":"s","content":{"x":1}}"#,
    );
    assert!(l.is_empty(), "{l:?}");
}

#[test]
fn system_subtypes_map_to_their_meta_leaves() {
    let td = labels(
        r#"{"type":"system","subtype":"turn_duration","durationMs":64911,"messageCount":908,"uuid":"u","timestamp":"t"}"#,
    );
    assert_eq!(td, vec![Class::MetaTurnDuration]);
    let aw = labels(
        r#"{"type":"system","subtype":"away_summary","content":"Two checks finished.","uuid":"u","timestamp":"t"}"#,
    );
    assert_eq!(aw, vec![Class::MetaAwaySummary]);
    let sh = labels(
        r#"{"type":"system","subtype":"stop_hook_summary","hookCount":1,"hookInfos":[{"command":"/x/h.sh","durationMs":9}],"uuid":"u","timestamp":"t"}"#,
    );
    assert_eq!(sh, vec![Class::MetaStopHooks]);
    // The boundary keeps its own leaf; every other subtype is the v0.10.1 catch-all
    // (`harness.meta.system`); a subtype-less system line stays unlabeled.
    let cb = labels(
        r#"{"type":"system","subtype":"compact_boundary","uuid":"u","timestamp":"t","compactMetadata":{"trigger":"auto"}}"#,
    );
    assert_eq!(cb, vec![Class::CompactionBoundary]);
    assert_eq!(
        labels(r#"{"type":"system","subtype":"agents_killed","uuid":"u"}"#),
        vec![Class::MetaSystem]
    );
    assert!(labels(r#"{"type":"system","uuid":"u"}"#).is_empty());
}

#[test]
fn file_history_lines_are_meta_snapshot() {
    let snap = labels(
        r#"{"type":"file-history-snapshot","messageId":"m","snapshot":{"messageId":"m","trackedFileBackups":{"/x/f.txt":{"version":3,"backupFileName":"f.txt@v3","backupTime":"t"}},"timestamp":"t"},"isSnapshotUpdate":true}"#,
    );
    assert_eq!(snap, vec![Class::MetaSnapshot]);
    let delta = labels(
        r#"{"type":"file-history-delta","messageId":"m","snapshotMessageId":"m","trackingPath":"/x/f.txt","backup":{"backupFileName":"f.txt@v4","version":4,"backupTime":"t"},"timestamp":"t"}"#,
    );
    assert_eq!(delta, vec![Class::MetaSnapshot]);
}

#[test]
fn promoted_leaves_are_all_llm_invisible_and_role_correct() {
    for c in [
        Class::UserQueued,
        Class::MetaTurnDuration,
        Class::MetaAwaySummary,
        Class::MetaStopHooks,
        Class::MetaSnapshot,
    ] {
        assert!(!c.llm_visible(), "{} must be invisible", c.path());
    }
    assert_eq!(Class::UserQueued.role(), Role::User);
    assert_eq!(Class::MetaSnapshot.role(), Role::Harness);
    // The session-state cache lines stay unmodeled (no uuid, no timestamp, no message).
    for line in [
        r#"{"type":"last-prompt","leafUuid":"x","sessionId":"s","lastPrompt":"do it"}"#,
        r#"{"type":"mode","mode":"normal","sessionId":"s"}"#,
        r#"{"type":"cost-state","totalCostUSD":1.0,"sessionId":"s"}"#,
    ] {
        assert!(labels(line).is_empty(), "{line}");
    }
}

#[test]
fn meta_system_is_the_catch_all_for_every_other_system_subtype() {
    // The Remote Control disconnect notice (CC 2.1.258): a `system`/`informational`
    // record with a level and a string content - the catch-all leaf, invisible.
    let info = r#"{"type":"system","subtype":"informational","level":"warning","uuid":"i1","timestamp":"2026-06-07T05:00:01.000Z","content":"Remote Control disconnected - run /remote-control"}"#;
    assert_eq!(labels(info), vec![Class::MetaSystem], "{info}");
    let rec: Record = serde_json::from_str(info).unwrap();
    assert_eq!(rec.promoted_class(), Some(Class::MetaSystem));
    assert_eq!(rec.level.as_deref(), Some("warning"));
    assert!(!Class::MetaSystem.llm_visible());
    assert_eq!(Class::MetaSystem.role(), Role::Harness);
    assert_eq!(Class::MetaSystem.path(), "harness.meta.system");
    // Every unmodeled subtype lands here, whatever its content shape.
    for sub in [
        "api_error",
        "model_refusal_fallback",
        "model_refusal_no_fallback",
        "agents_killed",
        "local_command",
        "scheduled_task_fire",
        "some_future_subtype",
    ] {
        let line = format!(
            r#"{{"type":"system","subtype":"{sub}","uuid":"x","timestamp":"2026-06-07T05:00:01.000Z","content":{{"k":1}}}}"#
        );
        assert_eq!(labels(&line), vec![Class::MetaSystem], "{line}");
    }
    // The modeled subtypes keep their own leaves; the boundary is never the catch-all.
    let boundary = r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","timestamp":"2026-06-07T05:00:03.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"auto"}}"#;
    let rec: Record = serde_json::from_str(boundary).unwrap();
    assert_eq!(rec.promoted_class(), None);
    assert!(!labels(boundary).contains(&Class::MetaSystem), "{boundary}");
    let td = r#"{"type":"system","subtype":"turn_duration","uuid":"t1","timestamp":"2026-06-07T05:00:03.000Z","durationMs":5}"#;
    assert_eq!(labels(td), vec![Class::MetaTurnDuration]);
    // A system record with NO subtype stays unmodeled (nothing to name it by).
    let bare =
        r#"{"type":"system","uuid":"z","timestamp":"2026-06-07T05:00:03.000Z","content":"x"}"#;
    assert!(labels(bare).is_empty(), "{bare}");
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
