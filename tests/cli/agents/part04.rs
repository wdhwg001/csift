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
fn turns_defaults_to_top_level_only_no_subagent_span() {
    // FOOTGUN FIX: `turns <uuid>` with NO flags must reconstruct ONLY the top-level thread —
    // it must NOT span the session's subagents (unlike files/search). So a bare run prints no
    // `(subagent transcript)` blocks and no scope banner (one session in scope, rendered).
    let h = populated_home();
    let out = h.run(&["verbatim", at(SESS).as_str(), "--budget", "40000"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("SESSION {SESS}")),
        "the top-level thread must render: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("(subagent transcript)"),
        "turns must NOT span subagents by default: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("scope  "),
        "a single top-level session prints no scope banner: {}",
        out.stdout
    );
}

#[test]
fn turns_rich_filters_subagent_runs_too() {
    // The shared code path: a SUBAGENT transcript carrying a long agent run is richness-
    // filtered with the same flags (explicit --include-subagents opt-in). The subagent's pure
    // declarations collapse; its rich member + EOT survive.
    let h = turns_home();
    // A subagent sidecar with a long agent run under the session.
    let mut sub = String::new();
    sub.push_str(r#"{"type":"user","isSidechain":true,"agentId":"subrun","timestamp":"2026-06-07T09:00:00.000Z","message":{"role":"user","content":"subagent kicks off a long chain"}}"#);
    sub.push('\n');
    let msgs = [
        "SUBRICHFIRST found the cause in src/z.rs:7",
        "let me SUBDECL a",
        "now i will SUBDECL b",
        "let me SUBDECL c",
        "now let me SUBDECL d",
        "next i SUBDECL e",
        "let me SUBDECL f",
        "the SUBEOT final subagent answer",
    ];
    let mut ts = 1;
    for m in msgs {
        sub.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"2026-06-07T09:00:{ts:02}.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"s","name":"Bash","input":{{}}}}]}}}}"#
        ));
        sub.push('\n');
        ts += 1;
        sub.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"2026-06-07T09:00:{ts:02}.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"{m}"}}]}}}}"#
        ));
        sub.push('\n');
        ts += 1;
    }
    h.write(&format!("{ENC}/{SESS}/subagents/agent-subrun.jsonl"), &sub);

    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "rich",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("SUBRICHFIRST"),
        "subagent rich member kept: {}",
        out.stdout
    );
    assert!(out.stdout.contains("SUBEOT"), "subagent EOT kept");
    assert!(
        !out.stdout.contains("SUBDECL"),
        "subagent pure declarations collapse under the shared richness path: {}",
        out.stdout
    );
}

/// `--no-subagents` is the only span flag on the default-ON commands and suppresses the
/// fan-out the user asked to drop. The former no-op `--include-subagents` is GONE there, so the
/// only way to restrict span is `--no-subagents` — and it always restricts.
#[test]
fn no_subagents_restricts_span_end_to_end() {
    let h = populated_home();
    let span = |out: &Output| out.stdout.contains("sessions in scope");
    // `--no-subagents` suppresses the banner (top-level only) on every default-on command.
    assert!(!span(&h.run(&[
        "list",
        at(SESS).as_str(),
        "--no-subagents"
    ])));
    assert!(!span(&h.run(&[
        "files",
        at(SESS).as_str(),
        "--by",
        "file",
        "--no-subagents"
    ])));
    assert!(!span(&h.run(&[
        "search",
        "carry",
        at(SESS).as_str(),
        "--no-subagents"
    ])));
    // The removed `--include-subagents` is now an unknown argument on a default-on command.
    let gone = h.run(&["list", at(SESS).as_str(), "--include-subagents"]);
    assert!(
        !gone.success,
        "list --include-subagents must be rejected: {}",
        gone.stdout
    );
}

/// `--subagents-only` is GONE crate-wide (no user-facing flag, no hidden migration no-op). On
/// every span-aware subcommand it now falls through to the generic clap "unexpected argument"
/// rejection — the acceptable outcome once the pointed-migration machinery was removed.
#[test]
fn subagents_only_is_an_unknown_argument_everywhere() {
    let h = populated_home();
    for sub in ["verbatim", "recover", "list"] {
        let out = h.run(&[sub, at(SESS).as_str(), "--subagents-only"]);
        assert!(!out.success, "{sub} --subagents-only should fail");
        assert!(
            out.stderr.contains("unexpected argument"),
            "{sub}: expected the generic unknown-argument error, got: {}",
            out.stderr
        );
    }
    // search too (pattern positional first).
    let out = h.run(&["search", "x", at(SESS).as_str(), "--subagents-only"]);
    assert!(!out.success);
    assert!(
        out.stderr.contains("unexpected argument"),
        "search: {}",
        out.stderr
    );
    // files itself rejects it as an unknown argument (the user-facing flag was removed earlier).
    let gone = h.run(&[
        "files",
        at(SESS).as_str(),
        "--subagents-only",
        "--by",
        "file",
    ]);
    assert!(
        !gone.success,
        "files --subagents-only must now be rejected: {}",
        gone.stdout
    );
    assert!(
        gone.stderr.contains("unexpected argument"),
        "files --subagents-only should be an unknown argument: {}",
        gone.stderr
    );
}

/// turns text now brands a subagent block `SUBAGENT <hex> · parent SESSION <uuid>` (uniform
/// with list/files/search), never tokening a bare subagent hex as `SESSION`.
#[test]
fn turns_text_brands_subagent_uniformly() {
    let h = populated_home();
    let out = h.run(&["verbatim", at(SESS).as_str(), "--subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The subagent block carries the SUBAGENT token + the re-feedable parent uuid.
    assert!(
        out.stdout.contains("SUBAGENT") && out.stdout.contains(&format!("parent SESSION {SESS}")),
        "turns subagent branding missing:\n{}",
        out.stdout
    );
}

#[test]
fn recover_subagent_input_fallback_skips_failed_edit() {
    // The DANGER case: a SUBAGENT records results as bare tool_result strings (no
    // toolUseResult), so content comes from the input-side fallback. A failed Edit there
    // (is_error:true) must be skipped, not replayed from its input.
    const PSESS: &str = "cccccccc-9999-9999-9999-999999999999";
    let h = Home::new();
    h.write(
        &format!("{ENC}/{PSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"spawn a worker"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"on it"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-deadbeef.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"make g.md then fix it"}}"#, "\n",
            // Write via input fallback (bare success result).
            r#"{"type":"assistant","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:11.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sw","name":"Write","input":{"file_path":"/p/g.md","content":"aa\nbb\ncc\n"}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:11.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"sw","content":"File created successfully at: /p/g.md"}]}}"#, "\n",
            // FAILED edit (is_error) — must NOT be applied from the input.
            r#"{"type":"assistant","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:12.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sbad","name":"Edit","input":{"file_path":"/p/g.md","old_string":"NOPE","new_string":"GHOST"}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:12.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"sbad","content":"String to replace not found in file.","is_error":true}]}}"#, "\n",
            // SUCCESSFUL edit via input fallback (bare success result).
            r#"{"type":"assistant","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:13.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sok","name":"Edit","input":{"file_path":"/p/g.md","old_string":"bb","new_string":"bb-ok"}}]}}"#, "\n",
            r#"{"type":"user","isSidechain":true,"agentId":"deadbeef","timestamp":"2026-06-07T05:00:13.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"sok","content":"The file /p/g.md has been updated successfully."}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-deadbeef.meta.json"),
        r#"{"agentType":"general-purpose","description":"worker","toolUseId":"t0"}"#,
    );
    let out = h.run(&[
        "recover",
        at(PSESS).as_str(),
        "--file",
        "/p/g.md",
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("GHOST"),
        "ghost edit leaked: {}",
        out.stdout
    );
    assert_eq!(
        recon_lines_from_at_json(&out.stdout),
        vec!["aa", "bb-ok", "cc"],
        "subagent: only the good edit lands"
    );
}

#[test]
fn plan_surfaces_subagent_bound_plan() {
    // A SUBAGENT that entered Plan Mode binds a plan with an `-agent-<hex>` path; `plan`
    // (spanning subagents) must surface it, flagged as a subagent with its parent uuid.
    const PSESS: &str = "feedface-1111-2222-3333-444455556666";
    let h = Home::new();
    let plans_dir = h.root.join(".claude").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let sub_plan = plans_dir
        .join("goofy-finding-kettle-agent-aaaaaaaaaaaaaaaaa.md")
        .to_string_lossy()
        .into_owned();
    h.write(
        &format!("{ENC}/{PSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"spawn a planning worker"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#, "\n",
        ),
    );
    let sub_jsonl = concat!(
        r#"{"type":"user","isSidechain":true,"agentId":"feed01","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"plan the thing"}}"#, "\n",
        r#"{"type":"attachment","isSidechain":true,"agentId":"feed01","attachment":{"type":"plan_mode","reminderType":"full","isSubAgent":true,"planFilePath":"__SUBPLAN__","planExists":false},"uuid":"satt","timestamp":"2026-06-07T05:00:11.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}"#, "\n",
    )
    .replace("__SUBPLAN__", &sub_plan);
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-feed01.jsonl"),
        &sub_jsonl,
    );
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-feed01.meta.json"),
        r#"{"agentType":"general-purpose","description":"planner","toolUseId":"t0"}"#,
    );

    let out = h.run(&["plan", at(PSESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let v: serde_json::Value = out
        .stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| v["is_subagent"].as_bool() == Some(true))
        .unwrap_or_else(|| panic!("no subagent plan in:\n{}", out.stdout));
    assert_eq!(v["plan_file"].as_str(), Some(sub_plan.as_str()));
    assert_eq!(
        v["parent_session_id"].as_str(),
        Some(PSESS),
        "carries the re-feedable parent"
    );
}

#[test]
fn recover_file_plan_resolves_subagent_only_plan() {
    // The top-level session never planned, but a SUBAGENT did → @plan falls back to the
    // subagent's bound plan and reconstructs its Write+Edit history.
    const PSESS: &str = "99998888-7777-6666-5555-444433332222";
    let h = Home::new();
    let plans_dir = h.root.join(".claude").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let sub_plan = plans_dir
        .join("subagent-only-agent-ccccccccccccccccc.md")
        .to_string_lossy()
        .into_owned();
    h.write(
        &format!("{ENC}/{PSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"spawn a planning worker"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#, "\n",
        ),
    );
    let sub_jsonl = concat!(
        r#"{"type":"user","isSidechain":true,"agentId":"cccc01","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"plan + draft it"}}"#, "\n",
        r#"{"type":"attachment","isSidechain":true,"agentId":"cccc01","attachment":{"type":"plan_mode","reminderType":"full","isSubAgent":true,"planFilePath":"__SUB__","planExists":false},"uuid":"satt","timestamp":"2026-06-07T05:00:11.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}"#, "\n",
        r#"{"type":"assistant","isSidechain":true,"agentId":"cccc01","timestamp":"2026-06-07T05:00:12.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sw","name":"Write","input":{"file_path":"__SUB__","content":"D1\nD2\nD3\n"}}]}}"#, "\n",
        r#"{"type":"user","isSidechain":true,"agentId":"cccc01","timestamp":"2026-06-07T05:00:12.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"sw","content":"File created successfully at: __SUB__"}]}}"#, "\n",
        r#"{"type":"assistant","isSidechain":true,"agentId":"cccc01","timestamp":"2026-06-07T05:00:13.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"se","name":"Edit","input":{"file_path":"__SUB__","old_string":"D2","new_string":"D2-final"}}]}}"#, "\n",
        r#"{"type":"user","isSidechain":true,"agentId":"cccc01","timestamp":"2026-06-07T05:00:13.500Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"se","content":"The file __SUB__ has been updated successfully."}]}}"#, "\n",
    )
    .replace("__SUB__", &sub_plan);
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-cccc01.jsonl"),
        &sub_jsonl,
    );
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-cccc01.meta.json"),
        r#"{"agentType":"general-purpose","description":"planner","toolUseId":"t0"}"#,
    );

    let out = h.run(&[
        "recover",
        at(PSESS).as_str(),
        "--file",
        "@plan",
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("@plan resolved to")
            && out.stderr.contains("subagent-only-agent-")
            && out.stderr.contains("subagent"),
        "resolved to the subagent plan: {}",
        out.stderr
    );
    assert_eq!(
        recon_lines_from_at_json(&out.stdout),
        vec!["D1", "D2-final", "D3"],
        "subagent-only plan reconstructed: {}",
        out.stdout
    );
}

#[test]
fn turns_includes_pending_askuserquestion() {
    let h = sidecar_session_home();
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n",
            auq_pending_line(
                "toolu_AQ1",
                "2026-06-27T01:02:03.000Z",
                "Pick a deployment target"
            )
        ),
    );
    let out = h.run(&["verbatim", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("AskUserQuestion: Pick a deployment target"),
        "turns must include the pending AUQ as a unit:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("(elicitation sidecar)"),
        "the pending unit's locator must be `(elicitation sidecar)`:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("with elicitation sidecar"),
        "the merged-records note must appear:\n{}",
        out.stdout
    );

    // JSON header flags it; the unit carries source + null line_no.
    let j = h.run(&["verbatim", &at(SESS), "--format", "json"]);
    let header: serde_json::Value = serde_json::from_str(j.stdout.lines().next().unwrap()).unwrap();
    assert_eq!(header["with_elicitation_sidecar"], true);
    let unit = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["source"] == "elicitation-sidecar")
        .expect("a sidecar unit object");
    assert!(
        unit["line"].is_null(),
        "sidecar unit has null line_no: {unit}"
    );
}
