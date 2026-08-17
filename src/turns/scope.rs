//! Scope counts, automation breakdown, sidecar presence.

use super::*;

/// Count selected user + assistant UNITS in a plan (an assistant unit = one KEPT agent
/// message; collapsed placeholders are not units). With the richness model a turn's
/// assistant side can contribute more than one kept message, so this walks the lane.
pub(crate) fn count_sides(plan: &SessionPlan, cfg: &RichnessCfg) -> (usize, usize) {
    let mut u = 0;
    let mut a = 0;
    for s in &plan.selected {
        if shows_user(s.sides) && find_turn(plan, s.turn_index).is_some_and(|t| t.user.is_some()) {
            u += 1;
        }
        if let Some(turn) = find_turn(plan, s.turn_index) {
            a += shown_agent_lane(turn, s.sides, cfg)
                .iter()
                .filter(|r| matches!(r, AgentRender::Kept(_)))
                .count();
        }
    }
    (u, a)
}

/// True when this plan SELECTED ≥1 elicitation-sidecar unit (§3.10) — drives the per-session
/// `with elicitation sidecar` note (text) / the JSON header flag, so a consumer knows the
/// output includes hook-backfilled records.
pub(crate) fn plan_has_sidecar(plan: &SessionPlan) -> bool {
    plan.selected.iter().any(|sel| {
        find_turn(plan, sel.turn_index)
            .and_then(|t| t.user.as_ref())
            .is_some_and(|u| u.from_sidecar)
    })
}

/// The fan-out scope of an in-scope-session set. `--budget` is applied PER session, so a
/// `--subagents` query that spans S subagents realizes up to `budget × (1 + S)`
/// chars. The banner must report the TRUE scope (every discovered session) — NOT only what
/// fit in the budget — so a rendering knob (`--budget`) can never silently rewrite "scope"
/// and a targeted top-level uuid can never read as `0 top-level`. Returns the full
/// breakdown: how many sessions are in scope (split top-level vs subagent over ALL of them)
/// and how many actually rendered within budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScopeCounts {
    /// All discovered sessions in scope (regardless of whether they fit the budget).
    pub(crate) in_scope: usize,
    /// Top-level (`<uuid>.jsonl`) sessions among those in scope.
    pub(crate) in_scope_top: usize,
    /// Subagent (bare-hex) transcripts among those in scope.
    pub(crate) in_scope_sub: usize,
    /// How many of the in-scope sessions produced a non-empty plan (rendered within budget).
    pub(crate) rendered: usize,
}

pub(crate) fn scope_summary(sessions: &[ScanResult], plans: &[SessionPlan]) -> ScopeCounts {
    let mut in_scope = 0usize;
    let mut in_scope_top = 0usize;
    let mut rendered = 0usize;
    for (sr, plan) in sessions.iter().zip(plans.iter()) {
        in_scope += 1;
        // Discriminate via the AUTHORITATIVE path-derived `is_subagent` field (set from
        // subagent::is_subagent_path at scan time) — NOT a re-derived id-shape heuristic. This
        // is the same signal turns' own JSON, and every other surface, already brands on.
        if !sr.is_subagent {
            in_scope_top += 1;
        }
        if !plan.selected.is_empty() {
            rendered += 1;
        }
    }
    ScopeCounts {
        in_scope,
        in_scope_top,
        in_scope_sub: in_scope - in_scope_top,
        rendered,
    }
}

/// The minimum char cost to render a targeted session's FIRST (most-recent) complete
/// round-trip, used to tell a user how much to raise `--budget` when their targeted session
/// was skipped (its plan came back empty). Returns `None` when the session has no turn at
/// all (an empty session — a different, honest "nothing to restore" case). The estimate is
/// the doc-header reservation + the cheapest single side of the most-recent turn, a true
/// lower bound on what any non-empty plan for this session would cost.
pub(crate) fn min_render_chars(sr: &ScanResult, budget: usize, cfg: &RichnessCfg) -> Option<usize> {
    let header = doc_header_block_max_chars(sr, budget);
    let cheapest = sr
        .turns
        .iter()
        .map(|t| {
            let mut costs = Vec::new();
            if t.user.is_some() {
                costs.push(turn_cost(t, SelSides::UserOnly, cfg));
            }
            if !t.agents.is_empty() {
                costs.push(turn_cost(t, SelSides::AssistantOnly, cfg));
            }
            costs.into_iter().min().unwrap_or(usize::MAX)
        })
        .min()?;
    if cheapest == usize::MAX {
        return None;
    }
    Some(header + cheapest)
}

/// How many of the SELECTED user-showing units are MACHINE automation triggers
/// (`<task-notification>` openers) rather than human messages — the header's
/// human/automation split (`N user (M automation triggers)`).
pub(crate) fn count_automation(plan: &SessionPlan) -> usize {
    plan.selected
        .iter()
        .filter(|s| {
            shows_user(s.sides)
                && find_turn(plan, s.turn_index)
                    .is_some_and(|t| t.user.is_some() && t.is_automation)
        })
        .count()
}

/// The fixed automation-trigger classes, in stable render order, paired with their slug. The
/// per-class breakdown (text + JSON) iterates THIS so a reader sees the composition of the
/// lumped `(N automation triggers)` total — the lens demands segments be labeled by trigger
/// attribution at the SUMMARY level, not just per-unit.
pub(crate) const AUTOMATION_KINDS: [crate::model::AutomationKind; 5] = [
    crate::model::AutomationKind::BackgroundCommand,
    crate::model::AutomationKind::Agent,
    crate::model::AutomationKind::Workflow,
    crate::model::AutomationKind::Monitor,
    crate::model::AutomationKind::Task,
];

/// Per-class counts of the SELECTED automation triggers across a plan set, keyed by the
/// `AUTOMATION_KINDS` order. A trigger with no parsed `automation` (a malformed pulse still
/// flagged `is_automation`) is attributed to `Task` (its rendered slug). Returns a fixed-len
/// array aligned with `AUTOMATION_KINDS`.
pub(crate) fn automation_by_kind(plans: &[SessionPlan]) -> [usize; 5] {
    let mut by = [0usize; 5];
    for plan in plans.iter().filter(|p| !p.selected.is_empty()) {
        for s in &plan.selected {
            if !shows_user(s.sides) {
                continue;
            }
            let Some(t) = find_turn(plan, s.turn_index) else {
                continue;
            };
            if !(t.user.is_some() && t.is_automation) {
                continue;
            }
            let kind = t
                .automation
                .as_ref()
                .map(|a| a.kind)
                .unwrap_or(crate::model::AutomationKind::Task);
            if let Some(idx) = AUTOMATION_KINDS.iter().position(|k| *k == kind) {
                by[idx] += 1;
            }
        }
    }
    by
}

/// Per-class counts of EVERY in-scope automation trigger — the whole-session composition,
/// INDEPENDENT of which turns the budget selected. This is the honest denominator behind the
/// selected [`automation_by_kind`]: at a realistic budget the recency window often selects
/// ZERO of a monitor-heavy session's deep monitor pulses, so the SELECTED breakdown reads
/// `monitor:0` and misleads a reader into thinking there was no monitor activity. The header
/// emits this IN-SCOPE count alongside the selected one so the whole-session truth is never
/// reported as zero. (NOTE: isMeta ScheduleWakeup wakeup-TICKS do not open turns yet — that
/// segmentation is a separate deferred item — so they are not yet counted here either; this
/// captures every turn-OPENING automation pulse, e.g. the monitor `<task-notification>`s.)
pub(crate) fn automation_in_scope_by_kind(plans: &[SessionPlan]) -> [usize; 5] {
    let mut by = [0usize; 5];
    for plan in plans {
        for t in &plan.turns {
            if !(t.user.is_some() && t.is_automation) {
                continue;
            }
            let kind = t
                .automation
                .as_ref()
                .map(|a| a.kind)
                .unwrap_or(crate::model::AutomationKind::Task);
            if let Some(idx) = AUTOMATION_KINDS.iter().position(|k| *k == kind) {
                by[idx] += 1;
            }
        }
    }
    by
}

/// Render a per-class automation breakdown as `kind:count` pairs for the non-zero classes,
/// e.g. `2 background-command, 1 agent`. Empty when no class has a count (the caller then
/// shows just the total). Used by BOTH the text header detail and as the JSON `by_kind`
/// source.
pub(crate) fn automation_breakdown_text(by: &[usize; 5]) -> String {
    by.iter()
        .zip(AUTOMATION_KINDS.iter())
        .filter(|(n, _)| **n > 0)
        .map(|(n, k)| format!("{n} {}", k.slug()))
        .collect::<Vec<_>>()
        .join(", ")
}
