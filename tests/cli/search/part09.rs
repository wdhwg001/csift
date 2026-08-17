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
