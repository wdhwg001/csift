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
//!
//! The print lint that holds all of this is at the PRINT SITE, and that leaves one
//! residual a call cannot be read off the calling module: `text::emit_scope_banner` and
//! `path::emit_duplicate_banner` print with `println!` from files that carry no deny,
//! because both legitimately print. So a call into either from inside a buffered module
//! compiles clean, passes clippy, and puts a `println!` in the middle of a buffered stream.
//! The last test here is the sweep that closes it.

use crate::harness::*;

use std::io::Read as _;
use std::path::Path;

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

/// The inner attribute every module that builds or writes through an `Out` carries.
const PRINT_DENY: &str = "#![deny(clippy::print_stdout";

/// The two banner sinks that print with `println!`. Their writer twins (`write_scope_banner`
/// / `write_duplicate_banner`) are what a buffered surface calls, and neither name contains
/// either of these, so a plain substring separates them.
const PRINT_SINKS: [&str; 2] = ["emit_scope_banner", "emit_duplicate_banner"];

/// POSITIVE CONTROL for the CALLER half. These three modules print every row with
/// `println!` and reach a print-form sink today; if the sweep stops finding them it has
/// narrowed, and a narrowed sweep reads as a clean bill of health while checking nothing.
/// A module that moves out of this list either gained an `Out` (then it calls the writer
/// form and belongs in the list below) or stopped printing a banner (then say so here).
const PRINT_FORM_CALLERS: [&str; 3] = [
    "src/commands/recover/render.rs",
    "src/commands/session/render.rs",
    "src/commands/stats/render.rs",
];

/// POSITIVE CONTROL for the GUARDED half, which can narrow the same way: these five are
/// the buffered render entry points, and each must be seen carrying the deny.
const GUARDED_CONTROLS: [&str; 5] = [
    "src/commands/agents/render.rs",
    "src/commands/files/render.rs",
    "src/commands/image/render.rs",
    "src/commands/search/render.rs",
    "src/commands/turns/render.rs",
];

/// Every `.rs` under `src/`, as (repo-relative path, source with LINE COMMENTS removed).
/// Comments go because a doc line NAMING a sink is prose and not a call - `text/out.rs`
/// names both while calling neither. The crate uses no block comments, so a line-wise
/// strip is the whole of it. Read from the manifest directory, never the process cwd.
fn crate_sources() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut stack = vec![src.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read a directory under src/") {
            let path = entry.expect("a directory entry under src/").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).expect("read a source file");
                let code = text
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("//"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let rel = path
                    .strip_prefix(src.parent().expect("src has a parent"))
                    .expect("under the crate root")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, code));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn no_module_that_denies_the_print_lints_calls_a_println_form_banner_sink() {
    let sources = crate_sources();
    assert!(
        sources.len() > 100,
        "the walk found only {} source files - it is not reaching src/, so everything \
         below is vacuously true",
        sources.len()
    );

    let guarded: Vec<&str> = sources
        .iter()
        .filter(|(_, code)| code.contains(PRINT_DENY))
        .map(|(path, _)| path.as_str())
        .collect();
    for want in GUARDED_CONTROLS {
        assert!(
            guarded.contains(&want),
            "{want} builds an `Out` and must be seen carrying `{PRINT_DENY}`; the guarded \
             set found was {guarded:?}"
        );
    }

    let callers: Vec<&str> = sources
        .iter()
        .filter(|(_, code)| PRINT_SINKS.iter().any(|sink| code.contains(sink)))
        .map(|(path, _)| path.as_str())
        .collect();
    for want in PRINT_FORM_CALLERS {
        assert!(
            callers.contains(&want),
            "{want} calls a print-form banner sink and must be seen doing it; the callers \
             found were {callers:?}"
        );
    }

    let offenders: Vec<&str> = callers
        .iter()
        .copied()
        .filter(|path| guarded.contains(path))
        .collect();
    assert!(
        offenders.is_empty(),
        "{offenders:?} deny the print lints, so they write through an `Out` - and they name \
         one of {PRINT_SINKS:?}, which print with `println!` from a file that carries no \
         deny. Clippy cannot see that: the print is in `text.rs` / `path/dedup.rs`, which \
         legitimately print. Such a call lands ahead of everything the buffer holds and \
         aborts the process on a closed pipe. Call `write_scope_banner` / \
         `write_duplicate_banner` on the surface's own writer instead."
    );
}
