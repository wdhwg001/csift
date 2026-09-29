//! An empty target token is refused by name on every command that takes targets.

use crate::harness::*;

/// `""` read as a path is the empty relative path, which resolves to the current directory's
/// project: `csift search x ""` searched it and exited 0. Every target-taking command - the
/// twelve that take a path target and the four channel spots that take a lane string -
/// refuses the token at parse time (exit 2) with the grammar in the message.
#[test]
fn every_target_taking_command_refuses_an_empty_target() {
    let h = populated_home();
    let argvs: [&[&str]; 16] = [
        &["list", ""],
        &["search", "x", ""],
        &["stats", ""],
        &["files", ""],
        &["recover", ""],
        &["plan", ""],
        &["image", ""],
        &["verbatim", ""],
        &["agents", ""],
        &["show", "", "--line", "1"],
        &["status", ""],
        &["wait", "", "--until", "stop", "--timeout", "1"],
        // The channel's lane targets are strings rather than paths, and each was read as the
        // current directory's project: `send ""` queued a message there.
        &["send", "", "hello"],
        &["whoami", "--to", ""],
        &["msg", "--lane", ""],
        &["ack", "0123456789abcdef", "--lane", ""],
    ];
    for argv in argvs {
        let out = h.run(argv);
        assert_eq!(out.code, Some(2), "{argv:?}: {}{}", out.stdout, out.stderr);
        assert!(
            out.stderr.contains("EMPTY target") && out.stderr.contains("@<uuid>"),
            "{argv:?}: {}",
            out.stderr
        );
    }
}

/// The empty PATTERN is not a target: `search ""` stays the pure filter it always was.
#[test]
fn the_empty_search_pattern_is_still_a_pattern() {
    let h = populated_home();
    let out = h.run(&["search", "", at(SESS).as_str(), "-c"]);
    assert!(out.success, "{}", out.stderr);
}
