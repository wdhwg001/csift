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
/// NOTE: this raw grouper trusts file order and does NOT suppress superseded drafts. The
/// production surfaces use [`group_turn_indices_deduped`], which additionally drops the
/// abandoned-draft openers an esc-cancel / edit-resend leaves behind (§6.4.1). This bare
/// form stays for the lightweight bool-fixture tests and any caller that has no `Record`.
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

/// Indices of turn-opening records that are SUPERSEDED DRAFTS - an earlier sibling of a
/// later turn-opener sharing the SAME non-null `parentUuid` (§6.4.1). This is the on-disk
/// shape of the "type a message, ESC-cancel / edit, resend" loop (and any rewind that
/// re-opens a turn from the same point): Claude Code appends every draft as its own
/// `type:"user"` record, yet only ONE - the last in file order - was actually delivered to
/// the model. The earlier siblings are abandoned drafts.
///
/// WHY last-in-file is the survivor (verified on real `~/.claude/projects` data): distinct
/// real turns never share a `parentUuid` (each user turn is parented to the assistant
/// message that preceded it), so same-parent openers are ALWAYS alternative versions of one
/// logical turn; and across the corpus the last sibling's subtree is the one that reaches
/// furthest toward the leaf (the live branch). A content-similarity heuristic would miss the
/// common case where the user *prepended/inserted* text on the edit (`look…` → `take a closer look…`),
/// so the parent-uuid identity - not text - is the load-bearing signal.
///
/// `rec` projects each element to its `Record` (works for `&Record`, `Record`, and the
/// search `Kept` wrapper alike). Records with a null/empty `parentUuid` are NEVER grouped
/// (grouping on "no parent" would merge unrelated first-message drafts); in real data a
/// genuine user always carries a parent, so this costs nothing.
///
/// HONEST BOUND: only the superseded OPENER is reported, not the downstream of a branch
/// abandoned AFTER it already drew replies (rewind-after-response). Those rare descendants
/// (≤2% of turns on the measured corpus) keep their own distinct parents and survive; fully
/// pruning them needs an active-leaf walk, which a compaction boundary severs - so we do not
/// risk silently dropping a live turn to chase them.
// Both named derivations below are reached only from `#[cfg(test)]` in the bin build:
// production takes `collapse_openers` directly, because it needs BOTH of its answers in
// ONE walk. They stay because they are the names SPEC 6.4.1, AGENTS 3.3 and the ledger
// cite for the draft contract, and deriving them from the one walk is what keeps those
// names from drifting away from what the grouper actually does.
#[allow(dead_code)]
#[must_use]
pub fn superseded_draft_indices<T>(
    records: &[T],
    rec: impl Fn(&T) -> &Record,
) -> std::collections::HashSet<usize> {
    collapse_openers(records, rec).drafts.into_keys().collect()
}

/// What ONE walk over a transcript's turn-openers decides.
///
/// Two different things make a same-parent opener not the turn it looks like, and they
/// need opposite treatment, so they are decided together:
///
/// - a SUPERSEDED DRAFT is an earlier sibling with its OWN uuid: the user typed it,
///   recalled it, edited it and sent the later one. The EARLIER record is the one that
///   was never delivered.
/// - a REPLAY COPY carries the SAME `uuid` as an opener already seen under that parent.
///   A compaction RE-ANCHOR re-appends a contiguous block of records with their uuids
///   PRESERVED (the copies differ in `promptId` alone), so the same logical message is
///   on disk twice. Grouping on `parentUuid` alone reads the copy as a later sibling and
///   marks the ORIGINAL as an abandoned draft - wrong twice over: that message WAS sent,
///   and the copy is not a second message. The LATER record is the redundant one.
///
/// So same-uuid openers collapse to their FIRST occurrence: the first keeps its label and
/// its turn, and the copy neither supersedes it nor opens a turn of its own. The copy is
/// NOT dropped - it stays a turn MEMBER, which keeps it addressable and matches how every
/// other record of a replayed block is treated (the replayed assistant and attachment
/// records have always rendered at both of their lines).
#[derive(Debug, Default)]
pub struct OpenerCollapse {
    /// Superseded draft index -> the index of the sibling that finally replaced it.
    pub drafts: std::collections::HashMap<usize, usize>,
    /// Indices of replay copies: a later opener whose uuid an earlier same-parent opener
    /// already carried.
    pub replays: std::collections::HashSet<usize>,
}

impl OpenerCollapse {
    /// The openers turn reconstruction must DROP entirely (drafts only - a replay copy is
    /// demoted to a member instead, see the type doc).
    #[must_use]
    pub fn dropped(&self) -> std::collections::HashSet<usize> {
        self.drafts.keys().copied().collect()
    }

    /// Does index `i` still open a turn? False for a replay copy; a draft is handled by
    /// the skip set instead.
    #[must_use]
    pub fn opens(&self, i: usize) -> bool {
        !self.replays.contains(&i)
    }
}

#[must_use]
pub fn collapse_openers<T>(records: &[T], rec: impl Fn(&T) -> &Record) -> OpenerCollapse {
    // parent -> (the last opener seen so far, the earlier siblings it supersedes).
    let mut groups: std::collections::HashMap<&str, (usize, Vec<usize>)> =
        std::collections::HashMap::new();
    // (parent, uuid) -> the FIRST opener carrying it: the replay detector.
    let mut first_uuid: std::collections::HashMap<(&str, &str), usize> =
        std::collections::HashMap::new();
    let mut out = OpenerCollapse::default();
    for (i, item) in records.iter().enumerate() {
        let r = rec(item);
        if !r.opens_turn() {
            continue;
        }
        let Some(parent) = r.parent_uuid.as_deref() else {
            continue; // null parent: never grouped (would merge unrelated records)
        };
        if parent.is_empty() {
            continue;
        }
        // A uuid this parent's openers already carried: the same logical record, re-appended
        // by a compaction re-anchor. It supersedes nothing and opens nothing.
        if let Some(uuid) = r.uuid.as_deref() {
            if first_uuid.insert((parent, uuid), i).is_some() {
                out.replays.insert(i);
                continue;
            }
        }
        // Keep the LAST opener per parent: when a new sibling appears, the previously-seen
        // one for that parent becomes a superseded draft.
        match groups.get_mut(parent) {
            Some(entry) => {
                let prev = std::mem::replace(&mut entry.0, i);
                entry.1.push(prev);
            }
            None => {
                groups.insert(parent, (i, Vec::new()));
            }
        }
    }
    // An n-way draft group (type, esc, edit, esc, edit, send) maps EVERY earlier sibling to
    // the same final survivor, not to its immediate successor: the intermediate versions
    // were never sent either.
    for (survivor, drafts) in groups.into_values() {
        for d in drafts {
            out.drafts.insert(d, survivor);
        }
    }
    out
}

/// [`superseded_draft_indices`] with the SURVIVOR named: each superseded opener maps to
/// the index of the sibling that finally replaced it (the LAST opener sharing its
/// `parentUuid` - the one that was delivered), so a caller can render the draft AGAINST
/// the message the user actually sent (the C-27 unsent diff line). A replay copy is never
/// a survivor and never a draft ([`collapse_openers`]).
#[allow(dead_code)]
#[must_use]
pub fn superseded_draft_map<T>(
    records: &[T],
    rec: impl Fn(&T) -> &Record,
) -> std::collections::HashMap<usize, usize> {
    collapse_openers(records, rec).drafts
}

/// [`group_turn_indices`] with the two [`collapse_openers`] corrections (§6.4.1): a
/// superseded DRAFT is dropped ENTIRELY - it neither opens a turn nor folds in as a
/// member - so a message the user edited away before sending can never resurface as a
/// phantom turn (nor leak its abandoned text into a neighbour); a REPLAY COPY stops
/// OPENING a turn but stays a member, so a compaction re-anchor cannot mint a second turn
/// for a message that was sent once. This is the delimiter every session-operating surface
/// (`turns` / `search` / `files` / `recover`) uses, so they stay byte-consistent on what
/// counts as a turn.
#[must_use]
pub fn group_turn_indices_deduped<T>(
    records: &[T],
    rec: impl Fn(&T) -> &Record,
) -> Vec<Vec<usize>> {
    let collapse = collapse_openers(records, |x| rec(x));
    let skip = collapse.dropped();
    group_turn_indices_core(
        records,
        |i, x| rec(x).opens_turn() && collapse.opens(i),
        &skip,
    )
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
