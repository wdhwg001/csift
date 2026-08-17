//! agents targeting and errors: bad ids, span-flag rejection, empty scopes.

use crate::harness::*;

#[test]
fn agents_bad_hex_errors_with_discovery_guidance() {
    // A typo'd / non-existent --agent hex is a HARD error (non-zero) with discovery
    // guidance — NOT the ambiguous `no subagents found` that a zero-subagent session prints.
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--agent", "deadbeefcafe"]);
    assert!(!out.success, "a bad hex must be a hard error");
    assert!(
        out.stderr.contains("no subagent matched") && out.stderr.contains("agents @<uuid>"),
        "error must name the bad id + the discovery path; stderr: {}",
        out.stderr
    );
}

#[test]
fn agents_rejects_no_subagents_with_pointed_error() {
    // `agents --no-subagents` (a flag it does not have) is rejected with a pointed message,
    // NOT swallowed as a bogus PATH value by allow_hyphen_values.
    let h = populated_home();
    let out = h.run(&["agents", at(SESS).as_str(), "--no-subagents"]);
    assert!(!out.success, "the no-op span flag must error");
    assert!(
        out.stderr.contains("no subagent-span flag"),
        "stderr should explain agents has no span flag; got: {}",
        out.stderr
    );
}

#[test]
fn agents_no_subagents_says_none() {
    let h = Home::new();
    // A session with no sidecar at all.
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    let out = h.run(&["agents", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no subagents found"),
        "got: {}",
        out.stdout
    );
}

#[test]
fn agents_unknown_session_errors() {
    let h = populated_home();
    let out = h.run(&[
        "agents",
        at("deadbeef-0000-0000-0000-000000000000").as_str(),
    ]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("no session file found"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn agents_via_project_path_target() {
    // Drive agents with a PATH target (not --session): resolve_target_sessions takes
    // the explicit-paths branch, enumerates the project's sessions, groups subagents.
    let h = populated_home();
    let out = h.run(&["agents", ENC]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("builtin-task"));
    assert!(out.stdout.contains("workflow"));
    // The agentType sub-label is rendered in [brackets].
    assert!(
        out.stdout.contains("[oh-my-claudecode:executor]")
            || out.stdout.contains("[workflow-subagent]")
    );
}

#[test]
fn agents_all_projects_default_scan() {
    // No PATH and no --session → scan every project (the all_project_dirs branch).
    let h = populated_home();
    let out = h.run(&["agents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("subagent(s)"));
}

#[test]
fn agents_path_with_no_sessions_and_no_session_flag_is_empty_not_error() {
    // A project dir that exists but has ZERO session files, with NO --session given →
    // `resolve_target_sessions` finds no files but does NOT bail (the `if let
    // Some(sid)` FALSE arm of the empty-files guard). Output: "no subagents found".
    let h = Home::new();
    std::fs::create_dir_all(h.projects().join(ENC)).unwrap(); // empty project dir
    let out = h.run(&["agents", ENC]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no subagents found"),
        "got: {}",
        out.stdout
    );
}
