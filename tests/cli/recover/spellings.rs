//! recover target spellings: a path is compared LEXICALLY, on the target and every record
//! path alike, so two spellings of one file are one file.

use crate::harness::*;

/// One session writing then editing each `(path, first, second)` file in order, every record
/// carrying `cwd` (none when empty). Input-side events, the shape a subagent transcript writes.
fn spelled_session(h: &Home, sess: &str, cwd: &str, files: &[(&str, &str, &str)]) {
    let cwd_field = if cwd.is_empty() {
        String::new()
    } else {
        format!(r#""cwd":"{cwd}","#)
    };
    let mut lines = vec![format!(
        r#"{{"type":"user","uuid":"u0",{cwd_field}"timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"write it"}}}}"#
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
        lines.push(format!(
            r#"{{"type":"assistant","uuid":"e{i}",{cwd_field}"timestamp":"2026-06-07T05:00:{:02}.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"te{i}","name":"Edit","input":{{"file_path":"{file}","old_string":"{first}","new_string":"{second}"}}}}]}}}}"#,
            s + 2
        ));
        lines.push(format!(
            r#"{{"type":"user","uuid":"re{i}",{cwd_field}"timestamp":"2026-06-07T05:00:{:02}.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"te{i}","content":"The file {file} has been updated."}}]}}}}"#,
            s + 3
        ));
    }
    h.write(&format!("{ENC}/{sess}.jsonl"), &(lines.join("\n") + "\n"));
}

fn restored(h: &Home, sess: &str, file: &str) -> String {
    let out = h.run(&[
        "recover",
        at(sess).as_str(),
        "--no-subagents",
        "--file",
        file,
    ]);
    assert!(out.success, "{sess} {file}: {}", out.stderr);
    out.stdout
}

/// One session: its cwd, the two spellings it writes then edits, the file's last content,
/// and every target spelling that must replay it.
struct Spelled {
    sess: &'static str,
    cwd: &'static str,
    files: [(&'static str, &'static str, &'static str); 2],
    want: &'static str,
    targets: &'static [&'static str],
}

/// `./a/lib.rs` then `a/lib.rs`, `a//lib.rs` then `a/lib.rs`, `/p//q/lib.rs` then
/// `/p/q/lib.rs` - with a cwd on every record and with none - are one file each: every
/// spelling of the target replays the file's whole history, where the spelling the first
/// write used replayed that write's events alone and called the stale content complete.
#[test]
fn every_spelling_of_one_file_replays_its_whole_history() {
    let h = Home::new();
    let cases = [
        Spelled {
            sess: "7d7e7f70-1111-4222-8333-944455566631",
            cwd: "/work/w_app",
            files: [("./a/lib.rs", "c1", "C2"), ("a/lib.rs", "C2", "C3")],
            want: "C3\n",
            targets: &[
                "./a/lib.rs",
                "a/lib.rs",
                "lib.rs",
                "a//lib.rs",
                "/work/w_app/a/lib.rs",
            ],
        },
        Spelled {
            sess: "7d7e7f70-1111-4222-8333-944455566632",
            cwd: "/work/w_app",
            files: [("a//lib.rs", "s1", "S2"), ("a/lib.rs", "S2", "S3")],
            want: "S3\n",
            targets: &["a//lib.rs", "a/lib.rs", "lib.rs", "/work/w_app/a/lib.rs"],
        },
        Spelled {
            sess: "7d7e7f70-1111-4222-8333-944455566633",
            cwd: "/work/w_app",
            files: [("/p//q/lib.rs", "t1", "T2"), ("/p/q/lib.rs", "T2", "T3")],
            want: "T3\n",
            targets: &[
                "/p//q/lib.rs",
                "/p/q/lib.rs",
                "q/lib.rs",
                "lib.rs",
                "/p/./q/lib.rs",
            ],
        },
        Spelled {
            sess: "7d7e7f70-1111-4222-8333-944455566634",
            cwd: "",
            files: [("./a/lib.rs", "e1", "E2"), ("a/lib.rs", "E2", "E3")],
            want: "E3\n",
            targets: &["./a/lib.rs", "a/lib.rs", "lib.rs"],
        },
    ];
    for c in cases {
        spelled_session(&h, c.sess, c.cwd, &c.files);
        for t in c.targets {
            assert_eq!(restored(&h, c.sess, t), c.want, "{} --file {t}", c.sess);
        }
    }
}

/// What needs the filesystem is not resolved: a `..` segment stays as written, and a leading
/// `//` stays (POSIX leaves it to the implementation; it is the Windows UNC shape), so each
/// pair below is two files to csift, refused as two, and each passed back replays its own.
#[test]
fn a_dot_dot_segment_and_a_leading_double_slash_are_not_normalized_away() {
    let h = Home::new();
    for (sess, a, b) in [
        (
            "7d7e7f70-1111-4222-8333-944455566635",
            "/p/x/../q/lib.rs",
            "/p/q/lib.rs",
        ),
        (
            "7d7e7f70-1111-4222-8333-944455566636",
            "//srv/q/lib.rs",
            "/srv/q/lib.rs",
        ),
    ] {
        spelled_session(&h, sess, "/work/w_app", &[(a, "a1", "A2"), (b, "b1", "B2")]);
        let out = h.run(&["recover", at(sess).as_str(), "--file", "lib.rs"]);
        assert!(!out.success, "{sess}: {}", out.stdout);
        assert!(
            out.stderr.contains("names 2 distinct files in scope"),
            "{}",
            out.stderr
        );
        assert_eq!(restored(&h, sess, a), "A2\n", "{a}");
        assert_eq!(restored(&h, sess, b), "B2\n", "{b}");
    }
}
