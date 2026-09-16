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
fn the_persons_mid_turn_steer_renders_its_payload_and_says_so() {
    // The one attachment view another render answers for: `record_text_emission` resolves
    // a `user.message` unit as the reconstructed opener text, then the VERBATIM payload,
    // and only then the seam this family answers at - so a steer whose record ALSO carries
    // a persisted conversion shows the payload JSON, and the body fact names the payload
    // rather than a conversion the unit is not showing.
    let h = attachment_home();
    let out = h.run(&["search", "zzatt also map", &at_sess(), "--attachments"]);
    assert!(out.success, "stderr {}", out.stderr);
    let steer = line_with(&out.stdout, "user.message");
    assert!(
        steer.contains(r#"{"type":"queued_command""#) && !steer.contains("ordinary turn"),
        "the steer's body is its payload JSON:\n{steer}"
    );
    assert!(
        steer.contains("[payload]") && steer.contains("[mid-turn]"),
        "and it says which copy that is, beside the mid-turn marker:\n{steer}"
    );
}
