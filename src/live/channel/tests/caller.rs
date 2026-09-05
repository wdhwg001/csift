//! Caller classification: the environment decides lane vs external, `--from` decides which
//! lane, and neither is allowed to invent the other.

use super::*;

use crate::live::channel::caller::{classify_with, session_transcript_for, SettingsDisclosure};
use crate::path::settings::{SourceReport, SCOPE_LOCAL, SCOPE_PLUGIN, SCOPE_PROJECT, SCOPE_USER};
use std::path::Path;

#[test]
fn the_session_variable_present_means_a_claude_code_lane() {
    let c = classify_with(None, Some(SESSION.to_string())).unwrap();
    assert_eq!(c.kind, SenderKind::Lane);
    assert_eq!(c.session.as_deref(), Some(SESSION));
    assert_eq!(
        c.lane.as_deref(),
        Some(SESSION),
        "with no --from the send is attributed to the top-level session"
    );
    assert!(
        !c.lane_exact,
        "and says so: the environment never names a subagent lane"
    );
    assert!(c.label.is_none());
}

#[test]
fn from_names_an_exact_lane_and_main_resolves_to_the_session() {
    let exact = classify_with(Some(&format!("@{AGENT}")), Some(SESSION.to_string())).unwrap();
    assert_eq!(exact.lane.as_deref(), Some(AGENT));
    assert!(exact.lane_exact);

    let teammate = classify_with(Some(&format!("@{TEAMMATE}")), Some(SESSION.to_string())).unwrap();
    assert_eq!(teammate.lane.as_deref(), Some(TEAMMATE));

    let main = classify_with(Some("@main"), Some(SESSION.to_string())).unwrap();
    assert_eq!(main.lane.as_deref(), Some(SESSION));
    assert!(
        main.lane_exact,
        "@main is an exact claim, not an assumption"
    );
}

#[test]
fn a_bare_label_inside_claude_code_is_refused_because_it_would_misattribute_a_lane() {
    let err = classify_with(Some("ci-runner"), Some(SESSION.to_string())).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("`@<lane>` form"), "{msg}");
    assert!(msg.contains("external-caller form"), "{msg}");
}

#[test]
fn a_from_that_is_not_a_lane_id_is_refused_with_the_grammar() {
    let err = classify_with(Some("@nope"), Some(SESSION.to_string())).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("not a lane id"), "{msg}");
    assert!(msg.contains("csift agents"), "{msg}");
}

#[test]
fn no_session_variable_means_external_with_a_free_label() {
    let anon = classify_with(None, None).unwrap();
    assert_eq!(anon.kind, SenderKind::External);
    assert_eq!(
        anon.label.as_deref(),
        Some("unknown"),
        "the default label is a literal, never the operating-system user"
    );
    assert!(anon.session.is_none() && anon.lane.is_none());

    let named = classify_with(Some("ci-runner"), None).unwrap();
    assert_eq!(named.label.as_deref(), Some("ci-runner"));
    assert_eq!(named.kind, SenderKind::External);
}

#[test]
fn an_external_caller_cannot_claim_a_lane() {
    let err = classify_with(Some(&format!("@{AGENT}")), None).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("free LABEL"), "{msg}");
    assert!(msg.contains("cannot verify a lane claim"), "{msg}");
}

#[test]
fn a_blank_from_falls_back_to_the_default_label_rather_than_an_empty_sender() {
    let c = classify_with(Some("   "), None).unwrap();
    assert_eq!(c.label.as_deref(), Some("unknown"));
}

/// Build a disclosure straight from source rows, so the reduction is tested without a
/// filesystem: which scopes contributed, which did not, and what stays unobservable.
fn disclosure(rows: &[(&'static str, &str, bool, Option<&str>)]) -> SettingsDisclosure {
    SettingsDisclosure {
        sources: rows
            .iter()
            .map(|(scope, path, read, note)| SourceReport {
                scope,
                path: PathBuf::from(path),
                read: *read,
                note: note.map(std::string::ToString::to_string),
            })
            .collect(),
        unobservable: vec!["--settings (a file path or inline JSON)", "MDM policy"],
    }
}

#[test]
fn the_disclosure_names_the_scopes_that_contributed_and_the_ones_that_did_not() {
    let d = disclosure(&[
        (
            SCOPE_PLUGIN,
            "/Users/dev/.claude/plugins/a/plugin.json",
            true,
            None,
        ),
        (SCOPE_USER, "/Users/dev/.claude/settings.json", true, None),
        (
            SCOPE_PROJECT,
            "/Users/dev/p/.claude/settings.json",
            false,
            None,
        ),
        (
            SCOPE_LOCAL,
            "/Users/dev/p/.claude/settings.local.json",
            false,
            None,
        ),
    ]);
    assert_eq!(d.read_scopes(), vec![SCOPE_PLUGIN, SCOPE_USER]);
    assert_eq!(d.absent_scopes(), vec![SCOPE_PROJECT, SCOPE_LOCAL]);
    let lines = d.lines();
    assert_eq!(
        lines[0], "read: plugin, user  ·  absent: project, local",
        "the scopes are named in fold order, read ones first"
    );
    assert_eq!(
        lines.last().unwrap(),
        "unobservable: --settings (a file path or inline JSON); MDM policy",
        "the unobservable list rides verbatim, so `unknown` always arrives with its reason"
    );
}

#[test]
fn a_scope_read_from_two_files_is_named_once_and_never_also_as_absent() {
    // The policy tier composes several files and a plugin walk emits one row per manifest, so
    // the text line answers "which scopes", not "which files" - and a scope that contributed
    // through one file is not ALSO reported as missing through another.
    let d = disclosure(&[
        (
            SCOPE_PLUGIN,
            "/Users/dev/.claude/plugins/a/hooks/hooks.json",
            true,
            None,
        ),
        (
            SCOPE_PLUGIN,
            "/Users/dev/.claude/plugins/b/plugin.json",
            true,
            None,
        ),
        (
            SCOPE_PLUGIN,
            "/Users/dev/.claude/plugins/c/plugin.json",
            false,
            None,
        ),
    ]);
    assert_eq!(d.read_scopes(), vec![SCOPE_PLUGIN]);
    assert!(d.absent_scopes().is_empty());
    assert_eq!(d.lines()[0], "read: plugin  ·  absent: none");
}

#[test]
fn a_broken_file_reaches_the_disclosure_as_a_note_and_the_json_carries_every_row() {
    // A note is the ONLY place a broken settings file is visible: it contributed nothing, so
    // no fold, no gate and no slot count records that it exists at all.
    let d = disclosure(&[
        (SCOPE_USER, "/Users/dev/.claude/settings.json", true, None),
        (
            SCOPE_PROJECT,
            "/Users/dev/p/.claude/settings.json",
            false,
            Some("malformed JSON: expected value at line 1 column 1"),
        ),
    ]);
    assert_eq!(
        d.notes(),
        vec!["project - malformed JSON: expected value at line 1 column 1"]
    );
    assert_eq!(
        d.lines()[1],
        "note: project - malformed JSON: expected value at line 1 column 1"
    );

    let json = d.json();
    let sources = json["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2, "one entry per FILE, in read order");
    assert_eq!(sources[0]["scope"], SCOPE_USER);
    assert_eq!(sources[0]["read"], true);
    assert!(sources[0]["note"].is_null());
    assert_eq!(sources[1]["read"], false);
    assert!(sources[1]["note"]
        .as_str()
        .unwrap()
        .contains("malformed JSON"));
    assert_eq!(json["unobservable"].as_array().unwrap().len(), 2);
}

#[test]
fn a_cascade_with_no_readable_file_says_so_rather_than_printing_an_empty_list() {
    let d = disclosure(&[(SCOPE_USER, "/Users/dev/.claude/settings.json", false, None)]);
    assert_eq!(d.lines()[0], "read: none  ·  absent: user");
}

#[test]
fn a_lanes_owning_session_transcript_is_found_through_the_subagents_component() {
    let top = Path::new("/p/-Users-dev-relay").join(format!("{SESSION}.jsonl"));
    assert_eq!(session_transcript_for(&top), top);

    let sub = Path::new("/p/-Users-dev-relay")
        .join(SESSION)
        .join("subagents")
        .join(format!("agent-{AGENT}.jsonl"));
    assert_eq!(session_transcript_for(&sub), top);

    let workflow = Path::new("/p/-Users-dev-relay")
        .join(SESSION)
        .join("subagents")
        .join("workflows")
        .join("wf_1")
        .join(format!("agent-{AGENT}.jsonl"));
    assert_eq!(
        session_transcript_for(&workflow),
        top,
        "a workflow lane sits deeper but under the same session"
    );
}
