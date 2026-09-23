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
    // and RETURNING, so the comm leaf REPLACES the payload's own leaf. The mechanism is worth
    // knowing: a comm label routes the render through `reconstructed_user_text`, which an
    // attachment has no `message` for, so a mislabelled payload does not merely read wrong, it
    // stops rendering at all.
    //
    // Two widenings measured here do NOT fail it. (1) The same read pushing `Class::CommInbox`
    // ADDITIVELY, without the return: the record then carries `labels: [inbox, attachment]` and
    // `label` stays the payload's own leaf, because `record_text_emission` walks the labels
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
        hit["label"], "harness.context.task",
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
    // Naming the LEAF is a gate of its own, which is what the `--additional-context`
    // retirement generalised: the payload surfaces without `--attachments`.
    let labeled = h.run(&[
        "search",
        "glowlantern",
        &at(ATTACH_SESS),
        "-t",
        "harness.context.file",
    ]);
    assert!(
        labeled.stdout.contains("harness.context.file"),
        "an explicit leaf admits the attachment line:\n{}",
        labeled.stdout
    );
    // A selector that reaches NO attachment leaf still does not.
    let other = h.run(&[
        "search",
        "glowlantern",
        &at(ATTACH_SESS),
        "-t",
        "user.message",
    ]);
    assert!(
        other.stdout.contains("no matching exchanges"),
        "an unrelated selector never opens the gate:\n{}",
        other.stdout
    );
}

#[test]
fn attachments_flag_surfaces_payloads_under_their_own_leaves() {
    let h = Home::new();
    attachment_scenario(&h);
    // The payload's VERBATIM JSON is the matchable text; an `edited_text_file` payload
    // is labeled harness.context.file at its real line.
    let out = h.run(&["search", "glowlantern", &at(ATTACH_SESS), "--attachments"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.context.file")
            && out.stdout.contains("L2")
            && out.stdout.contains("glowlantern"),
        "flag surfaces the payload under harness.context.file:\n{}",
        out.stdout
    );
    // SUPERSET: the flag also reaches a hook payload, still under its own context leaf.
    let hook = h.run(&[
        "search",
        "mistgate",
        &at(ATTACH_SESS),
        "--attachments",
        "-t",
        "harness.hook.context",
    ]);
    assert!(
        hook.stdout.contains("harness.hook.context"),
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
        "-t user excludes an attachment leaf even with the flag:\n{}",
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
        outj.stdout.contains(r#""label":"harness.context.file""#),
        "JSON label: {}",
        outj.stdout
    );
}

#[test]
fn count_by_attachment_censuses_payload_types_and_implies_the_gate() {
    let h = Home::new();
    attachment_scenario(&h);
    // NO --attachments flag: the axis is unanswerable without the gate, so it implies it.
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
    // The image annotation has to be ACTIONABLE, not decorative: one that names an image
    // without saying how to read it leaves a reader calling images unreadable. The first
    // image-bearing row carries a paste-ready hint (INPUT id forms: a #N handle as the
    // bare number), printed once per run, and a capability note rides the footer.
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

/// H2. A `queued_command` attachment whose PAYLOAD origin says `human` is the person's own
/// mid-turn steer, so it carries `user.message` - and a label nothing can reach is not a
/// label. Two independent failures put it out of reach and each is asserted here.
///
/// ONE: the arm read the record's TOP-LEVEL `origin`, which no `queued_command` line
/// carries (0 of 3,692 and 0 of 3,574 over two projects roots, against 219 and 217 that
/// carry `attachment.origin`), so on real data it never fired and every steer classified
/// as harness context.
///
/// TWO: even when it fired, the leaf routes the render through `reconstructed_user_text`,
/// which an attachment has no `message{}` for - so the record emitted NOTHING: no hit, no
/// census key, no `-t user` result, reachable only by `show --line`. That is the same
/// mechanism the peer-origin test above describes, reached from the other side.
#[test]
fn a_human_origin_queued_command_is_reachable_as_the_persons_message() {
    let h = Home::new();
    let sess = "5d4c3b2a-1e0f-4a98-8b7c-6d5e4f3a2b10";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-08T01:00:00.000Z","message":{"role":"user","content":"zzopener the turn"}}"#, "\n",
            // the person types again WHILE the turn runs: the harness folds it in as an
            // attachment, stamping its own provenance inside the payload
            r#"{"type":"attachment","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-08T01:00:05.000Z","attachment":{"type":"queued_command","prompt":"zzsteer actually check the other branch","commandMode":"prompt","origin":{"kind":"human"},"timestamp":"2026-06-08T01:00:05.000Z"}}"#, "\n",
            // the harness's OWN queued rider, same payload type, no origin at all
            r#"{"type":"attachment","uuid":"a2","parentUuid":"a1","timestamp":"2026-06-08T01:00:06.000Z","attachment":{"type":"queued_command","prompt":"zzpulse <task-notification>...</task-notification>","commandMode":"task-notification"}}"#, "\n",
            r#"{"type":"assistant","uuid":"s1","parentUuid":"a2","timestamp":"2026-06-08T01:00:07.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzreply noted"}]}}"#, "\n",
        ),
    );

    // Reachable by the leaf, by the bare role, and as a census key.
    for sel in ["user.message", "user"] {
        let out = h.run(&["search", "zzsteer", &at(sess), "-t", sel]);
        assert!(out.success, "stderr: {}", out.stderr);
        assert!(
            out.stdout.contains("zzsteer"),
            "-t {sel} reaches the person's steer:\n{}\nstderr: {}",
            out.stdout,
            out.stderr
        );
    }
    let census = h.run(&[
        "search",
        "",
        &at(sess),
        "-t",
        "user.message",
        "--count-by",
        "label",
    ]);
    assert!(census.success, "stderr: {}", census.stderr);
    assert!(
        census.stdout.contains("2  user.message"),
        "the opener and the steer both census under user.message:\n{}",
        census.stdout
    );

    // The harness's own rider keeps the context leaf - the origin, not the payload type,
    // is the discriminator.
    let rider = h.run(&[
        "search",
        "zzpulse",
        &at(sess),
        "--attachments",
        "--format",
        "json",
    ]);
    assert!(rider.success, "stderr: {}", rider.stderr);
    assert!(
        rider.stdout.contains("harness.context.task") && !rider.stdout.contains("user.message"),
        "a queued rider is harness context:\n{}",
        rider.stdout
    );

    // And the DEMOTION does not swallow the steer: a scan that named the leaf keeps the
    // record whole rather than reducing it to a chain-only spine row.
    let raw = h.run(&[
        "search",
        "zzsteer",
        &at(sess),
        "-t",
        "user.message",
        "--format",
        "json",
    ]);
    assert!(raw.success, "stderr: {}", raw.stderr);
    assert!(
        raw.stdout.contains("\"label\":\"user.message\""),
        "the hit carries the leaf:\n{}",
        raw.stdout
    );

    // A FLAGLESS scan is a SUPERSET of `-t user`, so the leaf the narrower query
    // reaches cannot be one the wider query misses. The needle is default-on for the same
    // reason the csift-channel delivery keep is: a message no ordinary query surfaces is a
    // message nobody can audit.
    let flagless = h.run(&["search", "zzsteer", &at(sess)]);
    assert!(flagless.success, "stderr: {}", flagless.stderr);
    assert!(
        flagless.stdout.contains("zzsteer") && flagless.stdout.contains("user.message"),
        "a flagless search reaches the person's steer:\n{}\nstderr: {}",
        flagless.stdout,
        flagless.stderr
    );
    let flagless_census = h.run(&["search", "", &at(sess), "--count-by", "label"]);
    assert!(
        flagless_census.success,
        "stderr: {}",
        flagless_census.stderr
    );
    assert!(
        flagless_census.stdout.contains("2  user.message"),
        "and it censuses under the leaf, beside the turn opener:\n{}",
        flagless_census.stdout
    );
    // The PARSE still decides: the harness's own queued rider is admitted by the same
    // needle and classifies as harness context, so a flagless scan gains the person's
    // message and no machinery.
    let rider = h.run(&["search", "zzpulse", &at(sess)]);
    assert!(rider.success, "stderr: {}", rider.stderr);
    assert!(
        !rider.stdout.contains("zzpulse"),
        "a queued rider stays behind its own gate on a flagless scan:\n{}",
        rider.stdout
    );
}

/// The twin of the case above on the other side of the conversation. A schema-bound
/// subagent's final answer rides a `structured_output` ATTACHMENT, and `agent.message.
/// structured` is LLM-visible because the MODEL produced the text. A flagless scan is a
/// superset of `-t agent`, so a leaf that query reaches cannot be one a bare
/// `csift search` misses - the same law, the same gate, the same needle shape.
#[test]
fn a_structured_answer_is_reachable_from_a_flagless_scan() {
    let h = Home::new();
    let sess = "4c3b2a19-0e1f-4a87-8b6c-5d4e3f2a1b09";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-09T01:00:00.000Z","message":{"role":"user","content":"zzask for the verdict"}}"#, "\n",
            r#"{"type":"assistant","uuid":"s1","parentUuid":"u1","timestamp":"2026-06-09T01:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzreply working on it"}]}}"#, "\n",
            // the schema-bound answer the harness attached, parented to the record that produced it
            r#"{"type":"attachment","uuid":"a1","parentUuid":"s1","timestamp":"2026-06-09T01:00:02.000Z","attachment":{"type":"structured_output","content":["zzverdict the region queue is throttled"]}}"#, "\n",
            // an ORDINARY attachment on the same chain: the coarse keep stays off, so this
            // one is still invisible to a flagless scan.
            r#"{"type":"attachment","uuid":"a2","parentUuid":"a1","timestamp":"2026-06-09T01:00:03.000Z","attachment":{"type":"edited_text_file","filePath":"/tmp/zzfile.rs","snippet":"zzsnippet fn main() {}"}}"#, "\n",
            r#"{"type":"assistant","uuid":"s2","parentUuid":"a2","timestamp":"2026-06-09T01:00:04.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzreply done"}]}}"#, "\n",
        ),
    );

    let flagless = h.run(&["search", "zzverdict", &at(sess)]);
    assert!(flagless.success, "stderr: {}", flagless.stderr);
    assert!(
        flagless.stdout.contains("zzverdict")
            && flagless.stdout.contains("agent.message.structured"),
        "a flagless search reaches the model's own answer:\n{}\nstderr: {}",
        flagless.stdout,
        flagless.stderr
    );
    let census = h.run(&["search", "", &at(sess), "--count-by", "label"]);
    assert!(census.success, "stderr: {}", census.stderr);
    assert!(
        census.stdout.contains("agent.message.structured"),
        "and the flagless census carries the leaf:\n{}",
        census.stdout
    );

    // The coarse keep is untouched: an ordinary attachment payload stays behind its gate,
    // so the flagless scan gained the model's words and no machinery.
    let ordinary = h.run(&["search", "zzsnippet", &at(sess)]);
    assert!(ordinary.success, "stderr: {}", ordinary.stderr);
    assert!(
        !ordinary.stdout.contains("zzsnippet"),
        "an ordinary attachment is still not parsed flaglessly:\n{}",
        ordinary.stdout
    );
    // ...and naming its leaf still finds it, so nothing was lost.
    let named = h.run(&[
        "search",
        "zzsnippet",
        &at(sess),
        "-t",
        "harness.context.file",
    ]);
    assert!(named.success, "stderr: {}", named.stderr);
    assert!(
        named.stdout.contains("zzsnippet"),
        "naming the leaf still reaches it:\n{}",
        named.stdout
    );
}
