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

/// One session, NO subagent sidecar: the scope banner is a `println!` that runs BEFORE the
/// buffer exists and is suppressed when nothing was spanned, so every stdout byte these
/// commands emit is a byte the buffer owns.
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
    // The three stream SHAPES the buffer carries: rendered text, the JSON envelope, and a
    // raw byte dump. Each is checked twice - once with an ordinary stdout, so a failure
    // below can only be the closed pipe, and once with the pipe closed.
    let modes: [(&str, Vec<&str>); 3] = [
        ("search text", vec!["search", "needle", t.as_str()]),
        (
            "search json",
            vec!["search", "needle", t.as_str(), "--format", "json"],
        ),
        (
            "show --raw",
            vec!["show", t.as_str(), "--line", "1..16", "--raw"],
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
