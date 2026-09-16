//! The ATTACHMENT family end to end: which body an attachment unit shows, what its label
//! zone says, which record it is for, and what the harness's own converter did with it.
//!
//! One fixture line per leaf the family owns, built from the on-disk shapes the inventory
//! names and never copied from a corpus. TWO of the nine `harness.context` leaves are
//! deliberately absent: `image` and `skill` are isMeta USER records, not attachment lines,
//! so their specimens belong to the injections family.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "7c1d2e3f-4a5b-4c6d-8e9f-a0b1c2d3e4f5";

/// The transcript every case here reads. One line per attachment-borne leaf, each with a
/// distinct parent so the handles do not collide, plus a hook run with a persisted
/// conversion and one without.
fn attachment_home() -> Home {
    let h = Home::new();
    let lines = [
        r#"{"type":"user","uuid":"aa000000-0000-4000-8000-000000000001","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzatt chart the reef"}}"#,
        // harness.hook.context, with the harness's own conversion persisted.
        r#"{"type":"attachment","uuid":"bb000000-0000-4000-8000-000000000002","parentUuid":"aa000000-0000-4000-8000-000000000001","timestamp":"2026-06-07T05:00:01.000Z","attachment":{"type":"hook_additional_context","hookName":"reef-brief","hookEvent":"UserPromptSubmit","toolUseID":"hook-bb000000","content":["zzatt the reef brief"]},"rendered":[{"content":"<system-reminder>\nzzatt the reef brief\n</system-reminder>"}]}"#,
        // harness.hook.outcome, a run the converter turned into nothing.
        r#"{"type":"attachment","uuid":"cc000000-0000-4000-8000-000000000003","parentUuid":"bb000000-0000-4000-8000-000000000002","timestamp":"2026-06-07T05:00:02.000Z","attachment":{"type":"hook_success","hookName":"reef-gate","hookEvent":"PostToolUse","toolUseID":"toolu_01zzreef","command":"zzatt-check --reef","content":"","stdout":"zzatt stdout reef ok","stderr":"","exitCode":0,"durationMs":42}}"#,
        // harness.hook.outcome, the conditional arm: a run the converter DID convert.
        r#"{"type":"attachment","uuid":"dd000000-0000-4000-8000-000000000004","parentUuid":"cc000000-0000-4000-8000-000000000003","timestamp":"2026-06-07T05:00:03.000Z","attachment":{"type":"hook_success","hookName":"reef-start","hookEvent":"SessionStart","toolUseID":"hook-dd000000","command":"zzatt-start --reef","content":"zzatt start notice","stdout":"zzatt start notice","stderr":"","exitCode":0,"durationMs":7},"rendered":[{"content":"<system-reminder>\nzzatt start notice\n</system-reminder>"}]}"#,
        // harness.context.listing
        r#"{"type":"attachment","uuid":"ee000000-0000-4000-8000-000000000005","parentUuid":"dd000000-0000-4000-8000-000000000004","timestamp":"2026-06-07T05:00:04.000Z","attachment":{"type":"skill_listing","content":"zzatt skills reef-mapper"}}"#,
        // harness.context.file with a NON-TEXT conversion: no persisted copy, still delivered.
        r#"{"type":"attachment","uuid":"ff000000-0000-4000-8000-000000000006","parentUuid":"ee000000-0000-4000-8000-000000000005","timestamp":"2026-06-07T05:00:05.000Z","attachment":{"type":"file","filename":"zzatt-reef.png","content":{"type":"image"}}}"#,
        // agent.message.structured, one of the converter's own drop arms.
        r#"{"type":"attachment","uuid":"a1000000-0000-4000-8000-000000000007","parentUuid":"ff000000-0000-4000-8000-000000000006","timestamp":"2026-06-07T05:00:06.000Z","attachment":{"type":"structured_output","toolUseID":"toolu_01zzout","output":{"zzatt":"reef depth 12m"}}}"#,
        // the person's own mid-turn steer, carrying the ordinary conversion and NOT the
        // human-turn one - the shape 20 of the 164 measured human-origin lines have, and
        // none of them carries the second key.
        r#"{"type":"attachment","uuid":"b2000000-0000-4000-8000-000000000008","parentUuid":"a1000000-0000-4000-8000-000000000007","timestamp":"2026-06-07T05:00:07.000Z","attachment":{"type":"queued_command","commandMode":"prompt","origin":{"kind":"human"},"prompt":"zzatt also map the channel"},"rendered":[{"content":"zzatt also map the channel, ordinary turn"}]}"#,
        // a queued RIDER: same payload type, no human origin, so it keeps the task leaf.
        r#"{"type":"attachment","uuid":"c3000000-0000-4000-8000-000000000009","parentUuid":"b2000000-0000-4000-8000-000000000008","timestamp":"2026-06-07T05:00:08.000Z","attachment":{"type":"queued_command","commandMode":"task-notification","prompt":"zzatt rider pulse"}}"#,
        // harness.context.reminder
        r#"{"type":"attachment","uuid":"d4000000-0000-4000-8000-000000000010","parentUuid":"c3000000-0000-4000-8000-000000000009","timestamp":"2026-06-07T05:00:09.000Z","attachment":{"type":"total_tokens_reminder","content":"zzatt budget 120k"}}"#,
        // harness.context.prompt
        r#"{"type":"attachment","uuid":"e5000000-0000-4000-8000-000000000011","parentUuid":"d4000000-0000-4000-8000-000000000010","timestamp":"2026-06-07T05:00:10.000Z","attachment":{"type":"instructions","content":"zzatt house rules"}}"#,
        // harness.context.mode
        r#"{"type":"attachment","uuid":"f6000000-0000-4000-8000-000000000012","parentUuid":"e5000000-0000-4000-8000-000000000011","timestamp":"2026-06-07T05:00:11.000Z","attachment":{"type":"plan_mode","content":"zzatt plan mode on"}}"#,
        // harness.context.ide
        r#"{"type":"attachment","uuid":"a7000000-0000-4000-8000-000000000013","parentUuid":"f6000000-0000-4000-8000-000000000012","timestamp":"2026-06-07T05:00:12.000Z","attachment":{"type":"opened_file_in_ide","filename":"zzatt-reef.rs"}}"#,
        // a second RIDER, carrying BOTH conversions - the shape every measured line with
        // the human-turn key has (all of them riders, none of them the person's own
        // steer). Appended LAST so no line number above it moves.
        r#"{"type":"attachment","uuid":"b8000000-0000-4000-8000-000000000014","parentUuid":"a7000000-0000-4000-8000-000000000013","timestamp":"2026-06-07T05:00:13.000Z","attachment":{"type":"queued_command","commandMode":"task-notification","prompt":"zzatt also map the channel"},"rendered":[{"content":"zzatt also map the channel, ordinary turn"}],"renderedInHumanTurn":[{"content":"zzatt also map the channel, human turn"}]}"#,
    ];
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{}\n", lines.join("\n")),
    );
    h
}

fn at_sess() -> String {
    format!("@{SESS}")
}

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
        converted.contains("[rendered]"),
        "a run the converter DID convert renders that text instead:\n{converted}"
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

#[test]
fn an_addressed_attachment_renders_the_same_unit_show_and_search_agree() {
    let h = attachment_home();
    for (line_no, expect) in [
        (2usize, "harness.hook.context"),
        (3, "harness.hook.outcome"),
        (5, "harness.context.listing"),
        (7, "agent.message.structured"),
        (8, "user.message"),
        (10, "harness.context.reminder"),
        (11, "harness.context.prompt"),
        (12, "harness.context.mode"),
        (13, "harness.context.ide"),
    ] {
        let out = h.run(&["show", &at_sess(), "--line", &line_no.to_string()]);
        assert!(out.success, "L{line_no}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains(expect),
            "L{line_no} should render {expect}:\n{}",
            out.stdout
        );
        assert!(
            out.stdout.contains("[rendered]") || out.stdout.contains("[payload]"),
            "L{line_no} says which of the two copies it shows:\n{}",
            out.stdout
        );
    }
}

#[test]
fn every_attachment_unit_says_which_record_it_is_for() {
    // Two `for` edges and a producer on every unit, each read from a key the line carries.
    // The parent RESOLVES to a line, which is an address a reader can fetch; the payload's
    // tool-call id is a wire id no line of this transcript carries, so the edge keeps the
    // id and says where it is not.
    let h = attachment_home();
    let out = h.run(&["search", "zzatt", &at_sess(), "--attachments"]);
    assert!(out.success, "stderr {}", out.stderr);
    let ctx = line_with(&out.stdout, "harness.hook.context");
    assert!(
        ctx.contains("[for: attached_to L1]"),
        "the injected context hangs from the person's message, resolved to its line:\n{ctx}"
    );
    assert!(
        ctx.contains("[for: for_tool_use hook-bb0 (not in this file)]"),
        "and answers for a tool call no line here carries:\n{ctx}"
    );
    let run = line_with(&out.stdout, "zzatt stdout reef ok");
    assert!(
        run.contains("[produced_by zzatt-check --reef]"),
        "a hook run names the command it executed, whole:\n{run}"
    );
}

#[test]
fn an_edge_with_no_key_on_the_line_renders_its_reason() {
    // Honest empties: a payload type that records no producer says so and names the type,
    // rather than leaving a reader to guess whether csift looked.
    let h = attachment_home();
    let out = h.run(&["search", "zzatt skills", &at_sess(), "--attachments"]);
    assert!(out.success, "stderr {}", out.stderr);
    let listing = line_with(&out.stdout, "harness.context.listing");
    assert!(
        listing.contains("[for: for_tool_use unknown ("),
        "a payload with no tool-call id says so:\n{listing}"
    );
    assert!(
        listing.contains("[produced_by unknown (no producer key; type=skill_listing)]"),
        "and the producer fact names the type it read:\n{listing}"
    );
}

#[test]
fn the_edges_ride_json_under_the_shared_for_key() {
    // Both edges accumulate into ONE `for` array - the shared merge rule - so a consumer
    // reads every provenance edge of every family the same way, and the producer rides its
    // own key beside them because it points at a command rather than at a record.
    let h = attachment_home();
    let out = h.run(&[
        "search",
        "zzatt the reef brief",
        &at_sess(),
        "--attachments",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr {}", out.stderr);
    let hit: serde_json::Value = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["hits"][0]["label"] == "harness.hook.context")
        .map(|v| v["hits"][0].clone())
        .unwrap_or_else(|| panic!("no hook.context hit in:\n{}", out.stdout));
    let edges = hit["for"].as_array().expect("the shared for array");
    assert_eq!(edges.len(), 2, "both edges accumulate: {edges:?}");
    let parent = edges
        .iter()
        .find(|e| e["kind"] == "attached_to")
        .expect("the parent edge");
    assert_eq!(
        parent["target"]["uuid"], "aa000000-0000-4000-8000-000000000001",
        "the WHOLE uuid, which `show --uuid` takes directly"
    );
    assert_eq!(parent["target"]["line"], 1);
    assert_eq!(parent["basis"], "key");
    let tool = edges
        .iter()
        .find(|e| e["kind"] == "for_tool_use")
        .expect("the tool edge");
    assert_eq!(tool["target"]["uuid"], "hook-bb000000");
    assert_eq!(
        tool["target"]["line"],
        serde_json::Value::Null,
        "a tool-call id is not a record uuid, so no line is fabricated"
    );
    assert_eq!(
        hit["produced_by"]["command"],
        serde_json::Value::Null,
        "injected context records no command"
    );
    assert_eq!(
        hit["produced_by"]["hook_name"], "reef-brief",
        "and the fact names what the line DOES carry"
    );
}

#[test]
fn an_addressed_unit_carries_the_same_edges_a_scanned_one_does() {
    // The same keys must not answer two ways depending on how the record was reached.
    let h = attachment_home();
    let out = h.run(&["show", &at_sess(), "--line", "3", "--format", "json"]);
    assert!(out.success, "stderr {}", out.stderr);
    let row: serde_json::Value = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["label"] == "harness.hook.outcome")
        .unwrap_or_else(|| panic!("no addressed row in:\n{}", out.stdout));
    assert_eq!(row["produced_by"]["command"], "zzatt-check --reef");
    assert_eq!(
        row["produced_by"]["hook_name"], "reef-gate",
        "the hook's name rides the command branch too"
    );
    let edges = row["for"].as_array().expect("the shared for array");
    assert_eq!(
        edges
            .iter()
            .find(|e| e["kind"] == "for_tool_use")
            .expect("the tool edge")["target"]["uuid"],
        "toolu_01zzreef"
    );
    assert_eq!(
        edges
            .iter()
            .find(|e| e["kind"] == "attached_to")
            .expect("the parent edge")["target"]["line"],
        2
    );
}

#[test]
fn the_help_names_every_marker_word_json_key_and_render_form_the_family_ships() {
    // Information parity: a reader of `--help` alone learns the same surface SKILL does.
    // Every word in these two lists is one this family actually prints.
    let h = Home::new();
    let search = h.run(&["search", "--help"]);
    assert!(search.success, "stderr {}", search.stderr);
    for word in [
        "ATTACHMENT LEAVES",
        "[rendered]",
        "[payload]",
        "body_source",
        "[human-turn render differs]",
        "human_turn_render_differs",
        "[hook <event> <name>]",
        "[context <payload type>]",
        "[structured]",
        "[mid-turn]",
        "[for: <kind> L<line>]",
        "[produced_by <command>]",
        "{kind,basis,target,reason}",
        "produced_by:{command,hook_name,payload_type}",
    ] {
        assert!(
            search.stdout.contains(word),
            "search --help should name {word}"
        );
    }
    let show = h.run(&["show", "--help"]);
    assert!(show.success, "stderr {}", show.stderr);
    for key in [
        "body_source",
        "human_turn_render_differs",
        "produced_by:{command,hook_name,payload_type}",
        "for:[{kind,basis,target:{line,uuid,session,note},reason}]",
    ] {
        assert!(show.stdout.contains(key), "show --help should name {key}");
    }
}

/// The first stdout line containing `needle`, for an assertion that wants ONE unit.
fn line_with(stdout: &str, needle: &str) -> String {
    stdout
        .lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no line carrying {needle:?} in:\n{stdout}"))
        .to_string()
}
