use super::*;

#[test]
fn agent_text_handles_bare_string_assistant_content() {
    // CC normally sends assistant content as a block array; a bare-string body is
    // a surprise we surface rather than drop (the Content::Text fallback arm).
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":"  surprise bare string  "}}"#,
    );
    assert_eq!(r.agent_text().as_deref(), Some("surprise bare string"));
}

#[test]
fn agent_text_none_for_empty_bare_string() {
    // A bare-string assistant body that is all whitespace normalizes to empty → None.
    let r = parse(r#"{"type":"assistant","message":{"role":"assistant","content":"   "}}"#);
    assert!(r.agent_text().is_none());
}

#[test]
fn agent_text_none_when_no_content() {
    let r = parse(r#"{"type":"assistant","message":{"role":"assistant"}}"#);
    assert!(r.agent_text().is_none());
}

#[test]
fn agent_text_skips_blank_text_blocks() {
    // A text block that is all whitespace is skipped; only the real one counts.
    let r = parse(
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"   "},{"type":"text","text":"real"}]}}"#,
    );
    assert_eq!(r.agent_text().as_deref(), Some("real"));
}

#[test]
fn flatten_blocks_ignores_non_text_blocks() {
    // genuine_user_text over blocks where a non-text block is interleaved: the
    // tool_use/image are filtered out, only text survives. (Exercised via a
    // genuine-user record whose blocks mix text with an image.)
    let r = parse(
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"keep me"},{"type":"image","source":{}}]}}"#,
    );
    assert_eq!(r.genuine_user_text().as_deref(), Some("keep me"));
}

#[test]
fn tool_result_content_text_object_fallback() {
    // A non-string/non-array content (e.g. an object) renders compactly so a
    // regex can still match structured payloads (the `other => to_string` arm).
    let obj = serde_json::json!({"k": "v", "n": 1});
    let out = tool_result_content_text(&obj);
    assert!(out.contains("\"k\""), "compact object render: {out}");
    // A bare number/bool/null also goes through the same arm.
    assert_eq!(tool_result_content_text(&serde_json::json!(42)), "42");
    assert_eq!(tool_result_content_text(&serde_json::json!(null)), "null");
}

#[test]
fn scrape_persisted_path_empty_after_marker_is_none() {
    // The marker is present but the path is blank → None (the `path.is_empty()`
    // true arm).
    assert!(scrape_persisted_path("Full output saved to:   \nnext").is_none());
    // Marker with the path running to EOF (no trailing newline) → the `unwrap_or`
    // line_end == rest.len() branch.
    assert_eq!(
        scrape_persisted_path("Full output saved to: /a/b.txt"),
        Some("/a/b.txt".to_string())
    );
}

#[test]
fn normalize_line_collapses_and_trims() {
    // Leading/trailing/internal whitespace runs collapse to single spaces; the
    // trailing-space pop loop runs.
    assert_eq!(normalize_line("  a\t\tb \n c   "), "a b c");
    assert_eq!(normalize_line(""), "");
    assert_eq!(normalize_line("   "), "");
}

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

// ── group_turn_indices (the shared §6.4 turn delimiter) ──

#[test]
fn group_turn_indices_basic_two_turns() {
    // bools = is_genuine_user; [user, member, member, user, member].
    let flags = [true, false, false, true, false];
    let turns = group_turn_indices(&flags, |b| *b);
    assert_eq!(turns, vec![vec![0, 1, 2], vec![3, 4]]);
}

#[test]
fn group_turn_indices_synthetic_lead_folds_into_first_turn() {
    // Leading non-genuine records (a synthetic lead) fold into turn 0 when a real
    // user follows, so indices stay 0-based on genuine users.
    let flags = [false, false, true, false, true];
    let turns = group_turn_indices(&flags, |b| *b);
    // The two lead members (0,1) join the first real turn (opening at idx 2).
    assert_eq!(turns, vec![vec![0, 1, 2, 3], vec![4]]);
}

#[test]
fn group_turn_indices_only_synthetic_lead_no_genuine_user() {
    // No genuine user ever → a single standalone turn 0 holding the orphans (the
    // `turns.len() > 1` false guard means no fold).
    let flags = [false, false, false];
    let turns = group_turn_indices(&flags, |b| *b);
    assert_eq!(turns, vec![vec![0, 1, 2]]);
}

#[test]
fn group_turn_indices_empty_is_empty() {
    let flags: [bool; 0] = [];
    assert!(group_turn_indices(&flags, |b| *b).is_empty());
}

#[test]
fn group_turn_indices_first_record_genuine_no_fold() {
    // The first record IS genuine → no synthetic lead → no fold.
    let flags = [true, false, true];
    let turns = group_turn_indices(&flags, |b| *b);
    assert_eq!(turns, vec![vec![0, 1], vec![2]]);
}

#[test]
fn opens_turn_grouping_splits_on_auq_answer_and_skips_interrupt() {
    // A realistic record sequence over the PRODUCTION predicate (`opens_turn`):
    //   0 genuine user      → turn 0 opener
    //   1 assistant AUQ tool_use (member of turn 0)
    //   2 AUQ answer carrier → turn 1 opener (the behavior change)
    //   3 assistant reply   (member of turn 1)
    //   4 interrupt marker  → NOT a boundary (member of turn 1)
    //   5 genuine user      → turn 2 opener
    let records: Vec<Record> = [
            r#"{"type":"user","message":{"role":"user","content":"pick one"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which?"}]}}]}}"#,
            r#"{"type":"user","toolUseResult":{"answers":{"which?":"the bold one"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"User has answered your questions: \"which?\"=\"the bold one\"."}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":"next question"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
    let turns = group_turn_indices(&records, |r| r.opens_turn());
    assert_eq!(
        turns,
        vec![vec![0, 1], vec![2, 3, 4], vec![5]],
        "AUQ answer opens turn 1; interrupt is a member, not a boundary"
    );
}

// ── §6.4.1 esc-cancel / edit-resend DRAFT SUPPRESSION ──
// Real shape (verified on ~/.claude/projects): the user submits, ESC-cancels or edits,
// and resends; CC appends EACH draft as its own genuine `type:"user"` record sharing the
// SAME `parentUuid`. Only the last in file order reached the model. `superseded_draft_indices`
// marks the earlier siblings; `group_turn_indices_deduped` drops them so they never become
// phantom turns. None of these patterns are reachable through bool fixtures — they need the
// real uuid/parentUuid tree, so they parse genuine record JSON.

#[test]
fn superseded_drafts_collapse_same_parent_edit_resend() {
    // u0 (parent root) → assistant a0 → THREE drafts of one turn under parent a0:
    // "draft v1" → edited "draft v2" → "draft v2, with a tail" (the one that continued) → assistant a1.
    // Only the last sibling survives; the two earlier ones are superseded drafts.
    let records: Vec<Record> = [
            r#"{"type":"user","uuid":"u0","parentUuid":"root","message":{"role":"user","content":"start"}}"#,
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"user","uuid":"d1","parentUuid":"a0","message":{"role":"user","content":"draft v1"}}"#,
            r#"{"type":"user","uuid":"d2","parentUuid":"a0","message":{"role":"user","content":"draft v2"}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","message":{"role":"user","content":"draft v2, with a tail"}}"#,
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","message":{"role":"assistant","content":[{"type":"text","text":"noted"}]}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();

    let sup = superseded_draft_indices(&records, |r| r);
    assert_eq!(
        sup.len(),
        2,
        "the two earlier same-parent drafts are superseded"
    );
    assert!(
        sup.contains(&2) && sup.contains(&3),
        "drafts d1,d2 superseded; u1 (last in file order) survives"
    );
    assert!(!sup.contains(&4));

    let turns = group_turn_indices_deduped(&records, |r| r);
    assert_eq!(
        turns,
        vec![vec![0, 1], vec![4, 5]],
        "two real turns; abandoned drafts vanish entirely (neither boundary nor member)"
    );
}

#[test]
fn superseded_drafts_exact_duplicate_collapses_to_one() {
    // The same message appears 3× verbatim under one parent (ESC-cancel re-submits) →
    // exactly one turn, not three.
    let records: Vec<Record> = [
            r#"{"type":"assistant","uuid":"a0","parentUuid":"root","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
            r#"{"type":"user","uuid":"d1","parentUuid":"a0","message":{"role":"user","content":"the same resent line"}}"#,
            r#"{"type":"user","uuid":"d2","parentUuid":"a0","message":{"role":"user","content":"the same resent line"}}"#,
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","message":{"role":"user","content":"the same resent line"}}"#,
        ]
        .iter()
        .map(|l| parse(l))
        .collect();
    // Leading assistant (idx0) is a synthetic lead that folds into the first real turn.
    assert_eq!(
        group_turn_indices_deduped(&records, |r| r),
        vec![vec![0, 3]],
        "3 identical drafts → 1 turn (opener idx3)"
    );
}
