use crate::harness::*;

#[test]
fn additional_context_flag_surfaces_hook_attachment_under_meta_hook() {
    let h = Home::new();
    hook_context_scenario(&h);
    // First array element matches; the hit is labeled harness.meta.hook at its real line.
    let out = h.run(&[
        "search",
        "quartzlantern",
        &at(HOOKCTX_SESS),
        "--additional-context",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.meta.hook")
            && out.stdout.contains("L2")
            && out.stdout.contains("quartzlantern"),
        "flag surfaces the attachment under harness.meta.hook:\n{}",
        out.stdout
    );
    // Second array element is part of the SAME joined text (the `\n` join seam).
    let out2 = h.run(&[
        "search",
        "harborlight",
        &at(HOOKCTX_SESS),
        "--additional-context",
    ]);
    assert!(
        out2.stdout.contains("harness.meta.hook"),
        "every content element is searchable:\n{}",
        out2.stdout
    );
    // The label filter still governs: -t user can never surface it.
    let out3 = h.run(&[
        "search",
        "quartzlantern",
        &at(HOOKCTX_SESS),
        "--additional-context",
        "-t",
        "user",
    ]);
    assert!(
        out3.stdout.contains("no matching exchanges"),
        "-t user excludes meta.hook even with the flag:\n{}",
        out3.stdout
    );
    // JSON: the hit carries the leaf as `label`, and the summary reconciles.
    let outj = h.run(&[
        "search",
        "quartzlantern",
        &at(HOOKCTX_SESS),
        "--additional-context",
        "--format",
        "json",
    ]);
    assert!(
        outj.stdout.contains(r#""label":"harness.meta.hook""#),
        "JSON hit label:\n{}",
        outj.stdout
    );
}

#[test]
fn sessions_with_matches_disclosed_cap_drop_on_stderr() {
    // Mutation pin: the -l + --max-count drop note fires exactly when dropped_by_cap > 0.
    let enc = "-Users-testuser-Projects-lcap";
    let h = Home::new();
    for i in 0..2u8 {
        h.write(
            &format!("{enc}/ee00000{i}-aaaa-4bbb-8ccc-00000000000{i}.jsonl"),
            &format!("{{\"type\":\"user\",\"uuid\":\"u0\",\"timestamp\":\"2026-06-07T0{i}:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"zzlcap hit\"}}}}\n"),
        );
    }
    let capped = h.run(&["search", "zzlcap", enc, "-l", "--max-count", "1"]);
    assert!(capped.success, "stderr: {}", capped.stderr);
    assert!(
        capped.stderr.contains("dropped by --max-count"),
        "cap drop disclosed on stderr: {}",
        capped.stderr
    );
    let full = h.run(&["search", "zzlcap", enc, "-l"]);
    assert!(
        !full.stderr.contains("dropped by --max-count"),
        "no note without a drop: {}",
        full.stderr
    );
}
