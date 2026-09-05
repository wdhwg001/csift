//! The lane probe behind the `self`, `parent` and `topology` sections.
//!
//! This is the LIGHT probe deliberately: `whoami` runs inside hooks, so a section may cost a
//! head read, a bounded tail read and one topology build, never a whole-transcript scan. The
//! heavy reader that also asks the harness for its terminal word about a lane
//! ([`super::caller::probe_receiver`]) belongs to the prediction path, where the caller asked
//! for exactly that answer.

use super::*;

/// The lane `whoami` resolved, and how exactly it knows it.
#[derive(Debug, Clone)]
pub(crate) struct LaneRef {
    pub(crate) path: PathBuf,
    /// False for the environment form: the variable names the TOP-LEVEL session in every lane,
    /// so the caller may be a subagent the environment cannot name. The sections say so rather
    /// than presenting the assumption as an identity.
    pub(crate) exact: bool,
    /// How the lane was resolved: `env`, `trap` or `target`.
    pub(crate) via: &'static str,
}

/// Everything the three sections need about one lane.
#[derive(Debug, Clone)]
pub(crate) struct LaneFacts {
    pub(crate) lane: String,
    pub(crate) session: String,
    pub(crate) session_path: PathBuf,
    pub(crate) path: PathBuf,
    pub(crate) kind: ReceiverKind,
    pub(crate) routing_id: Option<String>,
    /// Topology depth (0 = a direct subagent of the session, or the session itself). `None`
    /// when the lane has no node in the topology, which is UNKNOWN and never zero.
    pub(crate) depth: Option<usize>,
    pub(crate) parent_agent_id: Option<String>,
    pub(crate) state: ReceiverState,
    pub(crate) version: Option<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) last_activity_utc: Option<String>,
    pub(crate) socket_present: bool,
    pub(crate) headless: bool,
}

/// One live lane under a session: the row the topology and peer surfaces both print.
#[derive(Debug, Clone)]
pub(crate) struct LaneRow {
    pub(crate) lane: String,
    pub(crate) kind: ReceiverKind,
    /// `in-flight` | `generating` for a child lane; a registry status for a top-level lane.
    pub(crate) state: String,
    pub(crate) last_activity_utc: Option<String>,
}

/// The parent lane and the channel a reply to it would take.
#[derive(Debug, Clone)]
pub(crate) struct ParentFacts {
    pub(crate) lane: String,
    pub(crate) kind: ReceiverKind,
    /// `None` is UNKNOWN: no registry row, an unreadable transcript, a foreign pid domain.
    pub(crate) alive: Option<bool>,
    pub(crate) reply_channel: &'static str,
}

/// The three sections, built once.
#[derive(Debug, Clone)]
pub(crate) struct Sections {
    pub(crate) me: LaneFacts,
    pub(crate) exact: bool,
    pub(crate) via: &'static str,
    /// `None` for a top-level lane: it has no parent lane, which is an answer, not a gap.
    pub(crate) parent: Option<ParentFacts>,
    pub(crate) subtree: Vec<LaneRow>,
    /// Live lanes outside this lane and its subtree, counted over the sessions whose registry
    /// row is alive.
    pub(crate) others: usize,
}

/// Read one lane's facts.
pub(crate) fn lane_facts(path: &Path) -> Result<LaneFacts> {
    let lane = crate::subagent::session_id_from_path(path);
    let is_sub = crate::subagent::is_subagent_path(path);
    let session_path = caller::session_transcript_for(path);
    let session = crate::subagent::session_id_from_path(&session_path);
    let (version, cwd) = head_facts(path)?;
    let shape = tail_shape(path)?;
    let reg = if is_sub {
        None
    } else {
        registry_facts(&session).unwrap_or_default()
    };

    let mut facts = LaneFacts {
        lane: lane.clone(),
        session,
        session_path,
        path: path.to_path_buf(),
        kind: ReceiverKind::TopLevel,
        routing_id: None,
        depth: None,
        parent_agent_id: None,
        state: ReceiverState::Unknown,
        version,
        cwd,
        last_activity_utc: shape.last_ts_utc.clone(),
        socket_present: reg.as_ref().is_some_and(|r| r.socket_present),
        headless: reg.as_ref().is_some_and(|r| r.headless),
    };
    if is_sub {
        // ONE topology build answers the kind, the routing form, the depth and the spawning
        // agent: the on-disk layout is flat, so all four live only in the reconstruction.
        let nodes = crate::subagent::build_topology(&facts.session_path, false).unwrap_or_default();
        match nodes.iter().find(|n| n.agent_id == lane) {
            Some(n) => {
                facts.kind = receiver_kind_of(n.kind);
                facts.routing_id =
                    crate::subagent::routing_id(n.name.as_deref(), n.team_name.as_deref())
                        .filter(|_| n.kind == crate::subagent::SubagentKind::Teammate);
                facts.depth = Some(n.depth);
                facts.parent_agent_id = n.parent_agent_id.clone();
            }
            // A subagent transcript with no discoverable node: still a lane, just not
            // classifiable beyond "a subagent this session owns".
            None => facts.kind = ReceiverKind::UnnamedSubagent,
        }
        facts.state = lane_state(&shape, path);
    } else {
        facts.depth = Some(0);
        facts.state = session_state(reg.as_ref());
    }
    Ok(facts)
}

/// The on-disk subagent kind as the policy table names receivers.
pub(crate) fn receiver_kind_of(kind: crate::subagent::SubagentKind) -> ReceiverKind {
    match kind {
        crate::subagent::SubagentKind::Teammate => ReceiverKind::Teammate,
        crate::subagent::SubagentKind::Workflow => ReceiverKind::WorkflowLane,
        crate::subagent::SubagentKind::BuiltinTask => ReceiverKind::UnnamedSubagent,
    }
}

/// A subagent lane's state from its own tail. A lane is done when its tail ended cleanly AND
/// nothing it spawned is still running: a parent waiting on a child still has hook points.
fn lane_state(shape: &TailShape, path: &Path) -> ReceiverState {
    if shape.records_seen == 0 {
        return ReceiverState::Unknown;
    }
    if shape.unreturned_use.is_some() {
        return ReceiverState::Frozen;
    }
    let clean = shape.last_stop_reason.as_deref() == Some("end_turn");
    let live_children = children_report(path, &HashSet::new())
        .map(|r| r.live_count)
        .unwrap_or(0);
    if clean && live_children == 0 {
        ReceiverState::Completed
    } else {
        ReceiverState::Running
    }
}

/// A top-level lane's state: the registry row plus a pid probe, the only surface that knows a
/// session is alive. No row is UNKNOWN, never a claim of liveness either way.
fn session_state(reg: Option<&RegistryFacts>) -> ReceiverState {
    let Some(reg) = reg else {
        return ReceiverState::Unknown;
    };
    let Some(pid) = reg.pid else {
        return ReceiverState::Unknown;
    };
    match probe_pid(pid, reg.proc_start.as_deref(), reg.pid_domain.as_deref()) {
        PidLiveness::Alive { .. } => ReceiverState::Running,
        PidLiveness::Dead | PidLiveness::Reused => ReceiverState::Dead,
        PidLiveness::ForeignDomain(_) | PidLiveness::Unavailable => ReceiverState::Unknown,
    }
}

/// The Claude Code version and first cwd from the head of a transcript.
pub(crate) fn head_facts(path: &Path) -> Result<(Option<String>, Option<String>)> {
    let mut version = None;
    let mut cwd = None;
    crate::parse::head_records(path, |rec| {
        if version.is_none() {
            version.clone_from(&rec.version);
        }
        if cwd.is_none() {
            cwd.clone_from(&rec.cwd);
        }
        version.is_none() || cwd.is_none()
    })
    .with_context(|| format!("reading the head of {}", path.display()))?;
    Ok((version, cwd))
}

/// Build the three sections for one resolved lane.
pub(crate) fn build_sections(r: &LaneRef) -> Result<Sections> {
    let me = lane_facts(&r.path)?;
    let subtree_ids = subtree_ids(&me);
    let subtree = live_child_lanes(&me.session_path, subtree_ids.as_ref());
    let parent = parent_facts(&me)?;
    let mine: HashSet<&str> = subtree
        .iter()
        .map(|l| l.lane.as_str())
        .chain(std::iter::once(me.lane.as_str()))
        .collect();
    let others = live_lane_census()
        .unwrap_or_default()
        .iter()
        .filter(|p| !mine.contains(p.lane.as_str()))
        .count();
    Ok(Sections {
        me,
        exact: r.exact,
        via: r.via,
        parent,
        subtree,
        others,
    })
}

/// The agent ids under this lane. `None` for a top-level lane: every subagent of the session is
/// in its subtree, so no filter applies.
fn subtree_ids(me: &LaneFacts) -> Option<HashSet<String>> {
    if me.kind == ReceiverKind::TopLevel {
        return None;
    }
    let nodes = crate::subagent::build_topology(&me.session_path, false).unwrap_or_default();
    let mut ids: HashSet<String> = HashSet::new();
    // Walk the parent links outward until nothing new joins: the flat layout means a
    // grandchild's link reaches this lane only through its own parent, and a malformed cycle
    // terminates because a fixpoint loop only ever adds ids.
    loop {
        let before = ids.len();
        for n in &nodes {
            let Some(parent) = n.parent_agent_id.as_deref() else {
                continue;
            };
            if parent == me.lane || ids.contains(parent) {
                ids.insert(n.agent_id.clone());
            }
        }
        if ids.len() == before {
            break;
        }
    }
    Some(ids)
}

/// The live child lanes of one session, optionally restricted to one subtree.
pub(crate) fn live_child_lanes(
    session_path: &Path,
    only: Option<&HashSet<String>>,
) -> Vec<LaneRow> {
    let Ok(report) = children_report(session_path, &HashSet::new()) else {
        return Vec::new();
    };
    let subs = crate::subagent::discover_subagents(session_path).unwrap_or_default();
    report
        .children
        .iter()
        .filter(|c| c.state != "settled")
        .filter(|c| only.is_none_or(|set| set.contains(&c.session_id)))
        .map(|c| {
            let sub = subs.iter().find(|s| s.agent_id == c.session_id);
            LaneRow {
                lane: c.session_id.clone(),
                kind: sub.map_or(ReceiverKind::UnnamedSubagent, |s| receiver_kind_of(s.kind)),
                state: c.state.to_string(),
                last_activity_utc: sub
                    .and_then(|s| tail_shape(&s.path).ok())
                    .and_then(|t| t.last_ts_utc),
            }
        })
        .collect()
}

/// The parent lane and the channel a reply would take. A top-level lane has none.
fn parent_facts(me: &LaneFacts) -> Result<Option<ParentFacts>> {
    if me.kind == ReceiverKind::TopLevel {
        return Ok(None);
    }
    let path = match me.parent_agent_id.as_deref() {
        // The spawning agent, when the topology recovered one.
        Some(agent) => crate::subagent::subagent_transcript_files(&me.session_path)
            .unwrap_or_default()
            .into_iter()
            .find(|p| crate::subagent::session_id_from_path(p) == agent),
        None => None,
    };
    // No spawning agent (or none locatable) means the session's own main conversation is the
    // lane above this one - the honest reading of a flat layout with no recorded link.
    let parent = lane_facts(path.as_deref().unwrap_or(&me.session_path))?;
    let alive = match parent.state {
        ReceiverState::Running | ReceiverState::Frozen => Some(true),
        ReceiverState::Completed | ReceiverState::Dead | ReceiverState::StoppedByUser => {
            Some(false)
        }
        ReceiverState::Unknown => None,
    };
    Ok(Some(ParentFacts {
        lane: parent.lane.clone(),
        kind: parent.kind,
        alive,
        reply_channel: reply_channel(me, &parent),
    }))
}

/// Which channel a reply from this lane to its parent would take, decided by the same policy
/// table `csift send` runs. Only the CHANNEL is reported: the verdict also weighs the message's
/// size and the receiver's armed slots, which is what `whoami --to` and `send` are for.
fn reply_channel(me: &LaneFacts, parent: &LaneFacts) -> &'static str {
    let merged = crate::path::claude_home()
        .map(|home| settings::merged(&home, me.cwd.as_deref().map(Path::new)))
        .unwrap_or_else(|_| settings::merged(Path::new(""), None));
    let slots = ReachSlots::read(&merged);
    let caller = Caller {
        kind: SenderKind::Lane,
        session: Some(me.session.clone()),
        lane: Some(me.lane.clone()),
        label: None,
        lane_exact: true,
    };
    let ctx = send_context(
        &caller,
        &target_facts(parent),
        &merged,
        &slots,
        armed_for(parent).len(),
        Relation::Child,
    );
    policy::decide(&ctx).channel
}

/// The slots that have actually RUN in a lane (its armed marker).
pub(crate) fn armed_for(lane: &LaneFacts) -> Vec<u32> {
    let root = channel_dir(&lane.session_path.with_extension(""));
    read_armed(&root, &lane.lane)
        .ok()
        .flatten()
        .map(|m| m.slots_seen.iter().copied().collect())
        .unwrap_or_default()
}
