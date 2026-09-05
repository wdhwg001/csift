//! The send command's own pieces: the ttl grammar, the sender-to-receiver relation, the
//! gate-value reading, and the two slot lines the receipt prints.

use super::*;

use crate::live::channel::caller::{Caller, Receiver};
use crate::live::channel::policy::{ReceiverKind, ReceiverState};
use crate::live::channel::send::{
    armed_line, parse_ttl, relation_of, slot_line, truthy, SlotCensus,
};
use std::path::PathBuf;

fn lane_caller(session: &str, lane: &str) -> Caller {
    Caller {
        kind: SenderKind::Lane,
        session: Some(session.to_string()),
        lane: Some(lane.to_string()),
        label: None,
        lane_exact: true,
    }
}

fn receiver(session: &str, lane: &str) -> Receiver {
    let session_path = PathBuf::from("/p/-Users-dev-relay").join(format!("{session}.jsonl"));
    Receiver {
        path: session_path.clone(),
        lane: lane.to_string(),
        session: session.to_string(),
        session_path,
        kind: ReceiverKind::UnnamedSubagent,
        state: ReceiverState::Running,
        version: Some("2.1.258".to_string()),
        cwd: Some("/Users/dev/relay".to_string()),
        routing_id: None,
        socket_present: false,
        headless: false,
        teammate_lanes: 0,
    }
}

#[test]
fn the_ttl_grammar_is_the_site_wide_one_read_forward() {
    assert_eq!(parse_ttl("30s").unwrap(), 30);
    assert_eq!(parse_ttl("10m").unwrap(), 600);
    assert_eq!(parse_ttl("12h").unwrap(), 43_200);
    assert_eq!(parse_ttl("2d").unwrap(), 172_800);
    assert_eq!(parse_ttl("1w").unwrap(), 604_800);
    assert_eq!(parse_ttl("1mo").unwrap(), 2_592_000);
    assert_eq!(parse_ttl("1y").unwrap(), 31_536_000);
    assert_eq!(parse_ttl(" 12h ").unwrap(), 43_200);
}

#[test]
fn a_ttl_without_a_unit_or_with_an_unknown_one_is_a_loud_error() {
    for bad in ["12", "h", "", "12x", "12 h"] {
        let err = parse_ttl(bad).unwrap_err();
        assert!(
            format!("{err:#}").contains("--ttl"),
            "{bad} should name the flag"
        );
    }
}

#[test]
fn an_overflowing_ttl_is_reported_rather_than_wrapped() {
    let err = parse_ttl("999999999999999999999y");
    assert!(err.is_err(), "a quantity past u64 is an error, not a wrap");
    let err = parse_ttl(&format!("{}y", u64::MAX)).unwrap_err();
    assert!(format!("{err:#}").contains("--ttl"));
}

#[test]
fn the_top_level_lane_is_the_parent_of_its_own_subagent() {
    let c = lane_caller(SESSION, SESSION);
    let r = receiver(SESSION, AGENT);
    assert_eq!(relation_of(&c, &r), Relation::Parent);
}

#[test]
fn a_subagent_addressing_its_own_session_is_the_child() {
    let c = lane_caller(SESSION, AGENT);
    let r = receiver(SESSION, SESSION);
    assert_eq!(relation_of(&c, &r), Relation::Child);
}

#[test]
fn two_subagents_of_one_session_are_siblings() {
    let c = lane_caller(SESSION, AGENT);
    let r = receiver(SESSION, TEAMMATE);
    assert_eq!(relation_of(&c, &r), Relation::Sibling);
}

#[test]
fn a_lane_addressing_itself_claims_no_relation() {
    let c = lane_caller(SESSION, AGENT);
    let r = receiver(SESSION, AGENT);
    assert_eq!(relation_of(&c, &r), Relation::Unknown);
}

#[test]
fn an_external_caller_is_always_the_external_relation() {
    let c = Caller {
        kind: SenderKind::External,
        session: None,
        lane: None,
        label: Some("ci-runner".to_string()),
        lane_exact: true,
    };
    assert_eq!(
        relation_of(&c, &receiver(SESSION, AGENT)),
        Relation::External
    );
}

#[test]
fn a_settings_env_value_enables_only_when_it_is_not_a_falsey_spelling() {
    for on in ["1", "true", "TRUE", "yes", "on", "anything"] {
        assert!(truthy(on), "{on}");
    }
    for off in ["", "  ", "0", "false", "FALSE", "no", "off"] {
        assert!(!truthy(off), "{off}");
    }
}

#[test]
fn the_slot_line_names_every_event_and_says_so_when_there_are_none() {
    let empty = SlotCensus {
        per_event: Vec::new(),
        best_event: None,
        best: 0,
        async_rewake_on_stop: false,
    };
    assert_eq!(slot_line(&empty), "none configured on any delivery event");

    let some = SlotCensus {
        per_event: vec![
            ("SessionStart".to_string(), vec![1, 2]),
            ("Stop".to_string(), vec![1]),
        ],
        best_event: Some("SessionStart".to_string()),
        best: 2,
        async_rewake_on_stop: false,
    };
    assert_eq!(slot_line(&some), "SessionStart 1,2  Stop 1");
}

#[test]
fn the_armed_line_distinguishes_never_run_from_run() {
    assert_eq!(
        armed_line(&[]),
        "none (no delivery hook has run in this lane)"
    );
    assert_eq!(armed_line(&[1, 2, 4]), "1,2,4");
}
