//! The csift channel: a message channel between Claude Code lanes, and from outside
//! Claude Code, built entirely out of files csift owns.
//!
//! WHY it exists: the official channel is structurally absent for some receivers. A
//! running workflow lane cannot be reached by the send tool at all, an unnamed subagent
//! cannot reach its parent subagent, nothing reaches an idle top-level session that
//! published no socket, a sender outside Claude Code has no tool to call, and a build
//! below the version floors or with the gates off has nothing.
//!
//! WHERE it writes: one directory, `<session-sidecar-dir>/csift-channel/`, the `<uuid>/`
//! directory beside `subagents/`. Never a transcript, never the team mailbox, never the
//! messaging socket, never the session registry, never a settings file. csift also never
//! installs a hook: the delivery hook lines are pasted by the user, and every guarantee
//! (append-only outbox, per-lane ledger, dedupe by id, atomic append, chunking, slot
//! ordering, block-cap awareness) lives in this binary instead of in the hook line.
//!
//! This module is the DATA LAYER only - the formats, their readers and writers, and the
//! two coordination primitives (the armed marker and the slot chain). The commands that
//! use it (`send`, `deliver`, `msg`, `ack`) live beside it.
//!
//! Layout:
//! - [`types`] the message, its endpoints, the closed enums, ids and expiry
//! - [`paths`] the directory layout, lane-id validation, append and atomic rewrite
//! - [`envelope`] the rendered chunks and the detector that finds them again
//! - [`outbox`] the sender's `messages/<id>.json` plus `outbox.jsonl`
//! - [`inbox`] the receiver's append-only `inbox/<lane>.jsonl`
//! - [`ledger`] the per-lane `ledger/<lane>.jsonl` and its fold into per-message state
//! - [`marker`] the `armed/<lane>.json` runtime proof that hooks really run
//! - [`slots`] the temp-dir slot chain that orders concurrent hook processes
//! - [`reconcile`] the ledger-against-transcript join (intent versus fact)
//! - [`msg`] the `msg` and `ack` commands built on that join
//! - [`reach`] the `whoami` lane sections, the `--to` prediction and the peer census

// The commands that consume this layer (`deliver`, `send`, `msg`, `ack`, and the
// `whoami` reach prediction) land in the following commits of the same release. Until
// they do, the formats and the re-exports below have no caller outside the unit tests,
// which the dead-code and unused-import passes do not count. Scoped to this module, and
// removed once the commands land.
#![allow(dead_code, unused_imports)]

mod caller;
mod deliver;
mod deliver_emit;
mod deliver_plan;
mod envelope;
mod hook_input;
mod inbox;
mod ledger;
mod marker;
mod msg;
mod outbox;
mod paths;
mod policy;
mod reach;
mod recipe;
mod reconcile;
mod send;
mod slots;
mod types;

pub(crate) use deliver::*;
pub(crate) use deliver_emit::*;
pub(crate) use deliver_plan::*;
pub(crate) use envelope::*;
pub(crate) use hook_input::*;
pub(crate) use inbox::*;
pub(crate) use ledger::*;
pub(crate) use marker::*;
pub(crate) use msg::*;
pub(crate) use outbox::*;
pub(crate) use paths::*;
// Named re-exports, not a glob: the reach surface carries lane vocabulary (`LaneRef`,
// `Sections`) that only `whoami` needs, and flattening all of it into the channel namespace
// would put two meanings of "lane" in one scope.
pub(crate) use reach::{
    emit_lane_sections, external_answer, resolve_one as resolve_lane, run_peers, run_reach_to,
    LaneRef,
};
pub(crate) use recipe::*;
pub(crate) use reconcile::*;
pub(crate) use send::run_send;
pub(crate) use slots::*;
pub(crate) use types::*;

#[cfg(test)]
mod tests;
