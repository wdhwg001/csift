//! The two endpoints of a send: the caller csift is running as, and the receiver lane it
//! resolved.
//!
//! CALLER. Claude Code exports `CLAUDE_CODE_SESSION_ID` into every Bash child, in every lane,
//! and it always names the TOP-LEVEL session - a subagent's own id is withheld. So the
//! variable answers "am I inside Claude Code" definitively and "which lane am I" only
//! approximately, which is exactly how it is reported: with no `--from`, the send is
//! attributed to the top-level session and says so. Outside Claude Code the caller is
//! EXTERNAL and carries a free label the operator chose; csift never fills that from the
//! environment, because a username in a message header is a privacy leak the sender did not
//! ask for.
//!
//! GATES. The two feature gates that decide whether an official transport exists at all are
//! read here too, because their inputs are the same endpoint facts: a settings env value with
//! the scope that set it, the team directories on disk, and the receiver's own teammate lanes.
//! A gate csift cannot see enabled is reported UNKNOWN with that evidence, never assumed off.
//!
//! RECEIVER. Everything the policy table needs about the receiving lane, read once: its kind
//! (from the on-disk shape and, for a teammate, its meta), its state (registry row plus pid
//! probe for a top-level session, the transcript tail for a lane), its Claude Code version and
//! first cwd (the head of its transcript), and the harness's own terminal word about it (a
//! background-agent launch whose completion notification says killed or stopped).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::policy::{ReceiverKind, ReceiverState};
use super::{is_lane_id, SenderKind};
use crate::live::{
    background_report, children_report, probe_pid, registry_row_for, tail_shape, BackgroundLens,
    BgKind, BgState, PidLiveness,
};

/// The environment variable Claude Code exports into a lane's Bash children. Read directly
/// rather than through the shared `@main` resolver: that one also honours a Codex companion
/// variable, and a Codex session is not a Claude Code lane - it holds no hook points, so
/// classifying it as one would promise a delivery nothing can make.
const SESSION_ENV: &str = "CLAUDE_CODE_SESSION_ID";

/// The label an external caller gets when it names none.
const DEFAULT_EXTERNAL_LABEL: &str = "unknown";

/// Who is running this send.
#[derive(Debug, Clone)]
pub(crate) struct Caller {
    pub(crate) kind: SenderKind,
    /// The top-level session uuid (a Claude Code lane only).
    pub(crate) session: Option<String>,
    /// The lane the message is attributed to: `--from @<lane>` when given, else the session.
    pub(crate) lane: Option<String>,
    pub(crate) label: Option<String>,
    /// False when the lane was ASSUMED to be the top-level session because no `--from` named
    /// it. The receipt says so rather than presenting the assumption as a fact.
    pub(crate) lane_exact: bool,
}

/// Classify the caller from the environment and `--from`.
pub(crate) fn classify(from: Option<&str>) -> Result<Caller> {
    classify_with(
        from,
        std::env::var(SESSION_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty()),
    )
}

/// [`classify`] with the session variable supplied. The environment read is split off so the
/// rule can be tested without mutating a process-global the whole test binary shares.
pub(crate) fn classify_with(from: Option<&str>, session: Option<String>) -> Result<Caller> {
    match session {
        Some(session) => classify_lane(from, session),
        None => classify_external(from),
    }
}

fn classify_lane(from: Option<&str>, session: String) -> Result<Caller> {
    let (lane, exact) = match from {
        None => (session.clone(), false),
        Some(raw) => {
            let Some(id) = raw.strip_prefix('@') else {
                bail!(
                    "--from `{raw}`: inside Claude Code the sender is a LANE, so --from takes \
                     the `@<lane>` form (`@main`, or the `a...` id `csift agents` prints). A \
                     bare label is the external-caller form and would misattribute a real lane."
                );
            };
            if id == "main" {
                (session.clone(), true)
            } else if is_lane_id(id) {
                (id.to_string(), true)
            } else {
                bail!(
                    "--from `@{id}` is not a lane id: a lane is a top-level session uuid, a bare \
                     `a<16 hex>` agent id, or a teammate id `a<Name>-<16 hex>` - exactly what \
                     `csift agents` prints. `@main` names the calling top-level session."
                );
            }
        }
    };
    Ok(Caller {
        kind: SenderKind::Lane,
        session: Some(session),
        lane: Some(lane),
        label: None,
        lane_exact: exact,
    })
}

fn classify_external(from: Option<&str>) -> Result<Caller> {
    let label = match from {
        None => DEFAULT_EXTERNAL_LABEL.to_string(),
        Some(raw) if raw.starts_with('@') => bail!(
            "--from `{raw}`: outside Claude Code there is no lane to claim, so --from is a free \
             LABEL for the receipt (`--from ci-runner`), never an `@<lane>` id. csift cannot \
             verify a lane claim from a process it did not spawn."
        ),
        Some(raw) if raw.trim().is_empty() => DEFAULT_EXTERNAL_LABEL.to_string(),
        Some(raw) => raw.to_string(),
    };
    Ok(Caller {
        kind: SenderKind::External,
        session: None,
        lane: None,
        label: Some(label),
        lane_exact: true,
    })
}

/// The line printed on stderr when the lane was assumed. Kept beside the classifier so the
/// wording and the assumption cannot drift apart.
pub(crate) const LANE_ASSUMED_NOTE: &str =
    "csift: lane unknown, sending as the top-level session; pass --from @<your lane id> for an \
     exact sender (a subagent's own id is withheld from its environment - `csift whoami \
     @trap:<marker>` recovers it).";

/// One gate's verdict in the settings-model grammar: a gate is either enabled by a value csift
/// can READ, or unknown - because the shell environment and the CLI flags that also enable it
/// leave no trace on disk. An unknown gate is reported with the evidence that bears on it,
/// never resolved by assumption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GateVerdict {
    pub(crate) name: &'static str,
    pub(crate) verdict: String,
    pub(crate) enabled: bool,
}

impl GateVerdict {
    /// The teams gate. `scope` is the settings scope that set the env key, when one did;
    /// otherwise the two evidence counts a reader can weigh instead.
    pub(crate) fn teams(scope: Option<&str>, teams_dirs: usize, teammate_lanes: usize) -> Self {
        match scope {
            Some(s) => GateVerdict {
                name: "teams",
                verdict: format!("enabled via settings env ({s})"),
                enabled: true,
            },
            None => GateVerdict {
                name: "teams",
                verdict: format!(
                    "no settings-level enable; shell env and CLI flags are not observable -> \
                     unknown; use evidence: teams directories {teams_dirs}, teammate lanes \
                     {teammate_lanes}"
                ),
                enabled: false,
            },
        }
    }

    /// The harbor (cross-session messaging) gate. The registry's socket path is the one
    /// on-disk consequence of the gate being on AND bound, so it is the verdict.
    pub(crate) fn harbor(socket_present: bool) -> Self {
        GateVerdict {
            name: "harbor",
            verdict: if socket_present {
                "registry messagingSocketPath present -> on and bound".to_string()
            } else {
                "registry messagingSocketPath absent -> unknown".to_string()
            },
            enabled: socket_present,
        }
    }
}

/// Team directories under the Claude Code home: the evidence half of the teams gate verdict
/// when no settings scope enables it.
pub(crate) fn teams_dirs() -> usize {
    let Ok(home) = crate::path::claude_home() else {
        return 0;
    };
    std::fs::read_dir(home.join("teams"))
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).count())
        .unwrap_or(0)
}

/// Everything one send needs to know about the receiving lane.
#[derive(Debug, Clone)]
pub(crate) struct Receiver {
    /// The transcript-form lane id (a session uuid, or the agent id for a subagent lane).
    pub(crate) lane: String,
    /// The owning top-level session uuid (== `lane` for a top-level receiver).
    pub(crate) session: String,
    /// The top-level transcript of that session: where the sidecar directory hangs.
    pub(crate) session_path: PathBuf,
    pub(crate) kind: ReceiverKind,
    pub(crate) state: ReceiverState,
    pub(crate) version: Option<String>,
    /// The first `cwd` the transcript records: the receiver's project root, and therefore the
    /// root its project and local settings scopes are read from.
    pub(crate) cwd: Option<String>,
    pub(crate) routing_id: Option<String>,
    pub(crate) socket_present: bool,
    pub(crate) headless: bool,
    /// Teammate lanes in the receiver's session: the evidence half of the teams gate verdict.
    pub(crate) teammate_lanes: usize,
}

/// Read the receiver's facts off disk.
pub(crate) fn probe_receiver(path: &Path) -> Result<Receiver> {
    let lane = crate::subagent::session_id_from_path(path);
    let is_sub = crate::subagent::is_subagent_path(path);
    let session_path = session_transcript_for(path);
    let session = crate::subagent::session_id_from_path(&session_path);
    let (version, cwd) = head_facts(path)?;

    let (kind, routing_id, teammate_lanes) = classify_receiver(&session_path, &lane, is_sub)?;
    let row = if is_sub {
        None
    } else {
        registry_row_for(&session)?
    };
    let extras = if is_sub {
        RegistryExtras::default()
    } else {
        registry_extras(&session)?
    };
    let state = receiver_state(path, &session_path, &lane, is_sub, row.as_ref())?;

    Ok(Receiver {
        lane,
        session,
        session_path,
        kind,
        state,
        version,
        cwd,
        routing_id,
        socket_present: extras.socket_present,
        headless: extras.headless,
        teammate_lanes,
    })
}

/// The top-level transcript that owns a lane: the lane itself for a top-level session, else
/// the session file beside the `subagents/` directory the lane sits under.
pub(crate) fn session_transcript_for(path: &Path) -> PathBuf {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d.file_name().and_then(|n| n.to_str()) == Some("subagents") {
            if let Some(session_dir) = d.parent() {
                return session_dir.with_extension("jsonl");
            }
        }
        dir = d.parent();
    }
    path.to_path_buf()
}

/// The agent id that SPAWNED each of two lanes of one session, from ONE topology build.
///
/// The on-disk layout is flat - every subagent of a session sits in the same directory - so
/// the spawn link exists nowhere but the reconstructed topology, which prefers the harness's
/// own `parentAgentId` meta field and falls back to the tool_use spawn graph. Both lanes are
/// answered together because a caller asking "did either of these spawn the other" would
/// otherwise rebuild the same topology twice. `None` is UNKNOWN, not "no parent": an
/// unbuildable topology, a lane with no discoverable node, or a parent the harness never
/// recorded all land here, and a caller must keep its unresolved reading instead of guessing
/// an ancestry.
pub(crate) fn parent_agents_of(
    session_path: &Path,
    first: &str,
    second: &str,
) -> (Option<String>, Option<String>) {
    let Ok(nodes) = crate::subagent::build_topology(session_path, false) else {
        return (None, None);
    };
    let parent_of = |lane: &str| {
        nodes
            .iter()
            .find(|n| n.agent_id == lane)
            .and_then(|n| n.parent_agent_id.clone())
    };
    (parent_of(first), parent_of(second))
}

/// The receiver's kind, its routing form when it is a teammate, and how many teammate lanes
/// the session holds. All three come from ONE meta walk, which is also the only place a
/// teammate can be told apart from a built-in subagent (they share an on-disk location).
fn classify_receiver(
    session_path: &Path,
    lane: &str,
    is_sub: bool,
) -> Result<(ReceiverKind, Option<String>, usize)> {
    if !is_sub {
        return Ok((ReceiverKind::TopLevel, None, 0));
    }
    let subs = crate::subagent::discover_subagents(session_path).unwrap_or_default();
    let teammate_lanes = subs
        .iter()
        .filter(|s| s.kind == crate::subagent::SubagentKind::Teammate)
        .count();
    let Some(me) = subs.iter().find(|s| s.agent_id == lane) else {
        // A subagent transcript with no discoverable meta: still a lane, still deliverable,
        // just not classifiable beyond "not a teammate we can name".
        return Ok((ReceiverKind::UnnamedSubagent, None, teammate_lanes));
    };
    let kind = match me.kind {
        crate::subagent::SubagentKind::Teammate => ReceiverKind::Teammate,
        crate::subagent::SubagentKind::Workflow => ReceiverKind::WorkflowLane,
        crate::subagent::SubagentKind::BuiltinTask => ReceiverKind::UnnamedSubagent,
    };
    let routing =
        crate::subagent::routing_id(me.name.as_deref(), me.team_name.as_deref()).filter(|_| {
            matches!(kind, ReceiverKind::Teammate) // only a teammate has an official routing form
        });
    Ok((kind, routing, teammate_lanes))
}

/// The receiver's Claude Code version and first cwd, from the head of its transcript.
fn head_facts(path: &Path) -> Result<(Option<String>, Option<String>)> {
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

/// What the lane is doing. A top-level session answers from the registry plus a pid probe (the
/// only surface that knows a session is alive); a subagent lane answers from its own tail,
/// with the harness's terminal word about it outranking both.
fn receiver_state(
    path: &Path,
    session_path: &Path,
    lane: &str,
    is_sub: bool,
    row: Option<&crate::live::RegistryRow>,
) -> Result<ReceiverState> {
    if stopped_by_user(session_path, lane)? {
        return Ok(ReceiverState::StoppedByUser);
    }
    let shape = tail_shape(path)?;
    if shape.records_seen == 0 {
        return Ok(ReceiverState::Unknown);
    }
    if shape.unreturned_use.is_some() {
        return Ok(ReceiverState::Frozen);
    }
    if is_sub {
        // A lane is done when its own tail ended cleanly AND nothing it spawned is still
        // running: a parent that is waiting on a child has hook points left.
        let clean = shape.last_stop_reason.as_deref() == Some("end_turn");
        let children = children_report(path, &HashSet::new())
            .map(|r| r.live_count)
            .unwrap_or(0);
        return Ok(if clean && children == 0 {
            ReceiverState::Completed
        } else {
            ReceiverState::Running
        });
    }
    let Some(row) = row else {
        // No registry row: an old build, a non-interactive session, or one that has exited.
        // Never a claim of liveness either way.
        return Ok(ReceiverState::Unknown);
    };
    let Some(pid) = row.pid else {
        return Ok(ReceiverState::Unknown);
    };
    Ok(
        match probe_pid(pid, row.proc_start.as_deref(), row.pid_domain.as_deref()) {
            PidLiveness::Alive { .. } => ReceiverState::Running,
            PidLiveness::Dead | PidLiveness::Reused => ReceiverState::Dead,
            PidLiveness::ForeignDomain(_) | PidLiveness::Unavailable => ReceiverState::Unknown,
        },
    )
}

/// True when the harness's own record says this lane was killed or stopped. The instrument is
/// the background scan over the owning session: an async agent launch carries the lane's id,
/// and its completion notification carries the terminal status. The plain-text agents-stopped
/// notice names no id, so it cannot answer this and is deliberately not consulted.
fn stopped_by_user(session_path: &Path, lane: &str) -> Result<bool> {
    if !session_path.is_file() {
        return Ok(false);
    }
    let lens = BackgroundLens::from_args(None, &[])?;
    let report = background_report(session_path, false, &lens)?;
    Ok(report.tasks.iter().any(|t| {
        t.kind == BgKind::Agent
            && t.id.as_deref() == Some(lane)
            && matches!(t.state, BgState::Killed | BgState::Stopped)
    }))
}

/// The two registry fields the shared row does not model.
#[derive(Debug, Clone, Default)]
struct RegistryExtras {
    socket_present: bool,
    headless: bool,
}

/// Read `messagingSocketPath` and `entrypoint` for one session.
///
/// They are read here rather than by widening the shared registry row: that row is the
/// liveness surface the live-truth commands share, and these two fields answer a question only
/// the channel asks - whether an official cross-session arm exists at all, and whether the
/// receiver is a headless run csift must never promise delivery to.
fn registry_extras(session_id: &str) -> Result<RegistryExtras> {
    let dir = crate::path::claude_home()?.join("sessions");
    if !dir.is_dir() {
        return Ok(RegistryExtras::default());
    }
    for entry in std::fs::read_dir(&dir)?.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        if v.get("sessionId").and_then(serde_json::Value::as_str) != Some(session_id) {
            continue;
        }
        let socket = v
            .get("messagingSocketPath")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        let headless = v.get("entrypoint").and_then(serde_json::Value::as_str) == Some("sdk-cli");
        return Ok(RegistryExtras {
            socket_present: socket,
            headless,
        });
    }
    Ok(RegistryExtras::default())
}
