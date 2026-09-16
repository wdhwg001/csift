//! Where else the family's surface has to agree with itself: an addressed record against
//! a scanned one, the two help pages, and a payload type csift models no leaf for.

use super::*;

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

#[test]
fn an_addressed_unmodeled_attachment_carries_its_facts_in_json_and_its_shape_in_text() {
    // The 1:1 law's attachment case, and the ONE place the two surfaces differ on purpose.
    // A payload type csift models no leaf for still carries every key the line holds, so
    // JSON answers exactly as it does for a labelled record; the TEXT renders the shape in
    // place of a label, which is what a leafless hit does, so the edges are not there.
    let h = Home::new();
    let lines = [
        r#"{"type":"user","uuid":"aa000000-0000-4000-8000-000000000001","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzunm chart the reef"}}"#,
        r#"{"type":"attachment","uuid":"bb000000-0000-4000-8000-000000000002","parentUuid":"aa000000-0000-4000-8000-000000000001","timestamp":"2026-06-07T05:00:01.000Z","attachment":{"type":"zzunm_future_type","toolUseID":"toolu_01zzunm","note":"zzunm payload"}}"#,
    ];
    h.write(
        &format!("{ENC}/9a1b2c3d-4e5f-4061-8072-a3b4c5d6e7f8.jsonl"),
        &format!("{}\n", lines.join("\n")),
    );
    let target = "@9a1b2c3d-4e5f-4061-8072-a3b4c5d6e7f8";

    let text = h.run(&["show", target, "--line", "2"]);
    assert!(text.success, "stderr {}", text.stderr);
    assert!(
        text.stdout
            .contains("? unmodeled: attachment/zzunm_future_type"),
        "the shape stands in for the label:\n{}",
        text.stdout
    );
    assert!(
        !text.stdout.contains("[for: "),
        "a leafless unit renders no label zone, so no edge is in the text:\n{}",
        text.stdout
    );

    let json = h.run(&["show", target, "--line", "2", "--format", "json"]);
    assert!(json.success, "stderr {}", json.stderr);
    let row: serde_json::Value = json
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["unmodeled"] == "attachment/zzunm_future_type")
        .unwrap_or_else(|| panic!("no unmodeled row in:\n{}", json.stdout));
    let edges = row["for"].as_array().expect("the shared for array");
    assert_eq!(
        edges.len(),
        2,
        "both edges ride the leafless unit: {edges:?}"
    );
    assert_eq!(
        edges
            .iter()
            .find(|e| e["kind"] == "attached_to")
            .expect("the parent edge")["target"]["line"],
        1,
        "and the parent still resolves to its line"
    );
    assert_eq!(
        edges
            .iter()
            .find(|e| e["kind"] == "for_tool_use")
            .expect("the tool edge")["target"]["uuid"],
        "toolu_01zzunm"
    );
    assert_eq!(row["produced_by"]["payload_type"], "zzunm_future_type");
    assert_eq!(row["body_source"], "payload");
}
