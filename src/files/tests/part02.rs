use super::*;

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
fn timeline_sort_places_timestampless_last() {
    // A mutation with no timestamp sorts AFTER timestamped ones.
    assert!(timestamp_sort_key(Some("2026-06-07T05:00:00Z")) < timestamp_sort_key(None));
    assert!(
        timestamp_sort_key(Some("2026-06-07T05:00:00Z"))
            < timestamp_sort_key(Some("2026-06-07T06:00:00Z"))
    );
}

#[test]
fn turn_range_parsing() {
    assert_eq!(
        parse_turn_range("0..1").unwrap().resolve(100, false),
        (0, 1)
    );
    assert_eq!(
        parse_turn_range("5..5").unwrap().resolve(100, false),
        (5, 5)
    );
    assert!(parse_turn_range("3..1").is_err());
    assert!(parse_turn_range("notarange").is_err());
    assert!(parse_turn_range("a..b").is_err());
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

// ── Rendering branches (called directly so coverage does not depend on the
//    integration-binary merge; output goes to test stdout, harmless). ──

fn outcome(detail: FilesDetail, muts: Vec<TaggedMutation>) -> Outcome {
    // Derive the scope span from the mutations' distinct transcripts (test-only proxy for
    // the real `resolve_session_files` span) so a subagent fixture still drives the banner.
    // Compute the owned counts BEFORE moving `muts` into the struct.
    let mut subs = std::collections::BTreeSet::new();
    let mut tops = std::collections::BTreeSet::new();
    for m in &muts {
        if m.is_subagent {
            subs.insert(m.session_id.clone());
        } else {
            tops.insert(m.session_id.clone());
        }
    }
    let (scope_top, scope_sub) = (tops.len(), subs.len());
    Outcome {
        detail,
        mutations: muts,
        boundaries: Vec::new(),
        skipped_lines: 0,
        turn_range: None,
        time_window_bounded: false,
        scope_top,
        scope_sub,
    }
}

#[test]
fn render_text_all_detail_levels_run() {
    let muts = extract(&fixture());
    for d in [
        FilesDetail::Summary,
        FilesDetail::ByDir,
        FilesDetail::ByFile,
        FilesDetail::Timeline,
    ] {
        render_text(&outcome(d, muts.clone()));
    }
}

#[test]
fn render_text_empty_prints_none() {
    // The empty-mutations branch of render_text (→ "no file mutations found").
    render_text(&outcome(FilesDetail::Summary, Vec::new()));
}

#[test]
fn render_json_all_detail_levels_run() {
    let muts = extract(&fixture());
    for d in [
        FilesDetail::Summary,
        FilesDetail::ByDir,
        FilesDetail::ByFile,
        FilesDetail::Timeline,
    ] {
        render_json(&outcome(d, muts.clone())).expect("json render");
    }
}

#[test]
fn footer_filter_context_all_arms() {
    let muts = extract(&fixture());
    // turn arm.
    let mut o = outcome(FilesDetail::Summary, muts.clone());
    o.turn_range = Some("0..1".to_string());
    assert_eq!(filter_context(&o), "turn=0..1");
    // bounded time-window arm.
    let mut o2 = outcome(FilesDetail::Summary, muts.clone());
    o2.time_window_bounded = true;
    assert_eq!(filter_context(&o2), "time-window");
    // unbounded "all turns" arm.
    let o3 = outcome(FilesDetail::Summary, muts);
    assert_eq!(filter_context(&o3), "all turns");
}

#[test]
fn footer_reports_skipped_lines() {
    // The skipped-lines footer branch (> 0) fires.
    let mut o = outcome(FilesDetail::Summary, extract(&fixture()));
    o.skipped_lines = 3;
    print_footer(&o); // exercises the `skipped_lines > 0` true arm
}

#[test]
fn render_multi_session_separator() {
    // Two sessions → the per_session blank-line separator arm (`!first`) fires.
    let ln: Vec<usize> = (1..=fixture().len()).collect();
    let mut a = extract_mutations("aaaa-sess", &fixture(), &ln);
    let b = extract_mutations("bbbb-sess", &fixture(), &ln);
    a.extend(b);
    render_text(&outcome(FilesDetail::Summary, a));
}

#[test]
fn timeline_renders_heuristic_and_non_heuristic() {
    // The timeline heuristic-label ternary both arms: a Write (no label) and a
    // Bash mutation (heuristic label) in the same session.
    render_timeline(&outcome(FilesDetail::Timeline, extract(&fixture())));
}

#[test]
fn json_grouped_emits_per_group_with_timestamps() {
    // A by-dir JSON render where groups carry first/last timestamps (the
    // `first_local`/`last_local` Some arms via local_iso) + the distinct-file count.
    render_json(&outcome(FilesDetail::ByDir, extract(&fixture()))).expect("json");
}

// ── scan_one_file branch coverage (mmap-None, skipped-line counting) ──

use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
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
