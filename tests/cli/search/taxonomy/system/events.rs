//! What each event kind RENDERS, and the census keys the six leaves produce.

use super::*;

/// Each of the five event kinds renders its own head with the fields that make that event
/// useful - never the one-size subtype-and-level form.
#[test]
fn every_system_event_kind_renders_its_own_fields() {
    let h = system_home();
    let out = h.run(&["search", "", &at(SESS), "-t", "harness.system.*"]);
    assert!(out.success, "stderr: {}", out.stderr);
    for (leaf, text) in [
        (
            "harness.system.refusal",
            "[refusal model_refusal_fallback] originalModel=model-alpha fallbackModel=model-beta \
             apiRefusalCategory=cyber Switched models to finish this turn",
        ),
        (
            "harness.system.refusal",
            "[refusal model_refusal_no_fallback] originalModel=model-alpha fallbackModel=none \
             apiRefusalCategory=bio",
        ),
        (
            "harness.system.api-error",
            "[api-error status=529 retry 3/20 in 1522.5ms] overloaded_error Overloaded zzoverload",
        ),
        (
            "harness.system.notice",
            "[notice warning] Remote Control disconnected zznotice",
        ),
        ("harness.system.kill", "[kill]"),
        (
            "harness.system.command",
            "[command] <local-command-stdout>zzstdout ok</local-command-stdout>",
        ),
    ] {
        assert!(
            out.stdout.contains(leaf) && out.stdout.contains(text),
            "missing {leaf} / {text:?}:\n{}",
            out.stdout
        );
    }
}

/// `--count-by label` names every leaf this family owns, with the two refusal subtypes
/// folded into ONE key: whether a second model was tried is a field, not an event kind.
#[test]
fn the_label_census_names_every_system_event_key() {
    let h = system_home();
    let out = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.system.*",
        "-t",
        "harness.schedule.event",
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows = json_rows(&out.stdout, "census");
    let count = |key: &str| {
        rows.iter()
            .find(|r| r["key"] == key)
            .and_then(|r| r["records"].as_u64())
            .unwrap_or(0)
    };
    assert_eq!(count("harness.system.refusal"), 2, "{rows:?}");
    assert_eq!(count("harness.system.api-error"), 1, "{rows:?}");
    assert_eq!(count("harness.system.notice"), 1, "{rows:?}");
    assert_eq!(count("harness.system.kill"), 1, "{rows:?}");
    assert_eq!(count("harness.system.command"), 1, "{rows:?}");
    assert_eq!(count("harness.schedule.event"), 2, "{rows:?}");
}
