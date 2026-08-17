//! Verbatim budget constants + AgentMsgMode / Profile / RichnessCfg.

/// Pre-ellipsis full-keep ceiling for a USER unit (chars). Sized from the measured
/// user-message length distribution (median ~410, p90 ~2,574): a 600 cap keeps the
/// median turn whole and forces ellipsis only on the long tail.
pub(crate) const USER_CAP: usize = 600;

/// Pre-ellipsis full-keep ceiling for an ASSISTANT end-of-turn unit (chars). Larger
/// than [`USER_CAP`] because assistant EOT prose is 1.45–2.16× longer with more
/// newlines (measured) - so its head fraction is larger too (see [`ASST_HEAD_FRAC`]).
pub(crate) const ASST_CAP: usize = 900;

/// Headroom subtracted from `--window` to size the per-body cap in `--slices` (fixed-fleet) mode.
/// There the per-role 600/900 caps are REPLACED by a window cap: a body renders whole up to
/// `window - SLICE_BODY_HEADROOM`, so its rendered body LINE (incl. the `… [+K chars, L lines
/// elided] …` wrapper) AND the unit's separate header line each stay under one window - no single
/// line can force [`slice_into_windows`] to hard-split mid-content. Only a turn that ALONE exceeds
/// a window is ellipsized; everything else is kept verbatim (the user-directive recovery target).
pub(crate) const SLICE_BODY_HEADROOM: usize = 200;

/// Head fraction for an ASSISTANT unit's middle-truncation: EOT prose front-loads
/// context and back-loads the decision, so keep ≈⅔ head / ⅓ tail.
pub(crate) const ASST_HEAD_FRAC: f64 = 0.66;

/// Head fraction for a USER unit: the ask is front-loaded, slightly less tail needed.
pub(crate) const USER_HEAD_FRAC: f64 = 0.60;

/// Cost of the trailing `\n` that follows every emitted physical line (header lines,
/// body lines, marker lines, banner lines, and every line of the document header block).
/// Each `emit` callback appends exactly one `\n`, so every line the document contains
/// pays for it - charging it is what makes the summed cost equal the real emitted length.
pub(crate) const NEWLINE_COST: usize = 1;

/// Normalized-prefix length used for the summary-dedup fingerprint (§6.2): a unit whose
/// first `DEDUP_PREFIX` normalized chars match a summary bullet/quote is flagged
/// `also_in_summary` and demoted. Strict (long) prefix ⇒ a false positive is unlikely.
pub(crate) const DEDUP_PREFIX: usize = 80;

// ─────────────────────────────────────────────────────────────────────────────
// Multi-agent-message model + richness filtering (the model-expansion)
// ─────────────────────────────────────────────────────────────────────────────
//
// A genuine-user turn can own a LONG run of agent messages (a debugging/build chain the
// model narrates step by step) that a compaction summary clips to its single §9 EOT
// quote. The reconstruction's job is to restore the LOAD-BEARING members of that run
// without flooding the budget with pure "let me look into this" declarations. The model
// keeps EVERY agent message on the slice (`TurnSlice.agents`) but SELECTS a survivor set.
//
// WHY THE DEFAULT IS "LONGEST", NOT "LAST": the LAST agent message of a turn is frequently
// a ~50-char throwaway wrap-up ("Done.", "Let me know if you want X") while the
// SUBSTANTIVE Rich Response - the actual finding, the committed answer, the design - sits
// in a MIDDLE message. The pre-feature default kept `agents.last()`, so it silently
// DROPPED the substance of exactly those turns. The default now keeps the LONGEST agent
// message (by `full_chars`), which is the single best one-message proxy for "where the
// substance is". On a tie `max_by_key` returns the LAST maximum, so an all-equal run
// coincides with the old `agents.last()` pick.
//
// But "more than one message matters" is common, so `Longest` ALSO keeps:
//   • the LONGEST agent message - ALWAYS (the substantive Rich Response).
//   • the FIRST - when SUBSTANTIVE (`full_chars >= rich_min_chars`); the opening message
//     often states the plan / an early finding worth preserving.
//   • each MIDDLE that is RICH (`agent_msg_is_rich`); a major finding can live mid-run.
//   • everything else collapses into a placeholder.
// `--agent-rich-min-chars` tunes BOTH the first-substantive gate and the rich length arm.
//
// `Rich` is the OLDER keep-set, retained as an explicit mode (a long run only; short runs
// keep all): LAST always + FIRST by position privilege (under `--keep-first`) + each
// non-droppable MIDDLE.
//
//   • LAST agent message  - ALWAYS kept by `Rich` (the outcome / EOT anchor).
//   • FIRST agent message - kept UNCONDITIONALLY under `--keep-first` (position
//     privilege - kept merely for being first, even when not rich); with
//     `--no-keep-first` it is decided exactly as a MIDDLE.
//   • MIDDLE agent messages - kept UNLESS a PROVEN pure declaration; else collapsed
//     into a placeholder.
//
// "Rich" (the predicate) = a CHEAP single-pass heuristic combining LENGTH (a clearly-
// above-typical body is kept on length alone) with a SIGNAL test (a number-of-substance /
// commit hash / file:line ref / backtick code path / finding-or-decision lexeme). Keep-on-
// doubt is the spine: a wrongly-kept declaration costs one header + ≤cap body chars then
// clamps; a wrongly-DROPPED finding is unrecoverable, so DROP requires proof (a signal-
// less intent-verb opener under the declaration length), KEEP is the default fall-through.
//
// MODES. `AgentMsgMode::Longest` (the DEFAULT) keeps the longest + the first-if-
// substantive + the rich middles, as above. `AgentMsgMode::EotOnly` forces the old single-
// EOT behavior (only `agents.last()`, byte-identical to the pre-expansion output) - the
// "force last-only" escape. `AgentMsgMode::All` keeps every message (no filtering). The
// `Rich` mode's filtering is only attempted on a LONG run (`agents.len() > run_threshold`,
// default 6); short runs keep every message. `Longest` applies its longest+heuristic pick
// to EVERY multi-message turn (no short-run escape - the headline "long Rich Response then
// 50-char wrap-up" turn is only two messages, yet must still drop the wrap-up).

/// How a turn's agent-message run is reduced to a survivor set. The MASTER switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, Default)]
pub enum AgentMsgMode {
    /// DEFAULT. Keep the LONGEST agent message (by `full_chars` - the substantive Rich
    /// Response, which is frequently a MIDDLE message, not the last throwaway wrap-up) +
    /// the FIRST when substantive (`full_chars >= rich_min_chars`) + each RICH middle;
    /// collapse everything else into a placeholder. Applies to every multi-message turn
    /// (no short-run escape). On a tie the LAST maximum wins (an all-equal run coincides
    /// with the old `agents.last()` pick).
    #[default]
    Longest,
    /// Force the OLD single-EOT behavior: keep ONLY the last agent message per turn
    /// (byte-identical to the pre-expansion output). The "force last-only" escape.
    #[value(name = "eot-only")]
    EotOnly,
    /// Keep the last always + the first by position privilege +
    /// each rich middle; collapse the proven pure declarations. Gated by the run
    /// threshold (short runs keep all).
    Rich,
    /// Keep EVERY agent message - no filtering, no collapse (maximal-fidelity escape).
    All,
}

/// A convenience bundle of richness thresholds an LLM caller picks by reading the
/// compaction summary it supplements (heavy = restore the debugging narrative; light =
/// the summary is already rich, just restore phrasings + EOTs). Applied BEFORE the
/// individual flags so an explicit flag overrides the profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Profile {
    /// Maximal fidelity thresholds: threshold 4, rich-min 200, declaration-max 140 (does
    /// not change the master mode - bundled with the default `longest` unless `--agent-msgs
    /// rich` is also passed).
    Heavy,
    /// Lean thresholds: threshold 8, rich-min 360, declaration-max 240 (master mode
    /// unchanged - bundled with the default `longest` unless `--agent-msgs rich` is passed).
    Light,
}

/// The resolved (profile + flag) configuration the selection + richness functions read.
/// Defaults to the `Longest` mode (keep the longest agent message + the first-if-
/// substantive + the rich middles); `run_threshold` is only consulted in `Rich` mode,
/// `rich_min_chars` in both `Longest` and `Rich`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RichnessCfg {
    /// The master switch (`Longest` is the default).
    pub mode: AgentMsgMode,
    /// Only filter a turn whose `agents.len() > run_threshold` (default 6); a run at or
    /// below keeps every agent message verbatim (cheap path, no false-negative risk).
    /// Consulted by `Rich` ONLY - `Longest` has no short-run escape.
    pub run_threshold: usize,
    /// Arm-1 length gate: an agent message with `chars >= rich_min_chars` is RICH on
    /// length alone (default 280 ≈ 1.5× the measured 184-char median middle). In
    /// `Longest` mode this ALSO gates the "keep the first if substantive" decision.
    pub rich_min_chars: usize,
    /// Drop-predicate upper bound: a signal-less intent-verb-opening message shorter than
    /// this is droppable; at/above it is kept (default 200 - the pure-declaration band).
    pub declaration_max_chars: usize,
    /// Honor the first-matters privilege (default true): the first agent message is kept
    /// UNCONDITIONALLY (position privilege - kept merely for being first, rich or not).
    /// `false` routes it through the identical middle-collapse decision instead.
    pub keep_first: bool,
}

impl Default for RichnessCfg {
    fn default() -> Self {
        // The DEFAULT keeps the longest agent message + the first-if-substantive + the
        // rich middles (`Longest`). The thresholds carry the documented defaults so a bare
        // `--agent-msgs rich` (no other flag) behaves exactly as specified, and so the
        // `Longest` first-substantive / rich-middle gates use `rich_min_chars`.
        RichnessCfg {
            mode: AgentMsgMode::Longest,
            run_threshold: 6,
            rich_min_chars: 280,
            declaration_max_chars: 200,
            keep_first: true,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Role + intermediate model
// ─────────────────────────────────────────────────────────────────────────────
