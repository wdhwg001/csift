//! The registry read, the live-lane census, and the answer for a caller that is not a lane.
//!
//! LIVENESS RULE, stated because it bounds the census: a lane is live when the session that
//! owns it has a registry row whose owner process answers a pid probe, and, for a child lane,
//! its own tail is not settled. A child of a dead session cannot be live, so the census walks
//! only the sessions the registry says are running - which is also what keeps it cheap enough
//! for `whoami` to count peers on every run.
//!
//! ANTI-COLLUSION, the reason this file publishes so little: a peer row carries an id, a kind
//! and a state and nothing else. A description, an agent type, or a name read as a role is
//! exactly the material one lane uses to claim standing over another, and the census exists to
//! answer who is alive, not who should be obeyed.

use super::*;

/// The registry fields the channel asks for. The shared row reader answers liveness; these two
/// extras (an official cross-session arm, and whether the receiver is a headless run) are
/// channel questions, so they are read here from the same file rather than widening that row.
#[derive(Debug, Clone, Default)]
pub(crate) struct RegistryFacts {
    pub(crate) session: String,
    pub(crate) pid: Option<u32>,
    pub(crate) proc_start: Option<String>,
    pub(crate) pid_domain: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) socket_present: bool,
    pub(crate) headless: bool,
}

/// One live lane anywhere under the projects root.
#[derive(Debug, Clone)]
pub(crate) struct PeerRow {
    pub(crate) lane: String,
    pub(crate) kind: ReceiverKind,
    pub(crate) state: String,
    pub(crate) session: String,
    pub(crate) last_activity_utc: Option<String>,
}

/// Every registry row, parsed tolerantly: a mid-write or unreadable row is skipped, never fatal.
pub(crate) fn all_registry_rows() -> Result<Vec<RegistryFacts>> {
    let dir = crate::path::claude_home()?.join("sessions");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)?.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue; // the key files beside the rows are not rows
        }
        let Ok(raw) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let Some(session) = v.get("sessionId").and_then(serde_json::Value::as_str) else {
            continue;
        };
        out.push(RegistryFacts {
            session: session.to_string(),
            pid: v
                .get("pid")
                .and_then(serde_json::Value::as_u64)
                .and_then(|n| u32::try_from(n).ok()),
            proc_start: str_of(&v, "procStart"),
            pid_domain: str_of(&v, "pidDomain"),
            status: str_of(&v, "status"),
            socket_present: v
                .get("messagingSocketPath")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|s| !s.trim().is_empty()),
            headless: v.get("entrypoint").and_then(serde_json::Value::as_str) == Some("sdk-cli"),
        });
    }
    Ok(out)
}

fn str_of(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(serde_json::Value::as_str)
        .map(std::string::ToString::to_string)
}

/// The registry row for one session, if it has one.
pub(crate) fn registry_facts(session_id: &str) -> Result<Option<RegistryFacts>> {
    Ok(all_registry_rows()?
        .into_iter()
        .find(|r| r.session == session_id))
}

/// Every live lane under the projects root.
pub(crate) fn live_lane_census() -> Result<Vec<PeerRow>> {
    let mut out = Vec::new();
    for row in all_registry_rows()? {
        let Some(pid) = row.pid else { continue };
        if !matches!(
            probe_pid(pid, row.proc_start.as_deref(), row.pid_domain.as_deref()),
            PidLiveness::Alive { .. }
        ) {
            continue;
        }
        let Some(path) = locate_session(&row.session) else {
            // A live session whose transcript is not under this projects root (another
            // Claude home, a removed project): it is still a lane, reported by id alone.
            out.push(session_row(&row, None));
            continue;
        };
        out.push(session_row(&row, Some(&path)));
        for child in live_child_lanes(&path, None) {
            out.push(PeerRow {
                lane: child.lane,
                kind: child.kind,
                state: child.state,
                session: row.session.clone(),
                last_activity_utc: child.last_activity_utc,
            });
        }
    }
    Ok(out)
}

/// The top-level lane's own row. Its state is the registry's own word (the closed set
/// `busy | shell | idle | waiting`); a row without one is reported as running, which is what the
/// pid probe just proved and nothing more.
fn session_row(row: &RegistryFacts, path: Option<&Path>) -> PeerRow {
    PeerRow {
        lane: row.session.clone(),
        kind: ReceiverKind::TopLevel,
        state: row.status.clone().unwrap_or_else(|| "running".to_string()),
        session: row.session.clone(),
        last_activity_utc: path
            .and_then(|p| tail_shape(p).ok())
            .and_then(|t| t.last_ts_utc),
    }
}

/// Find `<session>.jsonl` under the projects root.
fn locate_session(session_id: &str) -> Option<PathBuf> {
    let filename = format!("{session_id}.jsonl");
    crate::path::all_project_dirs()
        .ok()?
        .into_iter()
        .find_map(|pd| {
            let candidate = pd.dir.join(&filename);
            candidate.is_file().then_some(candidate)
        })
}

/// `csift whoami --peers`.
pub(crate) fn run_peers(format: OutputFormat) -> Result<()> {
    let rows = live_lane_census()?;
    match format {
        OutputFormat::Text => {
            render_peers_text(&rows);
            Ok(())
        }
        OutputFormat::Json => render_peers_json(&rows),
    }
}
