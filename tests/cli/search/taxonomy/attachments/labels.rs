//! What each leaf of the family calls itself in the label zone, which census key it
//! produces, and what the harness's own converter decided about delivering it.

use super::*;

#[test]
fn every_leaf_of_the_family_names_itself_in_the_label_zone() {
    let h = attachment_home();
    let out = h.run(&["search", "zzatt", &at_sess(), "--attachments"]);
    assert!(out.success, "stderr {}", out.stderr);
    for (leaf, marker) in [
        ("harness.hook.context", "[hook UserPromptSubmit reef-brief]"),
        ("harness.context.listing", "[context skill_listing]"),
        ("harness.context.file", "[context file]"),
        (
            "harness.context.reminder",
            "[context total_tokens_reminder]",
        ),
        ("harness.context.prompt", "[context instructions]"),
        ("harness.context.mode", "[context plan_mode]"),
        ("harness.context.ide", "[context opened_file_in_ide]"),
        ("harness.context.task", "[context queued_command]"),
        ("agent.message.structured", "[structured]"),
    ] {
        let line = line_with(&out.stdout, leaf);
        assert!(
            line.contains(marker),
            "{leaf} should carry {marker}:\n{line}"
        );
    }
    let run = line_with(&out.stdout, "zzatt stdout reef ok");
    assert!(
        run.contains("[hook PostToolUse reef-gate exit 0 42ms]"),
        "a hook run names its event, its hook, its exit code and its duration:\n{run}"
    );
    let steer = line_with(&out.stdout, "[mid-turn]");
    assert!(
        steer.contains("user.message"),
        "the person's steer is a user message that arrived mid-turn:\n{steer}"
    );
}

#[test]
fn the_census_key_of_each_line_is_its_own_leaf() {
    // The reachability half: every leaf this family owns has a fixture line, and each
    // censuses under its own key. The two isMeta context leaves (`image`, `skill`) are not
    // attachment lines and belong to the injections family's fixture.
    let h = attachment_home();
    let out = h.run(&[
        "search",
        "zzatt",
        &at_sess(),
        "--attachments",
        "--count-by",
        "label",
    ]);
    assert!(out.success, "stderr {}", out.stderr);
    for (leaf, n) in [
        ("harness.hook.context", 1),
        ("harness.hook.outcome", 2),
        ("harness.context.reminder", 1),
        ("harness.context.listing", 1),
        ("harness.context.prompt", 1),
        ("harness.context.file", 1),
        ("harness.context.mode", 1),
        ("harness.context.ide", 1),
        ("harness.context.task", 2),
        ("agent.message.structured", 1),
        ("user.message", 2),
    ] {
        let row = line_with(&out.stdout, leaf);
        assert!(
            row.split_whitespace().any(|t| t == n.to_string()),
            "{leaf} should census {n}:\n{row}\nfull:\n{}",
            out.stdout
        );
    }
}

#[test]
fn a_steer_and_a_rider_split_on_the_origin_not_the_payload_type() {
    // Same payload type, two leaves: the person's own words stay a user message (and a
    // FLAGLESS scan reaches them), the harness's own rider is context.
    let h = attachment_home();
    let flagless = h.run(&["search", "zzatt", &at_sess()]);
    assert!(flagless.success, "stderr {}", flagless.stderr);
    assert!(
        flagless.stdout.contains("zzatt also map the channel"),
        "a flagless scan reaches the person's mid-turn steer:\n{}",
        flagless.stdout
    );
    assert!(
        !flagless.stdout.contains("zzatt rider pulse"),
        "...and not the harness's own queued rider:\n{}",
        flagless.stdout
    );
    let rider = h.run(&["search", "zzatt rider", &at_sess(), "--attachments"]);
    assert!(
        line_with(&rider.stdout, "zzatt rider pulse").contains("harness.context.task"),
        "the rider keeps the task leaf:\n{}",
        rider.stdout
    );
}

#[test]
fn delivery_reads_the_converters_own_answer_per_record() {
    // Three arms, three verdicts. A payload type the converter drops is not delivered
    // whatever its leaf says; a hook run is delivered exactly when it carries the
    // persisted conversion; and a NON-TEXT conversion writes none, so absence there is
    // not evidence of non-delivery and the leaf default stands.
    let h = attachment_home();
    let out = h.run(&["search", "zzatt", &at_sess(), "--attachments"]);
    assert!(out.success, "stderr {}", out.stderr);
    assert!(
        line_with(&out.stdout, "agent.message.structured").contains("[not delivered]"),
        "a drop-arm payload is not delivered:\n{}",
        out.stdout
    );
    assert!(
        line_with(&out.stdout, "zzatt stdout reef ok").contains("[not delivered]"),
        "a hook run with no persisted conversion is not delivered:\n{}",
        out.stdout
    );
    assert!(
        !line_with(&out.stdout, "zzatt start notice").contains("[not delivered]"),
        "a hook run that DID convert is delivered:\n{}",
        out.stdout
    );
    assert!(
        !line_with(&out.stdout, "harness.context.file").contains("[not delivered]"),
        "a file whose conversion mixed in an image is still delivered:\n{}",
        out.stdout
    );
}
