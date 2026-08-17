//! File-op extraction from structured tool_use blocks and their result carriers.

use super::*;

// ── File-mutation extraction (FileOp / FileMutation / Record helpers) ──

#[test]
fn file_op_label_all_variants() {
    assert_eq!(FileOp::Write.label(), "write");
    assert_eq!(FileOp::Edit.label(), "edit");
    assert_eq!(FileOp::NotebookEdit.label(), "notebook-edit");
    assert_eq!(FileOp::MultiEdit.label(), "multi-edit");
    assert_eq!(FileOp::BashMutation.label(), "bash");
}

#[test]
fn file_op_json_key_uses_underscores_for_multiword() {
    // The on-wire JSON spelling is UNDERSCORE-delimited so the timeline `op` field
    // matches the grouped per-op COUNT keys; single-word ops coincide with `label`.
    assert_eq!(FileOp::Write.json_key(), "write");
    assert_eq!(FileOp::Edit.json_key(), "edit");
    assert_eq!(FileOp::NotebookEdit.json_key(), "notebook_edit");
    assert_eq!(FileOp::MultiEdit.json_key(), "multi_edit");
    assert_eq!(FileOp::BashMutation.json_key(), "bash");
    // The two multi-word ops are the only ones that DIFFER from `label` (hyphen vs `_`).
    assert_ne!(
        FileOp::NotebookEdit.json_key(),
        FileOp::NotebookEdit.label()
    );
    assert_ne!(FileOp::MultiEdit.json_key(), FileOp::MultiEdit.label());
}

#[test]
fn file_op_is_heuristic_only_for_bash() {
    assert!(FileOp::BashMutation.is_heuristic());
    assert!(!FileOp::Write.is_heuristic());
    assert!(!FileOp::Edit.is_heuristic());
    assert!(!FileOp::NotebookEdit.is_heuristic());
    assert!(!FileOp::MultiEdit.is_heuristic());
}

#[test]
fn structured_tool_mutations_write_edit_multiedit() {
    let r = parse(
        r#"{"type":"assistant","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/tmp/a.md","content":"x"}},
                {"type":"tool_use","id":"t2","name":"Edit","input":{"file_path":"/p/spec/gaps.md","old_string":"a","new_string":"b","replace_all":false}},
                {"type":"tool_use","id":"t3","name":"MultiEdit","input":{"file_path":"/p/multi.rs","edits":[]}}
            ]}}"#,
    );
    let muts = r.structured_tool_mutations();
    assert_eq!(muts.len(), 3);
    assert_eq!(muts[0].op, FileOp::Write);
    assert_eq!(muts[0].path, "/tmp/a.md");
    assert_eq!(
        muts[0].timestamp_utc.as_deref(),
        Some("2026-06-07T05:00:00.000Z")
    );
    assert!(!muts[0].is_create, "no carrier on tool_use → unknown=false");
    assert_eq!(muts[1].op, FileOp::Edit);
    assert_eq!(muts[1].path, "/p/spec/gaps.md");
    assert_eq!(muts[2].op, FileOp::MultiEdit);
    assert_eq!(muts[2].path, "/p/multi.rs");
}

#[test]
fn structured_tool_mutations_notebook_uses_notebook_path() {
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","name":"NotebookEdit","input":{"notebook_path":"/p/nb.ipynb","new_source":"code"}}
            ]}}"#,
    );
    let muts = r.structured_tool_mutations();
    assert_eq!(muts.len(), 1);
    assert_eq!(muts[0].op, FileOp::NotebookEdit);
    assert_eq!(muts[0].path, "/p/nb.ipynb");
}

#[test]
fn structured_tool_mutations_reads_create_from_own_carrier() {
    // The rare case where the tool_use record ALSO carries the toolUseResult with
    // type:"create" (the `self_is_create` true arm).
    let r = parse(
        r#"{"type":"assistant","toolUseResult":{"type":"create","filePath":"/tmp/new.md"},"message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/tmp/new.md","content":"x"}}
            ]}}"#,
    );
    let muts = r.structured_tool_mutations();
    assert_eq!(muts.len(), 1);
    assert!(
        muts[0].is_create,
        "own carrier type:create → is_create true"
    );
}

#[test]
fn structured_tool_mutations_skips_missing_and_empty_path() {
    // A Write with NO file_path and an Edit with an EMPTY file_path → both skipped
    // (the defensive `path.is_empty()` arm). A non-mutating tool is ignored too.
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","name":"Write","input":{"content":"x"}},
                {"type":"tool_use","id":"t2","name":"Edit","input":{"file_path":"","old_string":"a"}},
                {"type":"tool_use","id":"t3","name":"Read","input":{"file_path":"/p/read.rs"}}
            ]}}"#,
    );
    assert!(r.structured_tool_mutations().is_empty());
}

#[test]
fn structured_tool_mutations_skips_tool_use_with_no_name() {
    // A tool_use block missing its `name` → the `name.as_deref()` None arm.
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[
                {"type":"tool_use","id":"t1","input":{"file_path":"/p/x.md"}}
            ]}}"#,
    );
    assert!(r.structured_tool_mutations().is_empty());
}

#[test]
fn structured_tool_mutations_none_for_string_content() {
    // A record with string content → blocks() None → empty (the early-return arm).
    let r = parse(r#"{"type":"user","message":{"role":"user","content":"plain"}}"#);
    assert!(r.structured_tool_mutations().is_empty());
}

#[test]
fn carrier_create_paths_create_update_and_missing() {
    // type:"create" → is_create true, joined to the tool_result block's id.
    let create = parse(
        r#"{"type":"user","toolUseResult":{"type":"create","filePath":"/tmp/new.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
    );
    assert_eq!(
        create.carrier_create_paths(),
        vec![("t1".to_string(), "/tmp/new.md".to_string(), true)]
    );
    // type:"update" → is_create false.
    let update = parse(
        r#"{"type":"user","toolUseResult":{"type":"update","filePath":"/p/gaps.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"ok"}]}}"#,
    );
    assert_eq!(
        update.carrier_create_paths(),
        vec![("t2".to_string(), "/p/gaps.md".to_string(), false)]
    );
    // No toolUseResult at all → empty (the `let Some(tur) else` arm).
    let none = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t3","content":"ok"}]}}"#,
    );
    assert!(none.carrier_create_paths().is_empty());
}

#[test]
fn carrier_create_paths_file_unchanged_is_not_create() {
    // type:"file_unchanged" (the edit-touched-but-no-change case) → is_create false.
    let r = parse(
        r#"{"type":"user","toolUseResult":{"type":"file_unchanged","filePath":"/p/same.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
    );
    assert_eq!(
        r.carrier_create_paths(),
        vec![("t1".to_string(), "/p/same.md".to_string(), false)]
    );
}

#[test]
fn carrier_create_paths_no_file_path_or_not_object() {
    // toolUseResult present but NO filePath → empty (the filePath None arm).
    let no_fp = parse(
        r#"{"type":"user","toolUseResult":{"type":"text","stdout":"x"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"x"}]}}"#,
    );
    assert!(no_fp.carrier_create_paths().is_empty());
    // toolUseResult is NOT an object (a bare string) → `.get("filePath")` is None.
    let not_obj = parse(
        r#"{"type":"user","toolUseResult":"just a string","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"x"}]}}"#,
    );
    assert!(not_obj.carrier_create_paths().is_empty());
    // filePath present but EMPTY → empty (the `file_path.is_empty()` arm).
    let empty_fp = parse(
        r#"{"type":"user","toolUseResult":{"type":"create","filePath":""},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"x"}]}}"#,
    );
    assert!(empty_fp.carrier_create_paths().is_empty());
}

#[test]
fn carrier_create_paths_skips_non_tool_result_blocks_and_no_id() {
    // A carrier with a filePath but whose only block is NOT a tool_result, OR a
    // tool_result with no tool_use_id → no joinable id, so empty.
    let no_id = parse(
        r#"{"type":"user","toolUseResult":{"type":"create","filePath":"/tmp/x.md"},"message":{"role":"user","content":[{"type":"text","text":"not a tool result"},{"type":"tool_result","content":"ok"}]}}"#,
    );
    assert!(no_id.carrier_create_paths().is_empty());
}

#[test]
fn carrier_create_paths_empty_when_no_blocks() {
    // filePath present but the record has string content (no blocks) → empty.
    let r = parse(
        r#"{"type":"user","toolUseResult":{"type":"create","filePath":"/tmp/x.md"},"message":{"role":"user","content":"string body"}}"#,
    );
    assert!(r.carrier_create_paths().is_empty());
}
