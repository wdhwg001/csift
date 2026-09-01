//! Text + JSON projections (summary / dir / file / timeline).

use super::*;

pub(crate) fn render_text(outcome: &Outcome) {
    // SCOPE banner FIRST (before the empty check) so a fan-out that touched no files still
    // announces it spanned N subagents - the same up-front disclosure `list`/`turns` give.
    crate::text::emit_scope_banner(outcome.scope_top, outcome.scope_sub);
    if outcome.mutations.is_empty() && outcome.boundaries.is_empty() {
        println!("no file mutations found");
        print_footer(outcome);
        return;
    }

    if !outcome.mutations.is_empty() {
        match outcome.detail {
            FilesDetail::Summary => render_summary(outcome),
            FilesDetail::ByDir => render_by_dir(outcome),
            FilesDetail::ByFile => render_by_file(outcome),
            FilesDetail::Timeline => render_timeline(outcome),
        }
    }
    render_boundaries_section(outcome);
    print_footer(outcome);
}

/// The Edit-before-Read boundary section - orthogonal to the mutation rollup, shown in every
/// detail mode (and on its own when a session ONLY hit boundaries, no mutations). Each row
/// carries the file, the jsonl line, turn, time, and kind so it joins back to the transcript
/// and feeds `recover --file <path> --coverage` for the precise per-boundary breakdown.
pub(crate) fn render_boundaries_section(outcome: &Outcome) {
    if outcome.boundaries.is_empty() {
        return;
    }
    println!();
    println!(
        "── Edit-before-Read boundaries ({}) — file changed OUTSIDE the tool stream (formatter / \
         git / external edit); recover with care ──",
        outcome.boundaries.len()
    );
    for b in &outcome.boundaries {
        let sub = if b.is_subagent {
            format!(
                "  ·  subagent {} (parent {})",
                b.session_id, b.parent_session_id
            )
        } else {
            String::new()
        };
        println!(
            "  ⚠ {}  ·  L{}  ·  turn {}  ·  {}  ·  {}{sub}",
            b.path,
            b.line_no,
            b.turn_index,
            format_timestamp(b.timestamp_utc.as_deref()),
            b.kind
        );
    }
}

/// Group mutations under their session header, then call `body` per session with that
/// session's mutations. Sessions render in sorted id order for determinism. A SUBAGENT
/// group's header is branded `SUBAGENT <hex> · parent SESSION <uuid>` (mirroring `search`'s
/// header + `turns`' `(subagent transcript)` annotation) so a consumer never reads a bare
/// subagent hex as a re-feedable `@<uuid>` target. All mutations in one group share the
/// same id-domain (same transcript), so the first row's flags brand the whole header.
pub(crate) fn per_session<F: Fn(&str, &[&TaggedMutation])>(outcome: &Outcome, body: F) {
    let mut by_session: BTreeMap<&str, Vec<&TaggedMutation>> = BTreeMap::new();
    for m in &outcome.mutations {
        by_session.entry(m.session_id.as_str()).or_default().push(m);
    }
    let mut first = true;
    for (sid, ms) in by_session {
        if !first {
            println!();
        }
        first = false;
        match ms.first() {
            Some(m) if m.is_subagent => {
                println!("SUBAGENT {sid}  ·  parent SESSION {}", m.parent_session_id);
            }
            _ => println!("SESSION {sid}"),
        }
        body(sid, &ms);
    }
}

pub(crate) fn render_summary(outcome: &Outcome) {
    per_session(outcome, |_sid, ms| {
        let owned: Vec<TaggedMutation> = ms.iter().map(|m| (*m).clone()).collect();
        let buckets = group_by(&owned, |m| bucket_key(&m.path));
        for (bucket, counts) in &buckets {
            println!("  {bucket}: {}", counts.ops_label());
        }
    });
}

pub(crate) fn render_by_dir(outcome: &Outcome) {
    per_session(outcome, |_sid, ms| {
        let owned: Vec<TaggedMutation> = ms.iter().map(|m| (*m).clone()).collect();
        let dirs = group_by(&owned, |m| {
            parent_dir(&m.path).unwrap_or_else(|| "./".to_string())
        });
        for (dir, counts) in &dirs {
            println!("  {dir}");
            println!(
                "    {}  ·  {} file(s)",
                counts.ops_label(),
                counts.files.len()
            );
            println!(
                "    first  {}",
                format_timestamp(counts.first_ts.as_deref())
            );
            println!("    last   {}", format_timestamp(counts.last_ts.as_deref()));
        }
    });
}

pub(crate) fn render_by_file(outcome: &Outcome) {
    per_session(outcome, |_sid, ms| {
        let owned: Vec<TaggedMutation> = ms.iter().map(|m| (*m).clone()).collect();
        let files = group_by(&owned, |m| m.path.clone());
        for (file, counts) in &files {
            println!("  {file}");
            println!("    {}", counts.ops_label());
            println!(
                "    first  {}",
                format_timestamp(counts.first_ts.as_deref())
            );
            println!("    last   {}", format_timestamp(counts.last_ts.as_deref()));
        }
    });
}

pub(crate) fn render_timeline(outcome: &Outcome) {
    per_session(outcome, |_sid, ms| {
        // Sort chronologically by timestamp (None last), then by original file order.
        let mut owned: Vec<&TaggedMutation> = ms.to_vec();
        owned.sort_by(|a, b| {
            timestamp_sort_key(a.mutation.timestamp_utc.as_deref())
                .cmp(&timestamp_sort_key(b.mutation.timestamp_utc.as_deref()))
        });
        for m in owned {
            let heuristic = if m.mutation.op.is_heuristic() {
                " (heuristic)"
            } else {
                ""
            };
            let detail = m
                .mutation
                .detail
                .as_deref()
                .map(|d| format!("  ({d})"))
                .unwrap_or_default();
            let errored = if m.mutation.command_errored {
                " (command errored)"
            } else {
                ""
            };
            println!(
                "  L{}  {}  turn {}  {}{}  {}{}{detail}",
                m.line_no,
                format_timestamp(m.mutation.timestamp_utc.as_deref()),
                m.turn_index,
                m.mutation.op.label(),
                heuristic,
                m.mutation.path,
                errored
            );
        }
    });
}

/// Sort key that places timestamp-less mutations LAST (after all timestamped ones) and
/// orders timestamped ones chronologically (ISO8601 sorts as text).
pub(crate) fn timestamp_sort_key(ts: Option<&str>) -> (bool, String) {
    match ts {
        Some(t) => (false, t.to_string()),
        None => (true, String::new()),
    }
}

pub(crate) fn print_footer(outcome: &Outcome) {
    let level = match outcome.detail {
        FilesDetail::Summary => "summary",
        FilesDetail::ByDir => "by-dir",
        FilesDetail::ByFile => "by-file",
        FilesDetail::Timeline => "timeline",
    };
    let filter = filter_context(outcome);
    println!();
    println!(
        "{} distinct file(s)  ·  {} mutation(s)  ·  {} Edit-before-Read boundary(ies)  ·  detail={level}  ·  {filter}",
        outcome.distinct_files(),
        outcome.mutations.len(),
        outcome.boundaries.len()
    );
    println!("(Bash mutations are heuristic — parsed from the command string.)");
    if outcome.skipped_lines > 0 {
        println!("({})", crate::text::malformed_note(outcome.skipped_lines));
    }
}

/// A short description of the active turn/time filter for the footer.
pub(crate) fn filter_context(outcome: &Outcome) -> String {
    if let Some(s) = &outcome.turn_range {
        format!("turn={s}")
    } else if outcome.time_window_bounded {
        "time-window".to_string()
    } else {
        "all turns".to_string()
    }
}

pub(crate) fn render_json(outcome: &Outcome) -> Result<()> {
    use serde_json::json;
    // envelope v2: header (always) → kind-tagged rows → summary (always).
    println!(
        "{}",
        serde_json::to_string(&crate::text::envelope_scope_header(
            "files",
            outcome.scope_top,
            outcome.scope_sub,
            json!({})
        ))?
    );
    match outcome.detail {
        FilesDetail::Summary => json_grouped(outcome, |m| bucket_key(&m.path), "bucket")?,
        FilesDetail::ByDir => json_grouped(
            outcome,
            |m| parent_dir(&m.path).unwrap_or_else(|| "./".to_string()),
            "dir",
        )?,
        FilesDetail::ByFile => json_grouped(outcome, |m| m.path.clone(), "file")?,
        FilesDetail::Timeline => {
            for m in &outcome.mutations {
                let obj = json!({
                    "kind": "mutation",
                    "session_id": m.session_id,
                    // Discriminate the id-domain: `is_subagent` + the always-re-feedable
                    // `parent_session_id` (= session_id for a top-level mutation) so a
                    // consumer can `csift verbatim <parent_session_id>` even on a subagent row.
                    "is_subagent": m.is_subagent,
                    "parent_session_id": m.parent_session_id,
                    "path": m.mutation.path,
                    // UNDERSCORE-delimited op token (json_key, NOT the hyphenated text label)
                    // so the timeline `op` spelling matches the grouped per-op COUNT keys
                    // (`notebook_edit`/`multi_edit`) - one on-wire spelling across both modes.
                    "op": m.mutation.op.json_key(),
                    "ts_utc": m.mutation.timestamp_utc,
                    "ts_local": m.mutation.timestamp_utc.as_deref().and_then(local_iso),
                    "turn_index": m.turn_index,
                    "line": m.line_no,
                    "is_create": m.mutation.is_create,
                    "heuristic": m.mutation.op.is_heuristic(),
                    // Bash-resolution provenance: `resolution` names how `path` was
                    // obtained (absolute / cwd-joined / cd-tracked / unresolved; null
                    // for structured tools and class markers), `path_verbatim` keeps
                    // the typed spelling when it differs, and `command_errored` flags
                    // a mutation kept from a partially-failed bash chain.
                    "resolution": m.mutation.resolution,
                    // Snapshot-instrument provenance on an external_write row (null
                    // on every tool-extracted mutation).
                    "detail": m.mutation.detail,
                    "path_verbatim": m.mutation.path_verbatim,
                    "command_errored": m.mutation.command_errored,
                });
                println!("{}", serde_json::to_string(&obj)?);
            }
        }
    }

    // Edit-before-Read boundary objects (orthogonal to the mutation rollup; emitted in every
    // detail mode so the recipe can `jq` them out of any `files --format json` run). Each carries
    // the id-domain discriminators + the jsonl line, so it joins back to the transcript and feeds
    // `recover --file <path> --coverage` for the precise per-boundary breakdown.
    for b in &outcome.boundaries {
        let obj = json!({
            "kind": "boundary",
            "session_id": b.session_id,
            "is_subagent": b.is_subagent,
            "parent_session_id": b.parent_session_id,
            "path": b.path,
            "line": b.line_no,
            "turn_index": b.turn_index,
            // WHAT changed the file out of band (formatter/git/external-editor/…) -
            // named `cause` so `kind` stays the envelope discriminator exclusively.
            "cause": b.kind,
            "ts_utc": b.timestamp_utc,
            "ts_local": b.timestamp_utc.as_deref().and_then(local_iso),
        });
        println!("{}", serde_json::to_string(&obj)?);
    }
    // envelope v2 summary. `detail_level` values equal the `--by` flag values verbatim.
    // `sessions` = distinct OWNING sessions among the emitted mutations/boundaries -
    // the summary core trio (`sessions`/`skipped_lines`[/`dropped_by_cap`]) every other
    // spanning command carries; files has no cap, so `dropped_by_cap` is deliberately
    // absent rather than a constant 0 implying one exists.
    let sessions: std::collections::BTreeSet<&str> = outcome
        .mutations
        .iter()
        .map(|m| m.parent_session_id.as_str())
        .chain(
            outcome
                .boundaries
                .iter()
                .map(|b| b.parent_session_id.as_str()),
        )
        .collect();
    let summary = crate::text::envelope_summary(json!({
        "sessions": sessions.len(),
        "distinct_files": outcome.distinct_files(),
        "total_mutations": outcome.mutations.len(),
        "edit_before_read_boundaries": outcome.boundaries.len(),
        "skipped_lines": outcome.skipped_lines,
        "detail_level": match outcome.detail {
            FilesDetail::Summary => "summary",
            FilesDetail::ByDir => "dir",
            FilesDetail::ByFile => "file",
            FilesDetail::Timeline => "timeline",
        },
    }));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

/// Emit one JSON object per group (bucket / dir / file), keyed per session.
pub(crate) fn json_grouped<F: Fn(&FileMutation) -> String>(
    outcome: &Outcome,
    key: F,
    row_kind: &str,
) -> Result<()> {
    use serde_json::json;
    // session_id → key → counts (deterministic order via BTreeMap).
    let mut by_session: BTreeMap<&str, Vec<&TaggedMutation>> = BTreeMap::new();
    for m in &outcome.mutations {
        by_session.entry(m.session_id.as_str()).or_default().push(m);
    }
    for (sid, ms) in by_session {
        // All mutations in this group share the id-domain (same transcript); the discriminator
        // (`is_subagent` + the re-feedable `parent_session_id`) brands every grouped row, the
        // SAME r5 shape the --timeline arm carries - so a grouped subagent row is now
        // distinguishable + re-feedable, not a bare hex on `session_id` alone.
        let (is_subagent, parent_session_id) = ms
            .first()
            .map(|m| (m.is_subagent, m.parent_session_id.clone()))
            .unwrap_or((false, sid.to_string()));
        let owned: Vec<TaggedMutation> = ms.iter().map(|m| (*m).clone()).collect();
        let groups = group_by(&owned, &key);
        for (k, counts) in &groups {
            let obj = json!({
                "kind": row_kind,
                "session_id": sid,
                "is_subagent": is_subagent,
                "parent_session_id": parent_session_id,
                // The grouping key is ALWAYS `path` (a bucket prefix / a dir / a file) -
                // one on-wire key across every `--by` mode, discriminated by `kind`.
                "path": k,
                "write": counts.write,
                "edit": counts.edit,
                "notebook_edit": counts.notebook_edit,
                "multi_edit": counts.multi_edit,
                "bash": counts.bash,
                "external_write": counts.external_write,
                "total": counts.total(),
                "distinct_files": counts.files.len(),
                "first_utc": counts.first_ts,
                "first_local": counts.first_ts.as_deref().and_then(local_iso),
                "last_utc": counts.last_ts,
                "last_local": counts.last_ts.as_deref().and_then(local_iso),
            });
            println!("{}", serde_json::to_string(&obj)?);
        }
    }
    Ok(())
}

/// Parse a `--turn` token into a [`RangeSpec`] (the shared grammar), resolved per-file
/// against each transcript's own turn count (0-based).
pub(crate) fn parse_turn_range(s: &str) -> Result<crate::text::RangeSpec> {
    crate::text::parse_range_spec(s, "--turn", false)
}
