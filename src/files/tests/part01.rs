use super::*;

#[test]
fn path_filter_none_keeps_everything() {
    let f = PathFilter::from_args(None, None).unwrap();
    assert!(f.keeps("/anything/at/all.rs"));
    assert!(f.keeps(""));
}

#[test]
fn path_filter_regex_matches_anywhere_in_full_path() {
    let f = PathFilter::from_args(Some(r"\.rs$"), None).unwrap();
    assert!(f.keeps("/Users/x/src/lib.rs"));
    assert!(!f.keeps("/Users/x/docs/readme.md"));
    // "anywhere" semantics: a mid-path match is enough.
    let mid = PathFilter::from_args(Some("src"), None).unwrap();
    assert!(mid.keeps("/Users/x/src/lib.rs"));
    assert!(!mid.keeps("/Users/x/docs/readme.md"));
}

#[test]
fn path_filter_glob_crosses_slash_with_double_star() {
    let f = PathFilter::from_args(None, Some("**/src/**")).unwrap();
    assert!(f.keeps("/Users/x/src/lib.rs"));
    assert!(!f.keeps("/Users/x/docs/readme.md"));
    let md = PathFilter::from_args(None, Some("**/*.md")).unwrap();
    assert!(md.keeps("/Users/x/docs/readme.md"));
    assert!(!md.keeps("/Users/x/src/lib.rs"));
}

#[test]
fn path_filter_regex_and_glob_are_anded() {
    let f = PathFilter::from_args(Some(r"\.rs$"), Some("**/src/**")).unwrap();
    assert!(f.keeps("/Users/x/src/lib.rs")); // both match
    assert!(!f.keeps("/Users/x/src/readme.md")); // glob yes, regex no
    assert!(!f.keeps("/Users/x/other/lib.rs")); // regex yes, glob no
}

#[test]
fn path_filter_invalid_patterns_error() {
    assert!(PathFilter::from_args(Some("("), None).is_err());
    assert!(PathFilter::from_args(None, Some("[abc")).is_err());
}

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
fn summary_bucketing_collapses_by_parent_dir() {
    let muts = extract(&fixture());
    let buckets = group_by(&muts, |m| bucket_key(&m.path));
    // /tmp bucket: two writes + the bash rm.
    let tmp = buckets.get("/tmp").expect("/tmp bucket");
    assert_eq!(tmp.write, 2, "two /tmp writes");
    assert_eq!(tmp.bash, 1, "one bash rm under /tmp");
    // /p/spec bucket: one edit (the fixture's parent dirs are ≤4 segments deep, so the
    // coarse rollup keeps them — the collapse only fires on DEEPER paths, see below).
    let spec = buckets.get("/p/spec").expect("/p/spec bucket");
    assert_eq!(spec.edit, 1);
    // /p/src bucket: one multi-edit.
    let src = buckets.get("/p/src").expect("/p/src bucket");
    assert_eq!(src.multi_edit, 1);
}

#[test]
fn summary_rollup_collapses_deep_paths_unlike_by_dir() {
    // The fix: `--summary` is a COARSE top-level rollup (≤SUMMARY_BUCKET_SEGMENTS
    // segments), NOT the full parent dir `--by-dir` keys on. Two deeply-nested files
    // sharing a top-level prefix collapse to ONE summary bucket but stay TWO by-dir rows.
    let deep_a = "/Users/testuser/Projects/demo_app/components/wireframe/tabs/Foo.tsx";
    let deep_b = "/Users/testuser/Projects/demo_app/spec/gaps.md";
    // Summary buckets BOTH under the first 4 segments.
    assert_eq!(bucket_key(deep_a), "/Users/testuser/Projects/demo_app");
    assert_eq!(bucket_key(deep_b), "/Users/testuser/Projects/demo_app");
    // by-dir keeps the full distinct parents.
    assert_eq!(
        parent_dir(deep_a).as_deref(),
        Some("/Users/testuser/Projects/demo_app/components/wireframe/tabs")
    );
    assert_eq!(
        parent_dir(deep_b).as_deref(),
        Some("/Users/testuser/Projects/demo_app/spec")
    );
    assert_ne!(
        parent_dir(deep_a),
        parent_dir(deep_b),
        "by-dir keeps them SEPARATE; summary collapses them — a real 4-level ladder"
    );
}

#[test]
fn summary_git_pseudo_path_is_its_own_bucket_not_the_dot_sink() {
    // `git:<sub>` pseudo-paths roll up under ONE `git:` bucket (out of the `./` relative
    // sink), so a genuine-relative-file bucket is not polluted by git subcommands.
    assert_eq!(bucket_key("git:commit"), "git:");
    assert_eq!(bucket_key("git:add"), "git:");
    assert_eq!(bucket_key("git:stash"), "git:");
    // A real relative file still buckets under `./`.
    assert_eq!(bucket_key("relative.md"), "./");
}

#[test]
fn bucket_key_edge_cases() {
    // A top-level file `/foo` → parent `/` → the root bucket.
    assert_eq!(bucket_key("/foo"), "/");
    // A shallow path keeps all the segments it has (fewer than the cap).
    assert_eq!(bucket_key("/tmp/x.md"), "/tmp");
    assert_eq!(bucket_key("/a/b/c.txt"), "/a/b");
    // A relative MULTI-segment path keeps its (relative) parent prefix.
    assert_eq!(bucket_key("src/wireframe/Foo.tsx"), "src/wireframe");
    // A deep path is capped to the first SUMMARY_BUCKET_SEGMENTS segments of the parent.
    assert_eq!(
        bucket_key("/a/b/c/d/e/f/g.rs"),
        "/a/b/c/d",
        "deep paths cap at the segment limit"
    );
}

#[test]
fn by_file_first_last_timestamps() {
    let muts = extract(&fixture());
    let files = group_by(&muts, |m| m.path.clone());
    // /tmp/a.md is Written at 05:00:01 and rm'd (bash) at 06:00:01.
    let a = files.get("/tmp/a.md").expect("/tmp/a.md row");
    assert_eq!(a.first_ts.as_deref(), Some("2026-06-07T05:00:01.000Z"));
    assert_eq!(a.last_ts.as_deref(), Some("2026-06-07T06:00:01.000Z"));
    assert_eq!(a.write, 1);
    assert_eq!(a.bash, 1);
}

#[test]
fn distinct_file_count() {
    let muts = extract(&fixture());
    let outcome = Outcome {
        detail: FilesDetail::Summary,
        mutations: muts,
        boundaries: Vec::new(),
        skipped_lines: 0,
        turn_range: None,
        time_window_bounded: false,
        scope_top: 1,
        scope_sub: 0,
    };
    // Distinct paths: /tmp/a.md, /tmp/b.md, /p/spec/gaps.md, /p/src/lib.rs = 4.
    assert_eq!(outcome.distinct_files(), 4);
}

#[test]
fn parent_dir_and_bucket_key_rules() {
    assert_eq!(parent_dir("/tmp/x.md").as_deref(), Some("/tmp"));
    assert_eq!(parent_dir("/p/spec/gaps.md").as_deref(), Some("/p/spec"));
    // A top-level path → parent is "/".
    assert_eq!(parent_dir("/foo").as_deref(), Some("/"));
    // A bare relative filename has no parent.
    assert_eq!(parent_dir("relative.md"), None);
    // A trailing slash is stripped before taking the parent (a dir target).
    assert_eq!(parent_dir("/a/b/").as_deref(), Some("/a"));
    // bucket_key falls back to "./" for a bare relative filename.
    assert_eq!(bucket_key("relative.md"), "./");
    assert_eq!(bucket_key("/tmp/x.md"), "/tmp");
}

#[test]
fn ops_label_omits_zeroes_and_flags_bash() {
    let mut c = OpCounts::default();
    assert_eq!(c.ops_label(), "0", "empty group → 0");
    c.add(&FileMutation {
        path: "/tmp/a".into(),
        op: FileOp::Write,
        timestamp_utc: None,
        is_create: true,
    });
    c.add(&FileMutation {
        path: "/tmp/b".into(),
        op: FileOp::BashMutation,
        timestamp_utc: None,
        is_create: false,
    });
    let label = c.ops_label();
    assert!(label.contains("1 write"), "got: {label}");
    assert!(label.contains("1 bash (heuristic)"), "got: {label}");
    assert!(!label.contains("edit"), "zero edits omitted: {label}");
}

#[test]
fn op_counts_and_label_cover_notebook_and_multiedit() {
    // Exercise the NotebookEdit + MultiEdit arms of OpCounts::add and ops_label
    // (the fixture has no NotebookEdit, so cover it explicitly).
    let mut c = OpCounts::default();
    for op in [FileOp::NotebookEdit, FileOp::MultiEdit, FileOp::Edit] {
        c.add(&FileMutation {
            path: format!("/p/nb-{}", op.label()),
            op,
            timestamp_utc: Some("2026-06-07T05:00:00.000Z".into()),
            is_create: false,
        });
    }
    let label = c.ops_label();
    assert!(label.contains("1 notebook-edit"), "got: {label}");
    assert!(label.contains("1 multi-edit"), "got: {label}");
    assert!(label.contains("1 edit"), "got: {label}");
    assert_eq!(c.total(), 3);
}
