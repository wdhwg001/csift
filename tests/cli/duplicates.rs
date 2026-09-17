//! One session id under two project directories: the merge, its disclosure and the one
//! command that keeps refusing.
//!
//! The shape is real on disk (a project renamed and its directory copied leaves it, and
//! the harness's own project-dir-name override and relocation fallback mint it too), and
//! every session-operating subcommand passes through the one resolver that decides it.

use crate::harness::*;

/// The SHORT project-directory name a rename leaves behind - it matches no record's cwd.
const ENC_SHORT: &str = "-Users-testuser-Projects-f";

/// Two records, both recording the cwd the LONG directory name encodes.
fn body_two() -> String {
    format!("{}{}", record(1, "first ask"), record(2, "second ask"))
}

fn record(n: u32, text: &str) -> String {
    format!(
        "{{\"type\":\"user\",\"uuid\":\"u{n}\",\"cwd\":\"/Users/testuser/Projects/foo\",\
         \"timestamp\":\"2026-06-07T0{n}:00:00.000Z\",\
         \"message\":{{\"role\":\"user\",\"content\":\"{text}\"}}}}\n"
    )
}

/// The same session id under two project directories, byte-identical.
fn identical_home() -> Home {
    let h = Home::new();
    let body = body_two();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &body);
    h.write(&format!("{ENC_SHORT}/{SESS}.jsonl"), &body);
    h
}

#[test]
fn list_gives_a_duplicated_session_one_row_that_names_its_other_copy() {
    let h = identical_home();
    let out = h.run(&["list", "--no-subagents"]);
    assert!(out.success, "{}", out.stderr);
    assert_eq!(
        out.stdout.matches("SESSION  ").count(),
        1,
        "one row per session, not one per copy:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("copies   1 other copy"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("identical"), "{}", out.stdout);
    assert!(
        out.stdout.contains(ENC_SHORT),
        "the row names where the dropped copy is:\n{}",
        out.stdout
    );
}

#[test]
fn the_resolver_names_the_merge_on_stderr_whatever_the_subcommand() {
    let h = identical_home();
    for args in [
        vec!["list", "--no-subagents"],
        vec!["stats", &at(SESS)],
        vec!["search", "ask", &at(SESS)],
    ] {
        let out = h.run(&args);
        assert!(out.success, "{:?}: {}", args, out.stderr);
        assert!(
            out.stderr.contains("project directories"),
            "{:?} said nothing about the merge:\n{}",
            args,
            out.stderr
        );
    }
}

#[test]
fn the_json_header_counts_distinct_sessions_and_says_what_the_merge_cost() {
    let h = identical_home();
    let out = h.run(&["list", "--no-subagents", "--format", "json"]);
    let header = json_rows(&out.stdout, "header");
    assert_eq!(header[0]["sessions_in_scope"], 1);
    assert_eq!(header[0]["duplicate_transcripts"], 1);
    // Both copies carry two records, so merging removed two lines from the scope.
    assert_eq!(header[0]["duplicate_records_dropped"], 2);

    let rows = json_rows(&out.stdout, "session");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["duplicate_relation"], "identical");
    assert_eq!(rows[0]["also_at_count"], 1);
    assert!(rows[0]["also_at"][0]
        .as_str()
        .expect("a path")
        .contains(ENC_SHORT));
}

#[test]
fn an_ordinary_session_carries_the_keys_with_nothing_in_them() {
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &body_two());
    let out = h.run(&["list", "--no-subagents", "--format", "json"]);
    let header = json_rows(&out.stdout, "header");
    assert_eq!(header[0]["duplicate_transcripts"], 0);
    assert_eq!(header[0]["duplicate_records_dropped"], 0);
    let rows = json_rows(&out.stdout, "session");
    assert!(rows[0]["duplicate_relation"].is_null());
    assert_eq!(rows[0]["also_at_count"], 0);
    assert!(
        !out.stderr.contains("project directories"),
        "no note when nothing was duplicated:\n{}",
        out.stderr
    );
}

#[test]
fn a_short_copy_that_is_a_byte_prefix_loses_to_the_longer_one() {
    let h = Home::new();
    // The SHORT-name directory holds the whole session; the long-name one holds a copy
    // taken while it was still being written. Size decides, never the directory name.
    h.write(&format!("{ENC}/{SESS}.jsonl"), &record(1, "first ask"));
    h.write(&format!("{ENC_SHORT}/{SESS}.jsonl"), &body_two());

    let out = h.run(&["list", "--no-subagents", "--format", "json"]);
    let rows = json_rows(&out.stdout, "session");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["duplicate_relation"], "prefix");
    assert!(
        rows[0]["path"]
            .as_str()
            .expect("a path")
            .contains(ENC_SHORT),
        "the longer copy wins: {}",
        rows[0]["path"]
    );
    let header = json_rows(&out.stdout, "header");
    assert_eq!(header[0]["duplicate_records_dropped"], 1);
}

#[test]
fn copies_that_disagree_both_stay_in_scope_and_say_so() {
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &record(1, "one branch"));
    h.write(
        &format!("{ENC_SHORT}/{SESS}.jsonl"),
        &record(1, "another branch"),
    );

    let out = h.run(&["list", "--no-subagents", "--format", "json"]);
    let rows = json_rows(&out.stdout, "session");
    assert_eq!(rows.len(), 2, "nothing merges when the bytes disagree");
    for r in &rows {
        assert_eq!(r["duplicate_relation"], "divergent");
        assert_eq!(r["also_at_count"], 1);
    }
    let header = json_rows(&out.stdout, "header");
    assert_eq!(header[0]["duplicate_transcripts"], 0);
    assert_eq!(header[0]["duplicate_records_dropped"], 0);
    assert!(out.stderr.contains("DIFFER"), "{}", out.stderr);

    // A divergent group drops nothing, so `show`'s refusal cannot key on what was dropped:
    // a reader facing two copies needs the paths whether or not either was merged away.
    let show = h.run(&["show", &at(SESS), "--line", "1"]);
    assert!(!show.success, "{}", show.stdout);
    assert!(show.stderr.contains("name a path"), "{}", show.stderr);
    assert!(show.stderr.contains(ENC_SHORT), "{}", show.stderr);
}

#[test]
fn search_counts_a_merged_session_once() {
    let h = identical_home();
    let out = h.run(&["search", "second ask", &at(SESS), "-c"]);
    assert!(out.success, "{}", out.stderr);
    assert_eq!(out.stdout.trim(), "1");
}

#[test]
fn stats_reports_a_merged_session_once() {
    let h = identical_home();
    let out = h.run(&["stats", &at(SESS), "--format", "json"]);
    let rows = json_rows(&out.stdout, "session");
    assert_eq!(rows.len(), 1, "{}", out.stdout);
}

#[test]
fn agents_lists_a_duplicated_sessions_subagents_once() {
    let h = identical_home();
    let agent = "{\"type\":\"user\",\"uuid\":\"a1\",\"timestamp\":\"2026-06-07T01:00:05.000Z\",\
                 \"message\":{\"role\":\"user\",\"content\":\"child\"}}\n";
    // A copied project directory carries the whole sidecar with it, which is why the
    // merge has to happen BEFORE the subagent transcripts are gathered.
    for enc in [ENC, ENC_SHORT] {
        h.write(
            &format!("{enc}/{SESS}/subagents/agent-abcdef0123456789.jsonl"),
            agent,
        );
    }
    let out = h.run(&["agents", &at(SESS), "--format", "json"]);
    let nodes = json_rows(&out.stdout, "agent");
    assert_eq!(nodes.len(), 1, "{}", out.stdout);
    let session = json_rows(&out.stdout, "session");
    assert_eq!(
        session[0]["agents"], 1,
        "the count row agrees with the rows it counts: {}",
        out.stdout
    );
}

#[test]
fn show_still_refuses_an_address_that_names_two_files_and_points_at_the_paths() {
    let h = identical_home();
    let out = h.run(&["show", &at(SESS), "--line", "1"]);
    assert!(!out.success, "{}", out.stdout);
    assert!(
        out.stderr.contains("exactly ONE transcript"),
        "{}",
        out.stderr
    );
    assert!(out.stderr.contains(ENC_SHORT), "{}", out.stderr);
    assert!(
        out.stderr.contains("name a path"),
        "an address that resolves to two copies has to say what does separate them:\n{}",
        out.stderr
    );
}

#[test]
fn a_path_target_reads_the_copy_it_names() {
    let h = identical_home();
    let dropped = h.projects().join(ENC_SHORT).join(format!("{SESS}.jsonl"));
    let out = h.run(&["show", dropped.to_str().expect("utf8 path"), "--line", "1"]);
    assert!(out.success, "{}", out.stderr);
    assert!(out.stdout.contains("first ask"), "{}", out.stdout);
}

/// A fixture with one Edit in each copy, so `recover` and `files` have something to replay.
fn identical_home_with_a_write() -> Home {
    let h = Home::new();
    let edit = "{\"type\":\"user\",\"uuid\":\"c0\",\"timestamp\":\"2026-06-07T03:00:00.000Z\",\
        \"toolUseResult\":{\"type\":\"create\",\"filePath\":\"/p/note.md\",\"content\":\"one\\n\"},\
        \"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\
        \"tool_use_id\":\"w0\",\"content\":\"ok\"}]}}\n";
    let body = body_two() + edit;
    h.write(&format!("{ENC}/{SESS}.jsonl"), &body);
    h.write(&format!("{ENC_SHORT}/{SESS}.jsonl"), &body);
    h
}

#[test]
fn every_spanning_surface_carries_the_two_header_keys() {
    let h = identical_home_with_a_write();
    let file = "/p/note.md";
    let sess = at(SESS);
    let cases: Vec<Vec<&str>> = vec![
        vec!["list", "--no-subagents", "--format", "json"],
        vec!["search", "ask", &sess, "--format", "json"],
        vec!["stats", &sess, "--format", "json"],
        vec!["files", &sess, "--format", "json"],
        vec![
            "recover",
            &sess,
            "--file",
            file,
            "--coverage",
            "--format",
            "json",
        ],
        vec!["verbatim", &sess, "--format", "json"],
        vec!["image", &sess, "--format", "json"],
        vec!["agents", &sess, "--format", "json"],
    ];
    for args in cases {
        let out = h.run(&args);
        assert!(out.success, "{:?}: {}", args, out.stderr);
        let header = json_rows(&out.stdout, "header");
        assert_eq!(
            header[0]["duplicate_transcripts"], 1,
            "{:?} header: {}",
            args, header[0]
        );
        assert_eq!(
            header[0]["duplicate_records_dropped"], 3,
            "{:?} header: {}",
            args, header[0]
        );
    }
}

#[test]
fn every_spanning_surface_names_the_merge_in_its_text_output() {
    let h = identical_home_with_a_write();
    let file = "/p/note.md";
    let sess = at(SESS);
    let cases: Vec<Vec<&str>> = vec![
        vec!["search", "ask", &sess],
        vec!["stats", &sess],
        vec!["files", &sess],
        vec!["recover", &sess, "--file", file, "--coverage"],
        vec!["verbatim", &sess],
        vec!["image", &sess],
        vec!["agents", &sess],
    ];
    for args in cases {
        let out = h.run(&args);
        assert!(out.success, "{:?}: {}", args, out.stderr);
        assert!(
            out.stdout.contains("copies ") && out.stdout.contains("project directories"),
            "{:?} printed no duplicate banner:\n{}",
            args,
            out.stdout
        );
    }
}

#[test]
fn the_terminal_modes_keep_a_pure_stdout() {
    let h = identical_home();
    // `-c` is a bare integer and `-l` a pure id stream - the merge is disclosed on stderr
    // for both, never in the stream a pipeline reads.
    let count = h.run(&["search", "ask", &at(SESS), "-c"]);
    assert_eq!(count.stdout.trim(), "2");
    assert!(
        count.stderr.contains("project directories"),
        "{}",
        count.stderr
    );
    let ids = h.run(&["search", "ask", &at(SESS), "-l"]);
    assert_eq!(ids.stdout.trim(), SESS);
    assert!(ids.stderr.contains("project directories"), "{}", ids.stderr);
}
