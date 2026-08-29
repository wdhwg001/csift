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
    for c in &a.children {
        println!("  child     {}  {}  {}", c.session_id, c.state, c.detail);
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
        "children": a.children.iter().map(|c| json!({
            "session_id": c.session_id,
            "state": c.state,
            "detail": c.detail,
        })).collect::<Vec<_>>(),
        "pending": a.pending,
        "notes": a.notes,
    });
    println!("{}", serde_json::to_string(&row)?);
    let summary = crate::text::envelope_summary(json!({"verdict": a.verdict.slug()}));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
