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

#[derive(Debug, Clone)]
pub(crate) struct TaskRow {
    pub(crate) id: String,
    pub(crate) subject: String,
    pub(crate) status: String,
    pub(crate) blocked_by: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TasksReport {
    /// Every non-completed task, in_progress first, then numeric-id order.
    pub(crate) open: Vec<TaskRow>,
    pub(crate) completed: usize,
    /// A tasks directory existed for this session (an absent dir means the session
    /// never used the task tools - no section, not an error).
    pub(crate) found: bool,
}

/// Read the task list for `owner_id` (a top-level session uuid). Missing dirs and
/// malformed files degrade silently to absence - this is advisory live state.
pub(crate) fn tasks_report(owner_id: &str) -> TasksReport {
    let mut report = TasksReport::default();
    let Ok(home) = crate::path::claude_home() else {
        return report;
    };
    let tasks_root = home.join("tasks");
    let mut dirs = vec![tasks_root.join(owner_id)];
    if let Some(prefix) = owner_id.get(..8) {
        dirs.push(tasks_root.join(format!("session-{prefix}")));
    }
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        report.found = true;
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
    // in_progress leads (the "what is being pushed right now" answer), then numeric id.
    report.open.sort_by(|a, b| {
        let rank = |t: &TaskRow| usize::from(t.status != "in_progress");
        (rank(a), numeric_id(&a.id)).cmp(&(rank(b), numeric_id(&b.id)))
    });
    report
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
