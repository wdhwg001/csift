//! Text + JSON projections.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_text(
    exchanges: &[Exchange],
    session_id: &str,
    is_subagent: bool,
    parent_session_id: &str,
    skipped: usize,
    dropped: usize,
    cap: usize,
    remainder_cmd: Option<&str>,
    non_record_lines: usize,
) {
    if is_subagent {
        println!("SUBAGENT {session_id} · parent SESSION {parent_session_id}");
    } else {
        println!("SESSION {session_id}");
    }
    let mut units = 0usize;
    let mut image_hint_done = false;
    for ex in exchanges {
        println!();
        println!(
            "t{}  {}",
            ex.turn_index,
            format_local_compact(ex.started_utc.as_deref())
        );
        for h in &ex.hits {
            print_record_line(role_glyph(h.class), h);
            if !image_hint_done {
                if let Some(l) = crate::search::image_hint_line(session_id, &h.image_ids) {
                    println!("{l}");
                    image_hint_done = true;
                }
            }
            units += 1;
        }
    }
    println!();
    println!("fetched {units} record unit(s)");
    if dropped > 0 {
        match remainder_cmd {
            Some(cmd) => println!(
                "+{dropped} more record unit(s) beyond the {cap}-unit cap · continue: {cmd}  \
                 (or --max-count 0 = uncapped)"
            ),
            None => println!(
                "+{dropped} more record unit(s) beyond the {cap}-unit cap — pass \
                 --max-count 0 (uncapped)"
            ),
        }
    }
    if non_record_lines > 0 {
        println!(
            "{non_record_lines} line(s) in the addressed range are not records \
             (metadata/attachment — inspect with --raw)"
        );
    }
    if merged_any_sidecar(exchanges) {
        println!("with elicitation sidecar");
    }
    if skipped > 0 {
        println!("({})", crate::text::malformed_note(skipped));
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_json(
    exchanges: &[Exchange],
    file: &std::path::Path,
    session_id: &str,
    is_subagent: bool,
    parent_session_id: &str,
    skipped: usize,
    dropped: usize,
    remainder_cmd: Option<&str>,
    non_record_lines: usize,
) -> Result<()> {
    use serde_json::json;
    let header = json!({
        "kind": "header",
        "command": "show",
        "session_id": session_id,
        "is_subagent": is_subagent,
        "parent_session_id": parent_session_id,
        "path": file.display().to_string(),
    });
    println!("{}", serde_json::to_string(&header)?);
    let mut units = 0usize;
    for ex in exchanges {
        for h in &ex.hits {
            units += 1;
            let (from, to) = match &h.direction {
                Some((f, t)) => (json!(f), json!(t)),
                None => (serde_json::Value::Null, serde_json::Value::Null),
            };
            let row = json!({
                "kind": "record",
                "session_id": session_id,
                "is_subagent": is_subagent,
                "parent_session_id": parent_session_id,
                "turn_index": ex.turn_index,
                // A merged elicitation-sidecar record has no physical line (null).
                "line": if h.from_sidecar { serde_json::Value::Null } else { json!(h.line) },
                "uuid": h.uuid,
                "label": h.class.path(),
                "labels": h.labels,
                "tool_name": h.tool_name,
                "from": from,
                "to": to,
                "pairing": crate::search::pairing_json(h.pair),
                "tool_use_id": h.tool_use_id,
                "source": if h.from_sidecar { json!("elicitation-sidecar") } else { serde_json::Value::Null },
                "ts_utc": h.timestamp_utc,
                "ts_local": h.timestamp_utc.as_deref().and_then(local_iso),
                "text": h.excerpt,
                "image_ids": h.image_ids,
            });
            println!("{}", serde_json::to_string(&row)?);
        }
    }
    let summary = json!({
        "kind": "summary",
        "records": units,
        "dropped_by_cap": dropped,
        "refetch_remainder": remainder_cmd,
        "non_record_lines": non_record_lines,
        "skipped_lines": skipped,
        "with_elicitation_sidecar": merged_any_sidecar(exchanges),
    });
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
