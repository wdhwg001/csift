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
            out.stderr.contains("pass one of these absolute paths"),
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
    assert!(
        out.stderr
            .contains("not replayed: --file `lib.rs` names 3 distinct files"),
        "{}",
        out.stderr
    );
    assert!(out.stdout.contains("1 ambiguous"), "{}", out.stdout);
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
