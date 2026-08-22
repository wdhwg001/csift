//! Summary bucketing and per-file rollups: keys, counts, op labels, ordering.

use super::*;
// ── scan_one_file branch coverage (mmap-None, skipped-line counting) ──

#[test]
fn summary_bucketing_collapses_by_parent_dir() {
    let muts = extract(&fixture());
    let buckets = group_by(&muts, |m| bucket_key(&m.path));
    // /tmp bucket: two writes + the bash rm.
    let tmp = buckets.get("/tmp").expect("/tmp bucket");
    assert_eq!(tmp.write, 2, "two /tmp writes");
    assert_eq!(tmp.bash, 1, "one bash rm under /tmp");
    // /p/spec bucket: one edit (the fixture's parent dirs are ≤4 segments deep, so the
    // coarse rollup keeps them - the collapse only fires on DEEPER paths, see below).
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
        path_verbatim: None,
        resolution: None,
        command_errored: false,
    });
    c.add(&FileMutation {
        path: "/tmp/b".into(),
        op: FileOp::BashMutation,
        timestamp_utc: None,
        is_create: false,
        path_verbatim: None,
        resolution: None,
        command_errored: false,
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
            path_verbatim: None,
            resolution: None,
            command_errored: false,
        });
    }
    let label = c.ops_label();
    assert!(label.contains("1 notebook-edit"), "got: {label}");
    assert!(label.contains("1 multi-edit"), "got: {label}");
    assert!(label.contains("1 edit"), "got: {label}");
    assert_eq!(c.total(), 3);
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
