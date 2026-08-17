//! recover batch mode and the @plan sigil.

use crate::harness::*;

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
fn recover_batch_reconstructs_many_files_in_one_scan() {
    let h = Home::new();
    let read_full = |uid: &str, path: &str, content: &str, total: usize| -> String {
        serde_json::json!({
            "type":"user","uuid":uid,"timestamp":"2026-06-07T05:00:00.000Z",
            "toolUseResult":{"file":{"filePath":path,"content":content,"startLine":1,"numLines":total,"totalLines":total}},
            "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":uid,"content":"ok"}]}
        }).to_string()
    };
    // Session 1 holds two files; a SECOND session holds a third — all recovered in ONE scan.
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!(
            "{}\n{}\n",
            read_full("r0", "/tmp/alpha.md", "# Alpha\nline two\nline three", 3),
            read_full("r1", "/tmp/beta.md", "beta one\nbeta two", 2)
        ),
    );
    let sess2 = "11112222-3333-4444-5555-666677778888";
    h.write(
        &format!("{ENC}/{sess2}.jsonl"),
        &format!(
            "{}\n",
            read_full("r2", "/tmp/gamma.md", "gamma only line", 1)
        ),
    );

    // Manifest: three real targets + a comment + an absent one.
    let manifest = h.root.join("manifest.txt");
    std::fs::write(
        &manifest,
        "/tmp/alpha.md\n/tmp/beta.md\n# a comment\n/tmp/gamma.md\n/tmp/absent.md\n",
    )
    .unwrap();
    let out_dir = h.root.join("recovered");
    let out = h.run(&[
        "recover",
        "--files-from",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);

    // Each present file is reconstructed to its raw content, mirrored under out-dir.
    assert_eq!(
        std::fs::read_to_string(out_dir.join("tmp/alpha.md")).unwrap(),
        "# Alpha\nline two\nline three\n"
    );
    assert_eq!(
        std::fs::read_to_string(out_dir.join("tmp/beta.md")).unwrap(),
        "beta one\nbeta two\n"
    );
    assert_eq!(
        std::fs::read_to_string(out_dir.join("tmp/gamma.md")).unwrap(),
        "gamma only line\n"
    );
    // The absent target writes no file and is reported as no-history.
    assert!(!out_dir.join("tmp/absent.md").exists());
    let report = std::fs::read_to_string(out_dir.join("recovery-report.tsv")).unwrap();
    assert!(
        report.contains("complete\t3\t3\t/tmp/alpha.md"),
        "report:\n{report}"
    );
    assert!(
        report.contains("no-history\t0\t0\t/tmp/absent.md"),
        "report:\n{report}"
    );
    assert!(out.stdout.contains("3 complete"), "summary: {}", out.stdout);

    // Re-running without --force skips the already-present files.
    let out2 = h.run(&[
        "recover",
        "--files-from",
        manifest.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
    ]);
    assert!(out2.success, "stderr: {}", out2.stderr);
    assert!(
        out2.stdout.contains("3 skipped"),
        "skip summary: {}",
        out2.stdout
    );
}

#[test]
fn recover_batch_requires_out_dir_and_excludes_file() {
    let h = recover_scenario_home();
    let manifest = h.root.join("m.txt");
    std::fs::write(&manifest, "/tmp/x.md\n").unwrap();
    let no_out = h.run(&["recover", "--files-from", manifest.to_str().unwrap()]);
    assert!(!no_out.success);
    assert!(no_out.stderr.contains("--out-dir"), "{}", no_out.stderr);
    let both = h.run(&[
        "recover",
        "--files-from",
        manifest.to_str().unwrap(),
        "--out-dir",
        h.root.join("o").to_str().unwrap(),
        "--file",
        "/tmp/x.md",
    ]);
    assert!(!both.success);
    assert!(
        both.stderr.contains("mutually exclusive"),
        "{}",
        both.stderr
    );
}

#[test]
fn recover_file_plan_magic_reconstructs_the_bound_plan() {
    let h = Home::new();
    let plans_dir = h.root.join(".claude").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let bound_abs = plans_dir
        .join("shimmying-spinning-cascade.md")
        .to_string_lossy()
        .into_owned();
    let other_abs = plans_dir.join("decoy.md").to_string_lossy().into_owned();
    write_planning_session(&h, SESS, &bound_abs, &other_abs);

    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "@plan",
        "--at",
        "@line:99999999",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The stderr note announces the resolution.
    assert!(
        out.stderr.contains("@plan resolved to")
            && out.stderr.contains("shimmying-spinning-cascade.md"),
        "missing @plan resolution note: {}",
        out.stderr
    );
    // The bound plan's full Write+Edit history is reconstructed (not the decoy plan).
    assert_eq!(
        recon_lines_from_at_json(&out.stdout),
        vec!["P1", "P2-revised", "P3"],
        "recovered the bound plan's content: {}",
        out.stdout
    );
}

#[test]
fn recover_file_plan_errors_when_no_plan_is_bound() {
    // A session that never entered Plan Mode has no bound plan → @plan must error clearly
    // (never fall back to guessing a plans/ path).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"just code"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "@plan",
        "--coverage",
    ]);
    assert!(!out.success, "should fail: {}", out.stdout);
    assert!(
        out.stderr.contains("no plan file is bound") && out.stderr.contains("plan_mode"),
        "unhelpful error: {}",
        out.stderr
    );
}

#[test]
fn recover_file_plan_errors_when_ambiguous_across_sessions() {
    // Two top-level sessions under one project, each bound to a DIFFERENT plan → @plan over
    // the whole project is ambiguous and must ask for --session, never silently pick one.
    const SESS2: &str = "abcdef01-2345-6789-abcd-ef0123456789";
    let h = Home::new();
    let plans_dir = h.root.join(".claude").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let a = plans_dir.join("plan-a.md").to_string_lossy().into_owned();
    let b = plans_dir.join("plan-b.md").to_string_lossy().into_owned();
    let decoy = plans_dir.join("decoy.md").to_string_lossy().into_owned();
    write_planning_session(&h, SESS, &a, &decoy);
    write_planning_session(&h, SESS2, &b, &decoy);

    let out = h.run(&["recover", ENC, "--file", "@plan", "--coverage"]);
    assert!(!out.success, "should be ambiguous: {}", out.stdout);
    assert!(
        out.stderr.contains("different bound plan files") && out.stderr.contains("@<uuid>"),
        "unhelpful ambiguity error: {}",
        out.stderr
    );
}
