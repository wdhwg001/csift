//! run_list: scope walk, flood-guard cap, activity ranking, candidates.

use super::*;

/// Entry point for `csift list`.
pub fn run_list(args: &ListArgs) -> Result<()> {
    // 1+2. Resolve targets → the concrete session jsonl files, via the SAME shared resolver
    //       every other session-operating subcommand uses (`path::resolve_session_files`).
    //       This routes an `@<uuid>` / `@<hex>` POSITIONAL (and a `*.jsonl` file) to the session
    //       filter, so `csift list @<uuid>` identifies that one session instead of erroring - and
    //       0 targets ⇒ every project. Subagent transcripts (built-in Task/Agent-tool + workflow /
    //       OMC agents) span by default; `--no-subagents` keeps only the top-level `<uuid>.jsonl`
    //       set. Workflow `journal.jsonl` event logs are never transcripts and are excluded by the
    //       resolver.
    let scope = SubagentScope::from(args.want_subagents());
    let mut session_files = path::resolve_targets_with_session_list(
        &args.paths,
        args.sessions_from.as_deref(),
        scope,
        path::Caller::Other,
    )?;
    session_files.sort();
    session_files.dedup();

    // 3. Parallel across files (head+tail read each), collect order-stable.
    let mut summaries: Vec<SessionSummary> = session_files
        .par_iter()
        .map(|p| summarize_session(p))
        .collect::<Result<Vec<_>>>()?;
    // Deterministic order regardless of rayon completion order: by path.
    summaries.sort_by(|a, b| a.path.cmp(&b.path));

    // `--since`/`--until`: keep a session iff its [first-activity, last-activity] span
    // intersects the window. The span endpoints are the timestamps this index ALREADY reads
    // (head+tail) - no full-file scan, the `list` performance contract holds; ISO-UTC sorts
    // lexicographically, so min/max over the raw strings is chronological.
    let window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;
    if !window.is_unbounded() {
        summaries.retain(|s| {
            let ts: Vec<&str> = [&s.first_user, &s.last_user, &s.last_agent]
                .into_iter()
                .flatten()
                .filter_map(|p| p.timestamp_utc.as_deref())
                .collect();
            window.intersects_span(ts.iter().min().copied(), ts.iter().max().copied())
        });
    }

    // Context-safety cap (T2.1): an unscoped `csift list` over a large corpus floods the
    // reader's context (the SPEC-noted many-thousand-line hazard). Bound the rows - NEVER
    // silently: the drop is reported with guidance. `--max-count` overrides; with no explicit
    // cap, ALL-projects listing defaults to 50 while a scoped query (a target / --sessions-from)
    // stays unlimited. The kept rows are the MOST RECENTLY active (what an unscoped list wants),
    // then restored to the deterministic path order for display.
    // SCOPE counts are captured BEFORE the flood-guard cap (and before the row cap only -
    // after the `--since`/`--until` window, which narrows what the listing covers): the
    // scope banner / JSON header answer "how big is the range this query covered", exactly
    // like every other spanning command, where `--max-count`-class ROW caps never shrink
    // the scope numbers. Without this, an unscoped all-projects `list` reported the capped
    // 50 as the scope - off from the real corpus by orders of magnitude (R7 §2.4).
    let scope_sub = summaries.iter().filter(|s| s.is_subagent).count();
    let scope_top = summaries.len() - scope_sub;

    let all_projects = args.paths.is_empty() && args.sessions_from.is_none();
    // `--max-count 0` = uncapped (the crate-wide convention) - it beats the default cap.
    let cap = match args.max_count {
        Some(0) => None,
        Some(n) => Some(n),
        None => all_projects.then_some(DEFAULT_LIST_CAP),
    };
    let mut dropped = 0usize;
    if let Some(n) = cap {
        if summaries.len() > n {
            summaries.sort_by(|a, b| last_activity(b).cmp(&last_activity(a)));
            dropped = summaries.len() - n;
            summaries.truncate(n);
            summaries.sort_by(|a, b| a.path.cmp(&b.path));
        }
    }

    // 4. Render.
    match args.format {
        OutputFormat::Text => render_text(&summaries, dropped, scope_top, scope_sub),
        OutputFormat::Json => render_json(&summaries, dropped, scope_top, scope_sub)?,
    }
    Ok(())
}

/// Default `list` row cap for an UNSCOPED (all-projects) invocation - the flood guard.
pub(crate) const DEFAULT_LIST_CAP: usize = 50;

/// A session's most-recent activity timestamp (max over first/last user + last agent), for the
/// recency-keep cap. `None` (no readable ts) sorts oldest.
pub(crate) fn last_activity(s: &SessionSummary) -> Option<&str> {
    [&s.first_user, &s.last_user, &s.last_agent]
        .into_iter()
        .flatten()
        .filter_map(|p| p.timestamp_utc.as_deref())
        .max()
}

/// The one-line PREVIEW body for a `list` first/last scan (the §1 / automation-label polish):
/// - a `<task-notification>` automation pulse → its parsed `[<kind> …] <summary>` attribution
///   label, never the raw `<task-notification>…<output-file>…` XML wrapper;
/// - an inbound `<teammate-message>` peer message → a CLEAN `agent.communication.{inbox,signal}
///   <from> ⇨ self  <body>` render (the wrapper tags + trailing harness footer stripped), never
///   the raw `<teammate-message …>` XML blob;
/// - otherwise the normal genuine-user / AUQ / rejection reconstruction.
///
/// Eligibility is UNCHANGED from the prior `reconstructed_user_text(None)` gate: a task-notification
/// and a teammate-message were already captured as first/last (the former via genuine-user, the
/// latter via the teammate fallback arm), so the head/tail scan stops on the SAME record and captures
/// the SAME identity fields - only the rendered text is now clean. (An isMeta `<agent-message>` stays
/// INELIGIBLE: it is gated behind `is_teammate_message_record`, so it never newly fronts a preview.)
pub(crate) fn preview_text(rec: &Record) -> Option<String> {
    if let Some(label) = rec.automation_label() {
        return Some(label);
    }
    if rec.is_teammate_message_record() {
        if let Some(ic) = rec.inbound_comm_preview() {
            return Some(format!(
                "{}  {} ⇨ self  {}",
                ic.class.path(),
                ic.from,
                ic.body
            ));
        }
    }
    rec.reconstructed_user_text(None)
}

/// Pre-JSON byte prefilter for the head/tail scans: every record `preview_text` /
/// `agent_text` can anchor on is a `role:user`/`role:assistant` MESSAGE record
/// (genuine users, AUQ answers, rejections, task-notifications, teammate messages,
/// assistant text all carry one of the two markers - the same needles `search`'s
/// stage-1 candidate filter trusts). Any other line - attachment,
/// file-history-snapshot, queue-operation, metadata - is skipped UNPARSED; those
/// are routinely the LARGEST lines in a transcript, so this is the difference
/// between a head/tail read and paying `serde_json` for megabyte noise lines.
pub(crate) fn line_is_list_candidate(line: &[u8]) -> bool {
    // R13: serialization-tolerant (whitespace around the colon is the same record).
    crate::parse::line_has_role_marker(line)
}
