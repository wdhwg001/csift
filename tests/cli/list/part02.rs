use crate::harness::*;

#[test]
fn plan_text_lists_top_level_then_subagent_plans() {
    // A session AND a subagent both planned → text output lists both, TOP-LEVEL FIRST, with
    // the subagent flagged and carrying its parent uuid.
    const PSESS: &str = "11112222-3333-4444-5555-666677778888";
    let h = Home::new();
    let plans_dir = h.root.join(".claude").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let top_path = plans_dir.join("top-level-plan.md");
    // The top-level plan EXISTS on disk; the subagent's does not → the [exists]/[missing]
    // flag must reflect disk reality, per-row.
    std::fs::write(&top_path, "the top plan\n").unwrap();
    let top = top_path.to_string_lossy().into_owned();
    let sub = plans_dir
        .join("worker-plan-agent-bbbbbbbbbbbbbbbbb.md")
        .to_string_lossy()
        .into_owned();
    let top_jsonl = concat!(
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"plan"}}"#, "\n",
        r#"{"type":"attachment","isSidechain":false,"attachment":{"type":"plan_mode","reminderType":"full","isSubAgent":false,"planFilePath":"__TOP__","planExists":false},"uuid":"att0","timestamp":"2026-06-07T05:00:01.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}"#, "\n",
    )
    .replace("__TOP__", &top);
    h.write(&format!("{ENC}/{PSESS}.jsonl"), &top_jsonl);
    let sub_jsonl = concat!(
        r#"{"type":"user","isSidechain":true,"agentId":"bbbb01","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"plan the subtask"}}"#, "\n",
        r#"{"type":"attachment","isSidechain":true,"agentId":"bbbb01","attachment":{"type":"plan_mode","reminderType":"full","isSubAgent":true,"planFilePath":"__SUB__","planExists":false},"uuid":"satt","timestamp":"2026-06-07T05:00:11.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}"#, "\n",
    )
    .replace("__SUB__", &sub);
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-bbbb01.jsonl"),
        &sub_jsonl,
    );
    h.write(
        &format!("{ENC}/{PSESS}/subagents/agent-bbbb01.meta.json"),
        r#"{"agentType":"general-purpose","description":"worker","toolUseId":"t0"}"#,
    );

    let out = h.run(&["plan", at(PSESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    let top_pos = out
        .stdout
        .find(&format!("session  {PSESS}"))
        .expect("top-level line");
    let sub_pos = out.stdout.find("(subagent)").expect("subagent line");
    assert!(top_pos < sub_pos, "top-level listed first:\n{}", out.stdout);
    assert!(out.stdout.contains("top-level-plan.md"), "{}", out.stdout);
    assert!(
        out.stdout.contains("worker-plan-agent-")
            && out.stdout.contains(&format!("parent   {PSESS}")),
        "subagent plan carries its parent:\n{}",
        out.stdout
    );
    // The on-disk top plan reads [exists]; the missing subagent plan reads [missing].
    assert!(
        out.stdout.contains("[exists]") && out.stdout.contains("[missing]"),
        "per-row exists/missing flag tracks disk:\n{}",
        out.stdout
    );
}

#[test]
fn image_lists_images_with_stable_ids() {
    let h = image_home();
    let out = h.run(&["image", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    // r0 = line 1 (1 image), r2 = line 3 (2 images): L1i1 png, L3i1 jpeg, L3i2 png.
    assert!(out.stdout.contains("L1i1"), "L1i1 missing:\n{}", out.stdout);
    assert!(out.stdout.contains("L3i1"), "L3i1 missing:\n{}", out.stdout);
    assert!(out.stdout.contains("L3i2"), "L3i2 missing:\n{}", out.stdout);
    assert!(out.stdout.contains("image/jpeg"));
    assert!(
        out.stdout.contains("3 image(s)"),
        "count line:\n{}",
        out.stdout
    );
}

#[test]
fn image_ambiguous_hash_n_errors_with_occurrence_list() {
    let h = ambiguous_hash_home();

    // Listing surfaces the `#N` handle and shows BOTH #1 images (distinct content → not deduped).
    let list = h.run(&["image", at(SESS).as_str(), "--no-subagents"]);
    assert!(list.success, "stderr: {}", list.stderr);
    assert!(list.stdout.contains("#1") && list.stdout.contains("#2"));
    assert!(
        list.stdout.contains("3 image(s)"),
        "all 3 distinct (no content-dedup):\n{}",
        list.stdout
    );

    // `--id 1` is AMBIGUOUS → it must ERROR (not silently pick one) and list every occurrence
    // with its turn / locator / uuid / time / excerpt so the consumer can disambiguate.
    let err = h.run(&["image", at(SESS).as_str(), "--no-subagents", "--id", "1"]);
    assert!(!err.success, "ambiguous 1 must fail, got:\n{}", err.stdout);
    assert!(err.stderr.contains("ambiguous"), "stderr: {}", err.stderr);
    assert!(
        err.stderr.contains("L1i1") && err.stderr.contains("L3i1"),
        "both occurrences listed: {}",
        err.stderr
    );
    assert!(
        err.stderr.contains("t0") && err.stderr.contains("t1"),
        "turn indices shown: {}",
        err.stderr
    );
    // the excerpt centers on the marker, and the uuid prefix is surfaced.
    assert!(
        err.stderr.contains("re-sharing") && err.stderr.contains("u1deadbe"),
        "excerpt + uuid in the list: {}",
        err.stderr
    );

    // `--id 2` is UNIQUE (only the line-1 red) → resolves fine.
    let two = h.run(&["image", at(SESS).as_str(), "--no-subagents", "--id", "2"]);
    assert!(two.success, "stderr: {}", two.stderr);
    assert!(two.stdout.contains("L1i2"), "{}", two.stdout);
}
