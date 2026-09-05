//! `whoami` reach: which lane this is, who its parent is, what is live around it, and - for
//! one named target - which channel would carry a message and what the sender would be told.
//!
//! WHY it lives beside the channel rather than inside `whoami.rs`: every answer here is decided
//! by the three readers the send path already uses - the receiver probe, the settings cascade
//! and the policy table. A second copy of any of them would drift into a prediction that
//! `csift send` does not actually make, which is worse than no prediction at all. `whoami` keeps
//! the identity resolution it has always done and hands the lane it resolved to this module.
//!
//! Nothing here writes. A prediction is a read of disk plus the pure policy table, so `whoami`
//! stays safe to run from a hook, and asking "would this reach?" never queues a message.
//!
//! Two honesty rules run through the whole module. A lane's state is read from its own tail and
//! a pid probe, never from the harness's internal state, so an agent prediction says so in one
//! sentence and names its fallback. And the peer surface publishes ids, kinds and states only:
//! a description, an agent type or a name read as a role is context one lane can use to boss
//! another, and the census exists to answer "who is alive", not "who should obey whom".
//!
//! Layout:
//! - [`facts`] the lane probe and the `self` / `parent` / `topology` section builders
//! - [`predict`] the `--to` reach prediction: the send context, the gates, the slot census
//! - [`peers`] the registry read, the live-lane census and the not-a-lane answer
//! - [`render`] the text and JSON projections of all four

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::json;

use super::caller::{self, Caller, GateVerdict, Receiver, LANE_ASSUMED_NOTE};
use super::policy::{self, Decision, ReceiverKind, ReceiverState, SendContext};
use super::send::{relation_of, truthy};
use super::{channel_dir, read_armed, Mode, Relation, SenderKind, Verdict};
use crate::cli::OutputFormat;
use crate::live::{children_report, probe_pid, tail_shape, PidLiveness, TailShape};
use crate::path::settings::{self, Merged};
use crate::text;

mod facts;
mod peers;
mod predict;
mod render;

pub(crate) use facts::*;
pub(crate) use peers::*;
pub(crate) use predict::*;
pub(crate) use render::*;

/// The env key whose value enables agent teams. The same key the send path reads; it is spelled
/// again here rather than shared because the send module keeps it private, and one of the two
/// spellings changing without the other would show up immediately as a gate verdict that
/// disagrees between `whoami --to` and `csift send` on the same tree.
const TEAMS_ENV: &str = "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS";

/// The delivery events a steer message may ride, in the order a receipt prints them. `whoami`
/// predicts for the default mode (steer), so the turn-boundary subset the queue mode is
/// restricted to is not needed here.
const STEER_EVENTS: [&str; 8] = [
    "SessionStart",
    "SubagentStart",
    "PreToolUse",
    "PostToolUse",
    "PostToolBatch",
    "UserPromptSubmit",
    "Stop",
    "SubagentStop",
];

/// The sentence every AGENT prediction carries. csift reads a lane's tail and probes a pid; the
/// harness's own view of that lane lives in process memory csift cannot see, so the prediction
/// is an inference and says which carrier remains when the inference is wrong.
const INFERENCE_NOTE: &str = "prediction is an inference from the transcript tail and a pid \
                              probe, not the harness's own state";

/// The reach prediction for one target, and the facts it was decided from.
#[derive(Debug, Clone)]
pub(crate) struct Reach {
    pub(crate) lane: String,
    pub(crate) routing_id: Option<String>,
    pub(crate) session: String,
    pub(crate) kind: ReceiverKind,
    pub(crate) state: ReceiverState,
    pub(crate) version: Option<String>,
    pub(crate) caller_kind: SenderKind,
    pub(crate) caller_label: String,
    pub(crate) decision: Decision,
    pub(crate) slots: ReachSlots,
    pub(crate) armed: Vec<u32>,
    pub(crate) teams: GateVerdict,
    pub(crate) harbor: GateVerdict,
    /// The inference sentence plus its fallback, for an agent target only. A top-level target
    /// is answered from the registry row the harness itself wrote, so it carries none.
    pub(crate) inference: Option<String>,
}
