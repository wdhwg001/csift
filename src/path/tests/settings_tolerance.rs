//! What the cascade reader does with files it cannot use.
//!
//! Every scope is a file another program writes and a human hand-edits, so the reader meets
//! shapes the schema does not describe: a path that is not a file at all, a value typed as
//! something other than a string, a hook group in the wrong shape. None of them may abort the
//! fold, and none may disappear without a note - a scope silently dropped turns a gate verdict
//! that should say "unknown" into a confident wrong answer.

use super::*;

use crate::path::settings::{
    deliver_slots, env_value, hooks_for_event, merged_in, SCOPE_PLUGIN, SCOPE_POLICY_REMOTE,
    SCOPE_USER,
};

#[test]
fn a_settings_path_that_is_not_a_readable_file_is_reported_with_its_reason() {
    // A directory where a settings file belongs reads as an error that is NOT "not found", so
    // it takes the reported-unreadable arm rather than the silent absent one.
    let t = Tree::new();
    let home = t.dir("home");
    t.dir("home/settings.json");
    t.write(
        "home/remote-settings.json",
        "{ this file exists and is not JSON",
    );

    let m = merged_in(&home, None, &t.path("managed"));
    let user = m
        .sources
        .iter()
        .find(|s| s.scope == SCOPE_USER)
        .expect("the user scope is always reported");
    assert!(!user.read);
    assert!(
        user.note
            .as_deref()
            .is_some_and(|n| n.contains("unreadable")),
        "an unreadable path is distinguished from a missing one: {:?}",
        user.note
    );

    // The policy tier is composed first-wins from the remote cache; a malformed cache is
    // reported too, and the tier falls through to the managed files.
    let remote = m
        .sources
        .iter()
        .find(|s| s.scope == SCOPE_POLICY_REMOTE)
        .expect("the remote policy cache is always reported");
    assert!(!remote.read);
    assert!(
        remote
            .note
            .as_deref()
            .is_some_and(|n| n.contains("malformed JSON")),
        "{:?}",
        remote.note
    );
}

#[test]
fn a_non_string_env_value_is_carried_as_text_rather_than_dropped() {
    // Claude Code types `env` as a string map, but a hand-edited file can carry a number or a
    // bool. Dropping one would report a key as unset that IS set, which is exactly the
    // difference between "the gate is off" and "csift cannot see the gate".
    let t = Tree::new();
    let home = t.dir("home");
    t.write(
        "home/settings.json",
        r#"{"env":{"CSIFT_COUNT":7,"CSIFT_ON":true,"CSIFT_LIST":["a"]}}"#,
    );

    let m = merged_in(&home, None, &t.path("managed"));
    assert_eq!(env_value(&m, "CSIFT_COUNT"), Some(("7", SCOPE_USER)));
    assert_eq!(env_value(&m, "CSIFT_ON"), Some(("true", SCOPE_USER)));
    assert_eq!(
        env_value(&m, "CSIFT_LIST"),
        None,
        "a value with no text form is not invented"
    );
}

#[test]
fn hook_shapes_the_schema_does_not_describe_are_tolerated_one_by_one() {
    // Three shapes real files carry: an event whose value is not an array at all, a bare
    // entry sitting directly in the event array (no matcher group around it), and an entry
    // with no command. The first two must not lose the rest of the file, and the third is
    // not a hook - a hook IS its command.
    let t = Tree::new();
    let home = t.dir("home");
    t.write(
        "home/settings.json",
        concat!(
            r#"{"hooks":{"PreToolUse":"not an array","#,
            r#""Stop":[{"type":"command","command":"csift deliver --slot 3"}],"#,
            r#""SubagentStop":[{"hooks":[{"type":"command"},"#,
            r#"{"type":"command","command":"csift deliver --slot 1"}]}]}}"#,
        ),
    );

    let m = merged_in(&home, None, &t.path("managed"));
    assert!(
        hooks_for_event(&m, "PreToolUse").is_empty(),
        "an event that is not an array contributes nothing"
    );
    assert_eq!(
        deliver_slots(&m, "Stop"),
        vec![3],
        "a bare entry in the event array is still a hook"
    );
    assert_eq!(
        hooks_for_event(&m, "SubagentStop").len(),
        1,
        "the entry with no command is dropped, its neighbour is kept"
    );
    assert_eq!(deliver_slots(&m, "SubagentStop"), vec![1]);
}

#[test]
fn a_plugin_with_no_usable_name_is_labelled_by_its_directory() {
    // The source label is what a reader traces a hook back to, so it always resolves to
    // something: a manifest with no `name`, and one whose name is blank, both fall back to
    // the directory the plugin was found in.
    let t = Tree::new();
    let home = t.dir("home");
    t.write(
        "home/plugins/anon/plugin.json",
        r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"csift deliver --slot 4"}]}]}}"#,
    );
    t.write(
        "home/plugins/blank/plugin.json",
        r#"{"name":"   ","hooks":{"Stop":[{"hooks":[{"type":"command","command":"csift deliver --slot 5"}]}]}}"#,
    );

    let m = merged_in(&home, None, &t.path("managed"));
    let sources: Vec<&str> = hooks_for_event(&m, "Stop")
        .iter()
        .map(|h| h.source.as_str())
        .collect();
    assert!(
        sources.contains(&"plugin:anon") && sources.contains(&"plugin:blank"),
        "{sources:?}"
    );
    assert_eq!(deliver_slots(&m, "Stop"), vec![4, 5]);
    assert!(
        m.sources.iter().any(|s| s.scope == SCOPE_PLUGIN),
        "plugin manifests are reported as their own scope"
    );
}

#[test]
fn the_plugin_walk_stops_at_its_budget_and_says_so() {
    // The walk is bounded so a settings read cannot cost an unbounded directory scan. Hitting
    // the bound means manifests were NOT read, which a caller must be able to see: an unread
    // plugin hook is a delivery that will not happen, reported as absent rather than as none.
    let t = Tree::new();
    let home = t.dir("home");
    for i in 0..520 {
        t.dir(&format!("home/plugins/filler{i:04}/empty"));
    }
    t.write(
        "home/plugins/filler0000/empty/keep.txt",
        "not a plugin manifest",
    );

    let m = merged_in(&home, None, &t.path("managed"));
    let stopped = m
        .sources
        .iter()
        .find(|s| s.scope == SCOPE_PLUGIN && !s.read)
        .expect("the exhausted walk is reported");
    assert_eq!(stopped.path, t.path("home/plugins"));
    assert!(
        stopped
            .note
            .as_deref()
            .is_some_and(|n| n.contains("deeper manifests were not read")),
        "{:?}",
        stopped.note
    );
}
