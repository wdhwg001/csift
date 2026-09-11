//! The tail checkpoint reader and the evidence it folds into an assessment.

use super::*;

fn tmp_dir(stem: &str) -> std::path::PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "csift-cp-{}-{}-{stem}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

const CP: &str = r#"{"type":"cost-state","sessionId":"s","totalCostUSD":0.1,"totalDuration":1000,"startTime":1780808940000,"modelUsage":{},"hasUnknownModelCost":false}"#;

#[test]
fn last_checkpoint_counts_the_physical_line_and_only_at_the_tail() {
    let dir = tmp_dir("tail");
    let p = dir.join("t.jsonl");

    // A checkpoint as the only line is line 1.
    std::fs::write(&p, format!("{CP}\n")).unwrap();
    assert_eq!(last_checkpoint(&p).unwrap().map(|c| c.line), Some(1));

    // Below other lines it carries their count; trailing blank lines do not shift it.
    let filler = r#"{"type":"user","uuid":"u","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"x"}}"#;
    std::fs::write(&p, format!("{filler}\n{filler}\n{CP}\n\n")).unwrap();
    let cp = last_checkpoint(&p).unwrap().expect("a tail checkpoint");
    assert_eq!(cp.line, 3);
    assert_eq!(cp.kind, "cost-state");

    // A line BELOW it means the harness kept appending: not a tail checkpoint.
    std::fs::write(&p, format!("{CP}\n{filler}\n")).unwrap();
    assert!(last_checkpoint(&p).unwrap().is_none());

    // A line of any other type at the tail is never one, however cost-like it reads.
    let decoy = r#"{"type":"user","uuid":"u","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"cost-state totalDuration"}}"#;
    std::fs::write(&p, format!("{decoy}\n")).unwrap();
    assert!(last_checkpoint(&p).unwrap().is_none());

    // An empty file answers nothing rather than fabricating line 1.
    std::fs::write(&p, "").unwrap();
    assert!(last_checkpoint(&p).unwrap().is_none());

    // The line number is counted over the WHOLE file, not the tail window.
    let mut big = String::new();
    for _ in 0..40_000 {
        big.push_str(filler);
        big.push('\n');
    }
    big.push_str(CP);
    big.push('\n');
    std::fs::write(&p, &big).unwrap();
    assert_eq!(last_checkpoint(&p).unwrap().map(|c| c.line), Some(40_001));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn attach_checkpoint_adds_evidence_under_the_tail_row_and_never_a_verdict() {
    let tail = TailShape {
        unreturned_use: None,
        last_stop_reason: Some("end_turn".to_string()),
        last_ts_utc: Some("2026-06-07T05:00:05.000Z".to_string()),
        records_seen: 3,
    };
    // A registry row FIRST, so the tail row is not the head of the list: the checkpoint
    // row has to land under the tail row specifically, not merely somewhere after the
    // first row.
    let reg = RegistryRow {
        pid: Some(7),
        status: Some("idle".to_string()),
        status_updated_at_ms: Some(jiff::Timestamp::now().as_millisecond()),
        proc_start: None,
        pid_domain: None,
        started_at_ms: None,
    };
    let mut a = assess(
        Some(&reg),
        None,
        &tail,
        &ChildrenReport::default(),
        &[],
        false,
    );
    let before = a.verdict;
    let tail_at = a
        .evidence
        .iter()
        .position(|e| e.surface == "tail")
        .expect("a tail row");
    assert!(tail_at > 0, "the registry row leads: {:?}", a.evidence);
    a.attach_checkpoint(Some(CheckpointTail {
        line: 42,
        kind: "cost-state",
    }));
    assert_eq!(a.verdict, before, "a checkpoint never moves the verdict");
    assert_eq!(
        a.evidence[tail_at + 1].surface,
        "checkpoint",
        "the row sits directly under the tail row it qualifies"
    );
    assert!(a.evidence[tail_at + 1]
        .value
        .starts_with("cost-state at L42"));
    assert_eq!(a.last_checkpoint.map(|c| c.line), Some(42));
    // The clause belongs to the no-registry-row note, and this session HAS a row.
    assert_eq!(
        a.notes
            .iter()
            .filter(|n| n.contains("the harness wrote its checkpoint"))
            .count(),
        0,
        "notes: {:?}",
        a.notes
    );
}

#[test]
fn attach_checkpoint_extends_the_no_registry_note_exactly_once() {
    let tail = TailShape {
        unreturned_use: None,
        last_stop_reason: Some("end_turn".to_string()),
        last_ts_utc: Some("2026-06-07T05:00:05.000Z".to_string()),
        records_seen: 3,
    };
    let mut a = assess(None, None, &tail, &ChildrenReport::default(), &[], false);
    a.attach_checkpoint(Some(CheckpointTail {
        line: 9,
        kind: "cost-state",
    }));
    let extended: Vec<&String> = a
        .notes
        .iter()
        .filter(|n| n.contains("the harness wrote its checkpoint"))
        .collect();
    assert_eq!(extended.len(), 1, "notes: {:?}", a.notes);
    assert!(
        extended[0].starts_with("no registry row for this session")
            && extended[0].ends_with("so the session closed or was handed over"),
        "the clause extends that note rather than standing alone: {}",
        extended[0]
    );
}

#[test]
fn attach_checkpoint_is_a_no_op_without_one() {
    let tail = TailShape {
        unreturned_use: None,
        last_stop_reason: Some("end_turn".to_string()),
        last_ts_utc: Some("2026-06-07T05:00:05.000Z".to_string()),
        records_seen: 3,
    };
    let mut a = assess(None, None, &tail, &ChildrenReport::default(), &[], false);
    let rows = a.evidence.len();
    let notes = a.notes.clone();
    a.attach_checkpoint(None);
    assert_eq!(a.evidence.len(), rows);
    assert_eq!(a.notes, notes);
    assert!(a.last_checkpoint.is_none());
}

#[test]
fn team_candidates_take_only_the_files_written_at_that_startup() {
    let dir = tmp_dir("teams");
    let write = |name: &str, created: i64| {
        let d = dir.join("teams").join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("config.json"),
            format!(r#"{{"name":"{name}","createdAt":{created},"members":[]}}"#),
        )
        .unwrap();
    };
    let start = 1_780_000_000_000i64;
    write("session-aaaaaaaa", start - 4999);
    write("session-bbbbbbbb", start + 5000);
    write("session-cccccccc", start - 5001);
    write("session-dddddddd", start + 60_000);
    // A malformed config and one with no name are skipped, never fatal.
    std::fs::create_dir_all(dir.join("teams").join("session-eeeeeeee")).unwrap();
    std::fs::write(
        dir.join("teams")
            .join("session-eeeeeeee")
            .join("config.json"),
        "{broken",
    )
    .unwrap();

    let got: Vec<String> = team_candidates(&dir, start)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(
        got,
        vec![
            "session-aaaaaaaa".to_string(),
            "session-bbbbbbbb".to_string()
        ],
        "the window is inclusive on both sides and nothing outside it competes"
    );
    std::fs::remove_dir_all(&dir).ok();
}
