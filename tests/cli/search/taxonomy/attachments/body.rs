//! Which of a record's two copies of itself the unit shows, and what it says about the
//! choice: the conversion the harness persisted, a field of the payload, or the payload JSON.

use super::*;

#[test]
fn an_attachment_body_is_the_text_the_model_received_when_the_record_carries_one() {
    // The rule: the harness's own conversion leads, marked `[rendered]`; a record with no
    // conversion falls to the payload, marked `[payload]`. The two hook runs in the
    // fixture differ in exactly that.
    let h = attachment_home();
    let out = h.run(&["search", "zzatt", &at_sess(), "--attachments"]);
    assert!(out.success, "stderr {}", out.stderr);

    let ctx = line_with(&out.stdout, "harness.hook.context");
    assert!(
        ctx.contains("[rendered]") && ctx.contains("<system-reminder> zzatt the reef brief"),
        "the injected context renders the wrapped form the model read:\n{ctx}"
    );
    let unconverted = line_with(&out.stdout, "zzatt stdout reef ok");
    assert!(
        unconverted.contains("[payload]"),
        "a run with no persisted conversion reads its own stdout, marked payload:\n{unconverted}"
    );
    let converted = line_with(&out.stdout, "zzatt start notice");
    assert!(
        converted.contains("[rendered]") && !converted.contains("zzatt stdout start raw"),
        "a run the converter DID convert renders the CONVERTED text, not its stdout - the \
         two carry different words in this fixture so the marker is not the only witness:\n{converted}"
    );
    let listing = line_with(&out.stdout, "harness.context.listing");
    assert!(
        listing.contains("[payload]") && listing.contains(r#"{"type":"skill_listing""#),
        "a payload with no conversion renders its verbatim JSON:\n{listing}"
    );
}

#[test]
fn a_second_conversion_for_a_human_turn_is_reported_never_chosen() {
    // The record carries two conversions and does not say which turn it was replayed
    // into, so the body stays the ordinary one and the difference is reported instead.
    let h = attachment_home();
    let out = h.run(&["search", "zzatt also map", &at_sess(), "--attachments"]);
    assert!(out.success, "stderr {}", out.stderr);
    let rider = line_with(&out.stdout, "harness.context.task");
    assert!(
        rider.contains("ordinary turn") && !rider.contains("human turn"),
        "the body is the ordinary conversion:\n{rider}"
    );
    assert!(
        rider.contains("[human-turn render differs]"),
        "and the second copy is reported:\n{rider}"
    );
    let json = h.run(&[
        "search",
        "zzatt also map",
        &at_sess(),
        "--attachments",
        "--format",
        "json",
    ]);
    assert!(
        json.stdout.contains(r#""human_turn_render_differs":true"#),
        "the same fact rides JSON:\n{}",
        json.stdout
    );
}

#[test]
fn the_persons_mid_turn_steer_renders_the_text_the_model_received() {
    // The steer is a `user.message` unit on an attachment line, and it takes the family's
    // ordinary body rule like every other view of that line: the conversion the harness
    // persisted, which is what the model received. `record_text_emission` used to try the
    // VERBATIM payload before falling through to the seam, so this ONE view showed the
    // payload JSON while an address on the same line showed the conversion - two readers,
    // two bodies, one record.
    let h = attachment_home();
    let out = h.run(&["search", "zzatt also map", &at_sess(), "--attachments"]);
    assert!(out.success, "stderr {}", out.stderr);
    let steer = line_with(&out.stdout, "user.message");
    assert!(
        steer.contains("ordinary turn") && !steer.contains(r#"{"type":"queued_command""#),
        "the steer's body is the conversion:\n{steer}"
    );
    assert!(
        steer.contains("[rendered]") && steer.contains("[mid-turn]"),
        "and it says which copy that is, beside the mid-turn marker:\n{steer}"
    );
    // And the SAME line addressed directly renders the same body - which is the whole
    // point of routing this view through the seam.
    let addressed = h.run(&["show", &at_sess(), "--line", "8"]);
    assert!(addressed.success, "stderr {}", addressed.stderr);
    assert!(
        addressed.stdout.contains("ordinary turn") && addressed.stdout.contains("[rendered]"),
        "an address answers as the scan does:\n{}",
        addressed.stdout
    );
}

#[test]
fn a_backslash_in_a_needle_matches_the_text_it_is_rendered_from() {
    // A converted body is DECODED text, and the section 7 prefilters stay sound over it
    // because a needle `matcher::required_needles` derives carries no whitespace and no
    // JSON-escaped character, and such a run sits verbatim inside the raw line's JSON
    // string. The BACKSLASH is one of those characters: a raw line stores it doubled, so a
    // needle carrying one occurs nowhere in the bytes the prefilter scans.
    //
    // `matcher::json_escapes_in_string` did not count it, so a needle carrying a backslash
    // was emitted and the line prefilter proved absence over text csift had just put on
    // screen. It counts it now, which SPLITS such a run exactly as a quote already splits
    // it - `C:\Users\dev` derives the needle `Users`, which is in the raw bytes.
    //
    // BOTH PAIRS MATCH, and the second pair is why the predicate is where the fix went:
    // the identical drop happened on a plain `user.message`, which no attachment code
    // touches.
    let h = Home::new();
    let lines = [
        r#"{"type":"user","uuid":"aa000000-0000-4000-8000-000000000001","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzbs plain zzuserC:\\Users\\devzz end"}}"#,
        r#"{"type":"attachment","uuid":"bb000000-0000-4000-8000-000000000002","parentUuid":"aa000000-0000-4000-8000-000000000001","timestamp":"2026-06-07T05:00:01.000Z","attachment":{"type":"total_tokens_reminder","content":"zzbs budget"},"rendered":[{"content":"<system-reminder>\nzzbs conv zzconvC:\\Users\\devzz end\n</system-reminder>"}]}"#,
    ];
    h.write(
        &format!("{ENC}/8b2c3d4e-5f60-4172-8183-b4c5d6e7f809.jsonl"),
        &format!("{}\n", lines.join("\n")),
    );
    let target = "@8b2c3d4e-5f60-4172-8183-b4c5d6e7f809";

    let shown = h.run(&["show", target, "--line", "2"]);
    assert!(shown.success, "stderr {}", shown.stderr);
    assert!(
        shown.stdout.contains(r"zzconvC:\Users\devzz end"),
        "the converted body IS on screen, backslashes and all:\n{}",
        shown.stdout
    );

    // (pattern, does it reach the attachment view). The two patterns of each pair
    // describe the SAME rendered text and differ only in whether the needle the prefilter
    // derives carries a backslash, so both of each pair must answer alike.
    for (pattern, attachments) in [
        (r"zzconvC:\\Users\\devzz", true),
        (r"zzconvC:.Users.devzz", true),
        (r"zzuserC:\\Users\\devzz", false),
        (r"zzuserC:.Users.devzz", false),
    ] {
        let mut argv = vec!["search", pattern, target];
        if attachments {
            argv.push("--attachments");
        }
        let out = h.run(&argv);
        assert!(
            out.success,
            "a zero-match search is a definitive absence at exit 0: {}",
            out.stderr
        );
        assert!(
            !out.stdout.contains("no matching exchanges"),
            "{pattern:?} should match the body csift renders for that record\n{}",
            out.stdout
        );
    }
}
