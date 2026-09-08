//! Search output types: Pairing / Hit / Exchange / SearchOutcome.

use super::*;

/// Max characters of a matched excerpt shown inline before truncation. Truncation
/// is ALWAYS explicit (`… (+N chars)`) - never silent (SPEC §0, §8.1).
///
/// Deliberately LONGER than `list`'s 200-char cap (`session::EXCERPT_MAX`): a search
/// hit wants enough of the matched exchange to be useful in context, whereas `list`
/// is a dense at-a-glance identity index. The difference is intentional.
pub(crate) const EXCERPT_MAX: usize = 400;

/// Render stand-in for a `redacted_thinking` block (GOLD §2 / oracle B3): the block carries
/// only an opaque/encrypted `data` payload (no readable text), so it surfaces this placeholder
/// while still classifying `agent.thinking` - so `-t agent.thinking` finds it without dumping
/// the opaque blob.
pub(crate) const REDACTED_THINKING_PLACEHOLDER: &str = "[redacted thinking]";

/// The `agent.tool.use ▹ agent.tool.result` pairing state of a tool hit (GOLD §7), joined by
/// `tool_use_id` across the transcript. Drives the render (`▹` / `(no result - pending)` /
/// `(use not in scope)`). `None` on a non-tool hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pairing {
    /// The use↔result partner is in scope (renders `agent.tool.use ▹ agent.tool.result`).
    Paired,
    /// An `agent.tool.use` whose `tool_result` is not in scope (frozen / elicitation / unreturned).
    PendingNoResult,
    /// An `agent.tool.result` whose `tool_use` is not in scope (compacted / sliced away).
    OrphanResult,
}

/// A single label-tagged hit inside an exchange.
#[derive(Debug, Clone)]
pub struct Hit {
    /// The matched LEAF [`Class`] - its [`Class::path`] is the rendered/JSON `label` (GOLD §6).
    /// `None` on the ONE shape that has no leaf: a record an explicit `show --line`/`--uuid`
    /// address named that [`Record::classify`] models no label for (an `isMeta` pseudo-turn
    /// matching no harness marker, a text-less block record). The refetch law says an address
    /// renders the record it names, so the unit is emitted with an honest empty label rather
    /// than dropped into a "no such record(s)" bail; a SCAN never produces one.
    pub class: Option<Class>,
    /// The full label path SET this record carries ([`Record::classify`]), for JSON `labels`.
    pub labels: Vec<&'static str>,
    /// The matched text excerpt (whitespace-normalized, explicitly truncated).
    pub excerpt: String,
    pub timestamp_utc: Option<String>,
    /// Tool name when the hit is a tool-use/tool-result block, for the header.
    pub tool_name: Option<String>,
    /// The source record's `message.model` (assistant records; string models only) - the
    /// `--count-by model` axis key. `None` when the record carries no model.
    pub model: Option<String>,
    /// The source record's attachment payload `type` (`attachment` records only) - the
    /// `--count-by attachment` axis key. `None` when the record carries no attachment.
    pub attachment_type: Option<String>,
    /// The source record's top-level Claude Code `version` stamp - the `--count-by
    /// version` axis key. `None` when the record carries none.
    pub version: Option<String>,
    /// RESULT-side error state: `Some(true)` = this tool_result carried `is_error`,
    /// `Some(false)` = a clean result, `None` = not a tool_result hit. Drives the text
    /// `[error]` decoration, JSON `is_error`, and the `--count-by result` axis (pairing
    /// answers "did a result come back"; this answers "was it good").
    pub is_error: Option<bool>,
    /// `from ⇨ to` comm direction ([`Record::direction`]) when the hit is `agent.communication.*`
    /// (GOLD §4); `None` otherwise. Rendered as `<from> ⇨ <to>`, JSON `from`/`to`.
    pub direction: Option<(String, String)>,
    /// The block's `tool_use_id` (the use's `id` / the result's `tool_use_id`) for the GOLD §7
    /// `▹` pairing join; `None` on a non-tool hit.
    pub tool_use_id: Option<String>,
    /// The resolved [`Pairing`] of a tool hit (filled by the per-file pairing pass); `None` on a
    /// non-tool hit or before the pass runs.
    pub pair: Option<Pairing>,
    /// 1-based PHYSICAL line number of the source record in its session jsonl - the stable
    /// address `csift show --line N` re-fetches. Backfilled by the turn collector (make_hit
    /// leaves it 0); 0 means "not located" (never happens for a real scanned hit).
    pub line: usize,
    /// The source record's `uuid` (jsonl's own globally-unique id), when present - the
    /// alternative `csift show --uuid U` address. `None` for records that carry no uuid.
    pub uuid: Option<String>,
    /// The record's VERBATIM source jsonl line - backfilled from the file mmap ONLY under
    /// `search --raw` (the bytes-out mode); `None` otherwise and for sidecar-merged records
    /// (no physical line).
    pub raw: Option<String>,
    /// Stable image ids the SOURCE RECORD carries (`#N` session handle, else `L<line>i<n>`) -
    /// the `[N image(s): …]` suffix, so a `search` hit on an image-bearing message exposes the
    /// SAME extractable id as `turns`/`image` (feed it to `csift image <session> --id <ID>`).
    /// Backfilled onto the record's first hit only (avoids repeating it per matched block).
    pub image_ids: Vec<String>,
    /// True when this hit came from a hook-backfilled ELICITATION SIDECAR record (§3.10) - an
    /// unresolved-pending AskUserQuestion/ExitPlanMode/MCP that is MISSING from the native
    /// transcript. Such a hit has NO physical `line` (it is not a real jsonl line), so it
    /// renders `(elicitation sidecar)` in place of `Lnnnn` and carries `source:"elicitation-
    /// sidecar"` in JSON. Backfilled with the address.
    pub from_sidecar: bool,
    /// v0.10.0: a `user.queued` hit's queue event (`enqueue` / `popAll` / `remove`), verbatim
    /// from the `queue-operation` line; `None` on every other hit. Rendered in the label
    /// zone, JSON `queue_operation`.
    pub queue_operation: Option<String>,
    /// v0.10.0: the `remove` reason when the line carries one (`absorbed_mid_turn` /
    /// `delivered_to_agent` - structural evidence the queued text was consumed); `None`
    /// otherwise. JSON `queue_reason`.
    pub queue_reason: Option<String>,
    /// The source record's DELIVERY override ([`Record::delivery_override`]): `Some(_)`
    /// when Claude Code's request-assembler drop predicate disagrees with the leaf
    /// default, `None` when the leaf default stands. Drives the bare-role selection,
    /// the `[not delivered]` label-zone marker and JSON `delivered`. Read
    /// [`Hit::delivered`] rather than this field for the verdict itself.
    pub delivery: Option<bool>,
    /// v0.12.0: on a `harness.resume.placeholder` hit ONLY, whether it closes a resume
    /// repair PAIR - `Some(true)` when its `parentUuid` is a `harness.resume.prompt`
    /// record in the same transcript, `Some(false)` when it is not (the same splice
    /// writes the unpaired form after any other trailing user record). `None` on every
    /// other hit, and on a placeholder whose file was never indexed for prompts. Drives
    /// the `[paired]`/`[unpaired]` label-zone marker and JSON `resume_paired`.
    pub resume_paired: Option<bool>,
    /// True when this hit's `excerpt` was CLIPPED to fit the default cap (its match-centered
    /// window dropped surrounding content) - i.e. the reader is seeing a fragment, not the
    /// whole record. ALWAYS false under `--no-truncate` and in `--line`/`--uuid` fetch
    /// mode (both lift the cap to `usize::MAX`), so it doubles as the "default truncation was in
    /// effect AND bit" signal that drives the trailing reader-caution note (`render_text`) and
    /// the JSON summary's `excerpts_truncated` flag.
    pub truncated: bool,
}

impl Hit {
    /// Did the model receive this record? The per-record override when there is one,
    /// else the matched leaf's own default ([`Class::llm_visible`]). An unlabeled unit has
    /// no leaf to ask, and only an explicit address produces one, so it falls back to the
    /// same answer the assembler gives a plain message record: delivered.
    #[must_use]
    pub fn delivered(&self) -> bool {
        self.delivery
            .unwrap_or_else(|| self.class.is_none_or(Class::llm_visible))
    }
}

/// C-27: how far a superseded draft sits from the message that replaced it, plus that
/// message's address. Present ONLY on a rendered draft unit whose superseding sibling is
/// in the same transcript (it always is - the sibling is what makes the record a draft -
/// unless a `--turn` window or a terminal count mode means nothing is rendered at all).
#[derive(Debug, Clone)]
pub struct DraftDiff {
    /// 1-based jsonl line of the superseding record (the message that WAS sent).
    pub superseding_line: usize,
    /// That record's uuid, when it carries one.
    pub superseding_uuid: Option<String>,
    /// Insertions + deletions of a shortest CHARACTER-level edit script between the
    /// draft's reconstructed text and the final's - the characters covered by the
    /// differing regions, never a length difference. A lower bound when `!exact`.
    pub chars: usize,
    /// `chars` as a percentage of the FINAL message's character count. `None` when the
    /// final text is empty (no denominator - never a fabricated 0 or 100). Can exceed
    /// 100 when the draft was the longer text.
    pub pct: Option<f64>,
    /// False when a bound stopped the walk: `chars` is then a proven floor and every
    /// rendering says "more than" (see [`crate::chardiff`]).
    pub exact: bool,
}

impl DraftDiff {
    /// The one-line rendering shared by `search` and `show` (text mode).
    pub(crate) fn text_line(&self) -> String {
        // A resend that changed nothing: the user recalled the message and sent it back
        // as it was. Saying "differs in 0 chars" would invite the reader to look for an
        // edit that is not there.
        if self.exact && self.chars == 0 {
            return "identical to the sent message".to_string();
        }
        let more = if self.exact { "" } else { "more than " };
        match self.pct {
            Some(p) => format!(
                "differs from the sent message in {more}{} chars ({more}{p:.1}% of the final): \
                 may carry an addition or a correction",
                self.chars
            ),
            None => format!(
                "differs from the sent message in {more}{} chars: may carry an addition or a \
                 correction",
                self.chars
            ),
        }
    }

    /// `pct` rounded to the ONE decimal the text renders, so the wire and the line agree.
    pub(crate) fn pct_json(&self) -> serde_json::Value {
        match self.pct {
            Some(p) => serde_json::json!((p * 10.0).round() / 10.0),
            None => serde_json::Value::Null,
        }
    }

    /// Attach the five C-27 keys to a `search` exchange row. Present only on a draft
    /// unit, the same lean-envelope rule the `siblings` block follows; `show`'s flat
    /// record rows carry them as explicit nulls instead.
    pub(crate) fn attach_json(&self, obj: &mut serde_json::Value) {
        obj["superseding_line"] = serde_json::json!(self.superseding_line);
        obj["superseding_uuid"] = serde_json::json!(self.superseding_uuid);
        obj["diff_chars"] = serde_json::json!(self.chars);
        obj["diff_pct"] = self.pct_json();
        obj["diff_exact"] = serde_json::json!(self.exact);
    }
}

/// A complete reconstructed request/response exchange (round-trip) containing the
/// hit(s).
#[derive(Debug, Clone)]
pub struct Exchange {
    /// The transcript's own id: a top-level session uuid, OR a subagent agent-id when the
    /// hit came from a subagent transcript (both round-trip as `@<id>` targets). THE
    /// LINE-DOMAIN RULE: line numbers are per-FILE, so a line-addressed fetch (`show
    /// --line`) MUST use THIS id (the transcript owning the line - a parent uuid + a
    /// subagent line silently fetches the wrong record); scope-level re-targeting uses
    /// `parent_session_id`. `is_subagent` discriminates.
    pub session_id: String,
    /// True when this exchange came from a subagent transcript (so `session_id` is an
    /// agent id, not a top-level uuid).
    pub is_subagent: bool,
    /// The OWNING top-level session uuid - the scope-token for re-targeting OTHER commands
    /// at the whole session. Equal to `session_id` for a top-level hit.
    pub parent_session_id: String,
    /// 0-based turn index (turns delimited by genuine-user messages).
    pub turn_index: usize,
    /// Turn-opening (genuine-user) record timestamp - this exchange's position in the
    /// COMBINED chronological timeline (top-level + subagent exchanges interleaved by
    /// absolute time). ISO-8601 UTC sorts lexicographically == chronologically. `None`
    /// when the opening record carries no timestamp (rare); such exchanges sort LAST,
    /// deterministically. Surfaced as `ts_utc`/`ts_local` on the JSON envelope and in
    /// the text header so the chronological position is visible per result.
    pub started_utc: Option<String>,
    pub hits: Vec<Hit>,
    /// Sibling records of this turn that did NOT themselves match - populated only under
    /// `--siblings`, so a matched user question can surface WITH the agent's reply. Each is
    /// rendered head-anchored (no match span) and filtered to the effective sibling
    /// categories; a record that produced a hit is never repeated here. Empty otherwise.
    pub siblings: Vec<Hit>,
    /// How many sibling units the FIXED `--siblings` policy capped away (0 when
    /// `--siblings` is off or nothing was capped) - surfaced, never silent.
    pub siblings_hidden: usize,
    /// The turn's physical jsonl line span `[first, last]` (0,0 when unknown) - the
    /// `csift show --line A..B` pointer the hidden-siblings note renders.
    pub turn_lines: (usize, usize),
    /// Uuids of every record stitched into this exchange (for traceability).
    pub record_uuids: Vec<String>,
    /// True for a superseded-draft unit (an opener replaced by a later same-parent
    /// sibling: esc-cancel / edit-resend). Such a record sits OUTSIDE turn numbering
    /// (`turn_index` is meaningless and renders null/annotated); its hits carry the
    /// single label `user.unsent`. A scan emits it when it matches (except under a
    /// `--turn` window); an explicit `show --line`/`--uuid` address always reaches it.
    pub superseded_draft: bool,
    /// C-27: the draft's distance from the message that replaced it (see [`DraftDiff`]).
    /// `None` on every non-draft exchange.
    pub draft_diff: Option<DraftDiff>,
}

/// Outcome of a search run, including the no-silent-truncation accounting.
#[derive(Debug, Clone, Default)]
pub struct SearchOutcome {
    pub exchanges: Vec<Exchange>,
    /// TRUE total of matching exchanges BEFORE any `--max-count` window (== `exchanges.len()`
    /// when uncapped). The head banner and tail footer both report THIS number (§6.2 both-ends
    /// law) - a capped run additionally discloses the emitted window.
    pub total_matched: usize,
    /// Distinct matching transcripts BEFORE the cap (pairs with `total_matched`).
    pub total_sessions: usize,
    /// How many matching exchanges were dropped by `--max-count` (0 if none).
    pub dropped_by_cap: usize,
    /// Total malformed lines skipped while scanning (surfaced, never hidden).
    pub skipped_lines: usize,
    /// Superseded-draft openers collapsed by turn reconstruction across the scanned
    /// files (esc-cancel / edit-resend siblings) - disclosed, never silent (C-18).
    pub superseded_drafts: usize,
    /// SCOPE-span counts of the RESOLVED transcript set (top-level + subagent files), from
    /// `resolve_session_files` - so the fan-out is announced even when a spanned subagent
    /// yields no hits. Drives the shared SCOPE banner / JSON header (suppressed when sub==0).
    pub scope_top: usize,
    pub scope_sub: usize,
}
