//! search --attachments: the generic attachment gate, labeling, and per-type census.

use crate::harness::*;

#[test]
fn attachments_are_invisible_by_default() {
    // The default scan never parses attachment lines: a pattern living only in an
    // attachment payload is a DEFINITIVE absence (exit 0), and the label alone (without
    // the flag) does not open the gate.
    let h = Home::new();
    attachment_scenario(&h);
    let out = h.run(&["search", "glowlantern", &at(ATTACH_SESS)]);
    assert!(out.success, "zero-match exits 0: {}", out.stderr);
    assert!(
        out.stdout.contains("no matching exchanges"),
        "default scan must not see attachment payloads:\n{}",
        out.stdout
    );
    let labeled = h.run(&[
        "search",
        "glowlantern",
        &at(ATTACH_SESS),
        "-t",
        "harness.meta.attachment",
    ]);
    assert!(
        labeled.stdout.contains("no matching exchanges"),
        "the selector alone does not imply the gate:\n{}",
        labeled.stdout
    );
}

#[test]
fn attachments_flag_surfaces_payloads_under_meta_attachment() {
    let h = Home::new();
    attachment_scenario(&h);
    // The payload's VERBATIM JSON is the matchable text; the hit is labeled
    // harness.meta.attachment at its real line.
    let out = h.run(&["search", "glowlantern", &at(ATTACH_SESS), "--attachments"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.meta.attachment")
            && out.stdout.contains("L2")
            && out.stdout.contains("glowlantern"),
        "flag surfaces the payload under harness.meta.attachment:\n{}",
        out.stdout
    );
    // SUPERSET: the flag also reaches a hook payload, still under its richer meta.hook leaf.
    let hook = h.run(&[
        "search",
        "mistgate",
        &at(ATTACH_SESS),
        "--attachments",
        "-t",
        "harness.meta.hook",
    ]);
    assert!(
        hook.stdout.contains("harness.meta.hook"),
        "--attachments is a superset of --additional-context:\n{}",
        hook.stdout
    );
    // The label filter still governs: -t user can never surface an attachment.
    let out3 = h.run(&[
        "search",
        "glowlantern",
        &at(ATTACH_SESS),
        "--attachments",
        "-t",
        "user",
    ]);
    assert!(
        out3.stdout.contains("no matching exchanges"),
        "-t user excludes meta.attachment even with the flag:\n{}",
        out3.stdout
    );
    // JSON: the hit carries the leaf as `label`.
    let outj = h.run(&[
        "search",
        "glowlantern",
        &at(ATTACH_SESS),
        "--attachments",
        "--format",
        "json",
    ]);
    assert!(
        outj.stdout.contains(r#""label":"harness.meta.attachment""#),
        "JSON label: {}",
        outj.stdout
    );
}

#[test]
fn count_by_attachment_censuses_payload_types_and_implies_the_gate() {
    let h = Home::new();
    attachment_scenario(&h);
    // NO --attachments flag: the axis implies the gate (the D7 implied-widening law).
    let out = h.run(&["search", "", &at(ATTACH_SESS), "--count-by", "attachment"]);
    assert!(out.success, "stderr: {}", out.stderr);
    for key in [
        "edited_text_file",
        "compact_file_reference",
        "hook_additional_context",
    ] {
        assert!(out.stdout.contains(key), "missing {key}:\n{}", out.stdout);
    }
    assert!(
        out.stderr.contains("2 record(s) have no attachment"),
        "non-attachment records are excluded AND disclosed on stderr:\n{}",
        out.stderr
    );
    // JSON: census rows carry the axis + exact per-type counts.
    let outj = h.run(&[
        "search",
        "",
        &at(ATTACH_SESS),
        "--count-by",
        "attachment",
        "--format",
        "json",
    ]);
    let rows: Vec<serde_json::Value> = outj
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    for key in [
        "edited_text_file",
        "compact_file_reference",
        "hook_additional_context",
    ] {
        assert!(
            rows.iter().any(|r| r["kind"] == "census"
                && r["axis"] == "attachment"
                && r["key"] == key
                && r["records"] == 1),
            "census row for {key}: {}",
            outj.stdout
        );
    }
    let summary = rows.last().unwrap();
    assert_eq!(summary["axis"], "attachment", "summary: {}", outj.stdout);
    assert_eq!(summary["excluded_records"], 2, "user + assistant excluded");
}

#[test]
fn show_renders_an_addressed_attachment_flag_free() {
    // The refetch law: an explicit address renders ANY attachment record with no flag.
    let h = Home::new();
    attachment_scenario(&h);
    let out = h.run(&["show", &at(ATTACH_SESS), "--line", "3"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("compact_file_reference") && out.stdout.contains("harbor.md"),
        "addressed attachment renders flag-free:\n{}",
        out.stdout
    );
}
