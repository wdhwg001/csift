use crate::harness::*;

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
