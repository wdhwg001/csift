//! The provenance edges a system record carries: the rewind target a refusal names
//! by key, and the slash wrapper a delivered command names by position.

use super::*;

/// The exposed keys of a refusal hit, and the ONE uuid key that resolves - as an EDGE
/// carrying the line, which `show --line` then fetches. The wire uuids are not exposed.
#[test]
fn a_refusal_hit_exposes_its_models_and_the_rewind_edge() {
    let h = system_home();
    let out = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.system.refusal",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hits = hits_of(&out.stdout);
    let fb = hits
        .iter()
        .find(|hh| hh["retried"] == serde_json::json!(true))
        .expect("the fallback refusal hit");
    assert_eq!(fb["original_model"], serde_json::json!("model-alpha"));
    assert_eq!(fb["fallback_model"], serde_json::json!("model-beta"));
    assert_eq!(fb["category"], serde_json::json!("cyber"));
    // The edge names the REWIND TARGET and resolves it to its own line, so the reader
    // has an address rather than an id to hunt for. `for` is an ARRAY, one object per
    // edge, so several edges on one unit accumulate rather than overwrite.
    assert_eq!(fb["for"][0]["kind"], serde_json::json!("refused"));
    assert_eq!(fb["for"][0]["basis"], serde_json::json!("key"));
    assert_eq!(fb["for"][0]["target"]["uuid"], serde_json::json!("u1"));
    assert_eq!(fb["for"][0]["target"]["line"], serde_json::json!(1));
    assert_eq!(fb["for"][0]["reason"], serde_json::Value::Null);
    // The wire uuids never resolve to a line, so they are not offered as a key.
    assert!(fb.get("retracted_message_uuids").is_none(), "{fb}");
    let no_fb = hits
        .iter()
        .find(|hh| hh["retried"] == serde_json::json!(false))
        .expect("the no-fallback refusal hit");
    assert!(no_fb.get("fallback_model").is_none(), "{no_fb}");
    // A null value is an HONEST unknown with its reason, never a missing fact.
    assert_eq!(no_fb["for"][0]["target"], serde_json::Value::Null);
    assert_eq!(
        no_fb["for"][0]["reason"],
        serde_json::json!("the record carries no such key")
    );
    // The text surface renders the same two answers in the label zone.
    let text = h.run(&["search", "", &at(SESS), "-t", "harness.system.refusal"]);
    assert!(text.stdout.contains("[for: refused L1]"), "{}", text.stdout);
    assert!(
        text.stdout
            .contains("[for: refused unknown (the record carries no such key)]"),
        "{}",
        text.stdout
    );
    // The line the edge names is a real address in this transcript.
    let target = h.run(&["show", &at(SESS), "--line", "1"]);
    assert!(
        target.success && target.stdout.contains("chart the harbor zzask"),
        "{}\n{}",
        target.stdout,
        target.stderr
    );
}

/// The delivered command names the slash WRAPPER it is the output of, through the chain
/// link rather than a payload key.
#[test]
fn the_delivered_command_names_the_wrapper_it_wraps() {
    let h = system_home();
    let out = h.run(&["search", "", &at(SESS), "-t", "harness.system.command"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("[for: wraps L7]"), "{}", out.stdout);
    let js = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.system.command",
        "--format",
        "json",
    ]);
    let hit = hits_of(&js.stdout).into_iter().next().expect("one hit");
    assert_eq!(hit["for"][0]["kind"], serde_json::json!("wraps"));
    // POSITION, not key: a run of local commands chains stdout to stdout, so the link
    // names the record ABOVE rather than a wrapper the harness pointed at.
    assert_eq!(hit["for"][0]["basis"], serde_json::json!("position"));
    assert_eq!(hit["for"][0]["target"]["line"], serde_json::json!(7));
    let wrapper = h.run(&["show", &at(SESS), "--line", "7"]);
    assert!(
        wrapper.success && wrapper.stdout.contains("/status"),
        "the line the edge names is the wrapper:\n{}",
        wrapper.stdout
    );
}
