//! The channel policy table: given one caller, one receiver and the gates, which channel
//! carries this message and what the sender is promised.
//!
//! It is a PURE function over facts already gathered ([`SendContext`] carries no paths and
//! reads no disk), for two reasons. The table is the part a reader has to be able to audit
//! row by row against the traced Claude Code behaviour, and every row is exercised by a unit
//! test that constructs the context directly - a table reachable only through a live session
//! would be a table nobody checks.
//!
//! The shape of the answer never changes: ONE verdict per send. `OK` promises delivery at a
//! named hook point, `FULL` says the message needs more slots than the receiver has, `MAY-FAIL`
//! names the risk that could swallow it, `UNPREDICTABLE` says csift cannot say WHEN (a receiver
//! that is not alive, or one with no delivery hook at all), and `REFUSED` is the only value
//! that queues nothing. An official transport is a DELEGATION, never an action: csift is a
//! binary and the official transports are model tools, so the decision carries the call the
//! caller must make itself.

use super::caller::GateVerdict;
use super::{Mode, Relation, SenderKind, Verdict};

/// The Claude Code floor for any official send path. Below it csift's own channel is the
/// only carrier, whatever the receiver's state.
pub(crate) const OFFICIAL_FLOOR: (u32, u32, u32) = (2, 1, 198);

/// Channel names, as the receipt prints them and the outbox records them.
pub(crate) const CH_MAILBOX: &str = "official mailbox";
pub(crate) const CH_IN_PROCESS: &str = "official in-process queue";
pub(crate) const CH_UDS: &str = "official uds";
pub(crate) const CH_RESUME: &str = "official resume";
pub(crate) const CH_STEER: &str = "csift steer";
pub(crate) const CH_QUEUE: &str = "csift queue";
pub(crate) const CH_NONE: &str = "none";

/// The official tool a delegation names. Kept as one constant so a rename is one edit and
/// the receipt, the outbox and the JSON row can never disagree about it.
const OFFICIAL_TOOL: &str = "SendMessage";

/// What kind of lane the receiver is. Read from the on-disk shape (a top-level transcript, a
/// subagent under `subagents/`, one under `workflows/`, a teammate meta), never from a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverKind {
    TopLevel,
    UnnamedSubagent,
    Teammate,
    WorkflowLane,
}

impl ReceiverKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ReceiverKind::TopLevel => "top-level session",
            ReceiverKind::UnnamedSubagent => "unnamed subagent",
            ReceiverKind::Teammate => "teammate",
            ReceiverKind::WorkflowLane => "workflow lane",
        }
    }
}

/// What the receiver lane is doing, as far as disk can say. `Completed` and `StoppedByUser`
/// are the two states that change what csift is allowed to promise; `Unknown` is a real
/// answer (an empty or unreadable transcript), never a guess dressed as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverState {
    Running,
    /// An unreturned tool_use at the tail: the lane is blocked there, not done.
    Frozen,
    /// A clean end_turn tail with no live child state.
    Completed,
    /// The harness's own terminal word for this lane (a killed or stopped background agent).
    StoppedByUser,
    /// A registry row whose owner process is gone.
    Dead,
    Unknown,
}

impl ReceiverState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ReceiverState::Running => "running",
            ReceiverState::Frozen => "frozen",
            ReceiverState::Completed => "completed",
            ReceiverState::StoppedByUser => "stopped-by-user",
            ReceiverState::Dead => "dead",
            ReceiverState::Unknown => "unknown",
        }
    }
}

/// The official call a delegation asks the caller to make. csift never performs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OfficialCall {
    pub(crate) tool: &'static str,
    /// The `to` value, in the routing or `a...` form. `None` when the official grammar has
    /// no arm csift can fill from disk (a peer top-level session: there is no bare-uuid arm).
    pub(crate) to: Option<String>,
    /// The line the receipt prints.
    pub(crate) call: String,
}

impl OfficialCall {
    fn addressed(to: &str) -> Self {
        OfficialCall {
            tool: OFFICIAL_TOOL,
            to: Some(to.to_string()),
            call: format!("{OFFICIAL_TOOL}(to: \"{to}\", message: <this message>)"),
        }
    }

    /// The peer-session arm: the official `to` grammar resolves `main`, a name, a `[ref]` or
    /// an `a...` id and has NO bare-uuid arm, so the receiver's session uuid is not a legal
    /// value and csift will not invent one.
    fn unaddressable() -> Self {
        OfficialCall {
            tool: OFFICIAL_TOOL,
            to: None,
            call: format!(
                "{OFFICIAL_TOOL}(to: <the peer as your harness names it>, message: <this \
                 message>) - the official `to` grammar has no bare-uuid arm, so the receiver's \
                 session uuid is not a legal value and csift will not invent one"
            ),
        }
    }
}

/// The facts one send is decided from. Every field is read once by the caller and handed in;
/// nothing here touches disk.
#[derive(Debug, Clone)]
pub(crate) struct SendContext {
    pub(crate) caller_kind: SenderKind,
    /// True when the caller named a subagent lane (its own `a...` id), not the session.
    pub(crate) caller_is_subagent: bool,
    pub(crate) relation: Relation,
    pub(crate) receiver_kind: ReceiverKind,
    pub(crate) receiver_state: ReceiverState,
    /// The receiver transcript's `version` field, when it carries one.
    pub(crate) receiver_version: Option<String>,
    /// The registry row's entrypoint is `sdk-cli`: a headless `-p` receiver.
    pub(crate) headless: bool,
    pub(crate) socket_present: bool,
    /// The receiver's TRANSCRIPT-form lane id. Legal as an official `to` in its own right
    /// (the official id arm accepts `a[Name-]<16 hex>`), and the fallback whenever a teammate
    /// carries no routing form.
    pub(crate) lane: String,
    /// The teammate routing form `Name@Team`, when the receiver has one.
    pub(crate) routing_id: Option<String>,
    pub(crate) mode: Mode,
    pub(crate) resume: bool,
    pub(crate) official_only: bool,
    pub(crate) teams: GateVerdict,
    pub(crate) harbor: GateVerdict,
    /// How many envelope chunks this message renders into.
    pub(crate) chunks: usize,
    /// The largest configured slot count on any event eligible for this mode.
    pub(crate) best_slots: usize,
    /// The event that carries `best_slots`, for the prediction sentence.
    pub(crate) best_event: Option<String>,
    /// Slots recorded as having actually RUN in this lane (the armed marker).
    pub(crate) armed: usize,
    /// An `asyncRewake` hook is configured on Stop: the one way to wake an idle top-level
    /// session that published no socket.
    pub(crate) async_rewake_on_stop: bool,
    /// The policy switch that emptied or replaced the receiver's hook set, when one fired.
    pub(crate) hooks_policy_switch: Option<String>,
}

impl SendContext {
    /// True when the receiver is new enough for any official path. An UNKNOWN version is
    /// treated as below the floor: the floor is a fact to be shown, not assumed.
    pub(crate) fn official_possible(&self) -> bool {
        self.receiver_version
            .as_deref()
            .and_then(parse_version)
            .is_some_and(|v| v >= OFFICIAL_FLOOR)
    }

    fn alive(&self) -> bool {
        matches!(
            self.receiver_state,
            ReceiverState::Running | ReceiverState::Frozen
        )
    }
}

/// The decision: one channel, one verdict, one prediction, the risks by name, and the
/// official call when the send is delegated.
#[derive(Debug, Clone)]
pub(crate) struct Decision {
    pub(crate) channel: &'static str,
    pub(crate) verdict: Verdict,
    pub(crate) prediction: String,
    pub(crate) risks: Vec<String>,
    pub(crate) official: Option<OfficialCall>,
    /// False only for a REFUSED send and for `--official-only`: nothing is written then.
    pub(crate) queued: bool,
}

/// The policy table.
pub(crate) fn decide(ctx: &SendContext) -> Decision {
    let mut d = route(ctx);
    if d.verdict == Verdict::Refused {
        d.queued = false;
        return d;
    }
    if ctx.official_only && d.official.is_some() {
        d.queued = false;
    }
    // The risks a ROW carries describe the official leg csift only delegates - the mailbox's
    // consume-deletes semantics, the in-process queue's "a record exists" return. They are
    // reported, but they never lower the verdict: the verdict is csift's promise about its OWN
    // queue, and every one of those rows queues the message too.
    let delegated_risks = d.risks.len();
    collect_risks(ctx, &mut d);
    d.verdict = verdict_for(ctx, &d, delegated_risks);
    if d.queued {
        d.prediction = format!("{} {}", d.prediction, csift_leg(ctx));
    }
    d
}

/// Which channel carries it, before the risks and the verdict are computed.
fn route(ctx: &SendContext) -> Decision {
    if ctx.receiver_state == ReceiverState::StoppedByUser {
        return refused(
            "the receiver lane was stopped by the user - the harness refuses a resume for this \
             state and instructs a sender to treat the work as cancelled, so csift queues \
             nothing it could never deliver",
        );
    }
    match ctx.receiver_kind {
        ReceiverKind::WorkflowLane => workflow_row(ctx),
        ReceiverKind::TopLevel => top_level_row(ctx),
        ReceiverKind::Teammate | ReceiverKind::UnnamedSubagent => agent_row(ctx),
    }
}

/// A workflow lane: the official transports fail closed for it (its transcript lives under
/// `workflows/<run>/`, a path the harness's own resume resolver never reaches), so csift's
/// channel is the ONLY carrier while it runs and there is no carrier at all once it ends.
fn workflow_row(ctx: &SendContext) -> Decision {
    if matches!(
        ctx.receiver_state,
        ReceiverState::Completed | ReceiverState::Dead
    ) {
        return refused(
            "a completed workflow lane has no re-entry point: an official resume by its id \
             resolves to a transcript path the lane never wrote to, and no hook fires in a \
             lane that has ended",
        );
    }
    Decision {
        channel: csift_channel(ctx),
        verdict: Verdict::Ok,
        prediction: "the csift channel is the only carrier for a workflow lane: the official \
                     send fails closed on it."
            .to_string(),
        risks: Vec::new(),
        official: None,
        queued: true,
    }
}

/// Another top-level session. The official arm is the messaging socket, whose presence in the
/// registry row is the only on-disk proof that it exists and is bound.
fn top_level_row(ctx: &SendContext) -> Decision {
    if ctx.headless {
        return Decision {
            channel: csift_channel(ctx),
            verdict: Verdict::Unpredictable,
            prediction: "the receiver is a headless run: csift never promises delivery to one."
                .to_string(),
            risks: vec![
                "the registry row's entrypoint is `sdk-cli`: a headless receiver has no \
                 approval surface for an inbound message and may end before any hook point \
                 is reached"
                    .to_string(),
            ],
            official: None,
            queued: true,
        };
    }
    if ctx.caller_kind == SenderKind::Lane && ctx.socket_present && ctx.official_possible() {
        return Decision {
            channel: CH_UDS,
            verdict: Verdict::Ok,
            prediction: "the receiver published a messaging socket, so the official \
                         cross-session send reaches it directly."
                .to_string(),
            risks: vec![
                "the official inbound message can be HELD for approval when the receiver's \
                 permission mode is bypass-class, and it is enqueued at the receiver's MAIN \
                 lane - a reply cannot come back to a subagent"
                    .to_string(),
            ],
            official: Some(OfficialCall::unaddressable()),
            queued: true,
        };
    }
    let mut risks = Vec::new();
    if !ctx.socket_present && !ctx.async_rewake_on_stop {
        risks.push(
            "no messaging socket in the registry and no `asyncRewake` hook on Stop: nothing \
             wakes an idle top-level session, so the message waits for its human"
                .to_string(),
        );
    }
    Decision {
        channel: csift_channel(ctx),
        verdict: Verdict::Ok,
        prediction: "no official arm is available for this receiver, so the csift channel \
                     carries it."
            .to_string(),
        risks,
        official: None,
        queued: true,
    }
}

/// A teammate or an unnamed subagent: the two receivers the official transports do reach, and
/// the two whose COMPLETED state turns a send into a respawn.
fn agent_row(ctx: &SendContext) -> Decision {
    if ctx.receiver_state == ReceiverState::Completed {
        return completed_agent_row(ctx);
    }
    if ctx.caller_kind == SenderKind::External || !ctx.official_possible() {
        return csift_only(ctx);
    }
    // A subagent sending to its own parent subagent. `Relation::Child` names the SENDER's
    // standing, so it covers both a subagent addressing the session's main conversation and one
    // addressing the agent that spawned it; the receiver kind separates them, and only the
    // second is this row. The official `to` grammar has a `main` arm and an `a...` id arm and no
    // parent arm at all: addressing `main` reaches the top-level conversation rather than the
    // spawning agent, so a delegation here would silently deliver to the wrong lane.
    if ctx.caller_is_subagent
        && ctx.relation == Relation::Child
        && ctx.receiver_kind == ReceiverKind::UnnamedSubagent
    {
        return Decision {
            channel: csift_channel(ctx),
            verdict: Verdict::Ok,
            prediction: "the official channel has no parent arm: addressing `main` reaches the \
                         top-level conversation, not the spawning agent, so the csift channel \
                         is the only carrier to a parent subagent."
                .to_string(),
            risks: Vec::new(),
            official: None,
            queued: true,
        };
    }
    match ctx.receiver_kind {
        ReceiverKind::Teammate if ctx.teams.enabled => Decision {
            channel: CH_MAILBOX,
            verdict: Verdict::Ok,
            prediction: "the teammate mailbox is the official arm and the receiver polls it \
                         itself."
                .to_string(),
            risks: vec![
                "a mailbox entry is DELETED when the receiver consumes it and its `read` flag \
                 is never flipped, so only the entry's disappearance joined to a record in \
                 the receiver's transcript proves delivery"
                    .to_string(),
            ],
            official: Some(OfficialCall::addressed(official_to(ctx))),
            queued: true,
        },
        ReceiverKind::Teammate => csift_only(ctx),
        _ => Decision {
            channel: CH_IN_PROCESS,
            verdict: Verdict::Ok,
            prediction: "the in-process queue is the official arm for a running unnamed \
                         subagent."
                .to_string(),
            risks: vec![
                "the official queue returns that a task record EXISTS, never that the message \
                 was delivered; the target must be addressed by its `a...` id, and the send \
                 is attributed to the session rather than to the calling lane"
                    .to_string(),
            ],
            official: Some(OfficialCall::addressed(official_to(ctx))),
            queued: true,
        },
    }
}

/// The `to` value for an agent receiver: a teammate's routing form when it has one (the form
/// the official tool documents), otherwise the transcript-form lane id, which the official id
/// arm also accepts. Never a session uuid - that arm does not exist.
fn official_to(ctx: &SendContext) -> &str {
    ctx.routing_id.as_deref().unwrap_or(&ctx.lane)
}

/// A completed teammate or unnamed subagent. Sending to one is not a delivery: the official
/// path RESPAWNS the lane, so it is refused unless the caller asked for exactly that.
fn completed_agent_row(ctx: &SendContext) -> Decision {
    if !ctx.official_possible() {
        return refused(
            "the receiver lane has completed and its Claude Code version is below the official \
             floor, so no resume path exists for it and a csift delivery would wait for a hook \
             point that will never come",
        );
    }
    if !ctx.resume {
        return refused(
            "the receiver lane has completed: reaching it is an official RESUME, which \
             respawns the lane rather than delivering to it. Pass --resume to delegate that \
             resume, or address a running lane instead",
        );
    }
    Decision {
        channel: CH_RESUME,
        verdict: Verdict::Ok,
        prediction: "this respawns the lane with its prior messages replayed; on Claude Code \
                     below 2.1.260 its completion notification goes to the main conversation, \
                     not to you."
            .to_string(),
        risks: vec![
            "a resume is an action, not a delivery: the lane is respawned, and a concurrent \
             resume of the same lane throws rather than queueing"
                .to_string(),
            "with background tasks disabled, or against the built-in web-fetch agent, the \
             resume runs INLINE: no completion notification is enqueued at all and the report \
             comes back inside the tool result instead"
                .to_string(),
        ],
        official: Some(OfficialCall::addressed(official_to(ctx))),
        queued: true,
    }
}

/// The csift-only arm: an external caller, a receiver below the official floor, or a gate
/// csift cannot see enabled.
fn csift_only(ctx: &SendContext) -> Decision {
    let why = if ctx.caller_kind == SenderKind::External {
        "the caller is outside Claude Code and holds no tool to call, so the csift channel is \
         the only carrier."
    } else if !ctx.official_possible() {
        "the receiver is below the official floor for any official send path, so the csift \
         channel is the only carrier."
    } else {
        "no official arm is provable from disk for this receiver, so the csift channel carries \
         it."
    };
    Decision {
        channel: csift_channel(ctx),
        verdict: Verdict::Ok,
        prediction: why.to_string(),
        risks: Vec::new(),
        official: None,
        queued: true,
    }
}

fn refused(reason: &str) -> Decision {
    Decision {
        channel: CH_NONE,
        verdict: Verdict::Refused,
        prediction: reason.to_string(),
        risks: Vec::new(),
        official: None,
        queued: false,
    }
}

fn csift_channel(ctx: &SendContext) -> &'static str {
    match ctx.mode {
        Mode::Steer => CH_STEER,
        Mode::Queue => CH_QUEUE,
    }
}

/// The sentence appended when csift also queues the message: what will actually carry it.
fn csift_leg(ctx: &SendContext) -> String {
    let event = ctx.best_event.as_deref().unwrap_or("no configured event");
    match ctx.mode {
        Mode::Steer => format!(
            "csift queued it too: the next `csift deliver` hook to run in this lane emits part \
             1 ({} of {} chunk(s) fit at {event}).",
            ctx.best_slots.min(ctx.chunks),
            ctx.chunks
        ),
        Mode::Queue => format!(
            "csift queued it too: delivery waits for a turn boundary ({} of {} chunk(s) fit at \
             {event}).",
            ctx.best_slots.min(ctx.chunks),
            ctx.chunks
        ),
    }
}

/// Risks that belong to the csift half of the send, whatever the channel.
fn collect_risks(ctx: &SendContext, d: &mut Decision) {
    if !d.queued {
        return;
    }
    if let Some(switch) = &ctx.hooks_policy_switch {
        d.risks.push(format!(
            "a policy switch rewrote the receiver's hook set ({switch}), so a configured \
             delivery hook may never run"
        ));
    }
    if ctx.best_slots == 0 {
        d.risks.push(
            "no `csift deliver --slot k` hook is configured on any delivery event in the \
             receiver's settings cascade: the message stays queued until one is installed"
                .to_string(),
        );
    } else if ctx.armed == 0 {
        d.risks.push(
            "the delivery slots are CONFIGURED but none has ever run in this lane (no armed \
             marker): configuration is not arming - the receiver's process may predate the \
             settings edit"
                .to_string(),
        );
    }
    if ctx.receiver_state == ReceiverState::Frozen {
        d.risks.push(
            "the receiver lane is frozen at an unreturned tool call: its next hook point comes \
             only when that call returns"
                .to_string(),
        );
    }
    if ctx.receiver_kind == ReceiverKind::Teammate && !ctx.teams.enabled {
        d.risks.push(format!("teams gate: {}", ctx.teams.verdict));
    }
}

/// One verdict per send, most severe first. `delegated_risks` is how many of the decision's
/// risks belong to the official leg (see [`decide`]); only the csift-side ones downgrade OK.
fn verdict_for(ctx: &SendContext, d: &Decision, delegated_risks: usize) -> Verdict {
    if d.verdict == Verdict::Refused {
        return Verdict::Refused;
    }
    if !d.queued {
        // `--official-only`: the official call IS the delivery and csift wrote nothing, so
        // there is no csift-side prediction to qualify.
        return Verdict::Ok;
    }
    if ctx.best_slots == 0 || !ctx.alive() || ctx.headless {
        return Verdict::Unpredictable;
    }
    if ctx.chunks > ctx.best_slots {
        return Verdict::Full;
    }
    if d.risks.len() > delegated_risks {
        Verdict::MayFail
    } else {
        Verdict::Ok
    }
}

/// `FULL`'s own sentence: how many slots this message would need.
pub(crate) fn full_note(ctx: &SendContext) -> String {
    format!(
        "{} chunk(s) against {} configured slot(s) at {}: {} more slot(s) on one delivery \
         event would carry the whole message in one hook round",
        ctx.chunks,
        ctx.best_slots,
        ctx.best_event.as_deref().unwrap_or("no configured event"),
        ctx.chunks.saturating_sub(ctx.best_slots)
    )
}

/// Parse a Claude Code version triple. `None` for anything that is not three integers, which
/// keeps an unreadable version out of a floor comparison instead of defaulting it to zero.
pub(crate) fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.trim().split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next()?.parse().ok()?;
    it.next().is_none().then_some((a, b, c))
}
