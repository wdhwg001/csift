//! Text + JSON projections for status/wait.

use super::*;
use serde_json::json;

pub(crate) fn render_status_text(session_id: &str, a: &Assessment) {
    println!("STATUS  {session_id}");
    println!("verdict  {}", a.verdict.slug());
    println!();
    for e in &a.evidence {
        let age = e
            .age_secs
            .map(|s| format!("  ({s}s ago)"))
            .unwrap_or_default();
        println!("  {:<9} {}{age}", e.surface, e.value);
    }
    let (live, settled): (Vec<_>, Vec<_>) = a.children.iter().partition(|c| c.state != "settled");
    for c in live {
        println!("  child     {}  {}  {}", c.session_id, c.state, c.detail);
    }
    if !settled.is_empty() {
        println!(
            "  child     {} settled lane(s) folded (ids: csift agents @{session_id})",
            settled.len()
        );
    }
    // Text shows the section only when there is at least one task (an existing but
    // empty dir stays JSON-visible as tasks:[] versus null).
    if a.tasks.found && (!a.tasks.open.is_empty() || a.tasks.completed > 0) {
        for t in &a.tasks.open {
            let blocked = if t.blocked_by.is_empty() {
                String::new()
            } else {
                format!("  (blocked by #{})", t.blocked_by.join(", #"))
            };
            println!(
                "  task      #{} {}  {}{blocked}",
                t.id,
                t.status,
                crate::text::collapse_and_truncate(&t.subject, 200)
            );
        }
        println!(
            "  tasks     {} open ; {} completed",
            a.tasks.open.len(),
            a.tasks.completed
        );
    }
    for n in &a.notes {
        println!("  note: {n}");
    }
}

pub(crate) fn render_status_json(
    session_id: &str,
    is_subagent: bool,
    parent_session_id: &str,
    a: &Assessment,
) -> Result<()> {
    let header = crate::text::envelope_header(
        "status",
        json!({
            "session_id": session_id,
            "is_subagent": is_subagent,
            "parent_session_id": parent_session_id,
        }),
    );
    println!("{}", serde_json::to_string(&header)?);
    let row = json!({
        "kind": "verdict",
        "verdict": a.verdict.slug(),
        "evidence": a.evidence.iter().map(|e| json!({
            "surface": e.surface,
            "value": e.value,
            "age_secs": e.age_secs,
        })).collect::<Vec<_>>(),
        "children": a.children.iter().filter(|c| c.state != "settled").map(|c| json!({
            "session_id": c.session_id,
            "state": c.state,
            "detail": c.detail,
        })).collect::<Vec<_>>(),
        "settled_children": a.children.iter().filter(|c| c.state == "settled").count(),
        "tasks": if a.tasks.found {
            json!(a.tasks.open.iter().map(|t| json!({
                "id": t.id,
                "subject": t.subject,
                "status": t.status,
                "blocked_by": t.blocked_by,
            })).collect::<Vec<_>>())
        } else {
            serde_json::Value::Null
        },
        "tasks_completed": if a.tasks.found { json!(a.tasks.completed) } else { serde_json::Value::Null },
        "pending": a.pending,
        "notes": a.notes,
    });
    println!("{}", serde_json::to_string(&row)?);
    let summary = crate::text::envelope_summary(json!({"verdict": a.verdict.slug()}));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
