//! The policy table, row by row: which channel carries a message, what the sender is
//! promised, and the four states that turn a send into a refusal.
//!
//! Every case builds the context directly, because the table is exactly the part that has to
//! be auditable without a live Claude Code session behind it.

use super::*;

use crate::live::channel::caller::GateVerdict;
use crate::live::channel::policy::{
    decide, full_note, parse_version, ReceiverKind, ReceiverState, SendContext, CH_IN_PROCESS,
    CH_MAILBOX, CH_NONE, CH_QUEUE, CH_RESUME, CH_STEER, CH_UDS,
};
use crate::live::channel::send::relation_of;

/// A running unnamed subagent, reachable, with two configured and two armed slots: the
/// baseline every row below varies ONE fact away from.
fn ctx() -> SendContext {
    SendContext {
        caller_kind: SenderKind::Lane,
        caller_is_subagent: false,
        relation: Relation::Parent,
        receiver_kind: ReceiverKind::UnnamedSubagent,
        receiver_state: ReceiverState::Running,
        receiver_version: Some("2.1.258".to_string()),
        headless: false,
        socket_present: false,
        lane: AGENT.to_string(),
        routing_id: None,
        mode: Mode::Steer,
        resume: false,
        official_only: false,
        teams: GateVerdict::teams(None, 0, 0),
        harbor: GateVerdict::harbor(false),
        chunks: 1,
        best_slots: 2,
        best_event: Some("SessionStart".to_string()),
        armed: 2,
        async_rewake_on_stop: false,
        hooks_policy_switch: None,
    }
}

fn teammate() -> SendContext {
    SendContext {
        receiver_kind: ReceiverKind::Teammate,
        lane: TEAMMATE.to_string(),
        routing_id: Some("Relay@harbor".to_string()),
        ..ctx()
    }
}

#[test]
fn running_teammate_with_the_teams_gate_enabled_delegates_the_mailbox() {
    let c = SendContext {
        teams: GateVerdict::teams(Some("user"), 0, 0),
        ..teammate()
    };
    let d = decide(&c);
    assert_eq!(d.channel, CH_MAILBOX);
    assert!(
        d.queued,
        "an official delegation still queues on the channel"
    );
    let official = d.official.expect("the mailbox row delegates");
    assert_eq!(official.to.as_deref(), Some("Relay@harbor"));
    assert!(
        official.call.contains("SendMessage(to: \"Relay@harbor\""),
        "the receipt prints the exact call: {}",
        official.call
    );
    assert!(
        d.risks.iter().any(|r| r.contains("DELETED")),
        "the mailbox's consume-deletes semantics is a named risk"
    );
}

#[test]
fn running_teammate_without_a_provable_teams_gate_falls_to_csift_steer() {
    let d = decide(&teammate());
    assert_eq!(d.channel, CH_STEER);
    assert!(d.official.is_none(), "an unprovable gate delegates nothing");
    assert!(
        d.risks.iter().any(|r| r.starts_with("teams gate:")),
        "the gate verdict rides as a risk: {:?}",
        d.risks
    );
    assert_eq!(d.verdict, Verdict::MayFail);
}

#[test]
fn running_unnamed_subagent_delegates_the_in_process_queue_addressed_by_its_own_id() {
    let d = decide(&ctx());
    assert_eq!(d.channel, CH_IN_PROCESS);
    let official = d.official.expect("the in-process row delegates");
    assert_eq!(official.to.as_deref(), Some(AGENT));
    assert!(official.call.contains(AGENT));
}

#[test]
fn a_workflow_lane_is_csift_only_while_it_runs() {
    let c = SendContext {
        receiver_kind: ReceiverKind::WorkflowLane,
        ..ctx()
    };
    let d = decide(&c);
    assert_eq!(d.channel, CH_STEER);
    assert!(d.official.is_none(), "the official send fails closed here");
    assert!(d.prediction.contains("only carrier"));
}

#[test]
fn a_completed_workflow_lane_is_refused_because_it_has_no_re_entry_point() {
    let c = SendContext {
        receiver_kind: ReceiverKind::WorkflowLane,
        receiver_state: ReceiverState::Completed,
        ..ctx()
    };
    let d = decide(&c);
    assert_eq!(d.verdict, Verdict::Refused);
    assert_eq!(d.channel, CH_NONE);
    assert!(!d.queued, "a refusal queues nothing");
    assert!(d.prediction.contains("no re-entry point"));
}

#[test]
fn a_stopped_by_user_lane_is_refused_whatever_its_kind() {
    for kind in [
        ReceiverKind::TopLevel,
        ReceiverKind::UnnamedSubagent,
        ReceiverKind::Teammate,
        ReceiverKind::WorkflowLane,
    ] {
        let c = SendContext {
            receiver_kind: kind,
            receiver_state: ReceiverState::StoppedByUser,
            ..ctx()
        };
        let d = decide(&c);
        assert_eq!(d.verdict, Verdict::Refused, "{}", kind.as_str());
        assert!(!d.queued);
        assert!(d.prediction.contains("stopped by the user"));
    }
}

#[test]
fn a_completed_agent_lane_is_refused_until_resume_is_asked_for() {
    for kind in [ReceiverKind::UnnamedSubagent, ReceiverKind::Teammate] {
        let c = SendContext {
            receiver_kind: kind,
            receiver_state: ReceiverState::Completed,
            ..ctx()
        };
        let d = decide(&c);
        assert_eq!(d.verdict, Verdict::Refused, "{}", kind.as_str());
        assert!(
            d.prediction.contains("--resume"),
            "the refusal names the flag that permits it: {}",
            d.prediction
        );
    }
}

#[test]
fn resume_delegates_the_official_respawn_and_says_where_the_completion_goes() {
    let c = SendContext {
        receiver_state: ReceiverState::Completed,
        resume: true,
        ..ctx()
    };
    let d = decide(&c);
    assert_eq!(d.channel, CH_RESUME);
    assert!(d.prediction.contains("respawns"));
    assert!(
        d.prediction.contains("2.1.260"),
        "the version-dependent notification routing is stated: {}",
        d.prediction
    );
    assert_eq!(d.official.expect("delegated").to.as_deref(), Some(AGENT));
}

#[test]
fn a_completed_lane_below_the_official_floor_is_refused_rather_than_queued_forever() {
    let c = SendContext {
        receiver_state: ReceiverState::Completed,
        receiver_version: Some("2.1.100".to_string()),
        resume: true,
        ..ctx()
    };
    let d = decide(&c);
    assert_eq!(d.verdict, Verdict::Refused);
    assert!(d.prediction.contains("below the official floor"));
}

#[test]
fn a_top_level_receiver_with_a_socket_delegates_the_cross_session_send() {
    let c = SendContext {
        receiver_kind: ReceiverKind::TopLevel,
        socket_present: true,
        harbor: GateVerdict::harbor(true),
        lane: SESSION.to_string(),
        ..ctx()
    };
    let d = decide(&c);
    assert_eq!(d.channel, CH_UDS);
    let official = d.official.expect("the uds row delegates");
    assert!(
        official.to.is_none(),
        "the official `to` grammar has no bare-uuid arm, so csift names none"
    );
    assert!(official.call.contains("no bare-uuid arm"));
}

#[test]
fn a_socketless_top_level_receiver_with_no_rewake_hook_names_that_risk() {
    let c = SendContext {
        receiver_kind: ReceiverKind::TopLevel,
        lane: SESSION.to_string(),
        ..ctx()
    };
    let d = decide(&c);
    assert_eq!(d.channel, CH_STEER);
    assert!(d
        .risks
        .iter()
        .any(|r| r.contains("no messaging socket") && r.contains("asyncRewake")));

    let armed = SendContext {
        async_rewake_on_stop: true,
        ..c
    };
    let d = decide(&armed);
    assert!(
        !d.risks.iter().any(|r| r.contains("asyncRewake")),
        "an armed rewake hook removes the risk: {:?}",
        d.risks
    );
    assert_eq!(d.verdict, Verdict::Ok);
}

#[test]
fn a_headless_receiver_is_never_promised_a_delivery() {
    let c = SendContext {
        receiver_kind: ReceiverKind::TopLevel,
        headless: true,
        lane: SESSION.to_string(),
        ..ctx()
    };
    let d = decide(&c);
    assert_eq!(d.verdict, Verdict::Unpredictable);
    assert!(d.official.is_none());
    assert!(d.risks.iter().any(|r| r.contains("sdk-cli")));
}

#[test]
fn the_parent_subagent_row_carries_on_the_csift_channel_and_delegates_nothing() {
    let c = SendContext {
        caller_is_subagent: true,
        relation: Relation::Child,
        ..ctx()
    };
    let d = decide(&c);
    assert_eq!(d.channel, CH_STEER);
    assert!(
        d.official.is_none(),
        "addressing `main` would reach the top-level conversation instead"
    );
    assert!(
        d.prediction.contains("no parent arm"),
        "the row says why nothing is delegated: {}",
        d.prediction
    );
}

/// The two cases below are the only ones in this file that touch disk. The fact they turn on -
/// which lane spawned which - lives nowhere but the reconstructed topology, so a hand-set
/// `relation` field would only test the assertion; the tree makes the resolver answer.
#[test]
fn a_lane_whose_meta_names_the_target_as_its_parent_reaches_the_parent_row() {
    let f = Fixture::new();
    let session_path = spawn_tree(&f);
    let child = tree_caller(CHILD_LANE);
    let parent_lane = tree_receiver(&session_path, PARENT_LANE);
    assert_eq!(
        relation_of(&child, &parent_lane),
        Relation::Child,
        "the meta's parentAgentId makes the target this lane's parent"
    );
    assert_eq!(
        relation_of(&child, &tree_receiver(&session_path, OTHER_LANE)),
        Relation::Sibling,
        "a lane that did not spawn the sender stays a sibling"
    );

    let d = decide(&SendContext {
        caller_is_subagent: true,
        relation: relation_of(&child, &parent_lane),
        lane: PARENT_LANE.to_string(),
        ..ctx()
    });
    assert_eq!(d.channel, CH_STEER);
    assert!(
        d.official.is_none(),
        "the official `to` grammar has no parent arm, so nothing is delegated"
    );
    assert!(
        d.prediction.contains("no parent arm") && d.prediction.contains("not the spawning agent"),
        "{}",
        d.prediction
    );
}

/// The mirror direction. It changes no policy row - the in-process queue reaches a child by
/// its own id either way - but the envelope's peer caution keys on the relation, and telling a
/// lane that its own parent has no authority over its task is the one place that sentence must
/// never appear.
#[test]
fn a_lane_addressing_a_target_it_spawned_is_the_parent_not_a_peer() {
    let f = Fixture::new();
    let session_path = spawn_tree(&f);
    let parent = tree_caller(PARENT_LANE);
    assert_eq!(
        relation_of(&parent, &tree_receiver(&session_path, CHILD_LANE)),
        Relation::Parent,
        "the target's meta names this lane as the agent that spawned it"
    );
    assert!(
        !Relation::Parent.needs_peer_caution(),
        "a parent is never introduced to its own child as a peer"
    );
    assert_eq!(
        relation_of(&parent, &tree_receiver(&session_path, OTHER_LANE)),
        Relation::Sibling,
        "a lane this one did not spawn stays a sibling, and does get the caution"
    );
    assert!(Relation::Sibling.needs_peer_caution());
}

#[test]
fn an_external_caller_is_never_told_to_call_a_tool() {
    for kind in [
        ReceiverKind::UnnamedSubagent,
        ReceiverKind::Teammate,
        ReceiverKind::TopLevel,
    ] {
        let c = SendContext {
            caller_kind: SenderKind::External,
            relation: Relation::External,
            receiver_kind: kind,
            socket_present: true,
            teams: GateVerdict::teams(Some("user"), 1, 1),
            ..ctx()
        };
        let d = decide(&c);
        assert!(d.official.is_none(), "{}", kind.as_str());
        assert!(d.channel.starts_with("csift"), "{}", d.channel);
    }
}

#[test]
fn a_receiver_below_the_official_floor_is_csift_only() {
    let c = SendContext {
        receiver_version: Some("2.1.197".to_string()),
        ..ctx()
    };
    let d = decide(&c);
    assert_eq!(d.channel, CH_STEER);
    assert!(d.official.is_none());
    assert!(d.prediction.contains("below the official floor"));
}

#[test]
fn an_unreadable_version_is_treated_as_below_the_floor_rather_than_assumed_past_it() {
    let c = SendContext {
        receiver_version: None,
        ..ctx()
    };
    assert!(!c.official_possible());
    assert!(decide(&c).official.is_none());
}

#[test]
fn more_chunks_than_slots_is_full_and_says_how_many_slots_are_needed() {
    let c = SendContext {
        chunks: 5,
        best_slots: 2,
        ..ctx()
    };
    let d = decide(&c);
    assert_eq!(d.verdict, Verdict::Full);
    assert!(d.queued, "FULL is queued, not refused");
    let note = full_note(&c);
    assert!(
        note.contains("5 chunk(s)") && note.contains("3 more slot(s)"),
        "{note}"
    );
}

#[test]
fn no_configured_slot_anywhere_is_unpredictable_and_names_the_missing_hook() {
    let c = SendContext {
        best_slots: 0,
        best_event: None,
        armed: 0,
        ..ctx()
    };
    let d = decide(&c);
    assert_eq!(d.verdict, Verdict::Unpredictable);
    assert!(d.risks.iter().any(|r| r.contains("csift deliver --slot k")));
}

#[test]
fn configured_but_never_armed_is_may_fail() {
    let c = SendContext { armed: 0, ..ctx() };
    let d = decide(&c);
    assert_eq!(d.verdict, Verdict::MayFail);
    assert!(d
        .risks
        .iter()
        .any(|r| r.contains("configuration is not arming")));
}

#[test]
fn a_policy_switch_over_the_receivers_hooks_is_a_named_risk() {
    let c = SendContext {
        hooks_policy_switch: Some("policy disableAllHooks: no hooks run".to_string()),
        ..ctx()
    };
    let d = decide(&c);
    assert_eq!(d.verdict, Verdict::MayFail);
    assert!(d.risks.iter().any(|r| r.contains("policy switch")));
}

#[test]
fn a_frozen_receiver_is_queued_with_the_freeze_named() {
    let c = SendContext {
        receiver_state: ReceiverState::Frozen,
        ..ctx()
    };
    let d = decide(&c);
    assert!(d.queued);
    assert_eq!(d.verdict, Verdict::MayFail);
    assert!(d.risks.iter().any(|r| r.contains("frozen")));
}

#[test]
fn a_dead_or_unknown_receiver_is_unpredictable() {
    for state in [ReceiverState::Dead, ReceiverState::Unknown] {
        let c = SendContext {
            receiver_kind: ReceiverKind::TopLevel,
            receiver_state: state,
            lane: SESSION.to_string(),
            ..ctx()
        };
        assert_eq!(decide(&c).verdict, Verdict::Unpredictable, "{state:?}");
    }
}

#[test]
fn official_only_queues_nothing_and_makes_no_csift_prediction() {
    let c = SendContext {
        official_only: true,
        ..ctx()
    };
    let d = decide(&c);
    assert!(d.official.is_some());
    assert!(!d.queued, "--official-only writes nothing");
    assert_eq!(d.verdict, Verdict::Ok);
    assert!(
        !d.prediction.contains("csift queued it too"),
        "nothing was queued, so nothing is predicted: {}",
        d.prediction
    );
}

#[test]
fn queue_mode_predicts_a_turn_boundary_and_steer_the_next_hook_point() {
    let steer = decide(&ctx());
    assert!(steer.prediction.contains("next `csift deliver` hook"));
    let queued = decide(&SendContext {
        mode: Mode::Queue,
        ..ctx()
    });
    assert!(queued.prediction.contains("turn boundary"));
    assert_eq!(
        queued.channel, CH_IN_PROCESS,
        "the mode never changes the official arm"
    );
}

#[test]
fn csift_channel_name_follows_the_mode_when_no_official_arm_exists() {
    let c = SendContext {
        receiver_kind: ReceiverKind::WorkflowLane,
        mode: Mode::Queue,
        ..ctx()
    };
    assert_eq!(decide(&c).channel, CH_QUEUE);
}

#[test]
fn the_teams_gate_verdict_uses_the_settings_grammar() {
    let enabled = GateVerdict::teams(Some("project"), 3, 2);
    assert!(enabled.enabled);
    assert_eq!(enabled.verdict, "enabled via settings env (project)");

    let unknown = GateVerdict::teams(None, 3, 2);
    assert!(!unknown.enabled);
    assert!(unknown.verdict.starts_with("no settings-level enable;"));
    assert!(unknown
        .verdict
        .contains("teams directories 3, teammate lanes 2"));
}

#[test]
fn the_harbor_gate_verdict_is_the_socket_field_itself() {
    assert_eq!(
        GateVerdict::harbor(true).verdict,
        "registry messagingSocketPath present -> on and bound"
    );
    assert_eq!(
        GateVerdict::harbor(false).verdict,
        "registry messagingSocketPath absent -> unknown"
    );
}

#[test]
fn version_parsing_accepts_only_a_triple() {
    assert_eq!(parse_version("2.1.258"), Some((2, 1, 258)));
    assert_eq!(parse_version(" 2.1.258 "), Some((2, 1, 258)));
    for bad in ["2.1", "2.1.258.1", "2.1.x", "", "v2.1.258"] {
        assert_eq!(parse_version(bad), None, "{bad}");
    }
}
