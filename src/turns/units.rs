//! Turn units: Role / TurnUnit / AgentMsg / TurnSlice / scan results.

use super::*;

/// One side of a turn the reconstruction can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    User,
    Assistant,
}

impl Role {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }

    /// The pre-ellipsis full-keep ceiling for this role.
    pub(crate) fn cap(self) -> usize {
        match self {
            Role::User => USER_CAP,
            Role::Assistant => ASST_CAP,
        }
    }

    /// The head fraction for this role's middle-truncation.
    pub(crate) fn head_frac(self) -> f64 {
        match self {
            Role::User => USER_HEAD_FRAC,
            Role::Assistant => ASST_HEAD_FRAC,
        }
    }
}

/// One side of a turn (user opener OR assistant end-of-turn), with the full verbatim
/// text plus the data needed to render + budget it.
#[derive(Debug, Clone)]
pub(crate) struct TurnUnit {
    pub(crate) line_no: usize,
    pub(crate) role: Role,
    /// `chars().count()` of the normalized one-line full text.
    pub(crate) full_chars: usize,
    /// Normalized one-line text (rendered verbatim or middle-truncated later).
    pub(crate) text: String,
    /// Newline count of the ORIGINAL (pre-normalization) text - drives the
    /// `L lines elided` note (omitted when 0, i.e. a single-line message).
    pub(crate) orig_newlines: usize,
    pub(crate) ts_utc: Option<String>,
    /// True once dedup flags this unit as already present in the newest summary.
    pub(crate) also_in_summary: bool,
    /// True when this unit was merged from the elicitation SIDECAR (§3.10) - an
    /// unresolved-pending AskUserQuestion/ExitPlanMode/MCP missing from the native transcript.
    /// Such a unit has no physical line (`line_no` 0); its header renders `(elicitation
    /// sidecar)` instead of `Lnnnn` and the JSON carries `source:"elicitation-sidecar"`.
    pub(crate) from_sidecar: bool,
    /// Set when this opener is an inbound peer/teammate communication (GOLD §1): the comm class +
    /// sender, so the header renders `<label>  <from> ⇨ self` IN PLACE OF the bare role word and the
    /// JSON carries `is_inbound_comm` + `comm_*` fields. `unit.text` holds the tag/footer-stripped
    /// body. `None` for an ordinary user/assistant unit. RENDER-ONLY - the turn count is unchanged
    /// (a peer opener still opens a turn via `opens_turn`).
    pub(crate) inbound: Option<crate::model::InboundComm>,
}

/// Position of an agent message within its turn's ordered agent-message run. A
/// 1-message turn's sole agent message is BOTH first and last → classified `Last` (the
/// always-keep anchor / EOT). Drives the `--keep-first` privilege and the always-keep
/// rule for the outcome message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentPos {
    First,
    Middle,
    Last,
}

/// One agent-text record in a turn (model-expansion: a turn now carries EVERY agent
/// message, not just the last EOT). Reuses [`TurnUnit`] verbatim for render+cost, and
/// carries the per-message tool/failed attribution the collapse placeholder needs.
#[derive(Debug, Clone)]
pub(crate) struct AgentMsg {
    /// The render/cost unit (line_no, full_chars, text, orig_newlines, ts_utc,
    /// also_in_summary, role = Assistant).
    pub(crate) unit: TurnUnit,
    /// First / Middle / Last within the turn's agent run (assigned after the push loop).
    pub(crate) pos: AgentPos,
    /// `tool_use` blocks in records strictly between the previous agent-text record (or
    /// turn start) and THIS one - the per-message attribution the placeholder `Y` needs.
    pub(crate) preceding_tool_calls: usize,
    /// erroring `tool_result` blocks in that same preceding span - placeholder `Z`.
    pub(crate) preceding_failed: usize,
}

/// One reconstructable turn: the user opener, the turn-wide tool-call count, and the
/// ORDERED run of every agent-text message in the turn (the model-expansion - replaces
/// the single `assistant_eot`). The derived `assistant_eot()` accessor keeps the old
/// "last == EOT" anchor for dedup / round-trip / render compatibility.
#[derive(Debug, Clone)]
pub(crate) struct TurnSlice {
    /// 0-based genuine-user turn index (from `group_turn_indices`).
    pub(crate) turn_index: usize,
    pub(crate) user: Option<TurnUnit>,
    /// `tool_use` block count across the turn → the `[N tool calls]` marker.
    pub(crate) tool_calls: usize,
    /// Stable `L<line>i<n>` ids of the images this turn carries (a pasted image, a tool
    /// screenshot) → the `[N image(s): …]` marker under the user line. Feed an id straight
    /// to `csift image <session> --id <ID> --out <dir>` to get the bytes back. Empty for a
    /// turn with no images.
    pub(crate) image_ids: Vec<String>,
    /// Every agent-text record in the turn, in file order (ascending line_no). EMPTY
    /// for a pure tool-call turn. The LAST element is the EOT anchor (`assistant_eot()`).
    pub(crate) agents: Vec<AgentMsg>,
    /// How many compaction boundaries sit between this turn and EOF (drives the
    /// boundary banners + dedup scope).
    pub(crate) compactions_before: usize,
    /// True when this turn's opener is a MACHINE-INJECTED automation trigger
    /// (`<task-notification>`) rather than a human message. The turn is still a real
    /// boundary (it opens a turn) and is selected/budgeted normally, but the header
    /// reports the automation/human split so a consumer sees which "user turns" were
    /// machine pulses (e.g. `selected 19 user units (3 automation triggers)`).
    pub(crate) is_automation: bool,
    /// The parsed automation trigger (kind / task-id / status / summary) when
    /// `is_automation` - `None` for a human turn. Carried so the JSON user-segment object
    /// can surface the trigger CLASS as STRUCTURED fields (`is_automation` / `trigger_kind`
    /// / `task_id` / `status`), not only as the inline `[<kind> …]` text prefix a consumer
    /// would otherwise have to regex out of the prose.
    pub(crate) automation: Option<crate::model::AutomationTrigger>,
}

impl TurnSlice {
    /// A round-trip-complete turn has BOTH a user opener and at least one agent message
    /// (the last of which is the EOT anchor). NOTE: this is the STRUCTURAL test - it counts
    /// an automation-pulse opener (`<task-notification>`) the same as a human opener; it
    /// governs Phase-2 fill (whether a `Both` selection is offered). The Phase-1 HARD FLOOR
    /// uses [`TurnSlice::is_human_round_trip`] instead, so a machine pulse never consumes the
    /// human-reserved `--round-trip-fraction` budget lane.
    pub(crate) fn is_round_trip(&self) -> bool {
        self.user.is_some() && !self.agents.is_empty()
    }

    /// A round-trip whose opener is a GENUINE HUMAN message (not an automation pulse). This
    /// is what the `--round-trip-fraction` HARD FLOOR reserves its budget for - the help /
    /// SKILL define that lane as "COMPLETE round-trips (user → … → assistant EOT)", i.e.
    /// human exchanges. An automation `<task-notification>` paired with an agent ack is a
    /// structural round-trip but NOT a human one, so it is excluded from the floor (it can
    /// still be picked in Phase-2 fill). Keeps the floor accounting and the header's
    /// human/automation split in agreement.
    pub(crate) fn is_human_round_trip(&self) -> bool {
        self.is_round_trip() && !self.is_automation
    }

    /// The EOT anchor: the LAST agent message's unit (the turn's outcome/decision, the
    /// dedup + round-trip key). `None` for a pure tool-call turn. This derived accessor
    /// preserves the whole existing call-graph that keyed on the old `assistant_eot`
    /// field with zero behavioural churn.
    pub(crate) fn assistant_eot(&self) -> Option<&TurnUnit> {
        self.agents.last().map(|a| &a.unit)
    }

    /// Mutable EOT-anchor accessor (dedup flips `also_in_summary` on it).
    pub(crate) fn assistant_eot_mut(&mut self) -> Option<&mut TurnUnit> {
        self.agents.last_mut().map(|a| &mut a.unit)
    }
}

/// The data a summary record contributes: its jsonl line + the dedup fingerprints of
/// the verbatim turns it already holds (§6 user bullets + §9 assistant quote).
#[derive(Debug, Clone)]
pub(crate) struct SummaryInfo {
    pub(crate) line_no: usize,
    /// Normalized-prefix fingerprints of everything the summary quotes verbatim.
    pub(crate) fingerprints: Vec<String>,
    /// Char length of the summary body (for the JSON boundary record).
    pub(crate) body_chars: usize,
}

/// A per-session scan result before global merge.
#[derive(Debug)]
pub(crate) struct ScanResult {
    pub(crate) session_id: String,
    /// True when this transcript is a SUBAGENT (so `session_id` is a bare hex, NOT a
    /// re-feedable `@<uuid>` target) - the r5 id-domain discriminator, now also on turns
    /// JSON (the text path already brands a subagent block `(subagent transcript)`).
    pub(crate) is_subagent: bool,
    /// The re-feedable PARENT session uuid (= `session_id` for a top-level file).
    pub(crate) parent_session_id: String,
    pub(crate) turns: Vec<TurnSlice>,
    /// Summary records in file order (oldest → newest), each with its line + dedup set.
    pub(crate) summaries: Vec<SummaryInfo>,
    pub(crate) skipped_lines: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────
