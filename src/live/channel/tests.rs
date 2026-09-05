//! Unit tests for the channel data layer: shared fixtures + feature modules.

use super::*;

mod envelope;
mod ledger;
mod marker;
mod paths;
mod records;
mod slots;
mod types;

use std::path::PathBuf;

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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(parent) = self.root.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
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
