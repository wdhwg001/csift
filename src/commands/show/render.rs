//! Text + JSON projections.
//!
//! Both go through ONE [`crate::text::Out`]: `show --turn ..` with `--max-count 0`
//! addresses a whole transcript, and a row at a time through `println!` is a syscall a
//! row. See the `text::out` module docs for what holding the lock obliges.
// The `text::out` obligation, mechanically: a stray print on this thread jumps the
// buffer's queue and one inside the parallel render closure hangs the process, so a
// print outside an `Out` is an error here unless it says why (see `text::out`).
#![deny(clippy::print_stdout, clippy::print_stderr)]

use std::io::Write as _;

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
    crossing: &crate::model::CrossingReport,
) -> Result<()> {
    let mut out = crate::text::Out::stdout();
    if is_subagent {
        writeln!(
            out,
            "SUBAGENT {session_id} · parent SESSION {parent_session_id}"
        )?;
    } else {
        writeln!(out, "SESSION {session_id}")?;
    }
    let mut units = 0usize;
    let mut image_hint_done = false;
    for ex in exchanges {
        writeln!(out)?;
        if ex.superseded_draft {
            // An abandoned record sits OUTSIDE turn numbering - name what it is, never
            // fabricate a t<N>.
            writeln!(
                out,
                "{}  {}",
                crate::commands::search::abandoned_tag(ex),
                format_local_compact(ex.started_utc.as_deref())
            )?;
            // The distance from the message that replaced it, so a fetched draft is
            // never read as the message the model received.
            if let Some(d) = &ex.draft_diff {
                writeln!(out, "  {}", d.text_line_for(ex.abandoned_kind))?;
            }
        } else {
            writeln!(
                out,
                "t{}  {}",
                ex.turn_index.unwrap_or_default(),
                format_local_compact(ex.started_utc.as_deref())
            )?;
        }
        for h in &ex.hits {
            write_record_line(&mut out, role_glyph(h.class), h)?;
            if !image_hint_done {
                if let Some(l) = crate::commands::search::image_hint_line(session_id, &h.image_ids)
                {
                    writeln!(out, "{l}")?;
                    image_hint_done = true;
                }
            }
            units += 1;
        }
    }
    writeln!(out)?;
    writeln!(out, "fetched {units} record unit(s)")?;
    if dropped > 0 {
        match remainder_cmd {
            Some(cmd) => writeln!(
                out,
                "+{dropped} more record unit(s) beyond the {cap}-unit cap · continue: {cmd}  \
                 (or --max-count 0 = uncapped)"
            )?,
            None => writeln!(
                out,
                "+{dropped} more record unit(s) beyond the {cap}-unit cap — pass \
                 --max-count 0 (uncapped)"
            )?,
        }
    }
    if non_record_lines > 0 {
        writeln!(
            out,
            "{non_record_lines} line(s) in the addressed range are not records \
             (metadata/attachment — inspect with --raw)"
        )?;
    }
    if merged_any_sidecar(exchanges) {
        writeln!(out, "with elicitation sidecar")?;
    }
    // An ADDRESS moves with the crossing budget - the record it names reads `live` at one
    // budget and `pre-cut` at another - so `show` discloses what the walk crossed to decide
    // that, in the same two lines every other survival-printing surface prints.
    for line in crossing.lines() {
        writeln!(out, "{line}")?;
    }
    if skipped > 0 {
        writeln!(out, "({})", crate::text::malformed_note(skipped))?;
    }
    out.flush()?;
    Ok(())
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
    crossing: &crate::model::CrossingReport,
) -> Result<()> {
    use serde_json::json;
    let mut out = crate::text::Out::stdout();
    let header = json!({
        "kind": "header",
        "command": "show",
        "session_id": session_id,
        "is_subagent": is_subagent,
        "parent_session_id": parent_session_id,
        "path": file.display().to_string(),
    });
    writeln!(out, "{}", serde_json::to_string(&header)?)?;
    let mut units = 0usize;
    for ex in exchanges {
        for h in &ex.hits {
            units += 1;
            let (from, to) = match &h.direction {
                Some((f, t)) => (json!(f), json!(t)),
                None => (serde_json::Value::Null, serde_json::Value::Null),
            };
            // On a draft row, the superseding message's address and the distance
            // between the two texts; null on every other row.
            let (sup_line, sup_uuid, diff_chars, diff_pct, diff_exact) = match &ex.draft_diff {
                Some(d) => (
                    json!(d.superseding_line),
                    json!(d.superseding_uuid),
                    json!(d.chars),
                    d.pct_json(),
                    json!(d.exact),
                ),
                None => (
                    serde_json::Value::Null,
                    serde_json::Value::Null,
                    serde_json::Value::Null,
                    serde_json::Value::Null,
                    serde_json::Value::Null,
                ),
            };
            let mut row = json!({
                "kind": "record",
                "session_id": session_id,
                "is_subagent": is_subagent,
                "parent_session_id": parent_session_id,
                // A superseded draft has NO turn index (outside numbering) - null,
                // never a fabricated number; the flag names why.
                "turn_index": ex.turn_index,
                "superseded_draft": ex.superseded_draft,
                // The SURVIVAL AXIS verdict, per record.
                "survival": h.survival.as_str(),
                "abandoned_root_line": ex.abandoned_root_line,
                "replay_copy_of": h.replay_copy_of,
                "superseding_line": sup_line,
                "superseding_uuid": sup_uuid,
                "diff_chars": diff_chars,
                "diff_pct": diff_pct,
                "diff_exact": diff_exact,
                // A merged elicitation-sidecar record has no physical line (null).
                "line": if h.from_sidecar { serde_json::Value::Null } else { json!(h.line) },
                "uuid": h.uuid,
                "label": h.class.map(crate::model::Class::path),
                "labels": h.labels,
                "tool_name": h.tool_name,
                "from": from,
                "to": to,
                "pairing": crate::commands::search::pairing_json(h.pair),
                "tool_use_id": h.tool_use_id,
                // The queue facts a `queue-operation` line carries, on the surface a
                // reader reaches by running a hit's own `refetch` command: a `user.queued`
                // hit named its event and its removal reason under `search` and answered
                // null here for the same record, which is the one thing a refetch must not
                // do. Null on every other line type, as it is there.
                "queue_operation": h.queue_operation,
                "queue_reason": h.queue_reason,
                "source": if h.from_sidecar { json!("elicitation-sidecar") } else { serde_json::Value::Null },
                "ts_utc": h.timestamp_utc,
                "ts_local": h.timestamp_utc.as_deref().and_then(local_iso),
                "text": h.excerpt,
                // The same record text with its newlines intact (`text` collapses them to
                // spaces). Non-null under a `--line`/`--uuid` address, which lifts the
                // excerpt cap; NULL under `--turn`, which does not - there `text` is a
                // capped fragment and the body would be a fragment too.
                "body": h.body,
                "image_ids": h.image_ids,
                // The compaction facts of a boundary/summary record; null elsewhere.
                "mode": crate::commands::search::compaction_mode_json(h),
                "compact_metadata": crate::commands::search::compact_metadata_json(h),
                // The provenance ROOT, the same shape a `search` hit carries: which
                // message this record corresponds to. A refetch must answer what the hit it
                // was copied from answered.
                "for_root": crate::commands::search::for_root_json(&h.for_root),
            });
            // The EXTENSION POINT, same law as `search`'s hit rows: every `HitFact`
            // projects its own key(s), so a fact a family adds reaches `show` with no
            // edit here. Appended after the fixed keys, only when the hit carries a
            // fact, and folded by the SAME `attach_facts` rule the hit rows use.
            crate::commands::search::attach_facts(&mut row, &h.facts);
            writeln!(out, "{}", serde_json::to_string(&row)?)?;
        }
    }
    let mut summary = json!({
        "kind": "summary",
        "records": units,
        "dropped_by_cap": dropped,
        "refetch_remainder": remainder_cmd,
        "non_record_lines": non_record_lines,
        "skipped_lines": skipped,
        "with_elicitation_sidecar": merged_any_sidecar(exchanges),
    });
    // The same six keys the scanning surfaces carry, and for the same reason: a machine
    // reader of an ADDRESS needs to know which budget produced the `survival` it just read.
    for (k, v) in crossing.json_fields() {
        summary[k] = v;
    }
    writeln!(out, "{}", serde_json::to_string(&summary)?)?;
    out.flush()?;
    Ok(())
}
