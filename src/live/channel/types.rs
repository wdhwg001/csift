//! Channel value types: the message, its two endpoints, and the closed enums every
//! channel file is written from.
//!
//! House rule (AGENTS.md section 4): JSON is hand-built with `serde_json::json!` in a
//! per-type projector - there is no `derive(Serialize)` anywhere in `src/`. The READ side
//! is deliberately tolerant: a value the current schema cannot interpret yields `None`
//! rather than an error, so a channel file written by a newer csift is skipped and
//! counted by the caller instead of crashing a delivery.

use serde_json::{json, Value};

/// Delivery mode. `queue` is a SUBSET of `steer`: a steer message may ride any eligible
/// hook point, a queue message only a turn-boundary one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Steer,
    Queue,
}

impl Mode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Mode::Steer => "steer",
            Mode::Queue => "queue",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "steer" => Some(Mode::Steer),
            "queue" => Some(Mode::Queue),
            _ => None,
        }
    }
}

/// How the sender lane stands to the receiver lane. Four of the seven values make the
/// sender a PEER, which the envelope must say out loud so the receiver does not read a
/// peer's message as an instruction from its parent or its user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Relation {
    /// The sender spawned the receiver: the top-level lane to one of its own subagents, or a
    /// subagent to one it spawned itself. Never a peer, so the caution never fires here.
    Parent,
    /// The sender was spawned by the receiver - by the session's main conversation, or by
    /// another subagent. The two share a value because they share a standing; what differs
    /// is which channel can carry the message, which the receiver KIND decides.
    Child,
    Sibling,
    CrossSession,
    CrossProject,
    External,
    Unknown,
}

impl Relation {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Relation::Parent => "parent",
            Relation::Child => "child",
            Relation::Sibling => "sibling",
            Relation::CrossSession => "cross-session",
            Relation::CrossProject => "cross-project",
            Relation::External => "external",
            Relation::Unknown => "unknown",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "parent" => Relation::Parent,
            "child" => Relation::Child,
            "sibling" => Relation::Sibling,
            "cross-session" => Relation::CrossSession,
            "cross-project" => Relation::CrossProject,
            "external" => Relation::External,
            "unknown" => Relation::Unknown,
            _ => return None,
        })
    }

    /// True when the receiver must be told the sender has no authority over it.
    pub(crate) fn needs_peer_caution(self) -> bool {
        matches!(
            self,
            Relation::Sibling
                | Relation::CrossSession
                | Relation::CrossProject
                | Relation::External
        )
    }
}

/// Whether the sender was a Claude Code lane or a process outside Claude Code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SenderKind {
    Lane,
    External,
}

impl SenderKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            SenderKind::Lane => "lane",
            SenderKind::External => "external",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "lane" => Some(SenderKind::Lane),
            "external" => Some(SenderKind::External),
            _ => None,
        }
    }
}

/// The sender half of a message.
#[derive(Debug, Clone)]
pub(crate) struct MessageFrom {
    pub(crate) kind: SenderKind,
    pub(crate) session: Option<String>,
    pub(crate) lane: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) cwd: Option<String>,
}

impl MessageFrom {
    /// The `from=` token of the envelope header: the sender's lane id, or
    /// `external:<label>` when the sender is not a Claude Code lane. The header is a
    /// single space-separated line inside one `[...]`, so a label's whitespace and any
    /// `]` would break the parse back out of a rendered chunk - both are folded here,
    /// never at parse time.
    pub(crate) fn envelope_token(&self) -> String {
        match self.kind {
            SenderKind::Lane => self
                .lane
                .clone()
                .or_else(|| self.session.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            SenderKind::External => {
                let label = self.label.as_deref().unwrap_or("unknown");
                format!("external:{}", sanitize_header_value(label))
            }
        }
    }

    /// The first 8 characters of the sender's session uuid (the header's
    /// `from-session=`), or `unknown` when the sender named no session.
    pub(crate) fn session_prefix(&self) -> String {
        self.session
            .as_deref()
            .and_then(|s| s.get(..8))
            .unwrap_or("unknown")
            .to_string()
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "kind": self.kind.as_str(),
            "session": self.session,
            "lane": self.lane,
            "label": self.label,
            "cwd": self.cwd,
        })
    }

    pub(crate) fn from_json(v: &Value) -> Option<Self> {
        Some(MessageFrom {
            kind: SenderKind::parse(str_field(v, "kind")?.as_str())?,
            session: str_field(v, "session"),
            lane: str_field(v, "lane"),
            label: str_field(v, "label"),
            cwd: str_field(v, "cwd"),
        })
    }
}

/// Which id form the sender addressed the receiver by. The transcript form is unique
/// (random hex); the routing form `Name@Team` is not (two same-named teammates in one
/// team share it), so it is kept beside the transcript form, never instead of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetForm {
    Transcript,
    Routing,
}

impl TargetForm {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            TargetForm::Transcript => "transcript",
            TargetForm::Routing => "routing",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "transcript" => Some(TargetForm::Transcript),
            "routing" => Some(TargetForm::Routing),
            _ => None,
        }
    }
}

/// The receiver half of a message. `lane` is always the transcript form (what the
/// receiver sees as its own `agent_id` in a hook payload, and what names its file).
#[derive(Debug, Clone)]
pub(crate) struct MessageTo {
    pub(crate) session: String,
    pub(crate) lane: String,
    pub(crate) form: TargetForm,
    pub(crate) routing_id: Option<String>,
}

impl MessageTo {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "session": self.session,
            "lane": self.lane,
            "form": self.form.as_str(),
            "routing_id": self.routing_id,
        })
    }

    pub(crate) fn from_json(v: &Value) -> Option<Self> {
        Some(MessageTo {
            session: str_field(v, "session")?,
            lane: str_field(v, "lane")?,
            form: TargetForm::parse(str_field(v, "form")?.as_str())?,
            routing_id: str_field(v, "routing_id"),
        })
    }
}

/// A message source, written once at send to `messages/<id>.json` and never rewritten.
/// Delivery state lives in the per-lane ledger, so this file stays a pure record of
/// what was sent.
#[derive(Debug, Clone)]
pub(crate) struct Message {
    pub(crate) id: String,
    pub(crate) ts_utc: String,
    pub(crate) from: MessageFrom,
    pub(crate) to: MessageTo,
    pub(crate) mode: Mode,
    pub(crate) ttl_secs: u64,
    pub(crate) relation: Relation,
    pub(crate) cross_project: bool,
    pub(crate) body: String,
}

impl Message {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "ts_utc": self.ts_utc,
            "from": self.from.to_json(),
            "to": self.to.to_json(),
            "mode": self.mode.as_str(),
            "ttl_secs": self.ttl_secs,
            "relation": self.relation.as_str(),
            "cross_project": self.cross_project,
            "body": self.body,
        })
    }

    pub(crate) fn from_json(v: &Value) -> Option<Self> {
        Some(Message {
            id: str_field(v, "id")?,
            ts_utc: str_field(v, "ts_utc")?,
            from: MessageFrom::from_json(v.get("from")?)?,
            to: MessageTo::from_json(v.get("to")?)?,
            mode: Mode::parse(str_field(v, "mode")?.as_str())?,
            ttl_secs: v.get("ttl_secs").and_then(Value::as_u64).unwrap_or(0),
            relation: Relation::parse(str_field(v, "relation")?.as_str())?,
            cross_project: v
                .get("cross_project")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            body: str_field(v, "body").unwrap_or_default(),
        })
    }
}

/// How a chunk reached the model at a hook point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Vehicle {
    /// The hook's stdout JSON (`hookSpecificOutput.additionalContext`), exit 0.
    AdditionalContext,
    /// Stop / SubagentStop stderr with exit 2, which also blocks the turn from ending.
    Exit2,
}

impl Vehicle {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Vehicle::AdditionalContext => "additionalContext",
            Vehicle::Exit2 => "exit2",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "additionalContext" => Some(Vehicle::AdditionalContext),
            "exit2" => Some(Vehicle::Exit2),
            _ => None,
        }
    }
}

/// The single verdict a send reports. Only `Refused` means "not queued".
///
/// One-way, unlike the enums that ride a record csift reads back: a verdict is printed to
/// the caller and written to the write-only outbox, and never parsed from either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Ok,
    Full,
    MayFail,
    Unpredictable,
    Refused,
}

impl Verdict {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Verdict::Ok => "OK",
            Verdict::Full => "FULL",
            Verdict::MayFail => "MAY-FAIL",
            Verdict::Unpredictable => "UNPREDICTABLE",
            Verdict::Refused => "REFUSED",
        }
    }
}

/// Read a string field, treating an absent field and a JSON `null` alike.
pub(crate) fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(std::string::ToString::to_string)
}

/// Fold a free-text label into one header token: the header is a space-separated
/// key=value line closed by `]`, so whitespace and `]` are the two characters that
/// would make a rendered chunk unparseable.
fn sanitize_header_value(s: &str) -> String {
    let folded: String = s
        .chars()
        .map(|c| {
            if c.is_whitespace() || c == ']' || c == '[' || (c as u32) < 0x20 {
                '_'
            } else {
                c
            }
        })
        .collect();
    if folded.is_empty() {
        "unknown".to_string()
    } else {
        folded
    }
}

/// A fresh 16-lowercase-hex message id.
///
/// No RNG crate is pulled in for this: the id only has to be unique among the messages
/// of one machine, and the state mixed here (the wall clock in nanoseconds, the process
/// id, and a process-wide counter so two calls in the same nanosecond still differ) is
/// enough for that. The mixer is splitmix64, whose avalanche makes neighbouring seeds
/// produce unrelated ids, so the printed id does not leak the clock.
pub(crate) fn new_message_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0u64, |d| d.as_nanos() as u64);
    let seed =
        nanos ^ (u64::from(std::process::id()) << 33) ^ seq.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    format!("{:016x}", splitmix64(seed))
}

fn splitmix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The current instant as the channel writes it: RFC 3339 UTC at millisecond
/// precision, the same shape Claude Code stamps on a transcript record.
pub(crate) fn now_utc() -> String {
    let ts = jiff::Timestamp::now();
    ts.round(jiff::Unit::Millisecond).unwrap_or(ts).to_string()
}

/// `ts_utc` plus `ttl_secs`, or `None` when either end is unusable (an unparseable
/// stamp or a ttl past the representable range). A message with no expiry never
/// expires - the caller states that rather than inventing a deadline.
pub(crate) fn expires_at(ts_utc: &str, ttl_secs: u64) -> Option<String> {
    let ts: jiff::Timestamp = ts_utc.parse().ok()?;
    let secs = i64::try_from(ttl_secs).ok()?;
    let out = ts.checked_add(jiff::SignedDuration::from_secs(secs)).ok()?;
    Some(
        out.round(jiff::Unit::Millisecond)
            .unwrap_or(out)
            .to_string(),
    )
}

/// True only when both stamps parse AND the deadline is at or before `now`. An
/// unreadable stamp is never treated as expired: dropping a message on a parse failure
/// would be a silent loss.
pub(crate) fn is_expired(expires_utc: &str, now_utc: &str) -> bool {
    let (Ok(exp), Ok(now)) = (
        expires_utc.parse::<jiff::Timestamp>(),
        now_utc.parse::<jiff::Timestamp>(),
    ) else {
        return false;
    };
    exp <= now
}
