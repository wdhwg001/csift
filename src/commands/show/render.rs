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

/// What an address covered that renders no record: lines that EXIST and are not records, and
/// lines past the file's end a range was clamped off. Two counts because they are two facts - a
/// metadata line is there to fetch with `--raw`, a line past the end is not there at all.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Uncovered {
    pub(crate) non_record: usize,
    pub(crate) past_end: usize,
    /// The file's last line, which the past-the-end disclosure names.
    pub(crate) last_line: usize,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_text(
    exchanges: &[Exchange],
    session_id: &str,
    is_subagent: bool,
    parent_session_id: &str,
    skipped: usize,
    dropped: usize,
    cap: usize,
    remainder: Option<&str>,
    uncovered: Uncovered,
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
        let cont = remainder
            .map(|cmd| format!(" · continue: {cmd}"))
            .unwrap_or_default();
        writeln!(
            out,
            "+{dropped} more record unit(s) beyond the {cap}-unit cap{cont}  (or --max-count 0 \
             = the whole fetch)"
        )?;
    }
    if uncovered.non_record > 0 {
        writeln!(
            out,
            "{} line(s) in the addressed range are not records \
             (metadata/attachment — inspect with --raw)",
            uncovered.non_record
        )?;
    }
    if uncovered.past_end > 0 {
        writeln!(
            out,
            "{}",
            past_end_note(uncovered.past_end, uncovered.last_line)
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
    remainder: Option<&str>,
    uncovered: Uncovered,
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
            // Every key a `search` hit for this record carries, from the ONE builder both
            // surfaces call: this row is what that hit's `refetch` prints, so it has to
            // answer everything the hit answered.
            let mut row = crate::commands::search::record_row_json(h, ex.abandoned_root_line);
            if let Some(map) = row.as_object_mut() {
                for (key, value) in [
                    ("kind", json!("record")),
                    ("session_id", json!(session_id)),
                    ("is_subagent", json!(is_subagent)),
                    ("parent_session_id", json!(parent_session_id)),
                    // A superseded draft has NO turn index (outside numbering) - null,
                    // never a fabricated number; the flag names why.
                    ("turn_index", json!(ex.turn_index)),
                    ("superseded_draft", json!(ex.superseded_draft)),
                    ("superseding_line", sup_line),
                    ("superseding_uuid", sup_uuid),
                    ("diff_chars", diff_chars),
                    ("diff_pct", diff_pct),
                    ("diff_exact", diff_exact),
                    // The record's text, capped under `--turn` and whole under an address;
                    // `body` beside it keeps the newlines `text` collapses.
                    ("text", json!(h.excerpt)),
                ] {
                    map.insert(key.to_string(), value);
                }
            }
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
        "refetch_remainder": remainder,
        "non_record_lines": uncovered.non_record,
        "lines_past_end": uncovered.past_end,
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
