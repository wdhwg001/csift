//! Unit tests for the channel data layer: shared fixtures + feature modules.

use super::*;

mod caller;
mod deliver;
mod envelope;
mod ledger;
mod marker;
mod paths;
mod policy;
mod reach;
mod reconcile;
mod records;
mod send;
mod slots;
mod types;

use std::path::{Path, PathBuf};

/// Fixture ids: the nil uuid family plus the two agent id shapes. No real session id or
/// transcript ever enters a fixture.
const SESSION: &str = "00000000-0000-4000-8000-000000000001";
const RECEIVER: &str = "00000000-0000-4000-8000-000000000002";
const AGENT: &str = "a0123456789abcdef";
const TEAMMATE: &str = "aRelay-0123456789abcdef";
const MSG_ID: &str = "0123456789abcdef";

/// A scratch channel root, removed on drop.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        // A process-wide counter beside the pid and the clock: two fixtures built in
        // the same nanosecond on parallel test threads must not share a root, or one
        // Drop wipes the other's tree mid-run.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let root = std::env::temp_dir().join(format!(
            "csift-channel-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Fixture {
            root: channel_dir(&root),
        }
    }

    /// The scratch directory the channel root sits inside. A test that needs a whole session
    /// tree beside the channel - a transcript plus its subagent lanes - writes it here, so one
    /// Drop removes both.
    fn scratch(&self) -> &Path {
        self.root.parent().unwrap_or(&self.root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(parent) = self.root.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}

/// A minimal lane transcript: an opener and one assistant record that has not ended the turn.
/// It carries no `sessionId` - the topology reads a lane's owning session from the directory
/// it sits in, so the field would be decoration.
const LANE_TRANSCRIPT: &str = concat!(
    r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T05:00:00.000Z","version":"2.1.258","message":{"role":"user","content":"go"}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"r2","timestamp":"2026-06-07T05:00:05.000Z","version":"2.1.258","message":{"role":"assistant","stop_reason":null,"content":[{"type":"text","text":"working"}]}}"#,
    "\n",
);

/// Three lanes of one session: `CHILD_LANE`'s meta names `PARENT_LANE` as the agent that
/// spawned it, and `OTHER_LANE` is related to neither.
const PARENT_LANE: &str = "a0123456789abcde1";
const CHILD_LANE: &str = "a0123456789abcde2";
const OTHER_LANE: &str = "a0123456789abcde3";

/// Write that session tree inside the fixture's scratch directory, so one Drop removes it, and
/// return the main transcript's path (what a receiver carries as its `session_path`).
///
/// It is a real tree because the fact it exists for - which lane spawned which - lives nowhere
/// but the reconstructed topology, so a hand-set relation field would test only the assertion.
fn spawn_tree(f: &Fixture) -> PathBuf {
    let session_path = f.scratch().join(format!("{SESSION}.jsonl"));
    let subagents = f.scratch().join(SESSION).join("subagents");
    std::fs::create_dir_all(&subagents).unwrap();
    std::fs::write(&session_path, LANE_TRANSCRIPT).unwrap();
    for (id, parent) in [
        (PARENT_LANE, None),
        (CHILD_LANE, Some(PARENT_LANE)),
        (OTHER_LANE, None),
    ] {
        std::fs::write(subagents.join(format!("agent-{id}.jsonl")), LANE_TRANSCRIPT).unwrap();
        let link = parent.map_or_else(String::new, |p| format!(r#","parentAgentId":"{p}""#));
        std::fs::write(
            subagents.join(format!("agent-{id}.meta.json")),
            format!(r#"{{"agentType":"general-purpose"{link}}}"#),
        )
        .unwrap();
    }
    session_path
}

/// A lane caller of [`SESSION`] with an exact `--from`.
fn tree_caller(lane: &str) -> crate::live::channel::caller::Caller {
    crate::live::channel::caller::Caller {
        kind: SenderKind::Lane,
        session: Some(SESSION.to_string()),
        lane: Some(lane.to_string()),
        label: None,
        lane_exact: true,
    }
}

/// One running lane of the [`spawn_tree`] session as a send receiver.
fn tree_receiver(session_path: &Path, lane: &str) -> crate::live::channel::caller::Receiver {
    crate::live::channel::caller::Receiver {
        lane: lane.to_string(),
        session: SESSION.to_string(),
        session_path: session_path.to_path_buf(),
        kind: crate::live::channel::policy::ReceiverKind::UnnamedSubagent,
        state: crate::live::channel::policy::ReceiverState::Running,
        version: Some("2.1.258".to_string()),
        cwd: None,
        routing_id: None,
        socket_present: false,
        headless: false,
        teammate_lanes: 0,
    }
}

/// A lane-to-lane message with a one-line body.
fn message(body: &str) -> Message {
    Message {
        id: MSG_ID.to_string(),
        ts_utc: "2026-06-07T05:00:05Z".to_string(),
        from: MessageFrom {
            kind: SenderKind::Lane,
            session: Some(SESSION.to_string()),
            lane: Some(TEAMMATE.to_string()),
            label: None,
            cwd: Some("/Users/dev/relay".to_string()),
        },
        to: MessageTo {
            session: RECEIVER.to_string(),
            lane: RECEIVER.to_string(),
            form: TargetForm::Transcript,
            routing_id: None,
        },
        mode: Mode::Steer,
        ttl_secs: 43200,
        relation: Relation::Child,
        cross_project: false,
        body: body.to_string(),
    }
}

/// An emit line, the ledger's most-structured shape.
fn emit(part: u32, parts: u32, vehicle: Vehicle) -> LedgerLine {
    LedgerLine::Emit {
        id: MSG_ID.to_string(),
        event: "Stop".to_string(),
        slot: part,
        part,
        parts,
        vehicle,
        ts_utc: "2026-06-07T05:00:05Z".to_string(),
        hook_session: Some(SESSION.to_string()),
        hook_agent_id: Some(AGENT.to_string()),
        block_count: (vehicle == Vehicle::Exit2).then_some(1),
    }
}
