//! `ledger/<lane>.jsonl` - what csift DID about each message addressed to one lane.
//!
//! The ledger is INTENT, never fact. An `emit` line says csift printed a chunk at a hook
//! point; whether the model received it is answered only by the receiver's own
//! transcript, where the delivery lands as a `hook_additional_context` attachment
//! carrying the envelope id. Keeping the two apart is what lets a reconciliation report
//! `INTENT-ONLY` (csift emitted, no transcript record) as a distinct outcome from
//! `DELIVERED`, instead of quietly promoting an intention into a claim.
//!
//! Reading it is a fold, not a scan: [`states`] groups by message id, so a repeated
//! line (a redelivery, a re-run of the same slot) never double-counts a part.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use serde_json::{json, Value};

use super::{ledger_path, read_jsonl, str_field, Vehicle};

/// Why a message was redelivered. Both sources are re-entry points, not new sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RedeliverSource {
    /// `SessionStart` with `source:"compact"` - the context the delivery landed in is
    /// gone, so an unacked message is offered once more.
    Compact,
    /// `SessionStart` with `source:"resume"` - a held message finally has a lane.
    Resume,
}

impl RedeliverSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            RedeliverSource::Compact => "compact",
            RedeliverSource::Resume => "resume",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "compact" => Some(RedeliverSource::Compact),
            "resume" => Some(RedeliverSource::Resume),
            _ => None,
        }
    }
}

/// One ledger line. `Refused` carries an OPTIONAL id: a refusal can predate knowing
/// which message was involved (a hook input csift declined to act on at all).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LedgerLine {
    Emit {
        id: String,
        event: String,
        slot: u32,
        part: u32,
        parts: u32,
        vehicle: Vehicle,
        ts_utc: String,
        hook_session: Option<String>,
        hook_agent_id: Option<String>,
        /// The consecutive-block count this emit carried, recorded only on the exit2
        /// vehicle, where the harness overrides after a fixed number of blocks.
        block_count: Option<u32>,
    },
    Held {
        id: String,
        reason: String,
        ts_utc: String,
    },
    Expired {
        id: String,
        ts_utc: String,
    },
    Ack {
        id: String,
        ts_utc: String,
    },
    Redelivered {
        id: String,
        source: RedeliverSource,
        ts_utc: String,
    },
    Refused {
        id: Option<String>,
        reason: String,
        ts_utc: String,
    },
}

impl LedgerLine {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            LedgerLine::Emit { .. } => "emit",
            LedgerLine::Held { .. } => "held",
            LedgerLine::Expired { .. } => "expired",
            LedgerLine::Ack { .. } => "ack",
            LedgerLine::Redelivered { .. } => "redelivered",
            LedgerLine::Refused { .. } => "refused",
        }
    }

    pub(crate) fn id(&self) -> Option<&str> {
        match self {
            LedgerLine::Emit { id, .. }
            | LedgerLine::Held { id, .. }
            | LedgerLine::Expired { id, .. }
            | LedgerLine::Ack { id, .. }
            | LedgerLine::Redelivered { id, .. } => Some(id),
            LedgerLine::Refused { id, .. } => id.as_deref(),
        }
    }

    pub(crate) fn ts_utc(&self) -> &str {
        match self {
            LedgerLine::Emit { ts_utc, .. }
            | LedgerLine::Held { ts_utc, .. }
            | LedgerLine::Expired { ts_utc, .. }
            | LedgerLine::Ack { ts_utc, .. }
            | LedgerLine::Redelivered { ts_utc, .. }
            | LedgerLine::Refused { ts_utc, .. } => ts_utc,
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        match self {
            LedgerLine::Emit {
                id,
                event,
                slot,
                part,
                parts,
                vehicle,
                ts_utc,
                hook_session,
                hook_agent_id,
                block_count,
            } => json!({
                "id": id,
                "kind": "emit",
                "event": event,
                "slot": slot,
                "part": part,
                "parts": parts,
                "vehicle": vehicle.as_str(),
                "ts_utc": ts_utc,
                "hook_session": hook_session,
                "hook_agent_id": hook_agent_id,
                "block_count": block_count,
            }),
            LedgerLine::Held { id, reason, ts_utc } => json!({
                "id": id, "kind": "held", "reason": reason, "ts_utc": ts_utc,
            }),
            LedgerLine::Expired { id, ts_utc } => json!({
                "id": id, "kind": "expired", "ts_utc": ts_utc,
            }),
            LedgerLine::Ack { id, ts_utc } => json!({
                "id": id, "kind": "ack", "ts_utc": ts_utc,
            }),
            LedgerLine::Redelivered { id, source, ts_utc } => json!({
                "id": id, "kind": "redelivered", "source": source.as_str(), "ts_utc": ts_utc,
            }),
            LedgerLine::Refused { id, reason, ts_utc } => json!({
                "id": id, "kind": "refused", "reason": reason, "ts_utc": ts_utc,
            }),
        }
    }

    pub(crate) fn from_json(v: &Value) -> Option<Self> {
        let ts_utc = str_field(v, "ts_utc")?;
        Some(match str_field(v, "kind")?.as_str() {
            "emit" => LedgerLine::Emit {
                id: str_field(v, "id")?,
                event: str_field(v, "event")?,
                slot: u32_field(v, "slot")?,
                part: u32_field(v, "part")?,
                parts: u32_field(v, "parts")?,
                vehicle: Vehicle::parse(str_field(v, "vehicle")?.as_str())?,
                ts_utc,
                hook_session: str_field(v, "hook_session"),
                hook_agent_id: str_field(v, "hook_agent_id"),
                block_count: u32_field(v, "block_count"),
            },
            "held" => LedgerLine::Held {
                id: str_field(v, "id")?,
                reason: str_field(v, "reason").unwrap_or_default(),
                ts_utc,
            },
            "expired" => LedgerLine::Expired {
                id: str_field(v, "id")?,
                ts_utc,
            },
            "ack" => LedgerLine::Ack {
                id: str_field(v, "id")?,
                ts_utc,
            },
            "redelivered" => LedgerLine::Redelivered {
                id: str_field(v, "id")?,
                source: RedeliverSource::parse(str_field(v, "source")?.as_str())?,
                ts_utc,
            },
            "refused" => LedgerLine::Refused {
                id: str_field(v, "id"),
                reason: str_field(v, "reason").unwrap_or_default(),
                ts_utc,
            },
            _ => return None,
        })
    }
}

fn u32_field(v: &Value, key: &str) -> Option<u32> {
    v.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

/// Where one message stands, read off the ledger alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Enqueued and untouched: no emit, no hold, no terminal line.
    Pending,
    /// Held for a stated reason with no later emit.
    Held,
    /// Some parts emitted, the part count not yet complete.
    Emitting,
    /// Every part emitted. Still INTENT: the receiver's transcript is the fact.
    Emitted,
    Expired,
    Acked,
}

impl Phase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Phase::Pending => "pending",
            Phase::Held => "held",
            Phase::Emitting => "emitting",
            Phase::Emitted => "emitted",
            Phase::Expired => "expired",
            Phase::Acked => "acked",
        }
    }
}

/// The folded state of one message id.
#[derive(Debug, Clone, Default)]
pub(crate) struct MessageState {
    pub(crate) id: String,
    /// Part numbers emitted at least once. A set, so a redelivered part counts once.
    pub(crate) emitted_parts: BTreeSet<u32>,
    /// The part total the emits agreed on, or `None` before the first emit.
    pub(crate) parts_expected: Option<u32>,
    /// How many emits used the exit2 vehicle over the whole life of the message.
    pub(crate) exit2_emits: usize,
    pub(crate) held_reasons: Vec<String>,
    pub(crate) redelivered: Vec<RedeliverSource>,
    pub(crate) refused_reasons: Vec<String>,
    pub(crate) expired: bool,
    pub(crate) acked: bool,
    pub(crate) first_ts_utc: Option<String>,
    pub(crate) last_ts_utc: Option<String>,
}

impl MessageState {
    /// True once part 1 has been emitted: the test a delivery uses to decide whether a
    /// message is still pending, since a message whose first chunk went out is no
    /// longer waiting for its first hook point.
    pub(crate) fn first_part_emitted(&self) -> bool {
        self.emitted_parts.contains(&1)
    }

    pub(crate) fn has_exit2(&self) -> bool {
        self.exit2_emits > 0
    }

    /// The terminal states outrank the transient ones: an acked message is acked even
    /// if it was held first, and an emit after a hold means the hold is over.
    pub(crate) fn phase(&self) -> Phase {
        if self.acked {
            return Phase::Acked;
        }
        if self.expired {
            return Phase::Expired;
        }
        if !self.emitted_parts.is_empty() {
            let complete = self
                .parts_expected
                .is_some_and(|n| (1..=n).all(|k| self.emitted_parts.contains(&k)));
            return if complete {
                Phase::Emitted
            } else {
                Phase::Emitting
            };
        }
        if !self.held_reasons.is_empty() {
            return Phase::Held;
        }
        Phase::Pending
    }

    fn absorb(&mut self, line: &LedgerLine) {
        let ts = line.ts_utc().to_string();
        if self.first_ts_utc.is_none() {
            self.first_ts_utc = Some(ts.clone());
        }
        self.last_ts_utc = Some(ts);
        match line {
            LedgerLine::Emit {
                part,
                parts,
                vehicle,
                ..
            } => {
                self.emitted_parts.insert(*part);
                self.parts_expected = Some(*parts);
                if *vehicle == Vehicle::Exit2 {
                    self.exit2_emits += 1;
                }
            }
            LedgerLine::Held { reason, .. } => self.held_reasons.push(reason.clone()),
            LedgerLine::Expired { .. } => self.expired = true,
            LedgerLine::Ack { .. } => self.acked = true,
            LedgerLine::Redelivered { source, .. } => self.redelivered.push(*source),
            LedgerLine::Refused { reason, .. } => self.refused_reasons.push(reason.clone()),
        }
    }
}

/// Fold a lane's ledger into one state per message id, in id order. A line with no id
/// (a bare refusal) belongs to no message and is not folded here; the caller reads it
/// from the line list.
pub(crate) fn states(lines: &[LedgerLine]) -> BTreeMap<String, MessageState> {
    let mut out: BTreeMap<String, MessageState> = BTreeMap::new();
    for line in lines {
        let Some(id) = line.id() else { continue };
        let entry = out.entry(id.to_string()).or_insert_with(|| MessageState {
            id: id.to_string(),
            ..MessageState::default()
        });
        entry.absorb(line);
    }
    out
}

/// How many exit2 emits this lane has made in a row, counting back from the newest line
/// to the last emit that used another vehicle or the last ack.
///
/// The harness ends the turn itself once a Stop hook has blocked it a fixed number of
/// times in a row, so a queue delivery that leans on exit 2 has to know how close the
/// lane already is to that override. Only emits and acks move the count: a hold or an
/// expiry says nothing about blocking.
pub(crate) fn consecutive_exit2_blocks(lines: &[LedgerLine]) -> usize {
    let mut count = 0usize;
    for line in lines.iter().rev() {
        match line {
            LedgerLine::Emit { vehicle, .. } => {
                if *vehicle == Vehicle::Exit2 {
                    count += 1;
                } else {
                    break;
                }
            }
            LedgerLine::Ack { .. } => break,
            _ => {}
        }
    }
    count
}

/// Append one line to a lane's ledger.
pub(crate) fn append_ledger(root: &Path, lane: &str, line: &LedgerLine) -> Result<()> {
    let path = ledger_path(root, lane)?;
    super::append_line(&path, &serde_json::to_string(&line.to_json())?)
}

/// Read a lane's ledger in append order, with the count of unreadable lines.
pub(crate) fn read_ledger(root: &Path, lane: &str) -> Result<(Vec<LedgerLine>, usize)> {
    let path = ledger_path(root, lane)?;
    let (values, mut skipped) = read_jsonl(&path)?;
    let mut out = Vec::with_capacity(values.len());
    for v in &values {
        match LedgerLine::from_json(v) {
            Some(line) => out.push(line),
            None => skipped += 1,
        }
    }
    Ok((out, skipped))
}
