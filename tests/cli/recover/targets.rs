//! recover target resolution: a `--file` suffix names ONE file or is refused.

use crate::harness::*;

/// One session writing then editing three files that share the basename `lib.rs`, plus a
/// fourth file of its own name. Every event is carried input-side (no `toolUseResult`), the
/// shape a subagent transcript writes.
fn three_libs_home() -> Home {
    let h = Home::new();
    let mut lines = vec![
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"write three lib files"}}"#.to_string(),
    ];
    let mut n = 0;
    for (d, body) in [
        ("a", "a1\\na2\\n"),
        ("b", "b1\\nb2\\n"),
        ("c", "c1\\nc2\\n"),
        ("d", "d1\\n"),
    ] {
        n += 1;
        let file = if d == "d" {
            "/p/d/main.rs".to_string()
        } else {
            format!("/p/{d}/lib.rs")
        };
        lines.push(format!(
            r#"{{"type":"assistant","uuid":"w{n}","timestamp":"2026-06-07T05:00:{n:02}.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"tw{n}","name":"Write","input":{{"file_path":"{file}","content":"{body}"}}}}]}}}}"#
        ));
        lines.push(format!(
            r#"{{"type":"user","uuid":"r{n}","timestamp":"2026-06-07T05:00:{n:02}.500Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"tw{n}","content":"File created successfully at: {file}"}}]}}}}"#
        ));
    }
    h.write(&format!("{ENC}/{SESS}.jsonl"), &(lines.join("\n") + "\n"));
    h
}

/// A bare basename that three distinct files end with is refused before any replay, in
/// every mode, naming every candidate and the count - replaying the three together would
/// stitch their writes into one buffer.
#[test]
fn a_file_suffix_naming_several_files_is_refused_in_every_mode() {
    let h = three_libs_home();
    let at_sess = at(SESS);
    for mode in [
        vec![],
        vec!["--coverage"],
        vec!["--patches"],
        vec!["--salvage"],
        vec!["--at", "@latest"],
    ] {
        let mut args = vec!["recover", at_sess.as_str(), "--file", "lib.rs"];
        args.extend(mode.iter().copied());
        let out = h.run(&args);
        assert!(!out.success, "{args:?} must refuse: {}", out.stdout);
        assert!(
            out.stdout.is_empty(),
            "{args:?} replayed something: {}",
            out.stdout
        );
        assert!(
            out.stderr
                .contains("--file `lib.rs` names 3 distinct files in scope"),
            "{args:?}: {}",
            out.stderr
        );
        for p in ["/p/a/lib.rs", "/p/b/lib.rs", "/p/c/lib.rs"] {
            assert!(
                out.stderr.contains(&format!("  {p}\n")),
                "{args:?}: {}",
                out.stderr
            );
        }
        assert!(
            out.stderr.contains("pass one of these paths"),
            "{}",
            out.stderr
        );
    }
}

/// The same target spelled so that it names exactly one file keeps working: the absolute
/// path, a longer suffix, and a basename only one file carries.
#[test]
fn a_file_target_naming_one_file_still_replays() {
    let h = three_libs_home();
    let at_sess = at(SESS);
    for (target, want) in [
        ("/p/b/lib.rs", "b1\nb2\n"),
        ("b/lib.rs", "b1\nb2\n"),
        ("main.rs", "d1\n"),
    ] {
        let out = h.run(&["recover", at_sess.as_str(), "--file", target]);
        assert!(out.success, "{target}: {}", out.stderr);
        assert_eq!(out.stdout, want, "{target}");
    }
}

/// A `--files-from` line takes the same rule: an ambiguous line is reported `ambiguous` and
/// written nowhere, and the other lines are recovered as before.
#[test]
fn a_manifest_line_naming_several_files_is_reported_ambiguous_and_not_written() {
    let h = three_libs_home();
    let manifest = h.root.join("manifest.txt");
    std::fs::write(&manifest, "lib.rs\n/p/a/lib.rs\n").unwrap();
    let out_dir = h.root.join("recovered");
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--files-from",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The note names the MANIFEST line it is about, and the summary counts the unreplayed
    // line beside the recovered files rather than among them.
    assert!(
        out.stderr
            .contains("not replayed: manifest line `lib.rs` names 3 distinct files"),
        "{}",
        out.stderr
    );
    assert!(
        out.stdout.contains("recovered 1 file(s): 1 complete")
            && out.stdout.contains("· 1 ambiguous"),
        "{}",
        out.stdout
    );
    let report = std::fs::read_to_string(out_dir.join("recovery-report.tsv")).unwrap();
    assert!(
        report.contains("ambiguous\t0\t0\t0\t0\t0\tlib.rs\t\n"),
        "{report}"
    );
    assert!(
        !out_dir.join("lib.rs").exists(),
        "no stitched file is written"
    );
    assert_eq!(
        std::fs::read_to_string(out_dir.join("p/a/lib.rs")).unwrap(),
        "a1\na2\n"
    );
}

/// What decides ambiguity is every file the scope NAMES, not only the ones with a replayable
/// event: a subagent that merely read `/p/b/lib.rs` makes `lib.rs` name two files, in the
/// batch as in the single-file mode. And a transcript whose events name the target only by a
/// relative path still replays it - one relative path is one file, so nothing is refused.
#[test]
fn a_read_only_match_counts_for_ambiguity_and_a_relative_path_still_replays() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"write two files"}}"#, "\n",
            r#"{"type":"assistant","uuid":"w1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"tw1","name":"Write","input":{"file_path":"/p/a/lib.rs","content":"a1\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T05:00:01.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tw1","content":"File created successfully at: /p/a/lib.rs"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"w2","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"tw2","name":"Write","input":{"file_path":"notes/todo.md","content":"t1\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r2","timestamp":"2026-06-07T05:00:02.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tw2","content":"File created successfully at: notes/todo.md"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-a1000000000000009.jsonl"),
        concat!(
            r#"{"type":"assistant","isSidechain":true,"agentId":"a1000000000000009","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"tr1","name":"Read","input":{"file_path":"/p/b/lib.rs"}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"a1000000000000009","timestamp":"2026-06-07T05:00:03.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tr1","content":"b1"}]}}"#, "\n",
        ),
    );
    let single = h.run(&["recover", at(SESS).as_str(), "--file", "lib.rs"]);
    assert!(!single.success, "{}", single.stdout);
    assert!(
        single.stderr.contains("names 2 distinct files in scope"),
        "{}",
        single.stderr
    );

    let run_batch = |lines: &str, dir: &str| {
        let manifest = h.root.join(format!("{dir}.txt"));
        std::fs::write(&manifest, lines).unwrap();
        let out_dir = h.root.join(dir);
        let out = h.run(&[
            "recover",
            at(SESS).as_str(),
            "--files-from",
            manifest.to_str().unwrap(),
            "--out-dir",
            out_dir.to_str().unwrap(),
        ]);
        assert!(out.success, "stderr: {}", out.stderr);
        let report = std::fs::read_to_string(out_dir.join("recovery-report.tsv")).unwrap();
        (out, report)
    };
    let (out, report) = run_batch("lib.rs\nnotes/todo.md\n", "mixed");
    assert!(
        report.contains("ambiguous\t0\t0\t0\t0\t0\tlib.rs\t\n"),
        "{report}"
    );
    assert!(
        report.contains("complete\t1\t1\t0\t0\t0\tnotes/todo.md\t"),
        "{report}"
    );
    assert!(out.stdout.contains("· 1 ambiguous"), "{}", out.stdout);
    // A run with nothing ambiguous says nothing about it.
    let (clean, report) = run_batch("/p/a/lib.rs\n", "clean");
    assert!(
        report.contains("complete\t1\t1\t0\t0\t0\t/p/a/lib.rs\t"),
        "{report}"
    );
    assert!(!clean.stdout.contains("ambiguous"), "{}", clean.stdout);
}

/// One session in a project at `/Users/dev/Projects/w_app`, every record carrying that shell
/// cwd, writing then editing each `(path, first, second)` file.
fn cwd_session(h: &Home, sess: &str, files: &[(&str, &str, &str)]) {
    const CWD: &str = "/Users/dev/Projects/w_app";
    let mut lines = vec![format!(
        r#"{{"type":"user","uuid":"u0","cwd":"{CWD}","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"write the files"}}}}"#
    )];
    for (i, (file, first, second)) in files.iter().enumerate() {
        let s = 10 * (i + 1);
        lines.push(format!(
            r#"{{"type":"assistant","uuid":"w{i}","cwd":"{CWD}","timestamp":"2026-06-07T05:00:{s:02}.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"tw{i}","name":"Write","input":{{"file_path":"{file}","content":"{first}\n"}}}}]}}}}"#
        ));
        lines.push(format!(
            r#"{{"type":"user","uuid":"rw{i}","cwd":"{CWD}","timestamp":"2026-06-07T05:00:{:02}.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"tw{i}","content":"File created successfully at: {file}"}}]}}}}"#,
            s + 1
        ));
        lines.push(format!(
            r#"{{"type":"assistant","uuid":"e{i}","cwd":"{CWD}","timestamp":"2026-06-07T05:00:{:02}.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"te{i}","name":"Edit","input":{{"file_path":"{file}","old_string":"{first}","new_string":"{second}"}}}}]}}}}"#,
            s + 2
        ));
        lines.push(format!(
            r#"{{"type":"user","uuid":"re{i}","cwd":"{CWD}","timestamp":"2026-06-07T05:00:{:02}.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"te{i}","content":"The file {file} has been updated."}}]}}}}"#,
            s + 3
        ));
    }
    h.write(&format!("{ENC}/{sess}.jsonl"), &(lines.join("\n") + "\n"));
}

/// A RELATIVE record path is a candidate like any other: it names the file its record's
/// shell cwd resolves it to. `other/lib.rs` beside `/p/m/lib.rs`, and `a/lib.rs` beside
/// `b/lib.rs`, are two files each, so `--file lib.rs` is refused in both - where replaying
/// them together printed one file's last edit and called it complete. The candidates the
/// refusal names are resolved, and each one passed back replays its own file alone.
#[test]
fn a_suffix_naming_a_relative_record_path_beside_another_file_is_refused() {
    let h = Home::new();
    let mixed = "7a7b7c7d-1111-4222-8333-944455566601";
    let both_relative = "7a7b7c7d-1111-4222-8333-944455566602";
    cwd_session(
        &h,
        mixed,
        &[("/p/m/lib.rs", "m1", "M2"), ("other/lib.rs", "o1", "O2")],
    );
    cwd_session(
        &h,
        both_relative,
        &[("a/lib.rs", "ra1", "RA2"), ("b/lib.rs", "rb1", "RB2")],
    );
    for (sess, candidates) in [
        (
            mixed,
            ["/Users/dev/Projects/w_app/other/lib.rs", "/p/m/lib.rs"],
        ),
        (
            both_relative,
            [
                "/Users/dev/Projects/w_app/a/lib.rs",
                "/Users/dev/Projects/w_app/b/lib.rs",
            ],
        ),
    ] {
        for extra in [vec![], vec!["--coverage"]] {
            let mut argv = vec!["recover", "", "--no-subagents", "--file", "lib.rs"];
            let t = at(sess);
            argv[1] = t.as_str();
            argv.extend(extra.iter().copied());
            let out = h.run(&argv);
            assert!(!out.success, "{sess} {extra:?}: {}", out.stdout);
            assert!(out.stdout.is_empty(), "{sess} {extra:?}: {}", out.stdout);
            assert!(
                out.stderr.contains("names 2 distinct files in scope"),
                "{sess} {extra:?}: {}",
                out.stderr
            );
            for c in candidates {
                assert!(
                    out.stderr.contains(&format!("  {c}\n")),
                    "{sess} names {c}: {}",
                    out.stderr
                );
            }
        }
    }
    let one = |sess: &str, file: &str| -> String {
        let out = h.run(&[
            "recover",
            at(sess).as_str(),
            "--no-subagents",
            "--file",
            file,
        ]);
        assert!(out.success, "{file}: {}", out.stderr);
        out.stdout
    };
    assert_eq!(one(mixed, "/p/m/lib.rs"), "M2\n");
    assert_eq!(one(mixed, "other/lib.rs"), "O2\n");
    assert_eq!(
        one(mixed, "/Users/dev/Projects/w_app/other/lib.rs"),
        "O2\n",
        "the resolved candidate passed back finds the relative record path"
    );
    assert_eq!(one(both_relative, "a/lib.rs"), "RA2\n");
    assert_eq!(
        one(both_relative, "/Users/dev/Projects/w_app/b/lib.rs"),
        "RB2\n"
    );
}

/// A target naming many files lists the first ten candidates and COUNTS the rest - an
/// unscoped `--file README.md` can name hundreds - and says both things replaying them as
/// one file could do: stitch them in one transcript, or keep one file's history and drop
/// the others' across several.
#[test]
fn a_refusal_lists_ten_candidates_and_counts_the_rest() {
    let h = Home::new();
    let files: Vec<(String, String, String)> = (0..13)
        .map(|i| {
            (
                format!("/p/d{i:02}/README.md"),
                format!("r{i}"),
                format!("R{i}"),
            )
        })
        .collect();
    let borrowed: Vec<(&str, &str, &str)> = files
        .iter()
        .map(|(p, a, b)| (p.as_str(), a.as_str(), b.as_str()))
        .collect();
    let sess = "7b7c7d7e-1111-4222-8333-944455566611";
    cwd_session(&h, sess, &borrowed);
    let out = h.run(&["recover", at(sess).as_str(), "--file", "README.md"]);
    assert!(!out.success, "{}", out.stdout);
    assert!(
        out.stderr.contains(
            "--file `README.md` names 13 distinct files in scope - replayed as one file it \
             would stitch their edits into one buffer, or keep one file's history and drop the \
             others':"
        ),
        "{}",
        out.stderr
    );
    let listed = out
        .stderr
        .lines()
        .filter(|l| l.starts_with("  /p/"))
        .count();
    assert_eq!(listed, 10, "{}", out.stderr);
    assert!(
        out.stderr
            .contains("  ... +3 more (a longer suffix narrows the list)\n"),
        "{}",
        out.stderr
    );
    assert!(!out.stderr.contains("/p/d12/"), "{}", out.stderr);
}

/// One session whose shell starts in `/work/proj` (every conversation record carries that
/// cwd), writing then editing each `(path, first, second)` file, with a file-history snapshot
/// line after the first write keying `snapshot_key` RELATIVE and carrying no cwd - the shape
/// Claude Code writes. `cwd` empty writes records with no cwd at all.
fn snapshot_session(
    h: &Home,
    sess: &str,
    cwd: &str,
    snapshot_key: &str,
    files: &[(&str, &str, &str)],
) {
    let cwd_field = if cwd.is_empty() {
        String::new()
    } else {
        format!(r#""cwd":"{cwd}","#)
    };
    let mut lines = vec![format!(
        r#"{{"type":"user","uuid":"u0",{cwd_field}"timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"write the files"}}}}"#
    )];
    for (i, (file, first, second)) in files.iter().enumerate() {
        let s = 10 * (i + 1);
        lines.push(format!(
            r#"{{"type":"assistant","uuid":"w{i}",{cwd_field}"timestamp":"2026-06-07T05:00:{s:02}.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"tw{i}","name":"Write","input":{{"file_path":"{file}","content":"{first}\n"}}}}]}}}}"#
        ));
        lines.push(format!(
            r#"{{"type":"user","uuid":"rw{i}",{cwd_field}"timestamp":"2026-06-07T05:00:{:02}.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"tw{i}","content":"File created successfully at: {file}"}}]}}}}"#,
            s + 1
        ));
        if i == 0 && !snapshot_key.is_empty() {
            lines.push(format!(
                r#"{{"type":"file-history-snapshot","messageId":"m{i}","snapshot":{{"messageId":"m{i}","timestamp":"2026-06-07T05:00:{:02}.000Z","trackedFileBackups":{{"{snapshot_key}":{{"backupFileName":"feed0003@v1","version":1,"backupTime":"2026-06-07T05:00:{:02}.000Z"}}}}}},"isSnapshotUpdate":false}}"#,
                s + 2,
                s + 2
            ));
        }
        lines.push(format!(
            r#"{{"type":"assistant","uuid":"e{i}",{cwd_field}"timestamp":"2026-06-07T05:00:{:02}.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"te{i}","name":"Edit","input":{{"file_path":"{file}","old_string":"{first}","new_string":"{second}"}}}}]}}}}"#,
            s + 3
        ));
        lines.push(format!(
            r#"{{"type":"user","uuid":"re{i}",{cwd_field}"timestamp":"2026-06-07T05:00:{:02}.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"te{i}","content":"The file {file} has been updated."}}]}}}}"#,
            s + 4
        ));
    }
    h.write(&format!("{ENC}/{sess}.jsonl"), &(lines.join("\n") + "\n"));
}

/// A file-history key is relative to the directory the session started in and its line
/// carries no cwd, so it names the SAME file as the absolute Write/Edit path it is the tail
/// of: a suffix target replays that one file, in both modes and from a manifest, where
/// counting the raw key as a second file refused every suffix.
#[test]
fn a_relative_snapshot_key_names_the_file_under_the_transcripts_first_cwd() {
    let h = Home::new();
    let sess = "7c7d7e7f-1111-4222-8333-944455566621";
    snapshot_session(
        &h,
        sess,
        "/work/proj",
        "src/lib.rs",
        &[("/work/proj/src/lib.rs", "one", "two")],
    );
    for target in ["lib.rs", "src/lib.rs", "/work/proj/src/lib.rs"] {
        let out = h.run(&["recover", at(sess).as_str(), "--file", target]);
        assert!(out.success, "{target}: {}", out.stderr);
        assert_eq!(out.stdout, "two\n", "{target}");
        let cov = h.run(&["recover", at(sess).as_str(), "--file", target, "--coverage"]);
        assert!(cov.success, "{target} --coverage: {}", cov.stderr);
        assert!(
            cov.stdout.contains("recoverable: 1/1 lines"),
            "{}",
            cov.stdout
        );
    }
    let manifest = h.root.join("manifest.txt");
    std::fs::write(&manifest, "lib.rs\n").unwrap();
    let out_dir = h.root.join("recovered");
    let out = h.run(&[
        "recover",
        at(sess).as_str(),
        "--files-from",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
    ]);
    assert!(out.success, "{}", out.stderr);
    assert!(
        out.stdout.contains("recovered 1 file(s): 1 complete") && !out.stdout.contains("ambiguous"),
        "{}",
        out.stdout
    );
    assert_eq!(
        std::fs::read_to_string(out_dir.join("lib.rs")).unwrap(),
        "two\n"
    );
}

/// Every candidate the several-files refusal prints, passed back as `--file`, replays that
/// file alone: a resolved absolute candidate by the ordinary rule, and a raw relative one
/// (a transcript with no cwd anywhere) because the target IS that candidate - the other
/// file ends with it, so without that rule the refusal's own advice would be refused again.
/// Two absolute files sharing a tail stay refused; the relative key folds into its own.
#[test]
fn every_candidate_a_refusal_prints_replays_when_passed_back() {
    let h = Home::new();
    let keyed = "7c7d7e7f-1111-4222-8333-944455566622";
    let cwdless = "7c7d7e7f-1111-4222-8333-944455566623";
    snapshot_session(
        &h,
        keyed,
        "/work/proj",
        "src/lib.rs",
        &[
            ("/work/proj/src/lib.rs", "one", "two"),
            ("/other/src/lib.rs", "x1", "X2"),
        ],
    );
    snapshot_session(
        &h,
        cwdless,
        "",
        "",
        &[("a/lib.rs", "r1", "R2"), ("/w/a/lib.rs", "w1", "W2")],
    );
    for (sess, want) in [
        (
            keyed,
            vec![
                ("/other/src/lib.rs", "X2\n"),
                ("/work/proj/src/lib.rs", "two\n"),
            ],
        ),
        (cwdless, vec![("/w/a/lib.rs", "W2\n"), ("a/lib.rs", "R2\n")]),
    ] {
        let refused = h.run(&["recover", at(sess).as_str(), "--file", "lib.rs"]);
        assert!(!refused.success, "{sess}: {}", refused.stdout);
        let printed: Vec<&str> = refused
            .stderr
            .lines()
            .filter_map(|l| l.strip_prefix("  "))
            .filter(|l| !l.starts_with("..."))
            .collect();
        assert_eq!(
            printed,
            want.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
            "{}",
            refused.stderr
        );
        for (candidate, content) in &want {
            let out = h.run(&["recover", at(sess).as_str(), "--file", candidate]);
            assert!(out.success, "{sess} {candidate}: {}", out.stderr);
            assert_eq!(out.stdout, *content, "{sess} {candidate}");
        }
    }
    // The raw candidate passed back says what it did; a manifest line takes the same rule.
    let raw = h.run(&["recover", at(cwdless).as_str(), "--file", "a/lib.rs"]);
    assert!(
        raw.stderr.contains(
            "note: --file `a/lib.rs` is itself one of the 2 files it names in scope, every other \
             one ending with it: replaying `a/lib.rs` alone"
        ),
        "{}",
        raw.stderr
    );
    let manifest = h.root.join("manifest.txt");
    std::fs::write(&manifest, "a/lib.rs\n").unwrap();
    let out_dir = h.root.join("recovered");
    let batch = h.run(&[
        "recover",
        at(cwdless).as_str(),
        "--files-from",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
    ]);
    assert!(batch.success, "{}", batch.stderr);
    assert!(
        batch
            .stderr
            .contains("note: manifest line `a/lib.rs` is itself one of the 2 files"),
        "{}",
        batch.stderr
    );
    assert_eq!(
        std::fs::read_to_string(out_dir.join("a/lib.rs")).unwrap(),
        "R2\n"
    );
}
