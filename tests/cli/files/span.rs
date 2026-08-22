//! files subagent spanning and id-domain discrimination.

use crate::harness::*;

#[test]
fn files_spans_subagent_mutations() {
    // A subagent that Writes a file → its mutation is attributed under the session by
    // default (OMC fan-out edits happen in subagents). --no-subagents drops it.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-sub111.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"sub111","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"sub: write a file"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sw1","name":"Write","input":{"file_path":"/tmp/subagent-out.md","content":"z"}}]}}"#, "\n",
        ),
    );
    let with = h.run(&["files", at(SESS).as_str()]);
    assert!(with.success, "stderr: {}", with.stderr);
    assert!(
        with.stdout.contains("/tmp"),
        "subagent write spanned: {}",
        with.stdout
    );
    let without = h.run(&["files", at(SESS).as_str(), "--no-subagents"]);
    assert!(without.success, "stderr: {}", without.stderr);
    assert!(
        without.stdout.contains("no file mutations found"),
        "--no-subagents drops the subagent write: {}",
        without.stdout
    );
}

#[test]
fn files_default_spans_subagents_and_no_subagents_restricts() {
    let h = Home::new();
    subagents_only_scenario(&h);

    // Default (spans subagents): BOTH the parent and subagent files surface.
    let with = h.run(&["files", at(SESS).as_str(), "--by", "file"]);
    assert!(with.success, "stderr: {}", with.stderr);
    assert!(with.stdout.contains("/parent/p.md"), "got: {}", with.stdout);
    assert!(with.stdout.contains("/sub/s.md"), "got: {}", with.stdout);

    // --no-subagents: ONLY the parent file (the subagent mutation drops out).
    let top = h.run(&["files", at(SESS).as_str(), "--by", "file", "--no-subagents"]);
    assert!(top.success, "stderr: {}", top.stderr);
    assert!(top.stdout.contains("/parent/p.md"), "got: {}", top.stdout);
    assert!(
        !top.stdout.contains("/sub/s.md"),
        "--no-subagents must exclude the subagent file: {}",
        top.stdout
    );
}

#[test]
fn files_timeline_json_marks_subagent_rows_with_refeedable_parent() {
    // The timeline JSON discriminates the id-domain: a subagent row carries is_subagent=true
    // + the re-feedable parent uuid; a top-level row carries is_subagent=false and
    // parent_session_id == session_id (so a consumer can always `csift turns <parent>`).
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&[
        "files",
        at(SESS).as_str(),
        "--by",
        "timeline",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let parent_row = objs
        .iter()
        .find(|o| o["path"] == "/parent/p.md")
        .expect("parent row present");
    assert_eq!(parent_row["is_subagent"], serde_json::json!(false));
    assert_eq!(parent_row["session_id"], serde_json::json!(SESS));
    assert_eq!(parent_row["parent_session_id"], serde_json::json!(SESS));

    let sub_row = objs
        .iter()
        .find(|o| o["path"] == "/sub/s.md")
        .expect("subagent row present");
    assert_eq!(sub_row["is_subagent"], serde_json::json!(true));
    assert_eq!(sub_row["session_id"], serde_json::json!("sub111"));
    // The hex session_id is NOT re-feedable; the parent uuid IS.
    assert_eq!(sub_row["parent_session_id"], serde_json::json!(SESS));
}

#[test]
fn files_grouped_json_and_text_discriminate_subagent_id_domain() {
    // The r6 id-domain fix extends is_subagent + parent_session_id to the GROUPED views
    // (not just --timeline): a --by-file subagent row carries the discriminator in JSON and
    // is branded `SUBAGENT <hex> · parent SESSION <uuid>` in text (never a bare-hex SESSION).
    let h = Home::new();
    subagents_only_scenario(&h);

    let j = h.run(&[
        "files",
        at(SESS).as_str(),
        "--by",
        "file",
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let objs = json_lines(&j.stdout);
    let sub = objs
        .iter()
        .find(|o| o["kind"] == "file" && o["path"] == "/sub/s.md")
        .expect("subagent grouped row present");
    assert_eq!(sub["is_subagent"], serde_json::json!(true));
    assert_eq!(sub["session_id"], serde_json::json!("sub111"));
    assert_eq!(sub["parent_session_id"], serde_json::json!(SESS));
    let parent = objs
        .iter()
        .find(|o| o["kind"] == "file" && o["path"] == "/parent/p.md")
        .expect("parent grouped row present");
    assert_eq!(parent["is_subagent"], serde_json::json!(false));
    assert_eq!(parent["parent_session_id"], serde_json::json!(SESS));

    let t = h.run(&["files", at(SESS).as_str(), "--by", "file"]);
    assert!(t.success, "stderr: {}", t.stderr);
    // The subagent group's header is branded SUBAGENT + the re-feedable parent uuid.
    assert!(
        t.stdout
            .contains(&format!("SUBAGENT sub111  ·  parent SESSION {SESS}")),
        "subagent group not branded: {}",
        t.stdout
    );
    // The parent group's header keeps the plain SESSION <uuid> form.
    assert!(
        t.stdout.contains(&format!("SESSION {SESS}")),
        "top-level group lost its SESSION header: {}",
        t.stdout
    );
}

#[test]
fn files_resolves_relative_bash_operands_against_the_record_cwd() {
    // A record-level `cwd` is the recording shell's own working directory, so a
    // relative bash operand joins it deterministically (cwd-joined), an in-command
    // `cd` tracks lexically (cd-tracked), and the resolved spelling merges with the
    // absolute structured spelling of the same file in every rollup.
    let h = Home::new();
    let lines = concat!(
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"work"}}"#,
        "\n",
        // Structured Edit under the ABSOLUTE path.
        r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/work/proj","message":{"role":"assistant","content":[{"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"/work/proj/notes.md","old_string":"a","new_string":"b"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:02.000Z","cwd":"/work/proj","toolUseResult":{"filePath":"/work/proj/notes.md"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]}}"#,
        "\n",
        // Relative bash append to the SAME file: must resolve to the absolute spelling.
        r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:03.000Z","cwd":"/work/proj","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"echo x >> notes.md"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T05:00:04.000Z","cwd":"/work/proj","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"ok"}]}}"#,
        "\n",
        // cd-tracked: the operand sits after an in-command cd.
        r#"{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:00:05.000Z","cwd":"/work/proj","message":{"role":"assistant","content":[{"type":"tool_use","id":"b2","name":"Bash","input":{"command":"cd sub && touch made.txt"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"c2","timestamp":"2026-06-07T05:00:06.000Z","cwd":"/work/proj","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b2","content":"ok"}]}}"#,
        "\n",
    );
    h.write(&format!("{ENC}/{SESS}.jsonl"), lines);

    let out = h.run(&[
        "files",
        &format!("@{SESS}"),
        "--by",
        "timeline",
        "--format",
        "json",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .filter(|o: &serde_json::Value| o["kind"] == "mutation")
        .collect();

    let joined = rows
        .iter()
        .find(|o| o["op"] == "bash" && o["path"] == "/work/proj/notes.md")
        .expect("cwd-joined bash row under the resolved absolute path");
    assert_eq!(joined["resolution"], "cwd-joined");
    assert_eq!(joined["path_verbatim"], "notes.md");
    assert_eq!(joined["command_errored"], false);

    let tracked = rows
        .iter()
        .find(|o| o["path"] == "/work/proj/sub/made.txt")
        .expect("cd-tracked row");
    assert_eq!(tracked["resolution"], "cd-tracked");

    let edit = rows.iter().find(|o| o["op"] == "edit").expect("edit row");
    assert_eq!(edit["resolution"], serde_json::Value::Null);
    assert_eq!(edit["path_verbatim"], serde_json::Value::Null);

    // The per-file rollup buckets the bash append WITH the structured edit.
    let byfile = h.run(&[
        "files",
        &format!("@{SESS}"),
        "--by",
        "file",
        "--no-subagents",
    ]);
    assert!(byfile.success);
    let block: Vec<&str> = byfile
        .stdout
        .lines()
        .skip_while(|l| !l.contains("/work/proj/notes.md"))
        .take(2)
        .collect();
    assert!(
        block
            .get(1)
            .is_some_and(|l| l.contains("1 edit") && l.contains("1 bash (heuristic)")),
        "merged bucket: {:?}\nfull: {}",
        block,
        byfile.stdout
    );
}

#[test]
fn files_keeps_and_flags_mutations_from_a_partially_failed_bash_chain() {
    // `A && B; C` can mutate in A before C fails. The old behavior dropped every
    // mutation of an is_error command silently; now they are kept and flagged.
    let h = Home::new();
    let lines = concat!(
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        "\n",
        r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/p","message":{"role":"assistant","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"touch made.txt && grep -q missing made.txt"}}]}}"#,
        "\n",
        r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:02.000Z","cwd":"/p","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"exit 1","is_error":true}]}}"#,
        "\n",
    );
    h.write(&format!("{ENC}/{SESS}.jsonl"), lines);

    let out = h.run(&[
        "files",
        &format!("@{SESS}"),
        "--by",
        "timeline",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("/p/made.txt") && out.stdout.contains("(command errored)"),
        "kept and flagged: {}",
        out.stdout
    );

    let json = h.run(&[
        "files",
        &format!("@{SESS}"),
        "--by",
        "timeline",
        "--format",
        "json",
        "--no-subagents",
    ]);
    let row: serde_json::Value = json
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .find(|o: &serde_json::Value| o["kind"] == "mutation")
        .expect("mutation row");
    assert_eq!(row["command_errored"], true);
}

#[test]
fn files_surfaces_interpreter_writes_perl_and_class_markers() {
    // C3 parser increments through the files surface: a python-heredoc literal write
    // resolves against the record cwd, `perl -i` rows join like sed, and a formatter
    // run lands as an `fmt:` class marker with no resolution class.
    let h = Home::new();
    let py = "python3 - <<'PY'\\nfrom pathlib import Path\\nPath('report.md').write_text(s)\\nPY";
    let lines = format!(
        concat!(
            r#"{{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"work"}}}}"#,
            "\n",
            r#"{{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/work/proj","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"b1","name":"Bash","input":{{"command":"{py}"}}}}]}}}}"#,
            "\n",
            r#"{{"type":"assistant","uuid":"a2","timestamp":"2026-06-07T05:00:02.000Z","cwd":"/work/proj","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"b2","name":"Bash","input":{{"command":"perl -pi -e 's/a/b/' src/lib.rs"}}}}]}}}}"#,
            "\n",
            r#"{{"type":"assistant","uuid":"a3","timestamp":"2026-06-07T05:00:03.000Z","cwd":"/work/proj","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"b3","name":"Bash","input":{{"command":"cargo fmt"}}}}]}}}}"#,
            "\n",
        ),
        py = py
    );
    h.write(&format!("{ENC}/{SESS}.jsonl"), &lines);

    let out = h.run(&[
        "files",
        &format!("@{SESS}"),
        "--by",
        "timeline",
        "--format",
        "json",
        "--no-subagents",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .filter(|o: &serde_json::Value| o["kind"] == "mutation")
        .collect();

    let interp = rows
        .iter()
        .find(|o| o["path"] == "/work/proj/report.md")
        .expect("interp heredoc write resolved against the record cwd");
    assert_eq!(interp["resolution"], "cwd-joined");
    assert_eq!(interp["path_verbatim"], "report.md");

    let perl = rows
        .iter()
        .find(|o| o["path"] == "/work/proj/src/lib.rs")
        .expect("perl -i row resolved like sed -i");
    assert_eq!(perl["resolution"], "cwd-joined");

    let marker = rows
        .iter()
        .find(|o| o["path"] == "fmt:cargo")
        .expect("formatter class marker row");
    assert_eq!(marker["resolution"], serde_json::Value::Null);
    assert_eq!(marker["path_verbatim"], serde_json::Value::Null);
}
