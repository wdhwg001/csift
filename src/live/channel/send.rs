//! `csift send` - queue a message for one Claude Code lane, and say honestly what will
//! carry it.
//!
//! The command does four things and refuses to pretend about any of them. It resolves the
//! target to EXACTLY one transcript through the shared `@`-grammar (a routing form is
//! translated to the transcript form and the routing id kept for the official call). It reads
//! the receiver's state, configuration and runtime arming off disk. It runs the policy table
//! ([`super::policy`]) for one verdict. And it writes three files csift owns: the message
//! source and the receiver's inbox line, plus the sender's outbox line.
//!
//! What it never does is SEND on the official channel: those transports are model tools, and a
//! binary cannot call one. A delegated row therefore prints the exact call for the caller to
//! make and still queues the message on csift's own channel, so a delegation that the caller
//! forgets - or that the official arm swallows - is not a lost message.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::caller::{
    self, teams_dirs, Caller, GateVerdict, Receiver, SettingsDisclosure, LANE_ASSUMED_NOTE,
};
use super::policy::{self, Decision, SendContext};
use super::send_render::{render_json, render_text, Receipt};
use super::{
    append_inbox, append_outbox, channel_dir, expires_at, new_message_id, now_utc, read_armed,
    render, write_message, InboxLine, Message, MessageFrom, MessageTo, Mode, OfficialRef,
    OutboxLine, Relation, TargetForm, Verdict, CHUNK_BUDGET, STEER_EVENTS,
};
use crate::cli::{OutputFormat, SendArgs};
use crate::path::settings::{self, Merged};

/// The env key whose value enables agent teams. Only a value csift can READ counts; the shell
/// environment and the CLI flag that also enable it leave nothing on disk.
const TEAMS_ENV: &str = "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS";

/// The turn-boundary subset a queue message is restricted to. It is the CENSUS list, wider
/// than the hook entry's eligibility test by `SessionStart`: only its resume and compact
/// sources are a re-entry, and the settings cascade names an event, not a source.
const QUEUE_EVENTS: [&str; 4] = ["SessionStart", "UserPromptSubmit", "Stop", "SubagentStop"];

pub(crate) fn run_send(args: &SendArgs) -> Result<()> {
    let caller = caller::classify(args.from.as_deref())?;
    let body = read_body(args)?;
    let mode = Mode::parse(args.mode.as_str()).unwrap_or(Mode::Steer);
    let ttl_secs = parse_ttl(&args.ttl)?;

    let target = target_path(&args.target)?;
    let receiver = caller::probe_receiver(&target)?;
    let form = if crate::path::is_teammate_routing_id(args.target.trim_start_matches('@')) {
        TargetForm::Routing
    } else {
        TargetForm::Transcript
    };

    let merged = settings::merged(
        &crate::path::claude_home()?,
        receiver.cwd.as_deref().map(Path::new),
    );
    let slots = SlotCensus::read(&merged, mode);
    let settings_read = SettingsDisclosure::of(&merged);
    let armed = armed_slots(&receiver)?;

    // Read once: resolving a subagent sender's parent rebuilds the session topology, and the
    // envelope and the policy table must in any case be deciding from the SAME relation.
    let relation = relation_of(&caller, &receiver);
    let msg = build_message(&caller, &receiver, form, mode, ttl_secs, relation, body);
    let chunks = render(&msg, CHUNK_BUDGET)?.len();
    let ctx = context(&msg, &receiver, &merged, &slots, armed.len(), chunks, args);
    let decision = policy::decide(&ctx);

    if decision.queued {
        write_send(&receiver, &caller, &msg, &decision)?;
    }
    if !caller.lane_exact {
        eprintln!("{LANE_ASSUMED_NOTE}");
    }
    if decision.verdict == Verdict::Refused {
        eprintln!(
            "csift: REFUSED - nothing was queued. {}",
            decision.prediction
        );
    }
    let receipt = Receipt {
        msg: &msg,
        receiver: &receiver,
        ctx: &ctx,
        decision: &decision,
        slots: &slots,
        armed: &armed,
        settings: &settings_read,
    };
    match args.format {
        OutputFormat::Json => render_json(&receipt),
        OutputFormat::Text => {
            render_text(&receipt);
            Ok(())
        }
    }
}

/// The message body: the positional, `-f FILE`, or stdin - exactly one of them.
fn read_body(args: &SendArgs) -> Result<String> {
    match (&args.message, &args.file) {
        (Some(_), Some(_)) => bail!("give the message as a positional OR `-f FILE`, not both"),
        (Some(m), None) => Ok(m.clone()),
        (None, Some(f)) => std::fs::read_to_string(f)
            .with_context(|| format!("reading the message body from {}", f.display())),
        (None, None) => {
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
                .context("reading the message body from stdin")?;
            if buf.trim().is_empty() {
                bail!(
                    "no message: pass it as a positional, as `-f FILE`, or on stdin. An empty \
                     message would render an envelope with nothing in it."
                );
            }
            Ok(buf)
        }
    }
}

/// Resolve the target to EXACTLY one transcript. Zero and several are both hard errors: a
/// send addresses one lane, and picking one of several would deliver to the wrong one.
fn target_path(target: &str) -> Result<PathBuf> {
    let files = crate::path::resolve_session_files(
        std::slice::from_ref(&PathBuf::from(target)),
        crate::path::SubagentScope::TopLevelOnly,
        crate::path::Caller::Other,
    )?;
    match files.as_slice() {
        [one] => Ok(one.clone()),
        [] => bail!("`{target}` resolved to no transcript: a send addresses exactly one lane"),
        many => bail!(
            "`{target}` resolved to {} transcripts; a send addresses exactly ONE lane. Name it \
             by its own id: {}",
            many.len(),
            many.iter()
                .map(|p| crate::subagent::session_id_from_path(p))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The site-wide relative-duration grammar (`30s`, `10m`, `12h`, `2d`, `1w`, `3mo`, `1y`) read
/// FORWARD, as a lifetime rather than a cutoff in the past. The unit table mirrors the one the
/// `--since` bounds use, with the same calendar approximation (a month is 30 days, a year 365).
pub(crate) fn parse_ttl(raw: &str) -> Result<u64> {
    let s = raw.trim();
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits == s.len() {
        bail!(
            "--ttl `{raw}`: a duration is a number and a unit - s, m, h, d, w, mo or y (for \
             example `12h`, the default, or `2d`)"
        );
    }
    let (n, unit) = s.split_at(digits);
    let n: u64 = n
        .parse()
        .with_context(|| format!("--ttl quantity in `{raw}`"))?;
    let secs = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        "w" => 604_800,
        "mo" => 2_592_000,
        "y" => 31_536_000,
        _ => bail!("--ttl `{raw}`: unknown unit `{unit}` - use s, m, h, d, w, mo or y"),
    };
    n.checked_mul(secs)
        .with_context(|| format!("--ttl `{raw}` overflows the representable range"))
}

/// The configured delivery slots per event, for the events this mode may ride.
#[derive(Debug, Clone)]
pub(crate) struct SlotCensus {
    pub(crate) per_event: Vec<(String, Vec<u32>)>,
    /// The event carrying the most slots, and how many - what a prediction can lean on.
    pub(crate) best_event: Option<String>,
    pub(crate) best: usize,
    pub(crate) async_rewake_on_stop: bool,
}

impl SlotCensus {
    fn read(m: &Merged, mode: Mode) -> Self {
        let events: &[&str] = match mode {
            Mode::Steer => &STEER_EVENTS,
            Mode::Queue => &QUEUE_EVENTS,
        };
        let mut per_event = Vec::new();
        let mut best_event = None;
        let mut best = 0usize;
        for e in events {
            let slots = settings::deliver_slots(m, e);
            if slots.len() > best {
                best = slots.len();
                best_event = Some((*e).to_string());
            }
            if !slots.is_empty() {
                per_event.push(((*e).to_string(), slots));
            }
        }
        let async_rewake_on_stop = settings::hooks_for_event(m, "Stop")
            .iter()
            .any(|h| h.async_rewake);
        SlotCensus {
            per_event,
            best_event,
            best,
            async_rewake_on_stop,
        }
    }
}

/// The slots that have actually RUN in the receiver's lane (its armed marker).
fn armed_slots(receiver: &Receiver) -> Result<Vec<u32>> {
    let root = channel_dir(&sidecar_dir(&receiver.session_path));
    Ok(read_armed(&root, &receiver.lane)?
        .map(|m| m.slots_seen.iter().copied().collect())
        .unwrap_or_default())
}

/// The session sidecar directory (`<projects>/<encoded>/<session-uuid>/`), derived from the
/// transcript path rather than looked up: it may not exist yet, and a WRITER creates it.
fn sidecar_dir(session_path: &Path) -> PathBuf {
    session_path.with_extension("")
}

/// Assemble the message source.
fn build_message(
    caller: &Caller,
    receiver: &Receiver,
    form: TargetForm,
    mode: Mode,
    ttl_secs: u64,
    relation: Relation,
    body: String,
) -> Message {
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.display().to_string());
    Message {
        id: new_message_id(),
        ts_utc: now_utc(),
        from: MessageFrom {
            kind: caller.kind,
            session: caller.session.clone(),
            lane: caller.lane.clone(),
            label: caller.label.clone(),
            cwd,
        },
        to: MessageTo {
            session: receiver.session.clone(),
            lane: receiver.lane.clone(),
            form,
            routing_id: receiver.routing_id.clone(),
        },
        mode,
        ttl_secs,
        relation,
        cross_project: relation == Relation::CrossProject,
        body,
    }
}

/// How the sender stands to the receiver. The envelope prints it and adds the peer caution for
/// the four values that make the sender a peer, so a receiver never reads a sibling's message
/// as an instruction from its parent.
pub(crate) fn relation_of(caller: &Caller, receiver: &Receiver) -> Relation {
    let Some(sender_session) = caller.session.as_deref() else {
        return Relation::External;
    };
    let Some(sender_lane) = caller.lane.as_deref() else {
        return Relation::Unknown;
    };
    if sender_session != receiver.session {
        return if same_project(caller, receiver) {
            Relation::CrossSession
        } else {
            Relation::CrossProject
        };
    }
    if sender_lane == receiver.lane {
        return Relation::Unknown; // a lane addressing itself
    }
    if sender_lane == receiver.session {
        return Relation::Parent; // the top-level lane to one of its own subagents
    }
    if receiver.lane == receiver.session {
        return Relation::Child; // a subagent to its own session's main conversation
    }
    // Two subagents of one session are siblings UNLESS one of them spawned the other, and the
    // value names the SENDER's standing, exactly as the two cases above do. Each direction
    // matters for a different reason. Receiver-spawned-sender (CHILD) decides the CHANNEL: the
    // official `to` grammar has a `main` arm and an `a...` id arm but no parent arm, so a
    // delegation would reach the wrong lane. Sender-spawned-receiver (PARENT) leaves the
    // channel alone but decides the ENVELOPE: the peer caution tells a receiver that the
    // sender has no authority over its task, which is exactly what its own parent must never
    // be labelled. An unresolvable link - no topology, no node, no recorded parent - keeps the
    // sibling reading: a guessed ancestry would misroute one and mislabel the other.
    let (sender_parent, receiver_parent) =
        caller::parent_agents_of(&receiver.session_path, sender_lane, &receiver.lane);
    if sender_parent.as_deref() == Some(receiver.lane.as_str()) {
        return Relation::Child;
    }
    if receiver_parent.as_deref() == Some(sender_lane) {
        return Relation::Parent;
    }
    Relation::Sibling
}

/// Two lanes share a project when their transcripts sit under the same encoded project dir.
/// The sender's is not always knowable (its transcript may be elsewhere on disk), so an
/// unresolvable sender is reported as cross-session rather than guessed into cross-project.
fn same_project(caller: &Caller, receiver: &Receiver) -> bool {
    let Some(session) = caller.session.as_deref() else {
        return false;
    };
    let Ok(files) = crate::path::resolve_session_files(
        &[PathBuf::from(format!("@{session}"))],
        crate::path::SubagentScope::TopLevelOnly,
        crate::path::Caller::Other,
    ) else {
        return true;
    };
    match files.as_slice() {
        [one] => one.parent() == receiver.session_path.parent(),
        _ => true,
    }
}

/// Fold every read fact into the policy table's input.
///
/// The sender half is read off the assembled message rather than the caller: the envelope and
/// the policy table then decide from the same frozen facts, and the relation - whose resolution
/// can cost a topology build - is computed exactly once per send.
fn context(
    msg: &Message,
    receiver: &Receiver,
    merged: &Merged,
    slots: &SlotCensus,
    armed: usize,
    chunks: usize,
    args: &SendArgs,
) -> SendContext {
    let teams_scope = settings::env_value(merged, TEAMS_ENV)
        .filter(|(value, _)| truthy(value))
        .map(|(_, scope)| scope.to_string());
    let caller_is_subagent = msg
        .from
        .lane
        .as_deref()
        .is_some_and(|l| Some(l) != msg.from.session.as_deref());
    SendContext {
        caller_kind: msg.from.kind,
        caller_is_subagent,
        relation: msg.relation,
        receiver_kind: receiver.kind,
        receiver_state: receiver.state,
        receiver_version: receiver.version.clone(),
        headless: receiver.headless,
        socket_present: receiver.socket_present,
        lane: receiver.lane.clone(),
        routing_id: receiver.routing_id.clone(),
        mode: Mode::parse(args.mode.as_str()).unwrap_or(Mode::Steer),
        // A send really writes the queue, so its prediction speaks in the past tense.
        queues: true,
        resume: args.resume,
        official_only: args.official_only,
        teams: GateVerdict::teams(
            teams_scope.as_deref(),
            teams_dirs(),
            receiver.teammate_lanes,
        ),
        harbor: GateVerdict::harbor(receiver.socket_present),
        chunks,
        best_slots: slots.best,
        best_event: slots.best_event.clone(),
        armed,
        async_rewake_on_stop: slots.async_rewake_on_stop,
        hooks_policy_switch: merged.policy_switch.map(str::to_string),
    }
}

/// A settings env value counts as enabling when it is not one of the falsey spellings: the
/// harness reads these as booleans, and `"0"` is not an enable.
pub(crate) fn truthy(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "no" | "off"
    )
}

/// The three writes. The message source and the receiver's inbox line live under the RECEIVER's
/// session (that is where a delivery hook looks); the outbox line lives under the SENDER's, so
/// a sender can audit its own history without reading anyone's inbox. An external caller has no
/// session of its own, so its outbox line rides under the receiver - recorded somewhere real
/// beats recorded nowhere.
fn write_send(
    receiver: &Receiver,
    caller: &Caller,
    msg: &Message,
    decision: &Decision,
) -> Result<()> {
    let receiver_root = channel_dir(&sidecar_dir(&receiver.session_path));
    write_message(&receiver_root, msg)?;
    append_inbox(
        &receiver_root,
        &receiver.lane,
        &InboxLine {
            id: msg.id.clone(),
            enqueued_utc: msg.ts_utc.clone(),
            mode: msg.mode,
            expires_utc: expires_at(&msg.ts_utc, msg.ttl_secs),
        },
    )?;
    let sender_root = sender_root(caller).unwrap_or_else(|| receiver_root.clone());
    append_outbox(
        &sender_root,
        &OutboxLine {
            id: msg.id.clone(),
            ts_utc: msg.ts_utc.clone(),
            to_lane: receiver.lane.clone(),
            to_session: receiver.session.clone(),
            mode: msg.mode,
            verdict: decision.verdict,
            channel: decision.channel.to_string(),
            official: decision
                .official
                .as_ref()
                .map_or_else(OfficialRef::none, |o| OfficialRef {
                    delegated: true,
                    tool: Some(o.tool.to_string()),
                    to_form: o.to.clone(),
                }),
        },
    )
}

/// The sender's own channel root, when its session transcript can be located.
fn sender_root(caller: &Caller) -> Option<PathBuf> {
    let session = caller.session.as_deref()?;
    let files = crate::path::resolve_session_files(
        &[PathBuf::from(format!("@{session}"))],
        crate::path::SubagentScope::TopLevelOnly,
        crate::path::Caller::Other,
    )
    .ok()?;
    match files.as_slice() {
        [one] => Some(channel_dir(&sidecar_dir(one))),
        _ => None,
    }
}
