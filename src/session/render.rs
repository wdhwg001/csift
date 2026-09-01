//! Text + JSON projections.

use super::*;

pub(crate) fn render_text(
    summaries: &[SessionSummary],
    dropped: usize,
    scope_top: usize,
    scope_sub: usize,
) {
    if summaries.is_empty() {
        println!("no sessions found");
        return;
    }
    // SCOPE banner: `list` spans subagents by DEFAULT, so a bare `csift list <uuid>` can
    // return 1 top-level + N subagent rows - surface that split up front (mirroring
    // `turns --subagents` + now `files`/`search`/`recover`) so the default-span
    // surprise is announced, not buried. Printed only when the resolved set actually spans
    // ≥1 subagent. ONE shared emitter / wording across every spanning surface. The counts
    // are the PRE-CAP scope (the row flood-guard never shrinks them - R7 §2.4).
    crate::text::emit_scope_banner(scope_top, scope_sub);
    for (i, s) in summaries.iter().enumerate() {
        if i > 0 {
            println!();
        }
        // Brand a SUBAGENT row distinctly (mirroring `search`'s header + `turns`'
        // `(subagent transcript)` annotation): a subagent hex is NOT a re-feedable `@<uuid>`
        // target, so label it as such and surface the re-feedable parent uuid inline.
        if s.is_subagent {
            println!(
                "SUBAGENT  {}  ·  parent SESSION {}",
                s.session_id, s.parent_session_id
            );
        } else {
            println!("SESSION  {}", s.session_id);
        }

        // cwd + (branch, version) on one meta line.
        let cwd = s.cwd.as_deref().unwrap_or("<unknown cwd>");
        let mut meta = String::new();
        if let Some(b) = &s.git_branch {
            // Mid-session drift renders as first->last; a stable value stays bare.
            match &s.git_branch_first {
                Some(f) if f != b => meta.push_str(&format!("branch {f}->{b}")),
                _ => meta.push_str(&format!("branch {b}")),
            }
        }
        if let Some(v) = &s.version {
            if !meta.is_empty() {
                meta.push_str(", ");
            }
            match &s.version_first {
                Some(f) if f != v => meta.push_str(&format!("CC {f}->{v}")),
                _ => meta.push_str(&format!("CC {v}")),
            }
        }
        if meta.is_empty() {
            println!("  cwd      {cwd}");
        } else {
            println!("  cwd      {cwd}   ({meta})");
        }

        if let Some(bu) = &s.clone_boundary_uuid {
            match &s.clone_of {
                Some(origin) => {
                    println!("  clone    forked from SESSION {origin} at compaction boundary {bu}")
                }
                None => println!(
                    "  clone    forked at compaction boundary {bu} (origin not in this \
                     project dir)"
                ),
            }
        }
        print_preview("first ◂", s.first_user.as_ref());
        print_preview("last ◂ ", s.last_user.as_ref());
        print_preview("last ▸ ", s.last_agent.as_ref());

        // Currently-pending elicitation(s) merged from the sidecar (§3.10) - the session is
        // blocked on a human; this is its LATEST activity, missing from the native transcript.
        if !s.pending_elicitations.is_empty() {
            println!("  pending  with elicitation sidecar");
            for p in &s.pending_elicitations {
                println!(
                    "           ⏳ {}",
                    crate::text::truncate_excerpt(p, EXCERPT_MAX)
                );
            }
        }

        if s.skipped_lines > 0 {
            // R12: scope-qualify - `list` reads head/tail windows only, so its count is a
            // window census, not a whole-file verdict (that is `stats`, a full scan).
            println!(
                "  note     {} (among the head/tail lines read — full census: csift stats)",
                crate::text::malformed_note(s.skipped_lines)
            );
        }
    }
    // Context-safety cap (never silent): report the dropped rows + how to see more.
    if dropped > 0 {
        println!();
        println!(
            "… (+{dropped} more session(s) not shown — the most recently active are listed; \
             narrow with a target or --since, or raise --max-count)"
        );
    }
}

pub(crate) fn print_preview(label: &str, preview: Option<&MessagePreview>) {
    match preview {
        Some(p) => {
            println!(
                "  {label}  {}",
                format_timestamp(p.timestamp_utc.as_deref())
            );
            println!("           {}", p.excerpt);
        }
        None => {
            println!("  {label}  —");
        }
    }
}

// ── JSON rendering (deterministic, one object per session) ──

pub(crate) fn render_json(
    summaries: &[SessionSummary],
    dropped: usize,
    scope_top: usize,
    scope_sub: usize,
) -> Result<()> {
    use serde_json::json;
    // envelope v2: header (always) → kind-tagged session rows → summary (always).
    // Header scope = the PRE-CAP resolved range (the flood-guard caps ROWS, never the
    // scope numbers - R7 §2.4); the summary's `sessions` stays the emitted-row count.
    println!(
        "{}",
        serde_json::to_string(&crate::text::envelope_scope_header(
            "list",
            scope_top,
            scope_sub,
            json!({})
        ))?
    );
    let mut skipped_total = 0usize;
    for s in summaries {
        skipped_total += s.skipped_lines;
        let obj = json!({
            "kind": "session",
            "session_id": s.session_id,
            // Id-domain discriminator (the r5 shape): `is_subagent` flags a bare-hex
            // subagent row; `parent_session_id` is the always-re-feedable owning uuid
            // (= session_id for a top-level row). Never re-feed a subagent `session_id`.
            "is_subagent": s.is_subagent,
            "parent_session_id": s.parent_session_id,
            "path": s.path.to_string_lossy(),
            "cwd": s.cwd,
            // `version`/`git_branch` are LAST-seen (what the session is on now);
            // the *_first/*_last pairs make the window explicit (_last == the base).
            "version": s.version,
            "version_first": s.version_first,
            "version_last": s.version,
            "git_branch": s.git_branch,
            "git_branch_first": s.git_branch_first,
            "git_branch_last": s.git_branch,
            "first_user": preview_json(s.first_user.as_ref()),
            "last_user": preview_json(s.last_user.as_ref()),
            "last_agent": preview_json(s.last_agent.as_ref()),
            "skipped_lines": s.skipped_lines,
            // The tri-state: `sidecar_present` = the sidecar FILE exists (hook installed -
            // resolved pairs stay in the file), so present+no-pending genuinely means "not
            // blocked on an elicitation", while absent means "hook unknown - cannot conclude".
            "sidecar_present": s.sidecar_present,
            // Unresolved-pending elicitations merged from the sidecar (§3.10): the one-line
            // renders + a `with_elicitation_sidecar` flag (the machine echo of the text note).
            // Empty / false for a session with no pending and for every subagent row.
            "pending_elicitations": s.pending_elicitations,
            "with_elicitation_sidecar": !s.pending_elicitations.is_empty(),
            // C-19 clone lineage: a transcript whose first timestamped record is a
            // compaction boundary was minted by copying another session there. Note
            // the corollary for every spanning surface: a clone DOUBLE-COUNTS its
            // inherited records until scoped away.
            "is_clone": s.clone_boundary_uuid.is_some(),
            "clone_of": s.clone_of,
            "clone_boundary_uuid": s.clone_boundary_uuid,
        });
        println!("{}", serde_json::to_string(&obj)?);
    }
    let summary = crate::text::envelope_summary(json!({
        "sessions": summaries.len(),
        "skipped_lines": skipped_total,
        // Context-safety cap: rows dropped to bound an unscoped flood (0 = nothing capped).
        // The kept rows are the most recently active; raise --max-count / narrow scope for more.
        "dropped_by_cap": dropped,
    }));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

pub(crate) fn preview_json(preview: Option<&MessagePreview>) -> serde_json::Value {
    use serde_json::json;
    match preview {
        None => serde_json::Value::Null,
        Some(p) => {
            let ts_local = p.timestamp_utc.as_deref().and_then(local_iso);
            json!({
                "ts_utc": p.timestamp_utc,
                "ts_local": ts_local,
                "excerpt": p.excerpt,
            })
        }
    }
}
