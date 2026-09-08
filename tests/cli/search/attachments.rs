//! search --attachments: the generic attachment gate, labeling, and per-type census.

use crate::harness::*;

#[test]
fn a_peer_origin_nested_in_an_attachment_is_not_a_peer_message() {
    // T5. A peer origin reaches disk at TWO paths that never coincide: the TOP-LEVEL `origin` on
    // a user record, and a NESTED `attachment.origin` inside a `queued_command` payload (claim
    // TURN-031 - the corpus splits 23 top-level / 6 nested, which is why a top-level-only probe
    // returns 23 of the 29 lines). The nested one is a queued PULSE's provenance, not a delivered
    // message: the peer framing sits inside an attachment payload, never at a section boundary of
    // a `message.content` string, so it must not become `agent.communication.inbox` and must not
    // move `user.queued`.
    //
    // DISCRIMINATION, and the shape of the break matters. What fails the row-count assertion
    // below (left 0, right 1) is a CLASSIFY-level widening that labels the payload AS an inbound
    // comm: reading `attachment.origin.kind` and, when it is `peer`, pushing `Class::CommInbox`
    // and RETURNING, so the comm leaf REPLACES `harness.meta.attachment`. The mechanism is worth
    // knowing: a comm label routes the render through `reconstructed_user_text`, which an
    // attachment has no `message` for, so a mislabelled payload does not merely read wrong, it
    // stops rendering at all.
    //
    // Two widenings measured here do NOT fail it. (1) The same read pushing `Class::CommInbox`
    // ADDITIVELY, without the return: the record then carries `labels: [inbox, attachment]` and
    // `label` stays `harness.meta.attachment`, because `record_text_emission` walks the labels
    // richest-first, gets `None` from `reconstructed_user_text` for the comm view, and falls
    // through to the attachment view that renders. (2) Widening `peer_origin()` to fall back to
    // the nested path: that function's one caller is reachable only from a parsed
    // `message.content` peer section, which an attachment has none of.
    let h = Home::new();
    let enc = "-Users-dev-example-project";
    let sess = "00000000-0000-4000-8000-0000000000d1";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zznested chart the reef"}}"#, "\n",
            r#"{"type":"attachment","uuid":"att0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","attachment":{"type":"queued_command","command":"<agent-message from=\"relay-7\">\nzznested payload body\n</agent-message>","commandMode":"prompt","origin":{"kind":"peer","from":"relay-7","senderTaskId":"arelay-7-0123456789abcdef","name":"relay-7","body":"zznested payload body"}}}"#, "\n",
        ),
    );
    // A default scan never parses an attachment line, so the nested peer origin is invisible.
    let plain = h.run(&["search", "zznested payload", &at(sess)]);
    assert!(plain.success, "stderr: {}", plain.stderr);
    assert!(
        plain.stdout.contains("no matching exchanges"),
        "a nested peer origin does not open the attachment gate:\n{}",
        plain.stdout
    );
    // With the gate open it is an ATTACHMENT payload, never an inbound comm.
    let gated = h.run(&[
        "search",
        "zznested payload",
        &at(sess),
        "--attachments",
        "--format",
        "json",
    ]);
    assert!(gated.success, "stderr: {}", gated.stderr);
    let rows = json_rows(&gated.stdout, "exchange");
    assert_eq!(rows.len(), 1, "one exchange in:\n{}", gated.stdout);
    let hit = &rows[0]["hits"][0];
    assert_eq!(
        hit["label"], "harness.meta.attachment",
        "a peer framing quoted INSIDE a payload is payload, not a delivered message"
    );
    assert!(
        hit["from"].is_null() && hit["to"].is_null(),
        "and it carries no comm direction: {hit}"
    );
    // The queued census is untouched: the nested origin is not the human's queued text either.
    let census = h.run(&[
        "search",
        "",
        &at(sess),
        "-t",
        "user.queued",
        "--count-by",
        "label",
    ]);
    assert!(census.success, "stderr: {}", census.stderr);
    assert!(
        !census.stdout.contains("user.queued"),
        "no queue line exists here, and the attachment is not one:\n{}",
        census.stdout
    );
}

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

#[test]
fn image_bearing_rows_teach_extraction_once_and_in_input_form() {
    // C-11: the image annotation used to be decorative - fleets classified images as
    // unreadable without ever testing extraction. The first image-bearing row now
    // carries a paste-ready hint (INPUT id forms: a #N handle as the bare number),
    // printed once per run, and a capability note rides the footer.
    let h = image_home();
    let out = h.run(&["search", "screenshot", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("read the image(s): csift image @"),
        "inline hint under the first image row:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("--id #"),
        "the hint never uses the display form (#N is not valid --id input):\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("extractable, not decorative"),
        "footer capability note:\n{}",
        out.stdout
    );
    // Two image-bearing rows in one run: the inline hint appears exactly once.
    let both = h.run(&["search", "", at(SESS).as_str(), "-t", "user.message"]);
    assert_eq!(
        both.stdout.matches("read the image(s):").count(),
        1,
        "once per run:\n{}",
        both.stdout
    );

    // An image-less run prints NEITHER the inline hint NOR the footer note.
    let plain = h.run(&["search", "got it", at(SESS).as_str()]);
    assert!(
        !plain.stdout.contains("read the image(s):")
            && !plain.stdout.contains("extractable, not decorative"),
        "no image, no hint machinery:\n{}",
        plain.stdout
    );
    // The hint also rides a SIBLING row when the image sits on the turn's other side.
    let sib = h.run(&["search", "got it", at(SESS).as_str(), "--siblings"]);
    assert_eq!(
        sib.stdout.matches("read the image(s):").count(),
        1,
        "sibling image row teaches too, once:\n{}",
        sib.stdout
    );

    // show renders the same hint on an addressed image-bearing record.
    let shown = h.run(&["show", at(SESS).as_str(), "--line", "1"]);
    assert!(
        shown.stdout.contains("read the image(s): csift image @"),
        "show teaches extraction too:\n{}",
        shown.stdout
    );

    // The helps name the capability (SEE ALSO), closing the discovery gap.
    let sh = h.run(&["search", "--help"]);
    assert!(
        sh.stdout.contains("SEE ALSO") && sh.stdout.contains("csift image"),
        "search --help names image:\n{}",
        sh.stdout
    );
    let vh = h.run(&["verbatim", "--help"]);
    assert!(
        vh.stdout.contains("SEE ALSO") && vh.stdout.contains("csift image"),
        "verbatim --help names image:\n{}",
        vh.stdout
    );
}
