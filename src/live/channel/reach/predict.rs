//! `whoami --to <target>`: the reach prediction, with no message.
//!
//! The prediction runs the SAME policy table `csift send` runs, over the same receiver probe and
//! the same settings cascade, so "what would happen" and "what happened" cannot disagree. What
//! it does not have is a body, so it sizes ONE chunk - the floor any message costs. A long
//! message can still come back FULL from a real send, and the receipt says which event has how
//! many slots so the caller can see that coming.

use super::*;

/// The configured delivery slots per steer event, and the event that carries the most.
#[derive(Debug, Clone, Default)]
pub(crate) struct ReachSlots {
    pub(crate) per_event: Vec<(String, Vec<u32>)>,
    pub(crate) best_event: Option<String>,
    pub(crate) best: usize,
    pub(crate) async_rewake_on_stop: bool,
}

impl ReachSlots {
    pub(crate) fn read(m: &Merged) -> Self {
        let mut out = ReachSlots::default();
        for e in STEER_EVENTS {
            let slots = settings::deliver_slots(m, e);
            if slots.len() > out.best {
                out.best = slots.len();
                out.best_event = Some(e.to_string());
            }
            if !slots.is_empty() {
                out.per_event.push((e.to_string(), slots));
            }
        }
        out.async_rewake_on_stop = settings::hooks_for_event(m, "Stop")
            .iter()
            .any(|h| h.async_rewake);
        out
    }
}

/// The receiver half of a policy decision, from either reader: the light lane probe behind the
/// sections, or the full receiver probe behind a prediction.
#[derive(Debug, Clone)]
pub(crate) struct TargetFacts {
    pub(crate) kind: ReceiverKind,
    pub(crate) state: ReceiverState,
    pub(crate) version: Option<String>,
    pub(crate) lane: String,
    pub(crate) routing_id: Option<String>,
    pub(crate) socket_present: bool,
    pub(crate) headless: bool,
    pub(crate) teammate_lanes: usize,
}

pub(crate) fn target_facts(lane: &LaneFacts) -> TargetFacts {
    TargetFacts {
        kind: lane.kind,
        state: lane.state,
        version: lane.version.clone(),
        lane: lane.lane.clone(),
        routing_id: lane.routing_id.clone(),
        socket_present: lane.socket_present,
        headless: lane.headless,
        teammate_lanes: 0,
    }
}

fn receiver_target(r: &Receiver) -> TargetFacts {
    TargetFacts {
        kind: r.kind,
        state: r.state,
        version: r.version.clone(),
        lane: r.lane.clone(),
        routing_id: r.routing_id.clone(),
        socket_present: r.socket_present,
        headless: r.headless,
        teammate_lanes: r.teammate_lanes,
    }
}

/// Fold the read facts into the policy table's input. One builder for both callers, so a
/// prediction and a reply channel are never decided from differently shaped contexts.
pub(crate) fn send_context(
    caller: &Caller,
    t: &TargetFacts,
    merged: &Merged,
    slots: &ReachSlots,
    armed: usize,
    relation: Relation,
) -> SendContext {
    let teams_scope = settings::env_value(merged, TEAMS_ENV)
        .filter(|(value, _)| truthy(value))
        .map(|(_, scope)| scope.to_string());
    let caller_is_subagent = caller
        .lane
        .as_deref()
        .is_some_and(|l| Some(l) != caller.session.as_deref());
    SendContext {
        caller_kind: caller.kind,
        caller_is_subagent,
        relation,
        receiver_kind: t.kind,
        receiver_state: t.state,
        receiver_version: t.version.clone(),
        headless: t.headless,
        socket_present: t.socket_present,
        lane: t.lane.clone(),
        routing_id: t.routing_id.clone(),
        mode: Mode::Steer,
        // Nothing on this path writes, so the prediction says what WOULD be queued.
        queues: false,
        resume: false,
        official_only: false,
        teams: GateVerdict::teams(
            teams_scope.as_deref(),
            caller::teams_dirs(),
            t.teammate_lanes,
        ),
        harbor: GateVerdict::harbor(t.socket_present),
        // No body to measure: one chunk is the floor any message costs.
        chunks: 1,
        best_slots: slots.best,
        best_event: slots.best_event.clone(),
        armed,
        async_rewake_on_stop: slots.async_rewake_on_stop,
        hooks_policy_switch: merged.policy_switch.map(str::to_string),
    }
}

/// `csift whoami --to @<target>`: predict, print, write nothing.
pub(crate) fn run_reach_to(target: &str, format: OutputFormat) -> Result<()> {
    let caller = caller::classify(None)?;
    let path = resolve_one(target)?;
    let receiver = caller::probe_receiver(&path)?;
    let merged = settings::merged(
        &crate::path::claude_home()?,
        receiver.cwd.as_deref().map(Path::new),
    );
    let slots = ReachSlots::read(&merged);
    let armed = armed_slots(&receiver);
    let relation = relation_of(&caller, &receiver);
    let t = receiver_target(&receiver);
    let ctx = send_context(&caller, &t, &merged, &slots, armed.len(), relation);
    let decision = policy::decide(&ctx);

    if caller.kind == SenderKind::Lane && !caller.lane_exact {
        eprintln!("{LANE_ASSUMED_NOTE}");
    }
    let reach = Reach {
        inference: inference_note(&t, &decision, &ctx),
        lane: receiver.lane.clone(),
        routing_id: receiver.routing_id.clone(),
        session: receiver.session.clone(),
        kind: receiver.kind,
        state: receiver.state,
        version: receiver.version.clone(),
        caller_kind: caller.kind,
        caller_label: caller_label(&caller),
        decision,
        slots,
        armed,
        teams: ctx.teams.clone(),
        harbor: ctx.harbor.clone(),
        settings: SettingsDisclosure::of(&merged),
    };
    match format {
        OutputFormat::Text => {
            render_reach_text(&reach);
            Ok(())
        }
        OutputFormat::Json => render_reach_json(&reach),
    }
}

/// The sentence an AGENT target carries: what the prediction is made of, and which carrier is
/// left when it is wrong. A top-level target is answered from the registry row the harness
/// itself wrote, so it carries none.
pub(crate) fn inference_note(t: &TargetFacts, d: &Decision, ctx: &SendContext) -> Option<String> {
    if t.kind == ReceiverKind::TopLevel {
        return None;
    }
    let fallback = if d.queued && ctx.best_slots > 0 {
        match ctx.mode {
            Mode::Steer => "csift steer",
            Mode::Queue => "csift queue",
        }
    } else {
        // Nothing csift owns can carry it: no queue was written, or no delivery hook is
        // configured to emit one.
        "HELD"
    };
    Some(format!("{INFERENCE_NOTE}; fallback: {fallback}"))
}

fn caller_label(c: &Caller) -> String {
    match c.kind {
        SenderKind::Lane => c
            .lane
            .clone()
            .or_else(|| c.session.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        SenderKind::External => c.label.clone().unwrap_or_else(|| "unknown".to_string()),
    }
}

/// The slots that have actually RUN in the receiver's lane.
fn armed_slots(r: &Receiver) -> Vec<u32> {
    let root = channel_dir(&r.session_path.with_extension(""));
    read_armed(&root, &r.lane)
        .ok()
        .flatten()
        .map(|m| m.slots_seen.iter().copied().collect())
        .unwrap_or_default()
}

/// Resolve a target to EXACTLY one transcript. Zero and several are both hard errors: a reach
/// question is about one lane, and answering for one of several would answer about the wrong one.
pub(crate) fn resolve_one(target: &str) -> Result<PathBuf> {
    let files = crate::path::resolve_session_files(
        std::slice::from_ref(&PathBuf::from(target)),
        crate::path::SubagentScope::TopLevelOnly,
        crate::path::Caller::Other,
    )?;
    match files.as_slice() {
        [one] => Ok(one.clone()),
        [] => bail!("`{target}` resolved to no transcript: a reach question addresses one lane"),
        many => bail!(
            "`{target}` resolved to {} transcripts; a reach question addresses exactly ONE lane. \
             Name it by its own id: {}",
            many.len(),
            many.iter()
                .map(|p| crate::subagent::session_id_from_path(p))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
