//! Mutation extraction from records: structured tools, bash heuristics, scan.

use super::*;
// ── scan_one_file branch coverage (mmap-None, skipped-line counting) ──
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

#[test]
fn mutations_in_records_carrier_join_backfill_and_bash() {
    // A structured Write + its create carrier (is_create true joined by tool_use_id),
    // and a Bash `touch` (the heuristic arm). Covers the carrier-join + bash branches
    // of mutations_in_records (the shared subagent-topology extractor).
    let recs = vec![
        rec(
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/tmp/new.md","content":"x"}}]}}"#,
        ),
        rec(
            r#"{"type":"user","uuid":"c0","parentUuid":"a0","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"type":"create","filePath":"/tmp/new.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#,
        ),
        rec(
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"touch /tmp/bashed.txt"}}]}}"#,
        ),
    ];
    let muts = mutations_in_records(&recs);
    let new_md = muts
        .iter()
        .find(|m| m.path == "/tmp/new.md")
        .expect("Write surfaced");
    assert!(
        new_md.is_create,
        "the create carrier joined → is_create true"
    );
    let bashed = muts
        .iter()
        .find(|m| m.path == "/tmp/bashed.txt")
        .expect("Bash mutation surfaced");
    assert_eq!(bashed.op, FileOp::BashMutation);
    assert!(bashed.is_create, "touch is a create verb");
}

#[test]
fn mutations_in_records_excludes_cancelled_and_errored_writes() {
    // A Write whose RESULT is `is_error:true` (a failed Edit, or a `Cancelled: parallel tool
    // call … errored` when a sibling op in the same batch failed) NEVER landed → it must not
    // be counted as a mutation (the `files`↔`recover` consistency fix; recover already
    // excludes it via the same failed-id gate). A successful Write is still counted.
    let recs = vec![
        // turn 0: a SUCCESSFUL Write (create carrier, no error).
        rec(
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"wok","name":"Write","input":{"file_path":"/tmp/good.md","content":"real"}}]}}"#,
        ),
        rec(
            r#"{"type":"user","uuid":"c0","parentUuid":"a0","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"type":"create","filePath":"/tmp/good.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"wok","content":"ok"}]}}"#,
        ),
        // turn 1: a CANCELLED Write — its result is_error:true.
        rec(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        ),
        rec(
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"wbad","name":"Write","input":{"file_path":"/tmp/bad.md","content":"never landed"}}]}}"#,
        ),
        rec(
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T06:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"wbad","is_error":true,"content":"<tool_use_error>Cancelled: parallel tool call Bash(...) errored</tool_use_error>"}]}}"#,
        ),
    ];
    let muts = mutations_in_records(&recs);
    assert!(
        muts.iter().any(|m| m.path == "/tmp/good.md"),
        "successful write is still counted"
    );
    assert!(
        !muts.iter().any(|m| m.path == "/tmp/bad.md"),
        "cancelled/errored write is NOT counted: {muts:?}"
    );
}

#[test]
fn bash_verb_is_create_classification() {
    // Fresh-target verbs (incl. the new ln/install/rsync) are creates.
    for v in [
        "mkdir",
        "touch",
        "tee",
        ">",
        "cp",
        "mv",
        "install",
        "ln",
        "rsync",
        "curl",
        "wget",
        "dd",
        "zip",
        "tar",
        "flag-output",
    ] {
        assert!(bash_verb_is_create(v), "{v} should be a create");
    }
    // Append / delete / in-place / source / git are NOT creates (`tee-a` is the
    // non-truncating `tee --append`, the `>>` analogue of `>`).
    for v in [">>", "tee-a", "rm", "sed-i", "mv-from", "git", "unknown"] {
        assert!(!bash_verb_is_create(v), "{v} should NOT be a create");
    }
}

#[test]
fn turn_index_assigned_and_meta_not_a_delimiter() {
    let muts = extract(&fixture());
    // turn 0 carries the three structured edits; turn 1 the bash + multiedit.
    let turn0: Vec<&str> = muts
        .iter()
        .filter(|m| m.turn_index == 0)
        .map(|m| m.mutation.path.as_str())
        .collect();
    assert!(turn0.contains(&"/tmp/a.md"));
    assert!(turn0.contains(&"/tmp/b.md"));
    assert!(turn0.contains(&"/p/spec/gaps.md"));
    let turn1: Vec<&str> = muts
        .iter()
        .filter(|m| m.turn_index == 1)
        .map(|m| m.mutation.path.as_str())
        .collect();
    assert!(turn1.contains(&"/p/src/lib.rs"), "multiedit in turn 1");
    assert!(turn1.contains(&"/tmp/a.md"), "bash rm in turn 1");
    // No mutation is attributed to a turn index >= 2 (the isMeta did not open one).
    assert!(muts.iter().all(|m| m.turn_index <= 1));
}

#[test]
fn create_join_marks_writes_as_create_edits_as_not() {
    let muts = extract(&fixture());
    let by_path = |p: &str| -> &TaggedMutation {
        muts.iter()
            .find(|m| m.mutation.path == p && !m.mutation.op.is_heuristic())
            .unwrap_or_else(|| panic!("missing {p}"))
    };
    // The two Writes joined their carrier type:"create" → is_create true.
    assert!(by_path("/tmp/a.md").mutation.is_create);
    assert!(by_path("/tmp/b.md").mutation.is_create);
    // The Edit / MultiEdit joined type:"update" → is_create false.
    assert!(!by_path("/p/spec/gaps.md").mutation.is_create);
    assert!(!by_path("/p/src/lib.rs").mutation.is_create);
}

#[test]
fn bash_mutation_is_heuristic_op() {
    let muts = extract(&fixture());
    let bash = muts
        .iter()
        .find(|m| m.mutation.op == FileOp::BashMutation)
        .expect("a bash mutation");
    assert_eq!(bash.mutation.path, "/tmp/a.md");
    assert!(bash.mutation.op.is_heuristic());
    assert_eq!(bash.turn_index, 1);
}

#[test]
fn structured_notebook_edit_flows_through_extract() {
    // A NotebookEdit tool_use → a notebook-edit op in the extracted mutations.
    let records = vec![
        rec(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"edit nb"}}"#,
        ),
        rec(
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"n1","name":"NotebookEdit","input":{"notebook_path":"/p/analysis.ipynb","new_source":"code"}}]}}"#,
        ),
    ];
    let muts = extract(&records);
    assert_eq!(muts.len(), 1);
    assert_eq!(muts[0].mutation.op, FileOp::NotebookEdit);
    assert_eq!(muts[0].mutation.path, "/p/analysis.ipynb");
}

#[test]
fn tool_use_id_for_finds_first_and_none() {
    let with = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"x1","name":"Write","input":{"file_path":"/p/a"}}]}}"#,
    );
    assert_eq!(tool_use_id_for(&with).as_deref(), Some("x1"));
    // A record with no tool_use → None.
    let without = rec(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
    assert!(tool_use_id_for(&without).is_none());
    // A tool_use with no id → None.
    let no_id = rec(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Write","input":{"file_path":"/p/a"}}]}}"#,
    );
    assert!(tool_use_id_for(&no_id).is_none());
}

#[test]
fn line_prefilter_keeps_candidates_drops_noise() {
    assert!(line_is_files_candidate(
            br#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","name":"Edit"}]}}"#
        ));
    assert!(line_is_files_candidate(
        br#"{"type":"user","message":{"role":"user"}}"#
    ));
    assert!(line_is_files_candidate(
        br#"{"toolUseResult":{"filePath":"/p/x"}}"#
    ));
    // A pure-noise attachment line with none of the markers is dropped.
    assert!(!line_is_files_candidate(
        br#"{"type":"attachment","data":{"x":1}}"#
    ));
}

#[test]
fn extract_handles_missing_carrier_defaults_is_create_false() {
    // A Write tool_use with NO paired carrier in the turn → is_create stays false
    // (honest "unknown / treat as edit"), and the path comes from the tool_use.
    let records = vec![
        rec(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"q"}}"#,
        ),
        rec(
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/tmp/no-carrier.md","content":"x"}}]}}"#,
        ),
    ];
    let muts = extract(&records);
    assert_eq!(muts.len(), 1);
    assert_eq!(muts[0].mutation.path, "/tmp/no-carrier.md");
    assert!(!muts[0].mutation.is_create, "no carrier → is_create false");
}

#[test]
fn extract_empty_for_session_with_no_mutations() {
    let records = vec![
        rec(r#"{"type":"user","uuid":"u0","message":{"role":"user","content":"just chatting"}}"#),
        rec(
            r#"{"type":"assistant","uuid":"a0","message":{"role":"assistant","content":[{"type":"text","text":"sure"}]}}"#,
        ),
    ];
    assert!(extract(&records).is_empty());
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_jsonl(lines: &[&str]) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "csift-files-{}-{}.jsonl",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut f = std::fs::File::create(&p).unwrap();
    for l in lines {
        writeln!(f, "{l}").unwrap();
    }
    p
}

#[test]
fn scan_one_file_empty_is_safe() {
    // A zero-byte file → mmap_bytes None → empty result (the early-return arm).
    let p = std::env::temp_dir().join(format!(
        "csift-files-empty-{}-{}.jsonl",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::File::create(&p).unwrap();
    let fr = scan_one_file(&p).expect("scan empty");
    std::fs::remove_file(&p).ok();
    assert!(fr.mutations.is_empty());
    assert_eq!(fr.skipped_lines, 0);
}

#[test]
fn scan_one_file_extracts_mutations_and_counts_skips() {
    // A populated file: a genuine user, a Write tool_use + carrier, plus a
    // malformed line that survives the prefilter (carries "Write") → counted.
    let p = tmp_jsonl(&[
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/tmp/scan.md","content":"x"}}]}}"#,
        r#"{"type":"user","toolUseResult":{"type":"create","filePath":"/tmp/scan.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"ok"}]}}"#,
        r#"{"name":"Write" this is broken json after the marker}"#,
    ]);
    let fr = scan_one_file(&p).expect("scan");
    std::fs::remove_file(&p).ok();
    assert_eq!(fr.mutations.len(), 1, "one Write mutation");
    assert_eq!(fr.mutations[0].mutation.path, "/tmp/scan.md");
    assert!(fr.mutations[0].mutation.is_create, "carrier create joined");
    assert_eq!(fr.skipped_lines, 1, "the malformed Write line is counted");
}

#[test]
fn scan_one_file_skips_non_candidate_noise_lines() {
    // An attachment line with NONE of the mutation/role markers is dropped pre-JSON
    // (the `!line_is_files_candidate` true arm), leaving zero mutations + zero skips.
    let p = tmp_jsonl(&[
        r#"{"type":"attachment","data":{"x":1}}"#,
        r#"{"type":"file-history-snapshot","snapshot":{}}"#,
    ]);
    let fr = scan_one_file(&p).expect("scan");
    std::fs::remove_file(&p).ok();
    assert!(fr.mutations.is_empty());
    assert_eq!(
        fr.skipped_lines, 0,
        "noise lines are not malformed, just skipped"
    );
}

#[test]
fn subagent_mutation_carries_is_subagent_and_refeedable_parent_in_grouped_views() {
    // A subagent transcript path stamps is_subagent=true + the re-feedable PARENT uuid
    // onto every mutation, so the grouped (text + JSON) views can brand the row instead
    // of leaking the bare hex as a `SESSION` / re-feedable `session_id`.
    let dir = std::env::temp_dir().join(format!(
        "csift-files-sub-{}-{}/aaaabbbb-cccc-dddd-eeee-ffff00001111/subagents/workflows/wf_q",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("agent-c0ffee1234567890.jsonl");
    {
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, r#"{{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"go"}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"w1","name":"Write","input":{{"file_path":"/tmp/sub.md","content":"x"}}}}]}}}}"#).unwrap();
    }
    let fr = scan_one_file(&p).expect("scan subagent");
    std::fs::remove_file(&p).ok();
    assert_eq!(fr.mutations.len(), 1);
    let m = &fr.mutations[0];
    assert_eq!(
        m.session_id, "c0ffee1234567890",
        "bare hex (agent- stripped)"
    );
    assert!(
        m.is_subagent,
        "a subagents/ path tags the mutation subagent"
    );
    assert_eq!(
        m.parent_session_id, "aaaabbbb-cccc-dddd-eeee-ffff00001111",
        "parent is the re-feedable uuid dir before subagents/"
    );
    // Both grouped renders run without panic on a subagent-tagged outcome (covers the
    // branded SUBAGENT header arm + the json_grouped discriminator arm).
    let muts = vec![m.clone()];
    render_text(&outcome(FilesDetail::ByFile, muts.clone()));
    render_json(&outcome(FilesDetail::ByFile, muts)).expect("grouped json");
}
