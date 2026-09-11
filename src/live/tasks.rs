//! The harness task list: `<claude-home>/tasks/<owner>/*.json`, read point-in-time.
//!
//! Claude Code's TaskCreate/TaskUpdate tools persist one JSON file per task under a
//! per-session directory. Two directory-name forms exist on real disks (both verified):
//! the full session uuid, and the newer `session-<first 8 uuid chars>` form. Each file
//! carries `{id, subject, description, activeForm, status, blocks, blockedBy}` with
//! string ids. The set of `status` values is OPEN (pending / in_progress / completed
//! observed); anything that is not `completed` renders as an open row with its verbatim
//! status. This is a live-truth read (current values only, no history) - the same
//! carve-out `status` itself lives under.
//!
//! Both forms name the id the PROCESS started with, though, and a `/clear` mints a new
//! session id without touching the store - so the owner's own id is the first candidate
//! here, not the only one. See [`tasks_report_with`] for the chain and what each step
//! discloses.

#[derive(Debug, Clone)]
pub(crate) struct TaskRow {
    pub(crate) id: String,
    pub(crate) subject: String,
    pub(crate) status: String,
    pub(crate) blocked_by: Vec<String>,
}

/// One tasks directory that actually held files, and the candidate that named it.
#[derive(Debug, Clone)]
pub(crate) struct TaskStore {
    pub(crate) dir: String,
    pub(crate) via: &'static str,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TasksReport {
    /// Every non-completed task, in_progress first, then numeric-id order.
    pub(crate) open: Vec<TaskRow>,
    pub(crate) completed: usize,
    /// A tasks directory existed for this session (an absent dir means the session
    /// never used the task tools - no section, not an error).
    pub(crate) found: bool,
    /// The directories that hit, each with the candidate that named it. Disclosed
    /// because a store found through anything but the session's own id was found by
    /// inference, and the reader has to be able to see which one.
    pub(crate) stores: Vec<TaskStore>,
}

/// How far apart a registry row's `startedAt` and a team file's `createdAt` may sit and
/// still be the same process starting up. Measured over every current registry row, the
/// widest real gap was under four seconds and every row matched exactly one team file.
const TEAM_START_WINDOW_MS: i64 = 5000;

/// Read the task list, falling back through the candidates a cleared session needs.
///
/// The store is keyed by the id the PROCESS started with, not by the session id it
/// carries now, so a clear strands the session's own name: `/clear` mints a new session
/// id and never touches the task store, and a `--resume` launch names the store after a
/// startup id no transcript carries at all. The candidates, in order: the owner's own id
/// (both the full-uuid and `session-<first8>` forms, read together as they always were);
/// then the root of the `cleared_from` chain, because a process that started fresh named
/// its store after that first id; then, for a session with a registry row, the team file
/// whose `createdAt` sits within [`TEAM_START_WINDOW_MS`] of the row's `startedAt`, whose
/// `name` IS the list id. A tier is consulted only when the tier above it found nothing,
/// and a directory matching no candidate is never read.
pub(crate) fn tasks_report_with(
    owner_id: &str,
    cleared_root: Option<&str>,
    started_at_ms: Option<i64>,
) -> TasksReport {
    let mut report = TasksReport::default();
    let Ok(home) = crate::path::claude_home() else {
        return report;
    };
    let tasks_root = home.join("tasks");
    let tiers = [
        id_forms(owner_id, "own id"),
        cleared_root
            .filter(|r| *r != owner_id)
            .map(|r| id_forms(r, "cleared_from root"))
            .unwrap_or_default(),
        started_at_ms
            .map(|t| team_candidates(&home, t))
            .unwrap_or_default(),
    ];
    for tier in tiers {
        for (name, via) in tier {
            read_store(&tasks_root.join(&name), &name, via, &mut report);
        }
        if report.found {
            break;
        }
    }
    // in_progress leads (the "what is being pushed right now" answer), then numeric id.
    report.open.sort_by(|a, b| {
        let rank = |t: &TaskRow| usize::from(t.status != "in_progress");
        (rank(a), numeric_id(&a.id)).cmp(&(rank(b), numeric_id(&b.id)))
    });
    report
}

/// Both directory forms one session id can name, in the order they were always read.
fn id_forms(id: &str, via: &'static str) -> Vec<(String, &'static str)> {
    let mut out = vec![(id.to_string(), via)];
    if let Some(prefix) = id.get(..8) {
        out.push((format!("session-{prefix}"), via));
    }
    out
}

/// Every team whose config was written within the window of `started_at_ms`. The team's
/// `name` is the list id; `leadSessionId` is the STARTUP id, which a resumed launch
/// throws away, so it is deliberately not matched against anything.
pub(crate) fn team_candidates(
    home: &std::path::Path,
    started_at_ms: i64,
) -> Vec<(String, &'static str)> {
    let Ok(entries) = std::fs::read_dir(home.join("teams")) else {
        return Vec::new();
    };
    let mut out: Vec<(String, &'static str)> = Vec::new();
    for entry in entries.flatten() {
        let Ok(raw) = std::fs::read_to_string(entry.path().join("config.json")) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let (Some(created), Some(name)) = (
            v.get("createdAt").and_then(serde_json::Value::as_i64),
            v.get("name").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        if (created - started_at_ms).abs() <= TEAM_START_WINDOW_MS {
            out.push((name.to_string(), "team file"));
        }
    }
    out.sort();
    out
}

/// Read one candidate directory into the report. Missing dirs and malformed files
/// degrade silently to absence - this is advisory live state.
fn read_store(dir: &std::path::Path, name: &str, via: &'static str, report: &mut TasksReport) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    report.found = true;
    report.stores.push(TaskStore {
        dir: name.to_string(),
        via,
    });
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let status = v
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("(no status)")
            .to_string();
        if status == "completed" {
            report.completed += 1;
            continue;
        }
        report.open.push(TaskRow {
            id: json_id(v.get("id")),
            subject: v
                .get("subject")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("(no subject)")
                .to_string(),
            status,
            blocked_by: v
                .get("blockedBy")
                .and_then(serde_json::Value::as_array)
                .map(|a| a.iter().map(|x| json_id(Some(x))).collect())
                .unwrap_or_default(),
        });
    }
}

/// Ids are strings on disk ("13") but tolerate a bare number.
fn json_id(v: Option<&serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => "?".to_string(),
    }
}

fn numeric_id(id: &str) -> u64 {
    id.parse().unwrap_or(u64::MAX)
}
