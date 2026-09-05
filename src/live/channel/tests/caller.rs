//! Caller classification: the environment decides lane vs external, `--from` decides which
//! lane, and neither is allowed to invent the other.

use super::*;

use crate::live::channel::caller::{classify_with, session_transcript_for};
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
