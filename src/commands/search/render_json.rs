//! The `search --format json` projectors: the per-hit object, the refetch addresses and
//! the envelope stream. Split from `render.rs`, which keeps the TEXT surface (glyphs,
//! labels, headers, banners, footers); `show` borrows `pairing_json` from here for its
//! own flat record rows.

use super::*;

/// The JSON rendering of a tool hit's `▹` pairing state (shared with `show`).
pub(crate) fn pairing_json(p: Option<Pairing>) -> serde_json::Value {
    match p {
        Some(Pairing::Paired) => serde_json::json!("paired"),
        Some(Pairing::PendingNoResult) => serde_json::json!("pending"),
        Some(Pairing::OrphanResult) => serde_json::json!("orphan"),
        None => serde_json::Value::Null,
    }
}

/// Render one `Hit` (a match OR a `--siblings` context record) to its JSON object - the
/// shared per-hit shape used by both the `hits` and `siblings` envelope arrays.
/// The ready-to-run fetch command for a hit - `csift show` addressed at the transcript that
/// OWNS the hit's line number (`session_id`, never the parent uuid: line numbers are
/// per-file, so pointing the parent at a subagent's line would silently fetch the WRONG
/// record). A sidecar hit has no physical line → address by uuid; neither → null.
pub(crate) fn refetch_json(session_id: &str, h: &Hit) -> serde_json::Value {
    if !h.from_sidecar && h.line > 0 {
        serde_json::json!(format!("csift show @{session_id} --line {}", h.line))
    } else if let Some(u) = &h.uuid {
        serde_json::json!(format!("csift show @{session_id} --uuid {u}"))
    } else {
        serde_json::Value::Null
    }
}

/// The uuid-addressed twin of [`refetch_json`] (v0.10.5, ledger REC-093): a physical
/// line number is a durable address only on the append path. Three harness paths
/// rewrite a live transcript in place (a stream tombstone truncates and rewrites the
/// tail, a local compaction rewrite, a remote-ingress resume), and each shifts the
/// lines ABOVE the cut so a stale `Lnnnn` resolves silently to a different record;
/// only an address past the new end misses loudly. The record uuid survives every
/// rewrite, so a consumer that keeps a pointer across a live session refetches by it.
pub(crate) fn refetch_uuid_json(session_id: &str, h: &Hit) -> serde_json::Value {
    match &h.uuid {
        Some(u) => serde_json::json!(format!("csift show @{session_id} --uuid {u}")),
        None => serde_json::Value::Null,
    }
}

/// C-33: the compaction MODE of a boundary/summary hit as its stable slug (`compact` /
/// `summarize-from-here` / `summarize-up-to-here`), else null. Null covers both "not a
/// compaction record" and "the mode is not knowable here" - an unpaired boundary, or a
/// `direction` value csift does not model.
pub(crate) fn compaction_mode_json(h: &Hit) -> serde_json::Value {
    match h
        .compaction
        .as_ref()
        .and_then(|c| c.mode)
        .map(crate::model::SummarizeMode::slug)
    {
        Some(slug) => serde_json::json!(slug),
        None => serde_json::Value::Null,
    }
}

/// C-33: a boundary hit's `compactMetadata` object VERBATIM (the full `preservedMessages` /
/// `preservedSegment` uuid lists the one-line excerpt only counts), else null.
pub(crate) fn compact_metadata_json(h: &Hit) -> serde_json::Value {
    h.compaction
        .as_ref()
        .and_then(|c| c.metadata.clone())
        .unwrap_or(serde_json::Value::Null)
}

pub(crate) fn hit_json(ex: &Exchange, h: &Hit) -> serde_json::Value {
    let session_id: &str = &ex.session_id;
    // Comm direction (GOLD §4): `from`/`to` only for an `agent.communication.*` hit, else null.
    let (from, to) = match &h.direction {
        Some((f, t)) => (serde_json::json!(f), serde_json::json!(t)),
        None => (serde_json::Value::Null, serde_json::Value::Null),
    };
    // Tool pairing (GOLD §7): the `▹` join state of an agent.tool.use/result hit, else null.
    let pairing = pairing_json(h.pair);
    serde_json::json!({
        // The id TRIO rides EVERY hit row too (R9): bare `.hits[]` flattening is the single
        // most natural jq idiom against the most-piped command, and with the trio only on
        // the exchange row it yielded silent nulls - two independent audits tripped on it.
        // jq cannot fail loud on a missing key, so the data matches the natural access
        // pattern instead. (The exchange row keeps its copy; `refetch` stays the preferred
        // single-hit path.)
        "session_id": ex.session_id,
        "is_subagent": ex.is_subagent,
        "parent_session_id": ex.parent_session_id,
        // The matched dotted leaf path (`label`) + the record's FULL label set (`labels`).
        "label": h.class.map(Class::path),
        "labels": h.labels,
        // C-28: did the model RECEIVE this record? The matched leaf's default,
        // overridden per record by Claude Code's own request-assembler drop predicate.
        // `labels` is not the delivery verdict, which is why this rides beside it.
        "delivered": h.delivered(),
        "excerpt": h.excerpt,
        // The same section text the excerpt windows into, with its newlines intact -
        // `excerpt` is a match-centered fragment and collapses every newline to a space,
        // so a consumer reading it as the body lost every paragraph and table row.
        // Non-null ONLY under `--no-truncate`: the default stream stays one fragment per
        // hit rather than a fragment plus a full copy of the record.
        "body": h.body,
        "ts_utc": h.timestamp_utc,
        "ts_local": h.timestamp_utc.as_deref().and_then(local_iso),
        "tool_name": h.tool_name,
        // Comm direction (`agent.communication.*`); null on a non-comm hit.
        "from": from,
        "to": to,
        // Tool-pairing (§7): the use↔result join state + the joining `tool_use_id`; null on a
        // non-tool hit.
        "pairing": pairing,
        "is_error": h.is_error,
        "denial_kind": h.denial_kind,
        "tool_use_id": h.tool_use_id,
        // v0.10.0 queue facts (a `user.queued` hit); null on every other hit.
        "queue_operation": h.queue_operation,
        "queue_reason": h.queue_reason,
        // Every REAL task id this `harness.notification.*` pulse closes (empty array on every
        // other hit) plus, when the pulse is an orphan RECONCILIATION, the kind its
        // `__orphan_summary__:` sentinel names. A reconciliation closes several tasks at once,
        // so the ids are data here as well as text in the label.
        "task_ids": h.task_ids,
        "orphan_kind": h.orphan_kind,
        // v0.12.0: does this `harness.resume.placeholder` close a repair PAIR (its parent is
        // a `harness.resume.prompt` record)? Null on every other hit - one writer produces
        // both forms, so the fact rides the hit instead of splitting the leaf.
        "resume_paired": h.resume_paired,
        // C-31 SURVIVAL AXIS: is this record still in the conversation Claude Code's own
        // chain rule reconstructs, which branch head it hangs off when it is not, and
        // whether the line is an earlier copy of a record a later line carries.
        "survival": h.survival.as_str(),
        "abandoned_root_line": ex.abandoned_root_line,
        "replay_copy_of": h.replay_copy_of,
        // C-33 compaction facts; null on every non-compaction hit. `mode` names the gesture
        // (a plain compact, or one of the two `/rewind` summarize directions) and rides BOTH
        // the boundary and its summary; `compact_metadata` is the boundary's own object,
        // verbatim, uuid lists included.
        "mode": compaction_mode_json(h),
        "compact_metadata": compact_metadata_json(h),
        // v0.12.2: the instant a `harness.schedule.fire` prompt fired, verbatim from its
        // `system`/`scheduled_task_fire` sibling. Null on every other hit, and on a fired
        // prompt whose sibling this transcript does not hold.
        "scheduled_at": h.scheduled_at,
        // The `csift show --line/--uuid` address: 1-based source line + the record uuid (when
        // present). A merged elicitation-sidecar hit has NO physical line, so `line` is null and
        // `source:"elicitation-sidecar"` marks the provenance (§3.10); a native hit omits `source`.
        "line": if h.from_sidecar { serde_json::Value::Null } else { serde_json::json!(h.line) },
        "uuid": h.uuid,
        "source": if h.from_sidecar { serde_json::json!("elicitation-sidecar") } else { serde_json::Value::Null },
        // Extractable image ids (`#N`/`L<line>i<n>`) the record carries; empty array when none.
        "image_ids": h.image_ids,
        // The ready-to-run `csift show` command for this record - already addressed at the
        // RIGHT transcript (this row's session_id; a parent uuid + a subagent line number
        // fetches the wrong record).
        "refetch": refetch_json(session_id, h),
        "refetch_uuid": refetch_uuid_json(session_id, h),
    })
}

pub(crate) fn render_json(
    outcome: &SearchOutcome,
    diagnosis: Option<&EmptyDiagnosis>,
) -> Result<()> {
    use serde_json::json;
    // envelope v2: header (always) → kind-tagged exchange rows → summary (always).
    println!(
        "{}",
        serde_json::to_string(&crate::text::envelope_scope_header(
            "search",
            outcome.scope_top,
            outcome.scope_sub,
            json!({})
        ))?
    );
    for ex in &outcome.exchanges {
        let hits: Vec<_> = ex.hits.iter().map(|h| hit_json(ex, h)).collect();
        let mut obj = json!({
            "kind": "exchange",
            "session_id": ex.session_id,
            // Discriminate the id-domain so a consumer can tell a re-feedable parent UUID
            // from a non-re-feedable subagent transcript hex: `is_subagent` + the always-
            // re-feedable `parent_session_id` (= session_id for a top-level hit).
            "is_subagent": ex.is_subagent,
            "parent_session_id": ex.parent_session_id,
            "turn_index": ex.turn_index,
            "superseded_draft": ex.superseded_draft,
            // Envelope-level chronological position = the turn-opening timestamp, the key
            // the combined timeline is sorted on. `ts_local` is the same instant in the
            // host TZ. Per-hit `ts_utc` (in `hits`) can diverge for a deep tool_use match.
            "ts_utc": ex.started_utc,
            "ts_local": ex.started_utc.as_deref().and_then(local_iso),
            "hits": hits,
            "record_uuids": ex.record_uuids,
        });
        // C-27: on a draft unit, the address of the message that replaced it and the
        // distance between the two texts.
        if let Some(d) = &ex.draft_diff {
            d.attach_json(&mut obj);
        }
        // `--siblings`: attach the non-matched records of the turn (same per-hit shape).
        // Present only when there are siblings - absent ⇒ none (keeps the common envelope lean).
        if !ex.siblings.is_empty() || ex.siblings_hidden > 0 {
            let sibs: Vec<_> = ex.siblings.iter().map(|h| hit_json(ex, h)).collect();
            obj["siblings"] = json!(sibs);
            obj["siblings_hidden"] = json!(ex.siblings_hidden);
            obj["turn_lines"] = json!([ex.turn_lines.0, ex.turn_lines.1]);
        }
        println!("{}", serde_json::to_string(&obj)?);
    }
    // envelope v2 summary. `session_ids` = the distinct matching transcript ids (sorted,
    // first-100 capped with an EXPLICIT truncation flag - never silent) so "WHICH sessions
    // matched" is one `tail -1 | jq .session_ids` away, no per-row jq pipeline.
    let mut session_ids: Vec<&str> = outcome
        .exchanges
        .iter()
        .map(|ex| ex.session_id.as_str())
        .collect();
    session_ids.sort_unstable();
    session_ids.dedup();
    let ids_total = session_ids.len();
    let ids_truncated = ids_total > 100;
    session_ids.truncate(100);
    let mut summary_fields = json!({
        "matched": outcome.exchanges.len(),
        "sessions": distinct_session_count(&outcome.exchanges),
        // `transcript_ids` = the distinct MATCHING-TRANSCRIPT ids (a subagent hit contributes
        // its bare agent-id, a top-level hit its uuid). DELIBERATELY named apart from `-l`,
        // which emits the OWNING-session ids (`parent_session_id`) - the two answer different
        // "which sessions?" questions, so the wire names them differently.
        "transcript_ids": session_ids,
        "transcript_ids_truncated": ids_truncated,
        "dropped_by_cap": outcome.dropped_by_cap,
        "skipped_lines": outcome.skipped_lines,
        // C-18 + C-31: what the SURVIVAL AXIS took out of turn numbering. Real records,
        // outside numbering, fetchable by explicit address.
        "superseded_drafts": outcome.chain.drafts,
        "abandoned_records": outcome.chain.abandoned_records,
        "rewound_turns": outcome.chain.rewound_turns,
        "replay_copies": outcome.chain.replay_copies,
        "boundary_cut_line": outcome.chain.boundary_cut_line,
        "leaf_source": outcome.chain.leaf_source,
        // True when ≥1 emitted record was merged from the elicitation sidecar (§3.10) - the
        // machine echo of the `with elicitation sidecar` text note.
        "with_elicitation_sidecar": merged_any_sidecar(&outcome.exchanges),
        // True when ≥1 emitted excerpt was CLIPPED to the default cap - the machine echo of the
        // trailing reader-caution. A consumer seeing this should re-fetch the record in full
        // (per-hit `excerpt` is a match-centered fragment, not the whole text) via
        // `--no-truncate`, or one record via `csift show --line/--uuid`. Always false there.
        "excerpts_truncated": any_truncated_excerpt(&outcome.exchanges),
    });
    // Zero-match self-diagnosis (§T0.1): make the empty result machine-legible as a definitive
    // absence (never a syntax error), echo the active filters, and - when a `-t`/`-T` filter hid
    // otherwise-matching records - carry the excluded labels so a consumer can self-correct.
    if let Some(d) = diagnosis {
        summary_fields["definitive_absence"] = json!(true);
        summary_fields["active_filters"] = json!(d.active_filters);
        summary_fields["gated_leaves_unreached"] = json!(d.gated_unreached);
        summary_fields["excluded_by_label"] = match &d.excluded_by_label {
            Some((rows, recs)) => {
                let by: serde_json::Map<String, serde_json::Value> =
                    rows.iter().map(|(l, n)| (l.clone(), json!(n))).collect();
                json!({ "records": recs, "by_label": serde_json::Value::Object(by) })
            }
            None => serde_json::Value::Null,
        };
    }
    let summary = crate::text::envelope_summary(summary_fields);
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
