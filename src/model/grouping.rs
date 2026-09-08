//! PlanIndex + turn grouping (opens_turn segmentation) + content flattening.

use super::*;

/// An index of ExitPlanMode `tool_use_id → planFilePath` built from a session's records
/// (§4.2.4). A tool-use rejection-with-message ([`Record::plan_rejection_message`])
/// resolves the rejected `tool_use_id` through this index to surface a `[plan: <path>]`
/// pointer so a consuming LLM can go Read the plan. Built once per session via
/// [`PlanIndex::from_records`]; cheap (one `BTreeMap` of the few ExitPlanMode calls).
#[derive(Debug, Clone, Default)]
pub struct PlanIndex {
    by_id: std::collections::BTreeMap<String, String>,
}

impl PlanIndex {
    /// Build the index from a session's records: every ExitPlanMode tool_use's
    /// `id → planFilePath` (see [`Record::exit_plan_pointers`]). A block with no
    /// `planFilePath` is skipped (an empty path is not a useful pointer).
    #[must_use]
    pub fn from_records<'a, I>(records: I) -> Self
    where
        I: IntoIterator<Item = &'a Record>,
    {
        let mut by_id = std::collections::BTreeMap::new();
        for rec in records {
            for (id, path) in rec.exit_plan_pointers() {
                if !path.is_empty() {
                    by_id.insert(id, path);
                }
            }
        }
        Self { by_id }
    }

    /// The plan file path an ExitPlanMode tool_use with `id` pointed to, if known.
    #[must_use]
    pub fn plan_path(&self, id: &str) -> Option<&str> {
        self.by_id.get(id).map(String::as_str)
    }
}

/// `"s"` for plural counts, `""` for exactly one - for the `N question(s)` label.
pub(crate) fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Group records (in file order) into TURNS, returning one `Vec<usize>` of record
/// indices per turn - the outer index IS the 0-based turn index (genuine-user order).
///
/// The single source of truth for turn delimiting (§6.4), shared by `search`'s
/// exchange reconstruction and `files`'s mutation attribution so the two never drift:
///
/// - A turn opens on a boundary record (`is_genuine`); every record after it, up to the
///   next boundary, belongs to that turn (a non-boundary `tool_result`-carrier, an
///   `isMeta` pseudo-turn, and a compaction summary are turn MEMBERS, never delimiters).
/// - Records before the first boundary (rare: leading tool noise) seed turn 0 so they
///   are never lost. When such a synthetic lead exists AND a real user turn follows, the
///   lead is folded into the first real turn so indices stay 0-based on boundary
///   openers. With NO boundary at all, the orphans are a standalone turn 0.
///
/// `is_genuine` is a closure (rather than calling [`Record::opens_turn`] directly) only
/// so callers can test the grouping over lightweight bool fixtures; in production it is
/// always [`Record::opens_turn`] - which opens on a genuine human message, an answered
/// AskUserQuestion (the answer is the user's message, §4.4), OR a tool-use
/// rejection-with-message (§4.2.4). An AUQ answer / plan rejection becoming a turn
/// boundary is the sanctioned correct behavior change (a previously-MISSED genuine user
/// message); interrupts / `<local-command-stdout>` / `<command-name>` wrappers, formerly
/// spurious boundaries, are excluded by `is_genuine_user` (regression fixes).
///
/// NOTE: this raw grouper trusts file order and knows nothing of the SURVIVAL AXIS. The
/// production surfaces use [`group_turn_indices_deduped`], which drops every record
/// Claude Code's own conversation chain no longer reaches (§6.4.1). This bare form stays
/// for the lightweight bool-fixture tests and any caller that has no `Record`.
// Production now routes through `group_turn_indices_deduped`, so in the bin build this bare
// generic is reached only from `#[cfg(test)]` - kept as the documented base primitive +
// bool-fixture test entry (same retained-shape rationale as the `#[allow(dead_code)]` on
// `Record`).
#[allow(dead_code)]
#[must_use]
pub fn group_turn_indices<T>(records: &[T], is_genuine: impl Fn(&T) -> bool) -> Vec<Vec<usize>> {
    group_turn_indices_core(
        records,
        |_, r| is_genuine(r),
        &std::collections::HashSet::new(),
    )
}

/// [`group_turn_indices`] under the SURVIVAL AXIS ([`Chain`]): a turn opens only on a
/// record the conversation chain still reaches, and a record it does not reach belongs to
/// no numbered turn at all - neither as a boundary nor as a member. That covers what the
/// three former heuristics covered (a recalled draft, a compaction re-anchor's replay
/// copy, the history above a compaction cut) plus what none of them could see: a turn the
/// operator rewound past, and which child of a branch point the conversation continued
/// from. Abandoned records stay searchable, addressable and disclosed; they are simply not
/// numbered.
///
/// This is the delimiter every session-operating surface (`verbatim` / `search` / `files`
/// / `recover`) uses, so they stay byte-consistent on what counts as a turn.
///
/// CAUTION - this entry point builds the chain from whatever records the caller hands it,
/// and a caller whose prefilter dropped the `attachment` and `system` lines hands it a
/// DAG with holes. A chain that cannot see those lines cannot be trusted to say a whole
/// BRANCH is abandoned, so this form drops only what the pre-0.12.0 rule dropped: the
/// superseded OPENER itself. Every other record stays a member of the turn above it, and
/// a turn-keyed consumer (`files --by timeline`) keeps its row. A surface that splices
/// [`crate::parse::spine_record`] rows back in has the whole DAG and calls
/// [`group_turn_indices_chained`] with that chain instead, which drops the branch.
#[must_use]
pub fn group_turn_indices_deduped<T>(
    records: &[T],
    rec: impl Fn(&T) -> &Record,
) -> Vec<Vec<usize>> {
    let chain = Chain::build_by(records, &rec, None);
    let skip: std::collections::HashSet<usize> = (0..records.len())
        .filter(|&i| chain.kind(i).is_some())
        .collect();
    group_turn_indices_core(records, |i, x| rec(x).opens_turn() && chain.opens(i), &skip)
}

/// [`group_turn_indices_deduped`] with the chain already built - the path `search` takes,
/// because its scan splices the structural spine rows of the lines its prefilter drops
/// into the chain's view and must not pay for a second build.
#[must_use]
pub fn group_turn_indices_chained<T>(
    records: &[T],
    rec: impl Fn(&T) -> &Record,
    chain: &Chain,
) -> Vec<Vec<usize>> {
    let skip: std::collections::HashSet<usize> = (0..records.len())
        .filter(|&i| !chain.survival(i).selectable())
        .collect();
    group_turn_indices_core(records, |i, x| rec(x).opens_turn() && chain.opens(i), &skip)
}

/// Shared engine for [`group_turn_indices`] and [`group_turn_indices_deduped`]. Every index
/// in `skip` is omitted entirely (`continue`) - neither a turn boundary nor a member - which
/// is how superseded drafts are dropped. With an empty `skip` the behaviour is identical to
/// the original file-order grouper.
pub(crate) fn group_turn_indices_core<T>(
    records: &[T],
    is_genuine: impl Fn(usize, &T) -> bool,
    skip: &std::collections::HashSet<usize>,
) -> Vec<Vec<usize>> {
    let mut turns: Vec<Vec<usize>> = Vec::new();
    let mut first_emitted: Option<usize> = None;
    for (i, rec) in records.iter().enumerate() {
        if skip.contains(&i) {
            continue; // superseded draft: invisible to turn reconstruction
        }
        if first_emitted.is_none() {
            first_emitted = Some(i);
        }
        if is_genuine(i, rec) {
            turns.push(vec![i]);
        } else if let Some(last) = turns.last_mut() {
            last.push(i);
        } else {
            // Pre-first-user records seed turn 0 (a standalone turn 0 if no genuine
            // user ever opens).
            turns.push(vec![i]);
        }
    }
    // If the first EMITTED (non-skipped) record is a synthetic pre-user lead AND a real user
    // turn follows, fold the lead into the first real turn so indices align with genuine-user
    // order. Basing this on the first non-skipped record keeps behaviour identical when no
    // draft is skipped (`first_emitted` is then index 0, matching `records.first()`).
    let synthetic_lead = first_emitted.is_some_and(|i| !is_genuine(i, &records[i]));
    if synthetic_lead && turns.len() > 1 {
        let lead = turns.remove(0);
        if let Some(first_real) = turns.first_mut() {
            let mut merged = lead;
            merged.extend(first_real.iter().copied());
            *first_real = merged;
        }
    }
    turns
}

/// Flatten a `Content` to a single normalized line of its textual parts.
/// `string` → itself; `blocks` → all `text` blocks joined (other block types,
/// which never co-occur with a genuine user `text` block, are ignored).
pub(crate) fn flatten_content_text(content: &Content) -> String {
    match content {
        Content::Text(s) => normalize_line(s),
        Content::Blocks(blocks) => {
            let joined = blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            normalize_line(&joined)
        }
    }
}

/// Extract the textual payload of a `tool_result` block's `content` (§4.5). The
/// content is raw `serde_json::Value`: a bare string, OR an array of
/// `{type:"text",text}` / `{type:"image"}` / `{type:"tool_reference",tool_name}`
/// objects. We concatenate every `text` field found and, for `tool_reference`,
/// surface the `tool_name` (so a regex like `ToolSearch` still matches). Anything
/// else (images, unknown shapes) contributes nothing. Whitespace is NOT normalized
/// here - callers that excerpt do their own normalization; matchers want the raw
/// text. Returns an owned `String` (possibly empty).
#[must_use]
pub fn tool_result_content_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => {
            let mut parts: Vec<String> = Vec::new();
            for item in items {
                if let Some(t) = item.get("text").and_then(serde_json::Value::as_str) {
                    parts.push(t.to_string());
                } else if let Some(name) = item.get("tool_name").and_then(serde_json::Value::as_str)
                {
                    parts.push(name.to_string());
                }
            }
            parts.join("\n")
        }
        // Object/number/bool/null: render compactly so a regex can still match
        // structured payloads that aren't the common string/array shapes.
        other => other.to_string(),
    }
}

/// Scrape the inline persisted-output pointer (§4.6 fallback): the line
/// `Full output saved to: <ABSOLUTE_PATH>` inside a `<persisted-output>` block.
/// Returns the trimmed path, or `None` if the marker is absent.
pub(crate) fn scrape_persisted_path(text: &str) -> Option<String> {
    const MARKER: &str = "Full output saved to:";
    let idx = text.find(MARKER)?;
    let rest = &text[idx + MARKER.len()..];
    // The path runs to end-of-line.
    let line_end = rest.find('\n').unwrap_or(rest.len());
    let path = rest[..line_end].trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}
