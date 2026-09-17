//! The duplicate disclosure on the surfaces that SPAN sessions: the two header keys,
//! the `copies` line beside the scope banner, and the terminal modes that print neither.

use super::*;

#[test]
fn every_spanning_surface_counts_a_divergent_pair_as_one_session() {
    // Nothing merges when the bytes disagree, so both copies stay in scope - and the SCOPE
    // header is a session census, so every surface has to answer 1 where the disk holds one
    // session twice. Counting transcripts answered 2 on each of them.
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &record(1, "one branch"));
    h.write(
        &format!("{ENC_SHORT}/{SESS}.jsonl"),
        &record(1, "another branch"),
    );
    let sess = at(SESS);
    let cases: Vec<Vec<&str>> = vec![
        vec!["list", "--no-subagents", "--format", "json"],
        vec!["search", "branch", &sess, "--format", "json"],
        vec!["stats", &sess, "--format", "json"],
        vec!["files", &sess, "--format", "json"],
        vec!["verbatim", &sess, "--format", "json"],
    ];
    for args in cases {
        let out = h.run(&args);
        assert!(out.success, "{:?}: {}", args, out.stderr);
        let header = json_rows(&out.stdout, "header");
        assert_eq!(
            header[0]["sessions_in_scope"], 1,
            "{:?} header: {}",
            args, header[0]
        );
        assert_eq!(
            header[0]["top_level_sessions"], 1,
            "{:?} header: {}",
            args, header[0]
        );
    }
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
fn the_image_extract_header_carries_them_too() {
    // `image --out` writes its own header rather than going through the listing render,
    // so the sweep above would leave it the one `image` invocation that stays silent.
    let h = Home::new();
    let img = format!(
        "{{\"type\":\"user\",\"uuid\":\"i1\",\"timestamp\":\"2026-06-07T02:00:00.000Z\",\
         \"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"image\",\"source\":\
         {{\"type\":\"base64\",\"media_type\":\"image/png\",\"data\":\"{PNG_1X1}\"}}}}]}}}}\n"
    );
    let body = record(1, "first ask") + &img;
    h.write(&format!("{ENC}/{SESS}.jsonl"), &body);
    h.write(&format!("{ENC_SHORT}/{SESS}.jsonl"), &body);

    let out_dir = h.root.join("extracted");
    let out = h.run(&[
        "image",
        &at(SESS),
        "--out",
        out_dir.to_str().expect("utf8 path"),
        "--format",
        "json",
    ]);
    assert!(out.success, "{}", out.stderr);
    let header = json_rows(&out.stdout, "header");
    assert_eq!(header[0]["duplicate_transcripts"], 1, "{}", header[0]);
    assert_eq!(header[0]["duplicate_records_dropped"], 2, "{}", header[0]);
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
