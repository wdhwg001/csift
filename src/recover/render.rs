//! Render: cutoffs, line ranges, guarded writes, restore views.

use super::*;

/// Resolve an `--at <WHEN>` spec to a cutoff jsonl line over a session's events.
/// Supports `@latest` (no cutoff → the file's FINAL state), `@line:<N>`, `@turn:<N>` (first line
/// strictly after turn N), an ISO8601 / relative datetime (events with ts ≤ the bound - the bound
/// is INCLUSIVE), or returns `None` (no cutoff → replay everything) when `when` is empty.
pub(crate) fn resolve_cutoff(when: &str, events: &[FileEvent]) -> Result<Option<usize>> {
    let when = when.trim();
    // `@latest` / empty → the final reconstructed state (replay every event, no cutoff). The
    // clean way to ask for "the file's last form" without guessing a timestamp past the last
    // write (a datetime cutoff is ≤-inclusive, so a too-early ts simply yields less).
    if when.is_empty() || when == "@latest" {
        return Ok(None);
    }
    if let Some(rest) = when.strip_prefix("@line:") {
        let n: usize = rest
            .trim()
            .parse()
            .with_context(|| format!("--at @line:<N> needs an integer, got {rest:?}"))?;
        return Ok(Some(n));
    }
    if let Some(rest) = when.strip_prefix("@turn:") {
        let target: usize = rest
            .trim()
            .parse()
            .with_context(|| format!("--at @turn:<N> needs an integer, got {rest:?}"))?;
        // Cutoff = the last jsonl line whose turn_index ≤ target.
        let cutoff = events
            .iter()
            .filter(|e| e.turn_index <= target)
            .map(|e| e.line_no)
            .max();
        // If nothing is at/below the target turn, cutoff at 0 (empty snapshot).
        return Ok(Some(cutoff.unwrap_or(0)));
    }
    // Datetime bound: the cutoff is the highest line_no whose ts ≤ bound.
    let window = TimeWindow::from_args(None, Some(when))?;
    let cutoff = events
        .iter()
        .filter(|e| window.contains(e.timestamp_utc.as_deref()))
        .map(|e| e.line_no)
        .max();
    Ok(Some(cutoff.unwrap_or(0)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Render context + line-range filtering
// ─────────────────────────────────────────────────────────────────────────────

/// Everything the renderers need beyond the per-session scan results.
#[derive(Debug)]
pub(crate) struct RenderCtx {
    pub(crate) mode: RecoverMode,
    pub(crate) file: Option<String>,
    pub(crate) line_range: Option<crate::text::RangeSpec>,
    pub(crate) at: Option<String>,
    pub(crate) skipped_lines: usize,
    /// SCOPE-span counts of the resolved transcript set, captured BEFORE the
    /// reconstruction merge folds subagents into their parent (so the banner still
    /// announces the true `1 top-level + N subagent` fan-out).
    pub(crate) scope_top: usize,
    pub(crate) scope_sub: usize,
}

/// Restrict a `(line_no, text)` known-line vector to the `--file-lines` range, if any. The
/// spec's open/from-end forms (`N..`, `-20..` = the last 20) resolve against the highest known
/// line number (the reconstructed file's length), 1-based.
pub(crate) fn apply_line_range(
    lines: Vec<(usize, String)>,
    line_range: Option<crate::text::RangeSpec>,
) -> Vec<(usize, String)> {
    match line_range {
        Some(spec) => {
            let len = lines.iter().map(|(n, _)| *n).max().unwrap_or(0);
            let (lo, hi) = spec.resolve(len, true);
            lines
                .into_iter()
                .filter(|(n, _)| *n >= lo && *n <= hi)
                .collect()
        }
        None => lines,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Text rendering
// ─────────────────────────────────────────────────────────────────────────────

/// Write the `--out` artifact ONLY when it has content. An EMPTY reconstruction (no
/// recoverable history / over-budget) must NOT clobber the destination - a user reusing a
/// scratch path (the advertised `--out /tmp/restored.md` idiom) would otherwise lose
/// pre-existing content AND read a false `(wrote …)` success line. Returns `true` when a
/// write happened (so the caller prints its `(wrote …)` line) and `false` (with a stderr
/// note) when the blob was empty and the file was left untouched. Uniform across patches/at
/// + their JSON twins + turns.
pub(crate) fn write_out_guarded(p: &Path, blob: &str) -> Result<bool> {
    if blob.is_empty() {
        eprintln!(
            "note: nothing reconstructed in range; --out file {} left untouched",
            p.display()
        );
        return Ok(false);
    }
    std::fs::write(p, blob).with_context(|| format!("cannot write --out file {}", p.display()))?;
    Ok(true)
}

/// SCOPE-span counts of the resolved transcript set (one `ScanResult` per resolved file,
/// incl. empty/no-history subagents) - `(top_level, subagent)`. Drives the shared SCOPE
/// banner / JSON header so a bare `csift recover <uuid>` fan-out is announced like list/turns.
pub(crate) fn scope_span(sessions: &[ScanResult]) -> (usize, usize) {
    let sub = sessions.iter().filter(|s| s.is_subagent).count();
    (sessions.len() - sub, sub)
}

/// Chronological order of two optional ISO-8601 timestamps. Timestamped events sort before
/// timestamp-less ones; a stable sort then keeps the within-session order for ties/absent ts.
pub(crate) fn cmp_ts(a: &Option<String>, b: &Option<String>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Fold each top-level group (a session + ITS OWN subagents, keyed by `parent_session_id`)
/// into one cross-session reconstruction timeline. A group whose target-file events come
/// from <2 transcripts is returned unchanged (byte-identical legacy single-session path).
/// A group with ≥2 contributing transcripts is collapsed into ONE synthetic `ScanResult`
/// whose events are the union, stably sorted by wall-clock timestamp, with `line_no`
/// re-stamped to a monotonic 1..N over the merged order so `--at` cutoffs (`@line` / `@turn`
/// / datetime) and `replay`'s cutoff stay well-defined across the merged stream.
pub(crate) fn merge_groups_for_reconstruction(sessions: Vec<ScanResult>) -> Vec<ScanResult> {
    // Group by parent id, preserving BTreeMap key order (== session_id order for top-level
    // sessions, since a top-level's parent_session_id is its own id) for determinism.
    let mut groups: BTreeMap<String, Vec<ScanResult>> = BTreeMap::new();
    for s in sessions {
        groups
            .entry(s.parent_session_id.clone())
            .or_default()
            .push(s);
    }

    let mut out: Vec<ScanResult> = Vec::new();
    for (parent_key, group) in groups {
        let contributing = group.iter().filter(|s| !s.events.is_empty()).count();
        if contributing < 2 {
            // 0 or 1 transcript touched the file in this group → no merge; the renderers
            // skip the empty members exactly as before.
            out.extend(group);
            continue;
        }
        // Prefer the top-level session's own uuid as the merged id (re-feedable `@<uuid>`
        // target); fall back to the shared parent key if the group is subagent-only.
        let merged_id = group
            .iter()
            .find(|s| !s.is_subagent)
            .map(|s| s.session_id.clone())
            .unwrap_or_else(|| parent_key.clone());
        let mut tagged: Vec<(String, FileEvent)> = group
            .iter()
            .flat_map(|s| s.events.iter().cloned().map(|e| (s.session_id.clone(), e)))
            .collect();
        tagged.sort_by(|a, b| cmp_ts(&a.1.timestamp_utc, &b.1.timestamp_utc));
        // Re-stamp to the synthetic 1..N cutoff coordinate, remembering each event's
        // REAL home (transcript id + jsonl line) so display never loses the truth.
        let mut merged_line_origin: BTreeMap<usize, (String, usize)> = BTreeMap::new();
        let mut events: Vec<FileEvent> = Vec::with_capacity(tagged.len());
        for (i, (sid, mut e)) in tagged.into_iter().enumerate() {
            merged_line_origin.insert(i + 1, (sid, e.line_no));
            e.line_no = i + 1;
            events.push(e);
        }
        // Scope accounting merges too (ts-sorted; line numbers stay each transcript's
        // own real jsonl line - they are pointers for a follow-up search, never
        // cutoff coordinates like the re-stamped event line numbers above).
        let mut opaque: Vec<OpaqueCommand> = group
            .iter()
            .flat_map(|s| s.opaque.iter().cloned())
            .collect();
        opaque.sort_by(|a, b| cmp_ts(&a.timestamp_utc, &b.timestamp_utc));
        out.push(ScanResult {
            session_id: merged_id.clone(),
            is_subagent: false,
            parent_session_id: merged_id,
            events,
            opaque,
            merged_line_origin,
            skipped_lines: 0,
        });
    }
    out
}

/// Per-window accounting of what a replay could NOT include: the opaque
/// mutating-class commands (formatter/interpreter/pkg/extract/git-worktree markers,
/// whose touched files are not in the command text) and PowerShell commands (never
/// lexically parsed), plus the paste-runnable search that lists the window's tool
/// calls touching the file.
pub(crate) struct WindowDisclosure {
    /// marker -> count histogram of the class-marker commands in the window.
    pub(crate) classes: BTreeMap<String, usize>,
    /// Total class-marker commands (PowerShell excluded).
    pub(crate) opaque_classes: usize,
    /// PowerShell command count.
    pub(crate) powershell: usize,
    /// The ready-to-run, time-bounded `csift search` for the window.
    pub(crate) suggested: Option<String>,
}

impl WindowDisclosure {
    pub(crate) fn is_clean(&self) -> bool {
        self.opaque_classes == 0 && self.powershell == 0
    }
}

/// Build the disclosure for one (already windowed) scan result. The search bounds are
/// the session's own event span, so the command surfaces exactly the replayed window.
pub(crate) fn window_disclosure(s: &ScanResult, file: Option<&str>) -> WindowDisclosure {
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    let mut powershell = 0usize;
    for o in &s.opaque {
        if o.marker == "powershell" {
            powershell += 1;
        } else {
            *classes.entry(o.marker.clone()).or_default() += 1;
        }
    }
    let opaque_classes = classes.values().sum();
    let first = s
        .events
        .iter()
        .filter_map(|e| e.timestamp_utc.as_deref())
        .min();
    let last = s
        .events
        .iter()
        .filter_map(|e| e.timestamp_utc.as_deref())
        .max();
    let suggested =
        file.map(|f| suggested_search(basename_of(f), &s.parent_session_id, first, last));
    WindowDisclosure {
        classes,
        opaque_classes,
        powershell,
        suggested,
    }
}

/// The one-line text fragment for non-zero opaque counts, `None` when the window is
/// clean: `2 mutating-class command(s) whose file set is not in the command text
/// (fmt:cargo x2) and 1 PowerShell command(s), never parsed`.
pub(crate) fn opaque_note(d: &WindowDisclosure) -> Option<String> {
    if d.is_clean() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if d.opaque_classes > 0 {
        let list = d
            .classes
            .iter()
            .map(|(m, n)| {
                if *n > 1 {
                    format!("{m} x{n}")
                } else {
                    m.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!(
            "{} mutating-class command(s) whose file set is not in the command text ({list})",
            d.opaque_classes
        ));
    }
    if d.powershell > 0 {
        parts.push(format!(
            "{} PowerShell command(s), never parsed",
            d.powershell
        ));
    }
    Some(parts.join(" and "))
}

/// A boundary's display location: the real transcript line, named with its source
/// transcript when a cross-transcript merge re-stamped the event coordinates
/// (`merged_line_origin`); the bare replay line otherwise (then it IS the jsonl line).
pub(crate) fn boundary_loc(s: &ScanResult, line_no: usize) -> String {
    match s.merged_line_origin.get(&line_no) {
        Some((sid, real)) => format!("L{real} in {sid}"),
        None => format!("L{line_no}"),
    }
}

/// The JSON source-location pair of a boundary: `(source_session_id, source_line)` -
/// the merge origin when re-stamped, else the row's own transcript + the line as-is.
pub(crate) fn boundary_source(s: &ScanResult, line_no: usize) -> (&str, usize) {
    match s.merged_line_origin.get(&line_no) {
        Some((sid, real)) => (sid.as_str(), *real),
        None => (s.session_id.as_str(), line_no),
    }
}

/// A boundary's detail, extended with the FORMATTER-CLASS clue when it applies: an
/// `external_edit` (an `edited_text_file` attachment) is idiom-independent, so when a
/// formatter or interpreter class command ran in this window at or before the boundary
/// (same source transcript), the external change may well be that command's rewrite.
/// The clue names the command's marker and real line; without one in scope, the detail
/// is returned unchanged (the insertion never speculates).
pub(crate) fn boundary_detail_with_clue(s: &ScanResult, b: &Boundary) -> String {
    if b.kind != "external_edit" {
        return b.detail.clone();
    }
    let (sid, real) = boundary_source(s, b.line_no);
    let clue = s
        .opaque
        .iter()
        .filter(|o| {
            (o.marker.starts_with("fmt:") || o.marker.starts_with("interp:"))
                && o.session_id == sid
                && o.line_no <= real
        })
        .max_by_key(|o| o.line_no);
    match clue {
        Some(o) => format!(
            "{}; this can be an external edit, the project's formatter, or a hook: a \
             formatter-class command ({}) ran at L{} in this window, so check the \
             project's conventions",
            b.detail, o.marker, o.line_no
        ),
        None => b.detail.clone(),
    }
}

/// The ready-to-run `csift search` command that lists every record touching `basename`
/// inside the disclosed window: pattern = the regex-escaped basename, scope = the
/// owning (re-feedable) session, label = `agent.tool.use` (every tool call, shell or
/// structured), bounds = the window's first/last instants when known. Raw record
/// timestamps are full ISO8601 instants, which `--since`/`--until` parse directly, so
/// the emitted command is paste-runnable.
pub(crate) fn suggested_search(
    basename: &str,
    parent_session_id: &str,
    first_utc: Option<&str>,
    last_utc: Option<&str>,
) -> String {
    // Shell-single-quote the pattern; a `'` inside becomes the standard '\'' splice.
    let pattern = regex::escape(basename).replace('\'', "'\\''");
    let mut cmd = format!("csift search '{pattern}' @{parent_session_id} -t agent.tool.use");
    if let Some(t) = first_utc {
        cmd.push_str(&format!(" --since {t}"));
    }
    if let Some(t) = last_utc {
        cmd.push_str(&format!(" --until {t}"));
    }
    cmd
}

pub(crate) fn render_text(
    ctx: &RenderCtx,
    sessions: &[ScanResult],
    out_path: Option<&Path>,
) -> Result<()> {
    // Restore writes the RAW file to stdout (for piping) - no scope banner to pollute it.
    if matches!(ctx.mode, RecoverMode::Restore) {
        return render_restore(ctx, sessions, out_path, false);
    }
    crate::text::emit_scope_banner(ctx.scope_top, ctx.scope_sub);
    match ctx.mode {
        RecoverMode::Restore => unreachable!("handled above"),
        RecoverMode::Coverage => render_coverage_text(ctx, sessions),
        // Salvage == `--at @latest`: render_at_text reads ctx.at (None here) → empty `when`
        // → resolve_cutoff returns None → the final-state best-effort fragment.
        RecoverMode::At | RecoverMode::Salvage => render_at_text(ctx, sessions, out_path),
        RecoverMode::Patches => render_patches_text(ctx, sessions, out_path),
    }
}
