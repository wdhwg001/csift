//! A failed write to stdout is an ORDINARY error: never a panic, never silence.
//!
//! Every row-dumping surface writes its rows through one buffered `text::Out`, so the
//! bytes reach the OS at the flush rather than one per row. That moves WHERE a failed
//! write is seen, and the output contract fixes what must happen when it is: the error
//! reaches `main`'s one `Err` arm and is reported in csift's own voice on stderr, with a
//! non-zero exit, exactly like any other failure.
//!
//! Both ways of getting that wrong are reachable from here. `println!` PANICS on a write
//! error - exit 101 and a std backtrace in place of csift's message, which is what a
//! `| head` read used to produce. And `BufWriter::drop` SWALLOWS one - exit 0 with the
//! stream short and nothing said, which is the silent failure the output contract forbids
//! and which is what an `Out::flush` that returned `Ok(())` would leave behind.

use crate::harness::*;

use std::io::Read as _;

/// Run csift with stdout wired to a pipe whose READ END IS ALREADY CLOSED, returning the
/// exit code and stderr. Every write the child makes fails with a broken-pipe error from
/// its first byte, which is what makes this deterministic: a reader that goes away AFTER
/// the spawn (`| head -c 1`) leaves a window in which a small stream lands whole in the
/// pipe buffer and no write ever fails.
fn into_closed_pipe(h: &Home, args: &[&str]) -> (Option<i32>, String) {
    let (reader, writer) = std::io::pipe().expect("an anonymous pipe");
    drop(reader);
    let mut child = h
        .cmd(args)
        .stdout(std::process::Stdio::from(writer))
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn csift");
    let mut err = String::new();
    child
        .stderr
        .take()
        .expect("piped stderr")
        .read_to_string(&mut err)
        .expect("read stderr");
    let code = child.wait().expect("wait csift").code();
    (code, err)
}

/// One session, NO subagent sidecar and no second copy: neither banner fires, so every
/// stdout byte these commands emit is a row.
fn buffered_only_home() -> (Home, String) {
    let h = Home::new();
    let sess = "11111111-2222-4333-8444-555555555555";
    let mut jsonl = String::new();
    for i in 0..8 {
        jsonl.push_str(&format!(
            r#"{{"type":"user","uuid":"u{i}","timestamp":"2026-06-07T05:0{i}:00.000Z","message":{{"role":"user","content":"carry needle {i}"}}}}"#
        ));
        jsonl.push('\n');
        jsonl.push_str(&format!(
            r#"{{"type":"assistant","uuid":"a{i}","parentUuid":"u{i}","timestamp":"2026-06-07T05:0{i}:05.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"reply needle {i}"}}]}}}}"#
        ));
        jsonl.push('\n');
    }
    h.write(&format!("-Users-dev-proj/{sess}.jsonl"), &jsonl);
    (h, at(sess))
}

#[test]
fn a_closed_stdout_is_reported_as_an_error_on_every_buffered_stream() {
    let (h, t) = buffered_only_home();
    // The stream SHAPES the buffer carries: rendered text, the JSON envelope, a raw byte
    // dump, and `verbatim`'s two, which are the surface that joined the buffered set last
    // and so the one a later change is most likely to take back out. Each is checked twice
    // - once with an ordinary stdout, so a failure below can only be the closed pipe, and
    // once with the pipe closed.
    let modes: [(&str, Vec<&str>); 5] = [
        ("search text", vec!["search", "needle", t.as_str()]),
        (
            "search json",
            vec!["search", "needle", t.as_str(), "--format", "json"],
        ),
        (
            "show --raw",
            vec!["show", t.as_str(), "--line", "1..16", "--raw"],
        ),
        ("verbatim text", vec!["verbatim", t.as_str()]),
        (
            "verbatim json",
            vec!["verbatim", t.as_str(), "--format", "json"],
        ),
    ];
    for (what, args) in modes {
        let ok = h.run(&args);
        assert!(ok.success, "{what}: control run failed: {}", ok.stderr);
        assert!(
            !ok.stdout.is_empty(),
            "{what}: control run emitted nothing, so the closed-pipe run proves nothing"
        );

        let (code, err) = into_closed_pipe(&h, &args);
        assert!(
            code.is_some_and(|c| c != 0),
            "{what}: a write that could not land must exit non-zero, got {code:?} - an \
             `Out::flush` that cannot fail reports success on a stream the consumer never \
             received. stderr: {err}"
        );
        assert!(
            err.contains("csift: error:"),
            "{what}: the failure reaches main's one Err arm and is reported in csift's own \
             voice: {err}"
        );
        assert!(
            !err.contains("panicked"),
            "{what}: a write error is an error, not a panic - `println!` aborts here: {err}"
        );
    }
}

/// One session whose scope carries a SUBAGENT (the `scope` banner) and whose transcript
/// also sits under a second project directory (the `copies` banner). Both banners precede
/// every row their surface prints, and both were `println!` sites outside the buffer - so
/// the same query answered a closed pipe one way with a banner and another way without,
/// which is the split this fixture exists to keep shut.
fn banner_home() -> (Home, String) {
    let h = Home::new();
    let sess = "11111111-2222-4333-8444-555555555555";
    let one = "-Users-testuser-Projects-one";
    let img = img_block("image/png", PNG_1X1);
    let body = format!(
        "{}\n{}\n{}\n{}\n",
        serde_json::json!({
            "type":"user","uuid":"u0","sessionId":sess,"cwd":"/Users/testuser/Projects/one",
            "timestamp":"2026-06-07T05:00:00.000Z",
            "message":{"role":"user","content":[{"type":"text","text":"carry needle here"}, img]}
        }),
        serde_json::json!({
            "type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z",
            "message":{"role":"assistant","content":[{"type":"text","text":"reply needle"}]}
        }),
        serde_json::json!({
            "type":"assistant","uuid":"a1","parentUuid":"a0","timestamp":"2026-06-07T05:00:06.000Z",
            "message":{"role":"assistant","content":[{"type":"tool_use","id":"call0","name":"Write",
                "input":{"file_path":"/Users/testuser/Projects/one/greet.py","content":"print('hi')\n"}}]}
        }),
        serde_json::json!({
            "type":"user","uuid":"c0","parentUuid":"a1","timestamp":"2026-06-07T05:00:07.000Z",
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call0","content":"wrote it"}]}
        }),
    );
    h.write(&format!("{one}/{sess}.jsonl"), &body);
    // The SECOND copy, byte-identical: what a renamed project's copied directory leaves.
    // Its directory name matches no record's cwd, so the merge keeps the copy above and
    // every spanning surface prints a `copies` line for it.
    h.write(
        &format!("-Users-testuser-Projects-renamed/{sess}.jsonl"),
        &body,
    );
    h.write(
        &format!("{one}/{sess}/subagents/agent-aaa111.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aaa111","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"sub: carry needle"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"sub done"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{one}/{sess}/subagents/agent-aaa111.meta.json"),
        r#"{"agentType":"explore","description":"carry","toolUseId":"toolu_x"}"#,
    );
    (h, at(sess))
}

#[test]
fn a_banner_is_part_of_the_stream_it_heads_not_an_exception_to_it() {
    let (h, t) = banner_home();
    // Every buffered surface that can print a banner, with one present. The banner is the
    // FIRST stdout byte of each, so if it is written outside the buffer the whole row
    // stream answers a closed pipe with a process abort instead of csift's error - and
    // does so only on the scopes wide enough to print one, which is a carve-out no reader
    // can predict from the command they typed.
    let modes: [(&str, Vec<&str>); 6] = [
        ("search text", vec!["search", "needle", t.as_str()]),
        ("files", vec!["files", t.as_str()]),
        (
            "files --by timeline",
            vec!["files", t.as_str(), "--by", "timeline"],
        ),
        ("agents", vec!["agents", t.as_str()]),
        ("image", vec!["image", t.as_str()]),
        (
            "verbatim --subagents",
            vec!["verbatim", t.as_str(), "--subagents"],
        ),
    ];
    for (what, args) in modes {
        let ok = h.run(&args);
        assert!(ok.success, "{what}: control run failed: {}", ok.stderr);
        assert!(
            ok.stdout.starts_with("scope  ") || ok.stdout.starts_with("copies "),
            "{what}: this fixture exists so a BANNER is the first stdout byte; it is not, \
             so the run below would prove nothing:\n{}",
            ok.stdout
        );

        let (code, err) = into_closed_pipe(&h, &args);
        assert_eq!(
            code,
            Some(1),
            "{what}: a banner is part of the stream it heads - a write that cannot land is \
             reported like any other, not aborted inside `println!`. stderr: {err}"
        );
        assert!(
            err.contains("csift: error: Broken pipe"),
            "{what}: the failure reaches main's one Err arm: {err}"
        );
        assert!(
            !err.contains("panicked"),
            "{what}: the banner is written through the buffer, so nothing aborts: {err}"
        );
    }
}
