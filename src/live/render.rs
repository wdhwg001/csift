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
    render_background_text(&a.background);
    render_last_text(session_id, &a.last);
    for n in a.notes.iter().chain(a.background.notes.iter()) {
        println!("  note: {n}");
    }
}

/// The background section: every OPEN task (counted first, then ignored), one row each;
/// closed ones are folded into the evidence row's counts.
pub(crate) fn render_background_text(b: &BackgroundReport) {
    for t in b.tasks.iter().filter(|t| t.is_open()) {
        let launched = t
            .launched_utc
            .as_deref()
            .map(|ts| {
                let age = age_secs(Some(ts))
                    .map(|s| {
                        format!(
                            " ({} ago)",
                            crate::text::fmt_secs(u64::try_from(s).unwrap_or(0))
                        )
                    })
                    .unwrap_or_default();
                format!("launched {}{age}", crate::timez::format_timestamp(Some(ts)))
            })
            .unwrap_or_else(|| "launched (no timestamp)".to_string());
        let what = t
            .description
            .as_deref()
            .or(t.command.as_deref())
            .map(|d| format!("  \"{}\"", crate::text::collapse_and_truncate(d, 80)))
            .unwrap_or_default();
        let output = match (t.output_bytes, t.output_age_secs) {
            (Some(bytes), Some(age)) => format!(
                "  output {} B, last write {} ago",
                bytes,
                crate::text::fmt_secs(u64::try_from(age).unwrap_or(0))
            ),
            (Some(bytes), None) => format!("  output {bytes} B"),
            _ => String::new(),
        };
        let ignored = t
            .ignored_by
            .as_deref()
            .map(|r| format!("  [ignored: {r}]"))
            .unwrap_or_default();
        let lane = if t.lane.len() > 16 {
            String::new()
        } else {
            format!("  lane {}", t.lane)
        };
        println!(
            "  bg        {:<7} {:<18} {launched}{what}{output}{ignored}{lane}",
            t.kind.slug(),
            t.id.as_deref().unwrap_or(&t.tool_use_id)
        );
    }
}

/// The `last` section: newest prompt + newest assistant message, as excerpts, with the
/// refetch and the warning that an excerpt is not a review.
pub(crate) fn render_last_text(session_id: &str, last: &LastMessages) {
    let row = |glyph: &str, m: &LastMsg| {
        let ts = crate::timez::format_timestamp(m.ts_utc.as_deref());
        println!("  last {glyph}    {ts}  {}", m.text);
    };
    if let Some(u) = &last.user {
        row("◂", u);
    }
    if let Some(g) = &last.agent {
        row("▸", g);
    }
    if last.user.is_some() || last.agent.is_some() {
        println!(
            "  note: the last-message excerpts are a partial view of the final state, never a \
             review of the work (whole turn: csift show @{session_id} --turn -1)"
        );
    }
}

pub(crate) fn background_json(b: &BackgroundReport) -> serde_json::Value {
    let (c, f, k, s, t) = b.closed_counts();
    json!({
        "open": b.open_counted(),
        "ignored": b.open_ignored(),
        "completed": c,
        "failed": f,
        "killed": k,
        "stopped": s,
        "timed_out": t,
        "scanned_files": b.scanned_files,
        "tasks": b.tasks.iter().filter(|t| t.is_open()).map(|t| json!({
            "kind": t.kind.slug(),
            "id": t.id,
            "tool_use_id": t.tool_use_id,
            "lane": t.lane,
            "state": t.state.slug(),
            "description": t.description,
            "command": t.command,
            "launched_utc": t.launched_utc,
            "launched_local": t.launched_utc.as_deref().and_then(crate::timez::local_iso),
            "age_secs": age_secs(t.launched_utc.as_deref()),
            "output_file": t.output_file,
            "output_bytes": t.output_bytes,
            "output_age_secs": t.output_age_secs,
            "ignored_by": t.ignored_by,
        })).collect::<Vec<_>>(),
        "notes": b.notes,
    })
}

pub(crate) fn last_json(last: &LastMessages) -> serde_json::Value {
    let one = |m: &Option<LastMsg>| match m {
        Some(m) => json!({
            "ts_utc": m.ts_utc,
            "ts_local": m.ts_utc.as_deref().and_then(crate::timez::local_iso),
            "text": m.text,
            "truncated": m.truncated,
        }),
        None => serde_json::Value::Null,
    };
    json!({ "user": one(&last.user), "agent": one(&last.agent) })
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
        "background": background_json(&a.background),
        "last": last_json(&a.last),
        "tail_state": a.tail_state,
        "notes": a.notes.iter().chain(a.background.notes.iter()).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string(&row)?);
    let summary = crate::text::envelope_summary(json!({"verdict": a.verdict.slug()}));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
