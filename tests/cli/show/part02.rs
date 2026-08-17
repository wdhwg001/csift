use crate::harness::*;

#[test]
fn list_shows_with_elicitation_sidecar() {
    let h = sidecar_session_home();
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n",
            auq_pending_line(
                "toolu_AQ1",
                "2026-06-27T01:02:03.000Z",
                "Confirm the migration?"
            )
        ),
    );
    let out = h.run(&["list", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("with elicitation sidecar"),
        "list must annotate the pending session:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("AskUserQuestion: Confirm the migration?"),
        "list surfaces the pending kind:\n{}",
        out.stdout
    );

    let j = h.run(&["list", &at(SESS), "--format", "json"]);
    let row = json_rows(&j.stdout, "session").remove(0);
    assert_eq!(row["with_elicitation_sidecar"], true);
    assert!(row["pending_elicitations"].as_array().unwrap().len() == 1);
}

#[test]
fn addressed_show_renders_hook_attachment_without_the_flag() {
    // The refetch a search hit prints (`csift show @<id> --line N`) carries no flag — an
    // explicit line/uuid address must render the attachment record regardless.
    let h = Home::new();
    hook_context_scenario(&h);
    let out = h.run(&["show", &at(HOOKCTX_SESS), "--line", "2"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("quartzlantern") && out.stdout.contains("harborlight"),
        "show --line renders the joined hook context flag-free:\n{}",
        out.stdout
    );
    let out2 = h.run(&["show", &at(HOOKCTX_SESS), "--uuid", "att1"]);
    assert!(
        out2.stdout.contains("quartzlantern"),
        "show --uuid renders it too:\n{}",
        out2.stdout
    );
}
