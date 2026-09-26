//! One agent id at BOTH sidecar locations of one session: a workflow run's
//! `subagents/workflows/<run>/agent-<id>.jsonl` and the built-in `subagents/agent-<id>.jsonl`
//! the conversation continued in. Every surface reads both halves for `@<id>`, names where
//! they sit, and a `*.jsonl` path reads exactly the file it names.

use super::*;

const TWO_SESS: &str = "5e6f7a8b-9c0d-4e1f-8a2b-3c4d5e6f7a8b";
const TWO_AGENT: &str = "a0f1e2d3c4b5a6978";
const TWO_ENC: &str = "-Users-testuser-Projects-twoshapes";

fn line(uuid: &str, parent: Option<&str>, ts: &str, text: &str) -> String {
    let parent = parent.map_or("null".to_string(), |p| format!("\"{p}\""));
    format!(
        "{{\"type\":\"user\",\"uuid\":\"{uuid}\",\"parentUuid\":{parent},\"isSidechain\":true,\
         \"agentId\":\"{TWO_AGENT}\",\"cwd\":\"/Users/testuser/Projects/twoshapes\",\
         \"timestamp\":\"{ts}\",\"message\":{{\"role\":\"user\",\"content\":\"{text}\"}}}}\n"
    )
}

/// The session, a three-line workflow half and a two-line continuation parented to the
/// workflow half's last record, as the specimen on disk is.
fn two_shape_home() -> (Home, String, String) {
    let h = Home::new();
    h.write(
        &format!("{TWO_ENC}/{TWO_SESS}.jsonl"),
        "{\"type\":\"user\",\"uuid\":\"s1\",\"cwd\":\"/Users/testuser/Projects/twoshapes\",\
         \"timestamp\":\"2026-06-07T01:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"start\"}}\n",
    );
    let workflow =
        format!("{TWO_ENC}/{TWO_SESS}/subagents/workflows/wf_run1/agent-{TWO_AGENT}.jsonl");
    let builtin = format!("{TWO_ENC}/{TWO_SESS}/subagents/agent-{TWO_AGENT}.jsonl");
    h.write(
        &workflow,
        &[
            line(
                "w1",
                None,
                "2026-06-07T01:00:01.000Z",
                "harborworkflow task",
            ),
            line(
                "w2",
                Some("w1"),
                "2026-06-07T01:00:02.000Z",
                "harborworkflow step",
            ),
            line(
                "w3",
                Some("w2"),
                "2026-06-07T01:00:03.000Z",
                "harborworkflow done",
            ),
        ]
        .concat(),
    );
    h.write(
        &builtin,
        &[
            line(
                "c1",
                Some("w3"),
                "2026-06-07T01:00:04.000Z",
                "reefcontinued ask",
            ),
            line(
                "c2",
                Some("c1"),
                "2026-06-07T01:00:05.000Z",
                "reefcontinued reply",
            ),
        ]
        .concat(),
    );
    let root = h.projects();
    let abs = |rel: &str| root.join(rel).to_str().expect("utf-8").to_string();
    let (w, b) = (abs(&workflow), abs(&builtin));
    (h, w, b)
}

#[test]
fn an_agent_id_reads_both_halves_and_names_where_they_sit() {
    let (h, _, _) = two_shape_home();
    for (pattern, half) in [
        ("harborworkflow", "workflow"),
        ("reefcontinued", "built-in"),
    ] {
        let out = h.run(&["search", pattern, &at(TWO_AGENT), "-c"]);
        assert!(out.success, "stderr: {}", out.stderr);
        assert!(
            out.stdout.trim() != "0",
            "the {half} half is part of `@<id>`:\n{}",
            out.stdout
        );
    }
    let stats = h.run(&["stats", &at(TWO_AGENT), "--no-subagents"]);
    assert!(stats.success, "stderr: {}", stats.stderr);
    assert!(
        stats.stdout.contains("lines 3") && stats.stdout.contains("lines 2"),
        "both transcripts are in scope:\n{}",
        stats.stdout
    );
    assert!(
        stats
            .stderr
            .contains("under one session's sidecar at the built-in and the workflow locations")
            && !stats.stderr.contains("under 2 project directories"),
        "the note names where the two sit:\n{}",
        stats.stderr
    );
    // The whole-root enumeration meets the same pair and says the same thing.
    let list = h.run(&["list"]);
    assert!(list.success, "stderr: {}", list.stderr);
    assert!(
        list.stderr
            .contains("under one session's sidecar at the built-in and the workflow locations"),
        "{}",
        list.stderr
    );
    let agents = h.run(&["agents", &at(TWO_SESS)]);
    assert!(agents.success, "stderr: {}", agents.stderr);
    assert!(
        agents
            .stderr
            .contains("under one session's sidecar at the built-in and the workflow locations"),
        "{}",
        agents.stderr
    );
    // `show` still refuses an address that names two files, and says where they are.
    let show = h.run(&["show", &at(TWO_AGENT), "--line", "1"]);
    assert!(!show.success, "stdout: {}", show.stdout);
    assert!(
        show.stderr.contains("at different paths on disk"),
        "{}",
        show.stderr
    );
}

#[test]
fn a_jsonl_path_reads_the_file_it_names_and_names_the_other() {
    let (h, workflow, builtin) = two_shape_home();
    for (path, own, other) in [
        (&builtin, "lines 2", "lines 3"),
        (&workflow, "lines 3", "lines 2"),
    ] {
        let out = h.run(&["stats", path, "--no-subagents"]);
        assert!(out.success, "stderr: {}", out.stderr);
        assert!(
            out.stdout.contains(own) && !out.stdout.contains(other),
            "the path reads the file it names, never the other half:\n{}",
            out.stdout
        );
        assert!(
            out.stderr.contains("has 1 other transcript in scope")
                && out.stderr.contains("reads only the file it names"),
            "{}",
            out.stderr
        );
    }
    let show = h.run(&["show", &builtin, "--line", "1"]);
    assert!(show.success, "stderr: {}", show.stderr);
    assert!(show.stdout.contains("reefcontinued ask"), "{}", show.stdout);
    let search = h.run(&["search", "harborworkflow", &builtin, "-c"]);
    assert!(search.success, "stderr: {}", search.stderr);
    assert_eq!(
        search.stdout.trim(),
        "0",
        "the named file holds no workflow line"
    );
}
