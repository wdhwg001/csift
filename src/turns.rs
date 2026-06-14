//! `turns` subcommand — turn-fidelity reconstruction.
//!
//! A Claude Code COMPACTION SUMMARY preserves task STATE (the 9-section synthesis:
//! intent, file ledger, errors+fixes, plan, next step) in high fidelity, but provably
//! LOSES turn fidelity — its "All user messages" section clips ~22 real prose turns to
//! ~17 `...`-truncated bullets, and the assistant side collapses to a SINGLE verbatim
//! quote (the last pre-compaction message). `turns` SUPPLEMENTS (never replaces) the
//! summary: it re-emits the clipped user phrasings + discarded assistant end-of-turn
//! replies, in ORIGINAL ORDER, each line carrying the jsonl LINE NUMBER so a consumer
//! can `Read` the raw transcript at the cited line.
//!
//! ## Reuse, never re-parse
//!
//! This module sits squarely on the `recover` extraction layer: the same
//! mmap → forward line-numbered [`crate::parse::scan_lines_bytes`] scan (so the local
//! `line_no` counter is a 1:1 map to the jsonl), the same
//! [`crate::model::group_turn_indices`] turn delimiter, the same `Record` helpers
//! ([`Record::is_genuine_user`] / [`Record::genuine_user_text`] /
//! [`Record::agent_text`] / [`Record::blocks`] / `is_compact_summary`), the same
//! [`crate::path::resolve_session_files`] / [`crate::time_window::TimeWindow`] /
//! [`crate::timez`] rendering. The `Record`/`Block` model needs no change.
//!
//! ## Selection vs render order
//!
//! Selection walks BACKWARD from EOF (recency-first) so the budget is spent on what a
//! resumed agent most needs; the emitted document is sorted ASCENDING so it reads as a
//! forward transcript. The backward walk is TRANSPARENT to `isCompactSummary` records
//! (a summary is a turn member, never a delimiter — `src/model.rs`), so it reaches back
//! across multiple compaction boundaries by default.
//!
//! ## Multi-agent-message model + richness filtering (the model-expansion)
//!
//! A single genuine-user turn can own a LONG RUN of agent messages — a debugging/build
//! chain the model narrates step by step — that a compaction summary clips to its single
//! §9 EOT quote. Each [`TurnSlice`] therefore carries `agents: Vec<AgentMsg>` (EVERY
//! agent-text record of the turn, in file order), and a derived `assistant_eot()`
//! accessor returns the LAST element — the EOT anchor — preserving the whole existing
//! dedup / round-trip / render call-graph with zero churn.
//!
//! Selection ([`select_agent_messages`]) reduces the run to a survivor set, gated by the
//! master `--agent-msgs` mode:
//!   - `eot-only` (DEFAULT, non-breaking) — keep only the last agent message; the output
//!     is byte-identical to the pre-expansion single-EOT document.
//!   - `rich` — on a LONG run (`agents.len() > run_threshold`, default 6): the LAST is
//!     always kept; the FIRST is kept by position privilege under `--keep-first`; each
//!     MIDDLE is kept UNLESS it is a PROVEN pure declaration. Collapsed contiguous runs
//!     fuse into one `△ L…  [X agent messages, Y tool calls, Z failed]` placeholder
//!     carrying the fetchable jsonl line range + the per-message tool/failed attribution.
//!   - `all` — keep every agent message (maximal fidelity, no placeholder).
//!
//! "Rich" ([`agent_msg_is_rich`]) is a cheap single-pass OR of a LENGTH gate (kept on
//! length alone ≥ `rich_min_chars`) and a SIGNAL test (a number-of-substance, a commit
//! hash, a `file.rs:NNN` ref, a backtick code path, or a finding/decision lexeme).
//! KEEP-ON-DOUBT is the spine: [`agent_msg_is_droppable`] collapses ONLY a short,
//! signal-less, intent-verb opener — everything uncertain is kept (a wrongly-kept
//! declaration costs ≤ one capped body; a wrongly-dropped finding is unrecoverable). A
//! FUSED finding+declaration body trips a signal → kept WHOLE; its trailing declaration
//! is shed only by the existing within-message `ASST_CAP` char-ellipsis, never by
//! whole-message drop. `--profile heavy|light` bundles the thresholds; defaults equal
//! today's behavior so the whole feature is dead code until a non-default mode is chosen.
//!
//! ## Never fabricate, never silently drop
//!
//! An over-cap unit is MIDDLE-truncated (head+tail kept) with an explicit
//! `… [+K chars, L lines elided] …` marker that carries the exact elided counts; its
//! `Lnnnnn` points at the full record. Dedup against the live summary is
//! DEMOTE-AND-FLAG, never delete. A collapsed agent-message run is NEVER silently
//! dropped either — its placeholder carries the exact counts + the line range to fetch.

use std::path::Path;

use anyhow::{bail, Result};
use memchr::memmem;
use rayon::prelude::*;

use crate::cli::{BudgetUnit, OutputFormat, TurnsArgs};
use crate::model::{group_turn_indices_deduped, normalize_line, Block, Content, PlanIndex, Record};
use crate::parse::{mmap_bytes, scan_lines_bytes};
use crate::path;
use crate::time_window::TimeWindow;
use crate::timez::{format_timestamp, local_iso};

/// Characters-per-token heuristic for `--budget-unit tokens`. CC compaction summaries
/// measure ~17K chars ≈ ~3.5–4.5K tokens, i.e. ~4 chars/token (documented estimate;
/// `turns` never tokenizes — it converts a token budget to a char budget by this ratio).
pub const TOKEN_CHARS: f64 = 4.0;

/// Pre-ellipsis full-keep ceiling for a USER unit (chars). Sized from the measured
/// user-message length distribution (median ~410, p90 ~2,574): a 600 cap keeps the
/// median turn whole and forces ellipsis only on the long tail.
const USER_CAP: usize = 600;

/// Pre-ellipsis full-keep ceiling for an ASSISTANT end-of-turn unit (chars). Larger
/// than [`USER_CAP`] because assistant EOT prose is 1.45–2.16× longer with more
/// newlines (measured) — so its head fraction is larger too (see [`ASST_HEAD_FRAC`]).
const ASST_CAP: usize = 900;

/// Headroom subtracted from `--window` to size the per-body cap in `--slices` (fixed-fleet) mode.
/// There the per-role 600/900 caps are REPLACED by a window cap: a body renders whole up to
/// `window - SLICE_BODY_HEADROOM`, so its rendered body LINE (incl. the `… [+K chars, L lines
/// elided] …` wrapper) AND the unit's separate header line each stay under one window — no single
/// line can force [`slice_into_windows`] to hard-split mid-content. Only a turn that ALONE exceeds
/// a window is ellipsized; everything else is kept verbatim (the user-directive recovery target).
const SLICE_BODY_HEADROOM: usize = 200;

/// Head fraction for an ASSISTANT unit's middle-truncation: EOT prose front-loads
/// context and back-loads the decision, so keep ≈⅔ head / ⅓ tail.
const ASST_HEAD_FRAC: f64 = 0.66;

/// Head fraction for a USER unit: the ask is front-loaded, slightly less tail needed.
const USER_HEAD_FRAC: f64 = 0.60;

/// Cost of the trailing `\n` that follows every emitted physical line (header lines,
/// body lines, marker lines, banner lines, and every line of the document header block).
/// Each `emit` callback appends exactly one `\n`, so every line the document contains
/// pays for it — charging it is what makes the summed cost equal the real emitted length.
const NEWLINE_COST: usize = 1;

/// Normalized-prefix length used for the summary-dedup fingerprint (§6.2): a unit whose
/// first `DEDUP_PREFIX` normalized chars match a summary bullet/quote is flagged
/// `also_in_summary` and demoted. Strict (long) prefix ⇒ a false positive is unlikely.
const DEDUP_PREFIX: usize = 80;

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
// SUBSTANTIVE Rich Response — the actual finding, the committed answer, the design — sits
// in a MIDDLE message. The pre-feature default kept `agents.last()`, so it silently
// DROPPED the substance of exactly those turns. The default now keeps the LONGEST agent
// message (by `full_chars`), which is the single best one-message proxy for "where the
// substance is". On a tie `max_by_key` returns the LAST maximum, so an all-equal run
// coincides with the old `agents.last()` pick.
//
// But "more than one message matters" is common, so `Longest` ALSO keeps:
//   • the LONGEST agent message — ALWAYS (the substantive Rich Response).
//   • the FIRST — when SUBSTANTIVE (`full_chars >= rich_min_chars`); the opening message
//     often states the plan / an early finding worth preserving.
//   • each MIDDLE that is RICH (`agent_msg_is_rich`); a major finding can live mid-run.
//   • everything else collapses into a placeholder.
// `--agent-rich-min-chars` tunes BOTH the first-substantive gate and the rich length arm.
//
// `Rich` is the OLDER keep-set, retained as an explicit mode (a long run only; short runs
// keep all): LAST always + FIRST by position privilege (under `--keep-first`) + each
// non-droppable MIDDLE.
//
//   • LAST agent message  — ALWAYS kept by `Rich` (the outcome / EOT anchor).
//   • FIRST agent message — kept UNCONDITIONALLY under `--keep-first` (position
//     privilege — kept merely for being first, even when not rich); with
//     `--no-keep-first` it is decided exactly as a MIDDLE.
//   • MIDDLE agent messages — kept UNLESS a PROVEN pure declaration; else collapsed
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
// EOT behavior (only `agents.last()`, byte-identical to the pre-expansion output) — the
// "force last-only" escape. `AgentMsgMode::All` keeps every message (no filtering). The
// `Rich` mode's filtering is only attempted on a LONG run (`agents.len() > run_threshold`,
// default 6); short runs keep every message. `Longest` applies its longest+heuristic pick
// to EVERY multi-message turn (no short-run escape — the headline "long Rich Response then
// 50-char wrap-up" turn is only two messages, yet must still drop the wrap-up).

/// How a turn's agent-message run is reduced to a survivor set. The MASTER switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, Default)]
pub enum AgentMsgMode {
    /// DEFAULT. Keep the LONGEST agent message (by `full_chars` — the substantive Rich
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
    /// Keep the last always + the first by position privilege (under `--keep-first`) +
    /// each rich middle; collapse the proven pure declarations. Gated by the run
    /// threshold (short runs keep all).
    Rich,
    /// Keep EVERY agent message — no filtering, no collapse (maximal-fidelity escape).
    All,
}

/// A convenience bundle of richness thresholds an LLM caller picks by reading the
/// compaction summary it supplements (heavy = restore the debugging narrative; light =
/// the summary is already rich, just restore phrasings + EOTs). Applied BEFORE the
/// individual flags so an explicit flag overrides the profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Profile {
    /// Maximal fidelity thresholds: threshold 4, rich-min 200, declaration-max 140 (does
    /// not change the master mode — bundled with the default `longest` unless `--agent-msgs
    /// rich` is also passed).
    Heavy,
    /// Lean thresholds: threshold 8, rich-min 360, declaration-max 240 (master mode
    /// unchanged — bundled with the default `longest` unless `--agent-msgs rich` is passed).
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
    /// Consulted by `Rich` ONLY — `Longest` has no short-run escape.
    pub run_threshold: usize,
    /// Arm-1 length gate: an agent message with `chars >= rich_min_chars` is RICH on
    /// length alone (default 280 ≈ 1.5× the measured 184-char median middle). In
    /// `Longest` mode this ALSO gates the "keep the first if substantive" decision.
    pub rich_min_chars: usize,
    /// Drop-predicate upper bound: a signal-less intent-verb-opening message shorter than
    /// this is droppable; at/above it is kept (default 200 — the pure-declaration band).
    pub declaration_max_chars: usize,
    /// Honor the first-matters privilege (default true): the first agent message is kept
    /// UNCONDITIONALLY (position privilege — kept merely for being first, rich or not).
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

/// One side of a turn the reconstruction can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
}

impl Role {
    fn label(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }

    /// The pre-ellipsis full-keep ceiling for this role.
    fn cap(self) -> usize {
        match self {
            Role::User => USER_CAP,
            Role::Assistant => ASST_CAP,
        }
    }

    /// The head fraction for this role's middle-truncation.
    fn head_frac(self) -> f64 {
        match self {
            Role::User => USER_HEAD_FRAC,
            Role::Assistant => ASST_HEAD_FRAC,
        }
    }
}

/// One side of a turn (user opener OR assistant end-of-turn), with the full verbatim
/// text plus the data needed to render + budget it.
#[derive(Debug, Clone)]
struct TurnUnit {
    line_no: usize,
    role: Role,
    /// `chars().count()` of the normalized one-line full text.
    full_chars: usize,
    /// Normalized one-line text (rendered verbatim or middle-truncated later).
    text: String,
    /// Newline count of the ORIGINAL (pre-normalization) text — drives the
    /// `L lines elided` note (omitted when 0, i.e. a single-line message).
    orig_newlines: usize,
    ts_utc: Option<String>,
    /// True once dedup flags this unit as already present in the newest summary.
    also_in_summary: bool,
}

/// Position of an agent message within its turn's ordered agent-message run. A
/// 1-message turn's sole agent message is BOTH first and last → classified `Last` (the
/// always-keep anchor / EOT). Drives the `--keep-first` privilege and the always-keep
/// rule for the outcome message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentPos {
    First,
    Middle,
    Last,
}

/// One agent-text record in a turn (model-expansion: a turn now carries EVERY agent
/// message, not just the last EOT). Reuses [`TurnUnit`] verbatim for render+cost, and
/// carries the per-message tool/failed attribution the collapse placeholder needs.
#[derive(Debug, Clone)]
struct AgentMsg {
    /// The render/cost unit (line_no, full_chars, text, orig_newlines, ts_utc,
    /// also_in_summary, role = Assistant).
    unit: TurnUnit,
    /// First / Middle / Last within the turn's agent run (assigned after the push loop).
    pos: AgentPos,
    /// `tool_use` blocks in records strictly between the previous agent-text record (or
    /// turn start) and THIS one — the per-message attribution the placeholder `Y` needs.
    preceding_tool_calls: usize,
    /// erroring `tool_result` blocks in that same preceding span — placeholder `Z`.
    preceding_failed: usize,
}

/// One reconstructable turn: the user opener, the turn-wide tool-call count, and the
/// ORDERED run of every agent-text message in the turn (the model-expansion — replaces
/// the single `assistant_eot`). The derived `assistant_eot()` accessor keeps the old
/// "last == EOT" anchor for dedup / round-trip / render compatibility.
#[derive(Debug, Clone)]
struct TurnSlice {
    /// 0-based genuine-user turn index (from `group_turn_indices`).
    turn_index: usize,
    user: Option<TurnUnit>,
    /// `tool_use` block count across the turn → the `[N tool calls]` marker.
    tool_calls: usize,
    /// Every agent-text record in the turn, in file order (ascending line_no). EMPTY
    /// for a pure tool-call turn. The LAST element is the EOT anchor (`assistant_eot()`).
    agents: Vec<AgentMsg>,
    /// How many compaction boundaries sit between this turn and EOF (drives the
    /// boundary banners + dedup scope).
    compactions_before: usize,
    /// True when this turn's opener is a MACHINE-INJECTED automation trigger
    /// (`<task-notification>`) rather than a human message. The turn is still a real
    /// boundary (it opens a turn) and is selected/budgeted normally, but the header
    /// reports the automation/human split so a consumer sees which "user turns" were
    /// machine pulses (e.g. `selected 19 user units (3 automation triggers)`).
    is_automation: bool,
    /// The parsed automation trigger (kind / task-id / status / summary) when
    /// `is_automation` — `None` for a human turn. Carried so the JSON user-segment object
    /// can surface the trigger CLASS as STRUCTURED fields (`is_automation` / `trigger_kind`
    /// / `task_id` / `status`), not only as the inline `[<kind> …]` text prefix a consumer
    /// would otherwise have to regex out of the prose.
    automation: Option<crate::model::AutomationTrigger>,
}

impl TurnSlice {
    /// A round-trip-complete turn has BOTH a user opener and at least one agent message
    /// (the last of which is the EOT anchor). NOTE: this is the STRUCTURAL test — it counts
    /// an automation-pulse opener (`<task-notification>`) the same as a human opener; it
    /// governs Phase-2 fill (whether a `Both` selection is offered). The Phase-1 HARD FLOOR
    /// uses [`TurnSlice::is_human_round_trip`] instead, so a machine pulse never consumes the
    /// human-reserved `--round-trip-fraction` budget lane.
    fn is_round_trip(&self) -> bool {
        self.user.is_some() && !self.agents.is_empty()
    }

    /// A round-trip whose opener is a GENUINE HUMAN message (not an automation pulse). This
    /// is what the `--round-trip-fraction` HARD FLOOR reserves its budget for — the help /
    /// SKILL define that lane as "COMPLETE round-trips (user → … → assistant EOT)", i.e.
    /// human exchanges. An automation `<task-notification>` paired with an agent ack is a
    /// structural round-trip but NOT a human one, so it is excluded from the floor (it can
    /// still be picked in Phase-2 fill). Keeps the floor accounting and the header's
    /// human/automation split in agreement.
    fn is_human_round_trip(&self) -> bool {
        self.is_round_trip() && !self.is_automation
    }

    /// The EOT anchor: the LAST agent message's unit (the turn's outcome/decision, the
    /// dedup + round-trip key). `None` for a pure tool-call turn. This derived accessor
    /// preserves the whole existing call-graph that keyed on the old `assistant_eot`
    /// field with zero behavioural churn.
    fn assistant_eot(&self) -> Option<&TurnUnit> {
        self.agents.last().map(|a| &a.unit)
    }

    /// Mutable EOT-anchor accessor (dedup flips `also_in_summary` on it).
    fn assistant_eot_mut(&mut self) -> Option<&mut TurnUnit> {
        self.agents.last_mut().map(|a| &mut a.unit)
    }
}

/// The data a summary record contributes: its jsonl line + the dedup fingerprints of
/// the verbatim turns it already holds (§6 user bullets + §9 assistant quote).
#[derive(Debug, Clone)]
struct SummaryInfo {
    line_no: usize,
    /// Normalized-prefix fingerprints of everything the summary quotes verbatim.
    fingerprints: Vec<String>,
    /// Char length of the summary body (for the JSON boundary record).
    body_chars: usize,
}

/// A per-session scan result before global merge.
#[derive(Debug)]
struct ScanResult {
    session_id: String,
    /// True when this transcript is a SUBAGENT (so `session_id` is a bare hex, NOT a
    /// re-feedable `--session` target) — the r5 id-domain discriminator, now also on turns
    /// JSON (the text path already brands a subagent block `(subagent transcript)`).
    is_subagent: bool,
    /// The re-feedable PARENT session uuid (= `session_id` for a top-level file).
    parent_session_id: String,
    turns: Vec<TurnSlice>,
    /// Summary records in file order (oldest → newest), each with its line + dedup set.
    summaries: Vec<SummaryInfo>,
    skipped_lines: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Entry point for `csift turns`.
pub fn run_turns(args: &TurnsArgs) -> Result<()> {
    // Pointed error if the files-only `--subagents-only` was mistyped here.
    if let Some(msg) = args.span_flag_error() {
        bail!(msg);
    }
    // ── Validate window mutual-exclusion (same rule + wording as recover/files) ──
    if args.turn_range.is_some() && (args.since.is_some() || args.until.is_some()) {
        bail!("--turn-range is mutually exclusive with --since/--until");
    }
    if !(args.round_trip_fraction > 0.0 && args.round_trip_fraction < 1.0) {
        bail!(
            "--round-trip-fraction must be in the open interval (0.0, 1.0), got {}",
            args.round_trip_fraction
        );
    }
    if args.slices.is_none() && args.budget == 0 {
        bail!("--budget must be > 0");
    }
    if let Some(n) = args.slices {
        if n == 0 {
            bail!("--slices must be > 0 (it pins the fleet to N chunks)");
        }
        if args.slice.is_none() {
            bail!("--slices N sets the fleet size; pass --slice i to pick which chunk to emit");
        }
    }
    // ── Validate --slice / --window (chunked-output mode) ──
    if let Some(slice) = args.slice {
        if slice == 0 {
            bail!("--slice is 1-based: the first chunk is --slice 1");
        }
        if args.window == 0 {
            bail!("--window must be > 0");
        }
        if args.out.is_some() {
            bail!(
                "--slice and --out are mutually exclusive: --slice writes the selected chunk \
                 to stdout, --out writes the whole document to a file"
            );
        }
        if matches!(args.format, OutputFormat::Json) {
            bail!(
                "--slice requires the text format (the chunked-injection use case is verbatim \
                 text); drop --format json"
            );
        }
    }

    // Normalize the budget to characters. `--slices N` pins the FLEET size, so the budget is
    // derived as N windows (the slice COUNT is the hard constraint — a fixed set of registered
    // hooks must never need to grow); otherwise it is the requested char/token amount.
    let budget_chars = if let Some(n) = args.slices {
        n.saturating_mul(args.window)
    } else {
        match args.budget_unit {
            BudgetUnit::Chars => args.budget,
            // round-half-up; saturate so a giant token budget never overflows usize.
            BudgetUnit::Tokens => ((args.budget as f64) * TOKEN_CHARS).round() as usize,
        }
    };

    let turn_range = args
        .turn_range
        .as_deref()
        .map(parse_turn_range)
        .transpose()?;
    let time_window = TimeWindow::from_args(args.since.as_deref(), args.until.as_deref())?;

    let session_files = path::resolve_session_files(
        &args.paths,
        args.session.as_deref(),
        args.want_subagents().into(),
        path::Caller::Other,
    )?;

    // Parallel scan across files (default rayon pool = CPU count).
    let per_file: Vec<ScanResult> = session_files
        .par_iter()
        .map(|p| scan_one_file(p))
        .collect::<Result<Vec<_>>>()?;

    let mut skipped_lines = 0usize;
    let mut sessions: Vec<ScanResult> = Vec::new();
    for sr in per_file {
        skipped_lines += sr.skipped_lines;
        sessions.push(sr);
    }
    sessions.sort_by(|a, b| a.session_id.cmp(&b.session_id));

    // Apply the window to the turns of each session (a turn admitted by turn-index /
    // its user/assistant timestamp). The summary dedup set is computed from the FULL
    // (un-windowed) newest summary so a window never silently un-dedups.
    for sr in &mut sessions {
        sr.turns.retain(|t| {
            let ts = t
                .user
                .as_ref()
                .and_then(|u| u.ts_utc.as_deref())
                .or_else(|| t.assistant_eot().and_then(|a| a.ts_utc.as_deref()));
            window_admits(t.turn_index, ts, turn_range, &time_window)
        });
    }

    // Resolve the richness configuration (master mode + thresholds, profile applied
    // first then explicit flags). Defaults to EotOnly → today's single-EOT behavior.
    let cfg = args.richness_cfg();

    let plans: Vec<SessionPlan> = sessions
        .iter()
        .map(|sr| {
            plan_session(
                sr,
                budget_chars,
                args.round_trip_fraction,
                args.max_compactions,
                &cfg,
            )
        })
        .collect();

    let ctx = RenderCtx {
        budget_chars,
        rt_fraction: args.round_trip_fraction,
        skipped_lines,
        cfg,
    };

    match args.format {
        OutputFormat::Text => render_text(
            &ctx,
            &sessions,
            &plans,
            args.out.as_deref(),
            args.slice,
            args.window,
            args.slices,
        )?,
        OutputFormat::Json => render_json(&ctx, &sessions, &plans, args.out.as_deref())?,
    }
    Ok(())
}

/// True when a turn at `turn_index` / `ts` is admitted by the active window. A
/// timestamp-less turn never falls inside a BOUNDED time window (same rule as recover).
fn window_admits(
    turn_index: usize,
    ts: Option<&str>,
    turn_range: Option<(usize, usize)>,
    time_window: &TimeWindow,
) -> bool {
    if let Some((lo, hi)) = turn_range {
        if turn_index < lo || turn_index > hi {
            return false;
        }
    }
    time_window.contains(ts)
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-file scan
// ─────────────────────────────────────────────────────────────────────────────

/// Scan one session file: mmap → forward line-numbered scan → build the per-turn
/// `TurnSlice`s + the per-summary dedup sets. The forward `scan_lines_bytes` path is
/// mandatory (NOT head/tail): it visits every line including blanks, so the local
/// counter == the true jsonl line (the recover discipline, reused verbatim).
fn scan_one_file(path: &Path) -> Result<ScanResult> {
    // Canonical bare-hex id (subagent `agent-` prefix stripped) — the SAME derivation
    // every other surface uses, so a `turns` subagent unit's `session_id` is joinable to
    // `files`/`search`/`recover`/`agents` (id-form unification; a top-level uuid is
    // unaffected). See [`crate::subagent::session_id_from_path`].
    let session_id = crate::subagent::session_id_from_path(path);
    // Id-domain discriminator (the r5 shape, now on turns JSON): a subagent transcript's
    // `session_id` is a non-re-feedable bare hex; carry `is_subagent` + the re-feedable
    // parent uuid (the dir before `subagents/`). A top-level file is its own parent.
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());

    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(ScanResult {
            session_id,
            is_subagent,
            parent_session_id,
            turns: Vec::new(),
            summaries: Vec::new(),
            skipped_lines: 0,
        });
    };
    let bytes: &[u8] = &mmap;

    let mut records: Vec<(usize, Record)> = Vec::new();
    let mut skipped = 0usize;
    let mut line_no = 0usize;
    scan_lines_bytes(bytes, |line| {
        line_no += 1;
        if !line_is_turn_candidate(line) {
            return;
        }
        match crate::parse::parse_line(line) {
            Ok(Some(rec)) => records.push((line_no, rec)),
            Ok(None) => {}          // blank — counted above
            Err(_) => skipped += 1, // malformed — counted above
        }
    })?;

    let (turns, summaries) = build(&records);
    Ok(ScanResult {
        session_id,
        is_subagent,
        parent_session_id,
        turns,
        summaries,
        skipped_lines: skipped,
    })
}

/// Pre-JSON byte prefilter — a SUPERSET of recover's `line_is_recover_candidate`,
/// broadened so a pure-text assistant turn (no Edit/Write/Read/Bash) is never missed.
/// Coarse by design; the structural parse decides what each line really is.
fn line_is_turn_candidate(line: &[u8]) -> bool {
    memmem::find(line, br#""role":"user""#).is_some()
        || memmem::find(line, br#""role":"assistant""#).is_some()
        || memmem::find(line, br#""type":"assistant""#).is_some()
        || memmem::find(line, b"isCompactSummary").is_some() // summaries: seeds + boundaries
        || memmem::find(line, b"tool_use").is_some() // for the [N tool calls] count
}

// ─────────────────────────────────────────────────────────────────────────────
// Build: (line_no, Record) → TurnSlice + SummaryInfo
// ─────────────────────────────────────────────────────────────────────────────

/// Build the per-turn slices + summary dedup sets from a session's line-numbered
/// records. Turn segmentation reuses the single shared engine
/// [`group_turn_indices_deduped`], so an esc-cancel / edit-resend draft never surfaces as a
/// phantom turn (§6.4.1); a compaction summary is a turn MEMBER (it is excluded from
/// genuine-user), so the walk is transparent to it.
fn build(records: &[(usize, Record)]) -> (Vec<TurnSlice>, Vec<SummaryInfo>) {
    let recs: Vec<&Record> = records.iter().map(|(_, r)| r).collect();
    let turns = group_turn_indices_deduped(&recs, |r| *r);
    // ExitPlanMode plan pointers for this session, so a rejection-with-message turn
    // opener can surface `[plan: <path>]` (§4.2.4). Cheap; empty in a no-plan session.
    let plan_index = PlanIndex::from_records(recs.iter().copied());

    // Summary line numbers in file order (for compactions_before + boundary banners).
    let mut summaries: Vec<SummaryInfo> = Vec::new();
    for (line_no, rec) in records {
        if rec.is_compact_summary.unwrap_or(false) {
            if let Some(body) = compact_summary_body(rec) {
                summaries.push(SummaryInfo {
                    line_no: *line_no,
                    fingerprints: summary_fingerprints(&body),
                    body_chars: body.chars().count(),
                });
            }
        }
    }
    // Newest summary line (max), for compactions_before accounting.
    let summary_lines: Vec<usize> = summaries.iter().map(|s| s.line_no).collect();

    let mut slices: Vec<TurnSlice> = Vec::with_capacity(turns.len());
    for (turn_index, idxs) in turns.iter().enumerate() {
        let mut user: Option<TurnUnit> = None;
        let mut is_automation = false;
        let mut automation: Option<crate::model::AutomationTrigger> = None;
        let mut agents: Vec<AgentMsg> = Vec::new();
        let mut tool_calls = 0usize;
        // Per-message attribution: tool_use / erroring tool_result blocks seen since the
        // PREVIOUS agent-text record (or turn start). Consumed (and zeroed) on each push.
        let mut pending_tool_calls = 0usize;
        let mut pending_failed = 0usize;

        for &i in idxs {
            let (line_no, rec) = (records[i].0, &records[i].1);

            // Tool-call + erroring-tool-result counts for THIS record. The turn-wide
            // `tool_calls` accumulator (the `[N tool calls]` marker) is unchanged; the
            // `pending_*` counters additionally attribute the span to the NEXT agent msg.
            if let Some(blocks) = rec.blocks() {
                for b in blocks {
                    match b {
                        Block::ToolUse { .. } => {
                            tool_calls += 1;
                            pending_tool_calls += 1;
                        }
                        Block::ToolResult {
                            is_error: Some(true),
                            ..
                        } => {
                            pending_failed += 1;
                        }
                        _ => {}
                    }
                }
            }

            // The turn opener (genuine human, an answered AskUserQuestion, or a tool-use
            // rejection-with-message). `group_turn_indices` opens a turn on `opens_turn`,
            // so the FIRST such record is the opener; keep the earliest. The rendered
            // body is the unified reconstruction (Q+options+answer for an AUQ; the typed
            // instruction + a `[plan: …]` pointer for a rejection).
            if user.is_none() && rec.opens_turn() {
                // An automation trigger (`<task-notification>`) opens a turn like a human
                // message, but its body must render as the parsed `[workflow <id> …]
                // <summary>` ATTRIBUTION label — never the raw `<task-id>`/`<output-file>`
                // XML wrapper. `automation_label` wins; otherwise the normal user-text
                // reconstruction applies.
                if let Some(label) = rec.automation_label() {
                    is_automation = true;
                    automation = rec.automation_trigger();
                    user = Some(make_unit(line_no, Role::User, &label, rec));
                } else if let Some(text) = rec.reconstructed_user_text(Some(&plan_index)) {
                    user = Some(make_unit(line_no, Role::User, &text, rec));
                }
            }

            // EVERY agent-text record becomes an AgentMsg (the model-expansion). The
            // pending tool/failed counters since the previous agent record are attributed
            // to THIS message (the placeholder's per-message Y / Z), then zeroed.
            if let Some(text) = rec.agent_text() {
                agents.push(AgentMsg {
                    unit: make_unit(line_no, Role::Assistant, &text, rec),
                    // Provisional; reassigned by AgentPos after the loop.
                    pos: AgentPos::Last,
                    preceding_tool_calls: pending_tool_calls,
                    preceding_failed: pending_failed,
                });
                pending_tool_calls = 0;
                pending_failed = 0;
            }
        }

        // Assign positions: index 0 → First, last index → Last, the rest → Middle. A
        // single-element vec → that element is Last (the always-keep anchor: a 1-message
        // turn's sole reply is BOTH first and last, never dropped).
        let last = agents.len().saturating_sub(1);
        for (i, a) in agents.iter_mut().enumerate() {
            a.pos = if i == last {
                AgentPos::Last
            } else if i == 0 {
                AgentPos::First
            } else {
                AgentPos::Middle
            };
        }

        // `compactions_before` is keyed on the turn's CONTENT lines (its user opener /
        // agent messages), NOT on member records like a trailing summary that joins the
        // turn — a summary that opens a NEW compacted region must sit AFTER this turn's
        // content, so count summaries strictly above the turn's latest content line.
        let content_line = user
            .as_ref()
            .map(|u| u.line_no)
            .into_iter()
            .chain(agents.iter().map(|a| a.unit.line_no))
            .max()
            .unwrap_or(0);
        let compactions_before = summary_lines.iter().filter(|&&s| s > content_line).count();

        slices.push(TurnSlice {
            turn_index,
            user,
            tool_calls,
            agents,
            compactions_before,
            is_automation,
            automation,
        });
    }

    (slices, summaries)
}

/// Build a [`TurnUnit`] from a record's already-normalized one-line `text`. The
/// `orig_newlines` count is taken from the record's ORIGINAL (pre-normalization) text so
/// the `L lines elided` note is meaningful.
fn make_unit(line_no: usize, role: Role, text: &str, rec: &Record) -> TurnUnit {
    let orig_newlines = raw_body_newlines(rec);
    TurnUnit {
        line_no,
        role,
        full_chars: text.chars().count(),
        text: text.to_string(),
        orig_newlines,
        ts_utc: rec.timestamp.clone(),
        also_in_summary: false,
    }
}

/// Count newlines in a record's ORIGINAL message body (pre-normalization) — the basis
/// for the `L lines elided` note. A bare-string body is counted as-is; a block body is
/// the visible `text` blocks joined with `\n` (matching how they would print). Returns 0
/// when the body is unavailable (→ note omitted).
fn raw_body_newlines(rec: &Record) -> usize {
    let Some(msg) = rec.message.as_ref() else {
        return 0;
    };
    let Some(content) = msg.content.as_ref() else {
        return 0;
    };
    let raw = match content {
        Content::Text(s) => s.clone(),
        Content::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } if !text.trim().is_empty() => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };
    raw.matches('\n').count()
}

/// The body text of a compaction-summary record (a `type:"user"` `isCompactSummary`
/// record carrying string content). `genuine_user_text` filters summaries out, so we
/// read the `Content::Text` body directly. Returns `None` if it is not a string body.
fn compact_summary_body(rec: &Record) -> Option<String> {
    let content = rec.message.as_ref()?.content.as_ref()?;
    match content {
        Content::Text(s) => Some(s.clone()),
        // A summary always carries STRING content in real data; a block body would be
        // a genuine surprise — return None rather than guess.
        Content::Blocks(_) => None,
    }
}

/// Extract dedup fingerprints from a summary body: the §6 "All user messages" bullets
/// and the §9 verbatim last-assistant quote (the only verbatim turns a summary holds).
/// Each fingerprint is `normalize_line(text).to_lowercase()` truncated to the first
/// [`DEDUP_PREFIX`] chars. Conservative: when the structured sections are not found,
/// every `- ` bullet line in the body is fingerprinted (a superset — still strict per
/// line). Robust to summaries that omit the exact headers.
fn summary_fingerprints(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim_start();
        // §6 bullets render as `- "<text>" …` (quoted) or `- <text>` (bare); §9 quote is
        // prose carrying a quoted run. Prefer the QUOTED inner (the verbatim turn text);
        // for an UNQUOTED bullet fall back to the whole bullet body. A bullet WITH quotes
        // but an empty inner contributes nothing (it would only fingerprint the quotes).
        if let Some(rest) = trimmed.strip_prefix("- ") {
            let fp = match quoted_inner(rest) {
                Some(q) => fingerprint(&q),
                None => fingerprint(rest),
            };
            if !fp.is_empty() {
                out.push(fp);
            }
        } else if let Some(inner) = quoted_inner(trimmed) {
            let fp = fingerprint(&inner);
            if !fp.is_empty() {
                out.push(fp);
            }
        }
    }
    out
}

/// The text inside the FIRST pair of double-quotes on a line (the §9 quote / a quoted
/// §6 bullet body), or `None` if the line has no quoted run.
fn quoted_inner(s: &str) -> Option<String> {
    let start = s.find('"')?;
    let rest = &s[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Normalized-prefix fingerprint of a candidate text (lowercased, whitespace-collapsed,
/// first [`DEDUP_PREFIX`] chars). Empty input → empty (never matches).
fn fingerprint(s: &str) -> String {
    let normalized = normalize_line(s).to_lowercase();
    normalized.chars().take(DEDUP_PREFIX).collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Render cost model + ellipsis
// ─────────────────────────────────────────────────────────────────────────────

/// The rendered form of a unit's text body (verbatim if ≤ cap, else middle-truncated)
/// plus the metadata a JSON consumer needs.
#[derive(Debug, Clone)]
struct RenderedUnit {
    body: String,
    rendered_chars: usize,
    truncated: bool,
    elided_chars: usize,
    elided_lines: usize,
}

/// Middle-truncate a unit to a cap (`cap_override` when set — the fixed-fleet `--slices` window
/// cap that keeps whole turns — else the unit's per-role cap), keeping head+tail, with an explicit
/// elided marker. A unit at or below the cap renders verbatim. The cut is on `char` boundaries
/// (never mid-codepoint). The `L lines elided` note is included only when the original text spanned
/// ≥1 newline.
fn render_unit_body(unit: &TurnUnit, cap_override: Option<usize>) -> RenderedUnit {
    let cap = cap_override.unwrap_or_else(|| unit.role.cap());
    let chars: Vec<char> = unit.text.chars().collect();
    let total = chars.len();
    if total <= cap {
        return RenderedUnit {
            body: unit.text.clone(),
            rendered_chars: total,
            truncated: false,
            elided_chars: 0,
            elided_lines: 0,
        };
    }
    let head_keep = ((cap as f64) * unit.role.head_frac()).round() as usize;
    let head_keep = head_keep.min(cap);
    let tail_keep = cap - head_keep;
    let head: String = chars[..head_keep].iter().collect();
    let tail: String = chars[total - tail_keep..].iter().collect();
    let elided_chars = total - cap;
    // Lines elided: original newline count, surfaced only for multi-line bodies. The
    // rendered one-line form has no newlines, so we report the original's count as the
    // magnitude the consumer should expect in the raw record.
    let elided_lines = unit.orig_newlines;
    let nl_note = if elided_lines > 0 {
        format!(", {elided_lines} lines elided")
    } else {
        String::new()
    };
    let body = format!("{head} … [+{elided_chars} chars{nl_note}] … {tail}");
    RenderedUnit {
        body,
        rendered_chars: cap,
        truncated: true,
        elided_chars,
        elided_lines,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Richness function (the content gate) + agent-message selection
// ─────────────────────────────────────────────────────────────────────────────

/// The fixed substance-noun set the number-of-substance signal looks near (Arm 2a).
const SUBSTANCE_NOUNS: &[&str] = &[
    "passed", "failed", "tests", "test", "errors", "error", "files", "file", "chars", "lines",
    "line", "ops", "cases", "case",
];

/// The fixed finding/decision lexeme set (Arm 2e), case-insensitive substring. Includes
/// a CJK set matched on the normalized String (codepoint-safe). A message carrying any
/// of these is a finding/decision worth keeping at any length.
const FINDING_LEXEMES: &[&str] = &[
    "found",
    "confirmed",
    "verified",
    "proven",
    "proof",
    "root cause",
    "root-cause",
    "defer",
    "deferred",
    "fails",
    "failed",
    "failure",
    "error",
    "bug",
    "correction",
    "corrected",
    "fix",
    "fixed",
    "regression",
];

/// The intent-verb openers that mark a PURE declaration (Arm of the drop predicate),
/// case-insensitive prefix on the first ~24 trimmed chars. A message that opens with one
/// of these, is short, and carries no signal is the only thing collapsed.
const INTENT_VERB_OPENERS: &[&str] = &[
    "let me", "i'll", "i will", "now i", "now let", "next i", "next,", "let's",
];

/// Does a normalized agent message carry important info? A SHORT-CIRCUIT OR of two keep
/// arms over the normalized one-line text: ARM 1 the length gate (kept on length alone
/// when ≥ `rich_min_chars`), ARM 2 the signal test (a number-of-substance / commit hash /
/// file:line ref / backtick code path / finding-or-decision lexeme). Keep-on-doubt: this
/// returns true on ANY signal; the separate [`agent_msg_is_droppable`] is what proves a
/// message is a pure declaration safe to collapse.
fn agent_msg_is_rich(text: &str, cfg: &RichnessCfg) -> bool {
    // ARM 1 — LENGTH GATE.
    if text.chars().count() >= cfg.rich_min_chars {
        return true;
    }
    // ARM 2 — SIGNAL TEST (first match wins; single cheap scan per arm).
    let lower = text.to_lowercase();
    signal_number_of_substance(&lower)
        || signal_commit_hash(&lower)
        || signal_file_line_ref(text)
        || signal_backtick_code(text)
        || signal_finding_lexeme(&lower)
}

/// Arm 2a — a NUMBER-OF-SUBSTANCE: a ≥2-digit run, or an `N / M` / `N of M` ratio, sitting
/// within a ±16-char window of one of [`SUBSTANCE_NOUNS`]. Byte scan for ASCII digits,
/// then a bounded-window noun check — never a full regex pass. Operates on lowercased text.
fn signal_number_of_substance(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        if bytes[i].is_ascii_digit() {
            // Extent of this digit run.
            let start = i;
            while i < n && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let run_len = i - start;
            // An "N / M" or "N of M" ratio: a digit run, optional spaces, '/' or "of",
            // optional spaces, another digit run → always substance, no noun needed.
            if ratio_follows(bytes, i) {
                return true;
            }
            // A ≥2-digit integer within a ±16-byte window of a substance noun. The window
            // bounds are BYTE offsets that may land mid-codepoint (CJK text), so snap `lo`
            // DOWN and `hi` UP to the nearest char boundary before slicing — slicing a
            // non-boundary index panics. The substance nouns are ASCII, so a slightly wider
            // (boundary-snapped) window never changes a match decision.
            if run_len >= 2 {
                let mut lo = start.saturating_sub(16);
                while lo > 0 && !lower.is_char_boundary(lo) {
                    lo -= 1;
                }
                let mut hi = (i + 16).min(n);
                while hi < n && !lower.is_char_boundary(hi) {
                    hi += 1;
                }
                let window = &lower[lo..hi];
                if SUBSTANCE_NOUNS.iter().any(|noun| window.contains(noun)) {
                    return true;
                }
            }
            continue;
        }
        i += 1;
    }
    false
}

/// True when, starting at byte `i` (just past a digit run), an `[/ ]` or `of` separator
/// then another digit run forms a ratio (`12/40`, `3 of 5`).
fn ratio_follows(bytes: &[u8], mut i: usize) -> bool {
    let n = bytes.len();
    while i < n && bytes[i] == b' ' {
        i += 1;
    }
    if i < n && bytes[i] == b'/' {
        i += 1;
    } else if i + 1 < n && &bytes[i..i + 2] == b"of" {
        i += 2;
    } else {
        return false;
    }
    while i < n && bytes[i] == b' ' {
        i += 1;
    }
    i < n && bytes[i].is_ascii_digit()
}

/// Arm 2b — a COMMIT-HASH-LIKE HEX: a maximal `[0-9a-f]` run of length 7..=40 containing
/// at least one a–f letter (excludes plain decimals already caught by Arm 2a). Operates
/// on lowercased text.
fn signal_commit_hash(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    let n = bytes.len();
    let is_hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    let mut i = 0;
    while i < n {
        // A run must not be glued to a longer alnum token (so `deadbeef` inside a word is
        // still a run, but `g1a2b3c` won't include the leading g). Start a run on a hex
        // byte whose predecessor is not alphanumeric.
        let prev_alnum = i > 0 && bytes[i - 1].is_ascii_alphanumeric();
        if is_hex(bytes[i]) && !prev_alnum {
            let start = i;
            let mut has_alpha = false;
            while i < n && is_hex(bytes[i]) {
                if bytes[i].is_ascii_alphabetic() {
                    has_alpha = true;
                }
                i += 1;
            }
            let len = i - start;
            // The run must END at a non-alphanumeric boundary too (reject `a1b2c3z...`).
            let next_alnum = i < n && bytes[i].is_ascii_alphanumeric();
            if (7..=40).contains(&len) && has_alpha && !next_alnum {
                return true;
            }
            continue;
        }
        i += 1;
    }
    false
}

/// Arm 2c — a FILE-AND-LINE REF: a `name.rs:NNN` shape (a token with a `.` + alpha
/// extension followed by `:` + digits) OR a `src/…` / `tests/…`-rooted path token.
/// Operates on the ORIGINAL (case-preserving) text — paths are case-sensitive.
fn signal_file_line_ref(text: &str) -> bool {
    for tok in text.split(|c: char| c.is_whitespace()) {
        let tok = tok.trim_matches(|c: char| matches!(c, '`' | '(' | ')' | ',' | ';' | '"'));
        // `name.ext:NNN` — a dot, an alpha extension, a colon, then ≥1 digit.
        if let Some(colon) = tok.rfind(':') {
            let (path, after) = tok.split_at(colon);
            let line_part = &after[1..];
            if !line_part.is_empty()
                && line_part.bytes().all(|b| b.is_ascii_digit())
                && path_has_alpha_extension(path)
            {
                return true;
            }
        }
        // A `src/…` or `tests/…`-rooted path token (a file ledger reference).
        if (tok.starts_with("src/") || tok.starts_with("tests/")) && tok.len() > 4 {
            return true;
        }
    }
    false
}

/// True when `path` ends in `.<alpha…>` (a file extension), e.g. `turns.rs`, `foo.py`.
fn path_has_alpha_extension(path: &str) -> bool {
    match path.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty() && !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphabetic())
        }
        None => false,
    }
}

/// Arm 2d — a BACKTICK CODE PATH: at least one backtick-delimited span (`` `code` ``).
fn signal_backtick_code(text: &str) -> bool {
    let first = match text.find('`') {
        Some(i) => i,
        None => return false,
    };
    text[first + 1..].contains('`')
}

/// Arm 2e — a FINDING/DECISION LEXEME (case-insensitive substring against the fixed
/// [`FINDING_LEXEMES`] set). Operates on lowercased text (the CJK lexemes are
/// substring-matched on the same normalized String, codepoint-safe).
fn signal_finding_lexeme(lower: &str) -> bool {
    FINDING_LEXEMES.iter().any(|lex| lower.contains(lex))
}

/// Is a normalized agent message a PROVEN pure declaration (safe to collapse)? Requires
/// ALL of: NOT rich, AND opens with an intent verb (case-insensitive prefix on the first
/// ~24 trimmed chars), AND short (`chars < declaration_max_chars`). A message that is
/// neither clearly rich nor a proven declaration (no opener verb, no signal, mid-length)
/// is KEPT — drop requires proof, keep is default.
fn agent_msg_is_droppable(text: &str, cfg: &RichnessCfg) -> bool {
    if agent_msg_is_rich(text, cfg) {
        return false;
    }
    if text.chars().count() >= cfg.declaration_max_chars {
        return false;
    }
    let head: String = text
        .trim_start()
        .chars()
        .take(24)
        .collect::<String>()
        .to_lowercase();
    INTENT_VERB_OPENERS.iter().any(|v| head.starts_with(v))
}

/// One entry in a turn's rendered assistant lane: a SURVIVING agent message, or a
/// PLACEHOLDER standing in for a contiguous run of collapsed agent messages.
#[derive(Debug, Clone)]
enum AgentRender<'a> {
    Kept(&'a AgentMsg),
    Placeholder(PlaceholderSpan),
}

/// A contiguous span of collapsed agent messages → one placeholder line. Carries the
/// X/Y/Z counts + the first/last elided jsonl line numbers so a consumer can `Read` the
/// raw range.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PlaceholderSpan {
    /// X — collapsed agent messages in this span (≥1).
    messages: usize,
    /// Y — `tool_use` blocks owned by the collapsed span's preceding spans.
    tool_calls: usize,
    /// Z — erroring `tool_result` blocks in that same span.
    failed: usize,
    /// First / last jsonl line of the collapsed agent records (for the fetchable range).
    first_line: usize,
    last_line: usize,
}

/// Decide a turn's SURVIVING agent messages + the collapsed placeholder spans, per mode +
/// cfg (STAGE 1 — operates on WHOLE messages, never touches `ASST_CAP`):
///   • `Longest` (DEFAULT) — keep the LONGEST agent message (by `full_chars`) + the FIRST
///     when substantive (`full_chars >= rich_min_chars`) + each RICH middle; collapse
///     everything else into placeholders. Applies to every multi-message turn; on a
///     single-message turn the sole message is kept. Tie on length → the LAST maximum.
///   • `EotOnly` — only the last agent message (force last-only; never a placeholder).
///   • `All` — every agent message, no filtering, no placeholder.
///   • `Rich` — on a LONG run (`agents.len() > run_threshold`): the LAST is always kept;
///     the FIRST is kept by position privilege under `keep_first` (else decided as a
///     middle); each MIDDLE is kept UNLESS it is a proven pure declaration (keep-on-
///     doubt — drop requires proof). Contiguous dropped runs fuse into one placeholder.
///     A short run (`<= run_threshold`) keeps every agent message verbatim.
/// Produces an ordered list of `{ Kept | Placeholder }` in ascending agent order. EMPTY
/// for a pure tool-call turn (no agents).
fn select_agent_messages<'a>(turn: &'a TurnSlice, cfg: &RichnessCfg) -> Vec<AgentRender<'a>> {
    let agents = &turn.agents;
    if agents.is_empty() {
        return Vec::new();
    }
    match cfg.mode {
        AgentMsgMode::Longest => {
            // The DEFAULT. A single-message turn keeps its sole message (it is both first
            // and longest); no richness eval, no placeholder.
            if agents.len() == 1 {
                return vec![AgentRender::Kept(&agents[0])];
            }
            // The LONGEST agent message is ALWAYS kept (the substantive Rich Response).
            // `max_by_key` returns the LAST maximum on ties, so an all-equal run picks the
            // same index the old `agents.last()` default did — the documented tie rule.
            let longest = agents
                .iter()
                .enumerate()
                .max_by_key(|(_, a)| a.unit.full_chars)
                .map(|(i, _)| i)
                .expect("non-empty");
            let last = agents.len() - 1;
            // Per-message keep decision. Additive over the longest pick:
            //   • the LONGEST index — ALWAYS (the substantive response; may also be first/
            //     middle/last, the position privileges below merely add MORE survivors).
            //   • the FIRST — kept when SUBSTANTIVE (`full_chars >= rich_min_chars`); an
            //     opening plan / early finding worth preserving. A short "let me look"
            //     opener is below the gate → collapses.
            //   • the LAST — kept when SUBSTANTIVE or RICH (so a real closing answer
            //     survives, but a ~50-char throwaway wrap-up collapses — the headline
            //     case). When the last IS the longest it is already kept above.
            //   • each MIDDLE — kept when RICH (`agent_msg_is_rich`); a major finding can
            //     live mid-run.
            let keep = |i: usize, a: &AgentMsg| -> bool {
                if i == longest {
                    return true; // The substantive Rich Response — always.
                }
                if i == 0 {
                    return a.unit.full_chars >= cfg.rich_min_chars; // FIRST if substantive.
                }
                if i == last {
                    return agent_msg_is_rich(&a.unit.text, cfg); // LAST if it carries info.
                }
                agent_msg_is_rich(&a.unit.text, cfg) // MIDDLE if rich.
            };
            collapse_unkept(agents, keep)
        }
        AgentMsgMode::EotOnly => {
            // Only the last (the EOT anchor) — reproduces the pre-expansion output.
            vec![AgentRender::Kept(agents.last().expect("non-empty"))]
        }
        AgentMsgMode::All => agents.iter().map(AgentRender::Kept).collect(),
        AgentMsgMode::Rich => {
            // Short run (or exactly at the threshold) → keep everything verbatim.
            if agents.len() <= cfg.run_threshold {
                return agents.iter().map(AgentRender::Kept).collect();
            }
            let last = agents.len() - 1;
            // Per-message keep decision (KEEP-ON-DOUBT is the spine: collapse only PROVEN
            // pure declarations; keep everything uncertain):
            //   • LAST  — ALWAYS kept (the outcome / EOT anchor; position overrides drop).
            //   • FIRST — the first-matters / immediate-reply case. With `keep_first`
            //     (DEFAULT) the position privilege keeps it unconditionally (the opening
            //     message often states the plan / an early finding worth preserving). With
            //     `--no-keep-first` the privilege is dropped and the first is decided
            //     exactly as a MIDDLE (kept unless droppable — so a rich first still
            //     survives, a "let me look into this" declaration first collapses).
            //   • MIDDLE — kept unless droppable; a sudden rich middle survives whole.
            let keep = |i: usize, a: &AgentMsg| -> bool {
                if i == last {
                    return true; // LAST anchor — always (overrides the drop predicate).
                }
                if i == 0 && cfg.keep_first {
                    return true; // FIRST + position privilege — kept merely for being first.
                }
                // MIDDLE (and a `--no-keep-first` FIRST): keep unless proven droppable.
                !agent_msg_is_droppable(&a.unit.text, cfg)
            };
            collapse_unkept(agents, keep)
        }
    }
}

/// Walk an agent run, KEEPING each message the `keep` predicate accepts and FUSING every
/// contiguous run of un-kept messages into one [`PlaceholderSpan`] (X/Y/Z counts + the
/// first/last elided jsonl line). Shared by the `Longest` and `Rich` selection arms so the
/// placeholder accounting (and thus the summed-cost == summed-emitted invariant) is
/// identical for both. Produces `{ Kept | Placeholder }` in ascending agent order.
fn collapse_unkept<'a>(
    agents: &'a [AgentMsg],
    keep: impl Fn(usize, &AgentMsg) -> bool,
) -> Vec<AgentRender<'a>> {
    let mut out: Vec<AgentRender> = Vec::new();
    let mut span: Option<PlaceholderSpan> = None;
    for (i, a) in agents.iter().enumerate() {
        if keep(i, a) {
            if let Some(s) = span.take() {
                out.push(AgentRender::Placeholder(s));
            }
            out.push(AgentRender::Kept(a));
        } else {
            // Extend (or open) the current contiguous collapsed span.
            let line = a.unit.line_no;
            match span.as_mut() {
                Some(s) => {
                    s.messages += 1;
                    s.tool_calls += a.preceding_tool_calls;
                    s.failed += a.preceding_failed;
                    s.last_line = line;
                }
                None => {
                    span = Some(PlaceholderSpan {
                        messages: 1,
                        tool_calls: a.preceding_tool_calls,
                        failed: a.preceding_failed,
                        first_line: line,
                        last_line: line,
                    });
                }
            }
        }
    }
    if let Some(s) = span.take() {
        out.push(AgentRender::Placeholder(s));
    }
    out
}

/// Pluralize a noun by count: `1 thing` / `N things`.
fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// The EXACT placeholder line a collapsed span renders to (no trailing newline):
///   `△ L{first}–L{last}  [X agent message(s), Y tool call(s)[, Z failed]]`
/// X/Y are always shown (Y even at 0 — a zero-tool reasoning span is informative); the Z
/// clause is OMITTED when Z == 0. Pluralization is INDEPENDENT per noun; "failed" is an
/// adjective (never pluralized). A single-message span renders `L{n}` (no range dash).
fn agent_placeholder_line(span: &PlaceholderSpan) -> String {
    let range = if span.first_line == span.last_line {
        format!("L{}", span.first_line)
    } else {
        format!("L{}–L{}", span.first_line, span.last_line)
    };
    let msgs = plural(span.messages, "agent message");
    let tools = plural(span.tool_calls, "tool call");
    let body = if span.failed == 0 {
        format!("[{msgs}, {tools}]")
    } else {
        format!("[{msgs}, {tools}, {} failed]", span.failed)
    };
    format!("△ {range}  {body}")
}

/// The budget cost of one placeholder line as a physical line (`chars + NEWLINE_COST`).
/// The placeholder SUBSTITUTES the dropped bodies (they contribute zero unit cost), so
/// only this line's own chars are charged — keeping summed-cost == summed-emitted.
fn agent_placeholder_cost(span: &PlaceholderSpan) -> usize {
    agent_placeholder_line(span).chars().count() + NEWLINE_COST
}

/// The glyph that opens a unit's header line in the text render (`▽` user / `△` asst).
fn unit_glyph(role: Role) -> &'static str {
    match role {
        Role::User => "▽",
        Role::Assistant => "△",
    }
}

/// The EXACT header line a unit renders to in the text format (no trailing newline):
/// `▽ L{line}  {ROLE}  ({timestamp})[   (also in summary)]`. The renderer and the cost
/// model both call this, so the charged header length is byte-for-byte what is emitted —
/// the timestamp expansion (≈47 chars beyond the old flat-24 guess) is now counted, not
/// hidden. This is the core fix for the per-unit undercharge.
fn unit_header_line(unit: &TurnUnit) -> String {
    let dup = if unit.also_in_summary {
        "   (also in summary)"
    } else {
        ""
    };
    format!(
        "{} L{}  {}  ({}){dup}",
        unit_glyph(unit.role),
        unit.line_no,
        unit.role.label().to_uppercase(),
        format_timestamp(unit.ts_utc.as_deref())
    )
}

/// The budget cost of one unit: its REAL header line + the rendered body, each as a
/// physical line (`chars + NEWLINE_COST`). The body ALREADY includes the `… [+K …] …`
/// elision scaffolding when truncated, so this is measured against the SAME render used
/// for output — summed cost == summed emitted chars (the budget test relies on it). No
/// separate marker term (that would double-count). The header length is the true
/// timestamp-dependent line, not a flat estimate.
fn unit_cost(unit: &TurnUnit) -> usize {
    let header_chars = unit_header_line(unit).chars().count() + NEWLINE_COST;
    let body_chars = render_unit_body(unit, None).body.chars().count() + NEWLINE_COST;
    header_chars + body_chars
}

/// The `[N tool calls]` marker line render cost INCLUDING its trailing newline (0 ⇒
/// omitted, no cost). Matches the exact `  [N tool calls]` line the text renderer emits.
fn marker_cost(tool_calls: usize) -> usize {
    if tool_calls == 0 {
        0
    } else {
        // "  [N tool calls]" + the trailing newline the emit callback appends.
        format!("  [{tool_calls} tool calls]").chars().count() + NEWLINE_COST
    }
}

/// The EXACT compaction-boundary banner line a crossed summary renders to (no trailing
/// newline). The renderer and the budget reservation both call this so the reserved
/// banner length is byte-for-byte what is emitted.
fn boundary_banner_line(line_no: usize) -> String {
    format!(
        "{0} compaction boundary · summary at L{1} · (turns below predate it) {0}",
        "══", line_no
    )
}

/// The budget cost of one boundary banner as a physical line (`chars + NEWLINE_COST`).
fn banner_cost(line_no: usize) -> usize {
    boundary_banner_line(line_no).chars().count() + NEWLINE_COST
}

/// The EXACT total banner chars the render emits when the selected set spans `depth`
/// compaction boundaries: the render banners every summary ranked 1..=`depth` (rank from
/// newest = 1), each exactly once (`crossed_summaries` covers ranks `(0, depth]` across a
/// full ascending walk). `depth == 0` ⇒ no banners. This is charged INCREMENTALLY as
/// selection deepens the spanned count, so the banner budget is exact (never the
/// over-reservation of "all summaries"), keeping more room for real turns at small
/// budgets / summary-heavy sessions.
fn cumulative_banner_cost(summaries: &[SummaryInfo], depth: usize) -> usize {
    if depth == 0 {
        return 0;
    }
    // Rank by descending line number (newest = rank 1); the first `depth` of those are the
    // boundaries the ascending render crosses to reach a turn at that depth.
    let mut by_rank: Vec<usize> = summaries.iter().map(|s| s.line_no).collect();
    by_rank.sort_unstable_by(|a, b| b.cmp(a));
    by_rank.into_iter().take(depth).map(banner_cost).sum()
}

/// A worst-case (provable upper-bound) char count of the document header block emitted by
/// [`render_text`] (the `SESSION` line, the budget line, the selected line, the optional
/// dedup line, and the 60-wide rule). Every numeric placeholder is widened to its
/// session maximum so the real block is always ≤ this. The 60-wide rule glyph `─` and the
/// banner/units glyphs are multi-byte but counted by `chars()`, matching the render.
fn doc_header_block_max_chars(sr: &ScanResult, budget: usize) -> usize {
    let turns = sr.turns.len();
    let summaries = sr.summaries.len();
    // The assistant-units count printed in the selected line can EXCEED `turns` under the
    // richness model (a turn can keep >1 agent message — `All` mode keeps every one), so
    // its worst case is the total agent messages across all turns. The user-units count is
    // still ≤ turns (one opener per turn).
    let max_agent_units = sr.turns.iter().map(|t| t.agents.len()).sum::<usize>();
    let max_line = sr
        .summaries
        .iter()
        .map(|s| s.line_no)
        .chain(sr.turns.iter().map(turn_latest_line))
        .max()
        .unwrap_or(0);
    // Upper bounds: user units ≤ turns; assistant units ≤ total agent messages; char
    // figures ≤ budget; the summary line ≤ max_line; dedup count ≤ both anchors of every
    // turn (2·turns). Render each worst-case line with the SAME format strings the
    // renderer uses, then sum their char lengths (+ newline).
    let line_session = format!("SESSION {}", sr.session_id);
    let line_budget = format!(
        "  budget {} chars · round-trip-fraction {:.2} · spanned {} compaction boundaries",
        budget, 0.0_f64, summaries
    );
    // The `selected` line carries the automation note ` (N automation triggers)` ONLY when
    // the session actually HAS automation-trigger turns (N ≤ turns). Reserve that space
    // only then, so a session with no automation pulses keeps the exact pre-feature header
    // budget (the note is a no-op string otherwise).
    let has_automation = sr.turns.iter().any(|t| t.is_automation);
    let line_selected = if has_automation {
        format!(
            "  selected {} user ({} automation triggers) + {} assistant units across {} turns · {} / {} chars used",
            turns, turns, max_agent_units, turns, budget, budget
        )
    } else {
        format!(
            "  selected {} user + {} assistant units across {} turns · {} / {} chars used",
            turns, max_agent_units, turns, budget, budget
        )
    };
    let line_dedup = format!(
        "  dedup: {} units also present in summary L{} (demoted, flagged)",
        2 * turns,
        max_line
    );
    let line_rule = format!("  {}", "─".repeat(60));
    [
        line_session,
        line_budget,
        line_selected,
        line_dedup,
        line_rule,
    ]
    .iter()
    .map(|l| l.chars().count() + NEWLINE_COST)
    .sum()
}

/// The cost of a turn's ASSISTANT LANE under the richness selection: the sum of each
/// SURVIVING agent message's `unit_cost` + each collapsed placeholder's
/// `agent_placeholder_cost`. This is the SAME walk the renderer + json emitter use, so
/// summed cost == summed emitted chars. In `EotOnly` mode it equals the single-EOT
/// `unit_cost` exactly (the lane is just `[Kept(last)]`) — the non-breaking guarantee.
fn assistant_lane_cost(turn: &TurnSlice, cfg: &RichnessCfg) -> usize {
    select_agent_messages(turn, cfg)
        .iter()
        .map(|r| match r {
            AgentRender::Kept(a) => unit_cost(&a.unit),
            AgentRender::Placeholder(s) => agent_placeholder_cost(s),
        })
        .sum()
}

/// Cost of a whole turn at the chosen selection granularity (`sides`): both sides +
/// the `[N tool calls]` marker when both are taken; a single side (no marker) otherwise.
/// The assistant side now sums the kept agent messages + placeholders (richness model);
/// in `EotOnly` mode that reduces to the single EOT, so existing budgets are unchanged.
/// This is the SAME accounting the renderer uses, so summed cost == summed rendered chars
/// (the budget test relies on it).
fn turn_cost(turn: &TurnSlice, sides: SelSides, cfg: &RichnessCfg) -> usize {
    let mut c = 0;
    if matches!(sides, SelSides::Both | SelSides::UserOnly) {
        if let Some(u) = &turn.user {
            c += unit_cost(u);
        }
    }
    // The marker is only rendered BETWEEN the user and the assistant lane, so it is
    // charged only on a both-sides selection (a single-side emit shows no marker).
    if matches!(sides, SelSides::Both) {
        c += marker_cost(turn.tool_calls);
    }
    if matches!(sides, SelSides::Both | SelSides::AssistantOnly) {
        c += assistant_lane_cost(turn, cfg);
    }
    c
}

/// Which side(s) of a turn a selection takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelSides {
    Both,
    UserOnly,
    AssistantOnly,
}

// ─────────────────────────────────────────────────────────────────────────────
// Budget allocation (2-phase)
// ─────────────────────────────────────────────────────────────────────────────

/// A selection decision for one turn (which sides were chosen).
#[derive(Debug, Clone)]
struct Selected {
    turn_index: usize,
    sides: SelSides,
}

/// The per-session plan: the selected turns (already sorted ascending for render) +
/// accounting for the header.
#[derive(Debug)]
struct SessionPlan {
    selected: Vec<Selected>,
    /// The dedup-flagged + max-compaction-filtered turns the plan selected FROM. The
    /// renderer reads units (incl. the `also_in_summary` flag) from HERE — never from the
    /// un-flagged `ScanResult.turns` — so the dedup demote-flag reaches the output.
    turns: Vec<TurnSlice>,
    spanned_boundaries: usize,
    rendered_chars: usize,
    /// The newest summary line (if any) — for the dedup-note + banners.
    newest_summary_line: Option<usize>,
    dedup_demoted: usize,
}

/// Plan one session: dedup-flag turns against the newest summary, then run the 2-phase
/// recency-first budget allocation, then sort ascending for render.
fn plan_session(
    sr: &ScanResult,
    budget: usize,
    rt_fraction: f64,
    max_compactions: usize,
    cfg: &RichnessCfg,
) -> SessionPlan {
    // ── Dedup-flag (mutate a working copy of the turns) ──
    let newest = sr.summaries.iter().max_by_key(|s| s.line_no);
    let newest_summary_line = newest.map(|s| s.line_no);
    let mut turns: Vec<TurnSlice> = sr.turns.clone();
    let mut dedup_demoted = 0usize;
    if let Some(summary) = newest {
        for t in &mut turns {
            // Dedup is keyed on the live (compactions_before == 0) region primarily —
            // turns predating an OLDER boundary are genuinely gone from context and are
            // pure restoration, never deduped.
            if t.compactions_before != 0 {
                continue;
            }
            // Dedup keys on the SAME two anchors as before the expansion: the user
            // opener and the EOT (last) agent message. Middle agent messages are not
            // deduped (a summary never quotes them verbatim), so demote-flag scope is
            // unchanged. Borrow each anchor separately (the accessor borrows the slice).
            if let Some(u) = t.user.as_mut() {
                if unit_matches_summary(u, &summary.fingerprints) {
                    u.also_in_summary = true;
                    dedup_demoted += 1;
                }
            }
            if let Some(a) = t.assistant_eot_mut() {
                if unit_matches_summary(a, &summary.fingerprints) {
                    a.also_in_summary = true;
                    dedup_demoted += 1;
                }
            }
        }
    }

    // ── Apply --max-compactions: drop turns beyond the cap (0 = unlimited) ──
    if max_compactions > 0 {
        turns.retain(|t| t.compactions_before <= max_compactions);
    }

    // ── Reserve the document HEADER BLOCK up front (a fixed framing the render always
    // emits to stdout) so the chars left for the reconstruction body — boundary banners +
    // selected units — fit `available`. The header block is bounded by a provable
    // worst-case (§ doc_header_block_max_chars); the BANNERS are NOT pre-reserved in bulk
    // (that wasted ~½ a small budget on summary-heavy sessions) but charged INCREMENTALLY
    // as selection deepens the spanned count, so the banner budget is exact. Invariant
    // held below: `doc_header(real ≤ reservation) + banners(spanned) + units ≤ budget`. ──
    let doc_header_reservation = doc_header_block_max_chars(sr, budget);
    let available = budget.saturating_sub(doc_header_reservation);

    // Recency-first order = descending line_no of the turn's latest unit. Ties broken
    // by descending turn_index for determinism.
    let mut order: Vec<usize> = (0..turns.len()).collect();
    order.sort_by(|&a, &b| {
        turn_latest_line(&turns[b])
            .cmp(&turn_latest_line(&turns[a]))
            .then(turns[b].turn_index.cmp(&turns[a].turn_index))
    });

    // The round-trip reservation bounds UNIT chars (the back-and-forth content), computed
    // off `available` (post-header-block); banners are charged on top via `committed`.
    let rt_budget = ((available as f64) * rt_fraction).round() as usize;
    // `spent_units` = unit chars only; `spanned_depth` = max compactions_before among
    // chosen turns (drives the banner charge). The total committed against `available` is
    // `spent_units + cumulative_banner_cost(summaries, spanned_depth)`.
    let mut spent_units = 0usize;
    let mut spanned_depth = 0usize;
    let mut chosen: Vec<Option<SelSides>> = vec![None; turns.len()];

    // ── Phase 1: ROUND-TRIP GUARANTEE — spend rt_budget (unit chars) on complete pairs,
    // while the banner charge for the deepened span still keeps the WHOLE doc ≤ budget. ──
    // Non-dup complete turns first, then dup complete turns (demote, don't drop).
    for dedup_pass in [false, true] {
        for &ti in &order {
            if chosen[ti].is_some() {
                continue;
            }
            let t = &turns[ti];
            // The HARD FLOOR reserves its lane for HUMAN round-trips only — a machine
            // automation pulse is left for Phase-2 fill, so the protected budget the help
            // documents for "user → … → assistant EOT" is never silently spent on a
            // pulse→ack pair (which the header already reports as an automation trigger).
            if !t.is_human_round_trip() {
                continue;
            }
            if turn_is_dup(t) != dedup_pass {
                continue;
            }
            let c = turn_cost(t, SelSides::Both, cfg);
            let new_depth = spanned_depth.max(t.compactions_before);
            let banners = cumulative_banner_cost(&sr.summaries, new_depth);
            let unit_fits_rt = spent_units + c <= rt_budget;
            let doc_fits = spent_units + c + banners <= available;
            if unit_fits_rt && doc_fits {
                chosen[ti] = Some(SelSides::Both);
                spent_units += c;
                spanned_depth = new_depth;
            } else if spent_units == 0 && !dedup_pass && doc_fits {
                // The first (most-recent, non-dup) complete turn exceeds the round-trip
                // reservation but the WHOLE document (its cost + its banners + header
                // block) still fits `budget`: include it anyway — the most-recent exchange
                // is load-bearing and already ellipsis-capped by the role caps. Stop
                // Phase 1; Phase 2 fills the remainder. A round-trip that does NOT satisfy
                // `doc_fits` is left for Phase 2 to take a cheaper single side, so the
                // ≤-budget guarantee is never broken.
                chosen[ti] = Some(SelSides::Both);
                spent_units += c;
                spanned_depth = new_depth;
                break;
            }
            // else: skip (leave for Phase 2 to maybe pick a cheaper single side).
        }
    }

    // ── Phase 2: FILL — spend the rest of `available` (incl. unused rt reservation). ──
    for dedup_pass in [false, true] {
        for &ti in &order {
            if chosen[ti].is_some() {
                continue;
            }
            let t = &turns[ti];
            if turn_is_dup(t) != dedup_pass {
                continue;
            }
            // Prefer a complete turn if it fits; else the user side first (scarcer,
            // higher-signal loss), then the assistant side.
            let candidates: &[SelSides] = if t.is_round_trip() {
                &[SelSides::Both, SelSides::UserOnly, SelSides::AssistantOnly]
            } else if t.user.is_some() {
                &[SelSides::UserOnly]
            } else if !t.agents.is_empty() {
                &[SelSides::AssistantOnly]
            } else {
                &[]
            };
            for &sides in candidates {
                let c = turn_cost(t, sides, cfg);
                let new_depth = spanned_depth.max(t.compactions_before);
                let banners = cumulative_banner_cost(&sr.summaries, new_depth);
                if spent_units + c + banners <= available {
                    chosen[ti] = Some(sides);
                    spent_units += c;
                    spanned_depth = new_depth;
                    break;
                }
            }
        }
    }

    // ── Assemble selected set, ascending for render ──
    let mut selected: Vec<Selected> = Vec::new();
    for (ti, sel) in chosen.iter().enumerate() {
        if let Some(sides) = sel {
            selected.push(Selected {
                turn_index: turns[ti].turn_index,
                sides: *sides,
            });
        }
    }
    selected.sort_by_key(|s| s.turn_index);

    // Boundaries spanned = the GREATEST `compactions_before` among selected turns: the
    // oldest selected turn sits behind that many summaries, so the ascending render
    // crosses exactly that many compaction boundaries on the way to EOF.
    let spanned = spanned_boundary_count(&turns, &selected);

    // Report the conservative upper bound the selection enforced:
    //   doc_header_reservation (≥ the real header block) + banners(spanned) + unit chars.
    // Because selection guaranteed `spent_units + banners(spanned) <= available =
    // budget - doc_header_reservation`, this figure is ≤ budget by construction, and it is
    // ≥ the true emitted length (the real header block ≤ its reservation), so the header
    // line never UNDER-states the cost. `spanned == spanned_depth` here. An EMPTY selection
    // emits NO document at all (the renderer skips empty sessions), so it reports 0 — the
    // header-block reservation is only "spent" when a document is actually written.
    let rendered_chars = if selected.is_empty() {
        0
    } else {
        doc_header_reservation + cumulative_banner_cost(&sr.summaries, spanned) + spent_units
    };

    SessionPlan {
        selected,
        turns,
        spanned_boundaries: spanned,
        rendered_chars,
        newest_summary_line,
        dedup_demoted,
    }
}

/// The latest jsonl line a turn touches (for recency ordering): the max of its user
/// opener line and its LATEST agent message line (the EOT anchor == `agents.last()`,
/// which is the highest agent line by construction), 0 if neither.
fn turn_latest_line(t: &TurnSlice) -> usize {
    let u = t.user.as_ref().map(|x| x.line_no).unwrap_or(0);
    let a = t.assistant_eot().map(|x| x.line_no).unwrap_or(0);
    u.max(a)
}

/// True when EITHER dedup anchor of a turn is flagged (the user opener or the EOT agent
/// message — the only two sides dedup ever marks).
fn turn_is_dup(t: &TurnSlice) -> bool {
    t.user.as_ref().is_some_and(|u| u.also_in_summary)
        || t.assistant_eot().is_some_and(|a| a.also_in_summary)
}

/// True when a unit's fingerprint is a prefix-or-equal match of any summary fingerprint
/// (§6.2). The match is symmetric-prefix: a unit clipped to 80 chars matches a summary
/// bullet that begins with the same 80 chars, and vice-versa.
fn unit_matches_summary(unit: &TurnUnit, summary_fps: &[String]) -> bool {
    let unit_fp = fingerprint(&unit.text);
    if unit_fp.is_empty() {
        return false;
    }
    summary_fps.iter().any(|sfp| {
        !sfp.is_empty() && (unit_fp.starts_with(sfp.as_str()) || sfp.starts_with(unit_fp.as_str()))
    })
}

/// The number of compaction boundaries the selected turns span = the GREATEST
/// `compactions_before` among selected turns. The oldest selected turn sits behind that
/// many summaries, so the ascending render crosses exactly that many boundaries reaching
/// EOF. 0 when every selected turn is in the live (post-newest-summary) region.
fn spanned_boundary_count(turns: &[TurnSlice], selected: &[Selected]) -> usize {
    selected
        .iter()
        .filter_map(|s| turns.iter().find(|t| t.turn_index == s.turn_index))
        .map(|t| t.compactions_before)
        .max()
        .unwrap_or(0)
}

// ─────────────────────────────────────────────────────────────────────────────
// Window-range parsing (shared parser in `crate::text`)
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a `--turn-range START..END` into an inclusive 0-based `(lo, hi)` (shared parser).
fn parse_turn_range(s: &str) -> Result<(usize, usize)> {
    crate::text::parse_range(s, "--turn-range", false)
}

// ─────────────────────────────────────────────────────────────────────────────
// Rendering
// ─────────────────────────────────────────────────────────────────────────────

/// Everything the renderers need beyond the per-session scans + plans.
#[derive(Debug)]
struct RenderCtx {
    budget_chars: usize,
    rt_fraction: f64,
    skipped_lines: usize,
    /// The richness configuration — the renderer walks the same `select_agent_messages`
    /// survivor set the plan budgeted, so emitted == costed.
    cfg: RichnessCfg,
}

/// Look up the dedup-flagged `TurnSlice` for a selected turn index within the PLAN's
/// turns (NOT `ScanResult.turns`, which is un-flagged) so the renderer sees the
/// `also_in_summary` flag the plan set.
fn find_turn(plan: &SessionPlan, turn_index: usize) -> Option<&TurnSlice> {
    plan.turns.iter().find(|t| t.turn_index == turn_index)
}

fn render_text(
    ctx: &RenderCtx,
    sessions: &[ScanResult],
    plans: &[SessionPlan],
    out_path: Option<&Path>,
    slice: Option<usize>,
    window: usize,
    slices: Option<usize>,
) -> Result<()> {
    // ── Chunked-output mode (--slice): emit ONLY one ≤window-char chunk of the verbatim DOCUMENT
    // (the SAME body `--out` writes), with NO operational chrome — so a SessionStart hook can
    // inject it under the 10,000-char additionalContext cap. Two sub-modes:
    //
    //   • LEGACY (`--slice` alone): budget-driven. The document is whatever `--budget` selected,
    //     paginated into a VARIABLE number of chunks; `--slice i` emits the i-th. Concatenating
    //     1..K reproduces the document byte-for-byte. The per-role 600/900 body caps apply.
    //   • FIXED-FLEET (`--slices N`): the slice COUNT is the hard constraint (a fixed set of hooks
    //     can't grow). Bodies render whole up to one window — a turn is ellipsized ONLY if it
    //     ALONE exceeds a window — and only the NEWEST N chunks are kept; the oldest overflow is
    //     DISCARDED. So the emitted count is ALWAYS ≤N regardless of turn size. slice 1 = oldest
    //     KEPT, slice N = newest.
    //
    // An out-of-range index prints nothing (exit 0), so surplus hooks simply inject nothing. ──
    if let Some(n) = slice {
        // Fixed-fleet drops the per-role caps for a window cap (whole turns; ellipsize only a turn
        // bigger than a window). Legacy keeps the role caps (cap_override = None).
        let cap_override = slices.map(|_| window.saturating_sub(SLICE_BODY_HEADROOM).max(1));
        let doc = build_document_body(sessions, plans, &ctx.cfg, cap_override);
        let chunks = slice_into_windows(&doc, window);
        let idx = match slices {
            Some(n_slices) => {
                if n > n_slices {
                    return Ok(()); // index outside the fixed fleet → inject nothing
                }
                // Keep the newest n_slices chunks; drop the oldest (len - n_slices) overflow so
                // the count never exceeds the fleet. slice 1 maps to the oldest KEPT chunk.
                chunks.len().saturating_sub(n_slices) + (n - 1)
            }
            None => n - 1,
        };
        if let Some(chunk) = chunks.into_iter().nth(idx) {
            print!("{chunk}");
        }
        return Ok(());
    }

    let mut first = true;
    let mut any = false;
    let mut out_blob = String::new();

    // Fan-out scope banner. The banner reports the TRUE scope (EVERY discovered session,
    // split top-level/subagent) and — separately — how many rendered WITHIN budget, so the
    // budget value can never silently rewrite "scope" and a targeted top-level uuid can never
    // read as `0 top-level`. Printed whenever more than one session is in scope OR some
    // in-scope session was skipped by the budget; a lone session that rendered cleanly stays
    // silent (the common single-thread recovery case, zero added noise).
    let sc = scope_summary(sessions, plans);
    let any_skipped = sc.rendered < sc.in_scope;
    if sc.in_scope > 1 || any_skipped {
        // Reuse the shared `N session(s) in scope (X top-level + Y subagent)` wording (the same
        // fragment list/files/search/recover emit), then append turns' own budget clause.
        println!(
            "scope  {} · {} rendered within budget · budget {} chars is PER session → up to {} \
             chars total",
            crate::text::scope_span_fragment(sc.in_scope_top, sc.in_scope_sub),
            sc.rendered,
            ctx.budget_chars,
            ctx.budget_chars.saturating_mul(sc.rendered.max(1))
        );
        println!();
    }

    // A TARGETED top-level session that has restorable content but does NOT fit the budget
    // must be reported explicitly — never silently absent while unrelated subagents fill
    // stdout. Emit a per-session skip note (top-level sessions only; a skipped subagent is
    // fan-out noise the user did not ask for) carrying the budget it would need. A GENUINELY
    // EMPTY session (no restorable turns at all → `min_render_chars` is None) is left to the
    // terminal "no turns selected (empty session set …)" fallback — that case is already
    // honest and not a budget problem. `skipped_any` tracks only the budget-too-small notes,
    // separate from `any`, so the fallback still keys on whether a real block rendered.
    let mut skipped_any = false;
    for (sr, plan) in sessions.iter().zip(plans.iter()) {
        if plan.selected.is_empty() && !sr.is_subagent {
            if let Some(min) = min_render_chars(sr, ctx.budget_chars, &ctx.cfg) {
                println!(
                    "SESSION {}  skipped — its first round-trip needs ≥ {} chars; \
                     raise --budget (now {})",
                    sr.session_id, min, ctx.budget_chars
                );
                skipped_any = true;
            }
        }
    }
    if skipped_any {
        println!();
    }

    for (sr, plan) in sessions.iter().zip(plans.iter()) {
        if plan.selected.is_empty() {
            continue;
        }
        any = true;
        if !first {
            println!();
        }
        first = false;

        let (n_user, n_asst) = count_sides(plan, &ctx.cfg);
        let n_automation = count_automation(plan);
        // Brand a spanned SUBAGENT block with the SAME shape every other session-emitting
        // surface uses (`list`/`files`/`search`): `SUBAGENT <hex>  ·  parent SESSION <uuid>`
        // — never token a bare non-re-feedable subagent hex as `SESSION` (the id-domain
        // overload r6 removed elsewhere), and surface the re-feedable parent uuid inline so a
        // turns-text reader has a re-feed path. A top-level uuid block stays `SESSION <uuid>`.
        if sr.is_subagent {
            println!(
                "SUBAGENT {}  ·  parent SESSION {}  (subagent transcript)",
                sr.session_id, sr.parent_session_id
            );
        } else {
            println!("SESSION {}", sr.session_id);
        }
        println!(
            "  budget {} chars · round-trip-fraction {:.2} · spanned {} compaction boundaries",
            ctx.budget_chars, ctx.rt_fraction, plan.spanned_boundaries
        );
        // Header automation note carries a PER-CLASS breakdown, not just the lumped scalar,
        // so a reader sees the composition (`2 background-command, 1 agent`) without scanning
        // every `[kind …]` label line in the body.
        let automation_note = if n_automation > 0 {
            let breakdown =
                automation_breakdown_text(&automation_by_kind(std::slice::from_ref(plan)));
            format!(
                " ({n_automation} automation trigger{}: {breakdown})",
                if n_automation == 1 { "" } else { "s" }
            )
        } else {
            String::new()
        };
        println!(
            "  selected {} user{} + {} assistant units across {} turns · {} / {} chars used",
            n_user,
            automation_note,
            n_asst,
            plan.selected.len(),
            plan.rendered_chars,
            ctx.budget_chars
        );
        // Whole-session automation composition, INDEPENDENT of budget selection — so a
        // monitor-heavy session isn't silently read as "no automation" when the recency
        // window selected none of its deep pulses. Shown only when MORE automation exists in
        // scope than was selected (otherwise the selected note above already tells the truth).
        let in_scope_by = automation_in_scope_by_kind(std::slice::from_ref(plan));
        let in_scope_total: usize = in_scope_by.iter().sum();
        if in_scope_total > n_automation {
            println!(
                "  in scope (not all selected): {} automation trigger{} — {}",
                in_scope_total,
                if in_scope_total == 1 { "" } else { "s" },
                automation_breakdown_text(&in_scope_by)
            );
        }
        if let (Some(sline), true) = (plan.newest_summary_line, plan.dedup_demoted > 0) {
            println!(
                "  dedup: {} units also present in summary L{} (demoted, flagged)",
                plan.dedup_demoted, sline
            );
        }
        println!("  {}", "─".repeat(60));

        // Walk the ascending selected set, inserting boundary banners as
        // `compactions_before` decreases toward EOF.
        let mut prev_comp: Option<usize> = None;
        for sel in &plan.selected {
            let Some(turn) = find_turn(plan, sel.turn_index) else {
                continue;
            };
            maybe_boundary_banner(
                &mut prev_comp,
                turn.compactions_before,
                &sr.summaries,
                &mut |s| {
                    println!("{s}");
                    out_blob.push_str(&s);
                    out_blob.push('\n');
                },
            );
            render_turn_text(turn, sel.sides, &ctx.cfg, None, &mut |s| {
                println!("{s}");
                out_blob.push_str(&s);
                out_blob.push('\n');
            });
        }
    }

    // The terminal fallback fires only when NOTHING rendered AND no per-session skip note
    // already explained why (a skip note is the more specific, actionable message).
    if !any && !skipped_any {
        println!("no turns selected (empty session set or budget too small)");
    }
    if ctx.skipped_lines > 0 {
        println!();
        println!("({})", crate::text::malformed_note(ctx.skipped_lines));
    }
    if let Some(p) = out_path {
        if crate::recover::write_out_guarded(p, &out_blob)? {
            println!();
            println!("(wrote full reconstruction to {})", p.display());
        }
    }
    Ok(())
}

/// Build the verbatim DOCUMENT body (boundary banners + selected turn units) for every
/// in-scope session, with NO operational chrome. Byte-for-byte identical to the `out_blob`
/// that `render_text` accumulates for `--out` (same emit path: `maybe_boundary_banner` +
/// `render_turn_text`, each line followed by `\n`), so a `--slice` reconstruction and an
/// `--out` file carry the same content. Sessions concatenate with no separator (mirrors
/// `out_blob`); a `--slice` run is almost always a single top-level thread anyway.
fn build_document_body(
    sessions: &[ScanResult],
    plans: &[SessionPlan],
    cfg: &RichnessCfg,
    cap_override: Option<usize>,
) -> String {
    let mut blob = String::new();
    for (sr, plan) in sessions.iter().zip(plans.iter()) {
        if plan.selected.is_empty() {
            continue;
        }
        let mut prev_comp: Option<usize> = None;
        for sel in &plan.selected {
            let Some(turn) = find_turn(plan, sel.turn_index) else {
                continue;
            };
            maybe_boundary_banner(
                &mut prev_comp,
                turn.compactions_before,
                &sr.summaries,
                &mut |s| {
                    blob.push_str(&s);
                    blob.push('\n');
                },
            );
            render_turn_text(turn, sel.sides, cfg, cap_override, &mut |s| {
                blob.push_str(&s);
                blob.push('\n');
            });
        }
    }
    blob
}

/// Greedily pack a document's LINES into chunks of at most `window` CHARACTERS (Unicode
/// scalars — the unit Claude Code's 10,000-char additionalContext cap counts, so a CJK-heavy
/// document is NOT 3× over-counted the way a byte budget would). A line longer than the
/// window on its own is hard-split on a char boundary so NO emitted chunk ever exceeds
/// `window`. Concatenating the chunks in order reproduces `text` exactly (`split_inclusive`
/// keeps the newlines), so the slices reassemble losslessly across hook invocations.
fn slice_into_windows(text: &str, window: usize) -> Vec<String> {
    let window = window.max(1);
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_chars = 0usize;
    for line in text.split_inclusive('\n') {
        let line_chars = line.chars().count();
        if line_chars > window {
            // Oversized single line: flush the current chunk, then hard-split on char
            // boundaries so no emitted chunk exceeds the window. The trailing remainder
            // (< window) seeds the next chunk so following lines still pack onto it.
            if !cur.is_empty() {
                chunks.push(std::mem::take(&mut cur));
                cur_chars = 0;
            }
            let mut piece = String::new();
            let mut piece_chars = 0usize;
            for ch in line.chars() {
                piece.push(ch);
                piece_chars += 1;
                if piece_chars == window {
                    chunks.push(std::mem::take(&mut piece));
                    piece_chars = 0;
                }
            }
            if !piece.is_empty() {
                cur = piece;
                cur_chars = piece_chars;
            }
            continue;
        }
        if cur_chars + line_chars > window && !cur.is_empty() {
            chunks.push(std::mem::take(&mut cur));
            cur_chars = 0;
        }
        cur.push_str(line);
        cur_chars += line_chars;
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

/// Emit a `══ compaction boundary ══` banner for every summary the ascending walk
/// crosses on the way to a turn with `compactions_before == current`. Crossings are
/// keyed on the summary RANK from newest (newest = rank 1): moving from a turn at
/// cb=`prev` to one at cb=`current` (`current < prev`) crosses every summary ranked
/// `(current, prev]`, each bannered once, in ascending line order. The FIRST turn
/// (`prev == None`) crosses NOTHING — there are no restored turns below it, so the
/// summaries older than it (which it predates) are not bannered.
fn maybe_boundary_banner(
    prev: &mut Option<usize>,
    current: usize,
    summaries: &[SummaryInfo],
    emit: &mut dyn FnMut(String),
) {
    for s in crossed_summaries(summaries, *prev, current) {
        emit(boundary_banner_line(s.line_no));
    }
    *prev = Some(current);
}

/// The summaries crossed when the ascending cursor moves from a turn at cb=`from` to a
/// turn at cb=`to`. A summary's rank from newest is its 1-based position when sorted by
/// descending line number; it is crossed when `to < rank <= from`. The FIRST turn
/// (`from == None`) seeds the cursor at its OWN depth (crosses nothing) — a summary
/// older than every selected turn has no restored turn below it, so it is never
/// bannered. Total banners across a full walk therefore equal the GREATEST cb selected
/// (the spanned-boundary count). Returned in ascending line order so banners read
/// forward.
fn crossed_summaries(
    summaries: &[SummaryInfo],
    from: Option<usize>,
    to: usize,
) -> Vec<&SummaryInfo> {
    // The first turn seeds the cursor at its own depth → no crossing on arrival.
    let Some(from) = from else {
        return Vec::new();
    };
    if to >= from {
        return Vec::new();
    }
    // Rank by descending line number (newest = rank 1).
    let mut by_rank: Vec<&SummaryInfo> = summaries.iter().collect();
    by_rank.sort_by(|a, b| b.line_no.cmp(&a.line_no));
    let mut out: Vec<&SummaryInfo> = Vec::new();
    for (i, s) in by_rank.iter().enumerate() {
        let rank = i + 1; // newest = 1
        if rank > to && rank <= from {
            out.push(*s);
        }
    }
    out.sort_by_key(|s| s.line_no);
    out
}

/// Render one turn's selected side(s) to the text format.
/// Whether a selection shows the user side / assistant side.
fn shows_user(sides: SelSides) -> bool {
    matches!(sides, SelSides::Both | SelSides::UserOnly)
}
fn shows_assistant(sides: SelSides) -> bool {
    matches!(sides, SelSides::Both | SelSides::AssistantOnly)
}

/// The user side of a turn IF the selection shows it AND it exists. Centralizes the
/// `show_user && Some` logic so BOTH renderers share one (unit-testable) decision.
fn shown_user(turn: &TurnSlice, sides: SelSides) -> Option<&TurnUnit> {
    if shows_user(sides) {
        turn.user.as_ref()
    } else {
        None
    }
}
/// The assistant LANE of a turn IF the selection shows it: the richness-selected
/// survivor list (kept agent messages + collapsed placeholders). EMPTY when the selection
/// hides the assistant side or the turn has no agent messages. In `EotOnly` mode this is
/// just `[Kept(last)]`, so a single-EOT emit is reproduced byte-for-byte.
fn shown_agent_lane<'a>(
    turn: &'a TurnSlice,
    sides: SelSides,
    cfg: &RichnessCfg,
) -> Vec<AgentRender<'a>> {
    if shows_assistant(sides) {
        select_agent_messages(turn, cfg)
    } else {
        Vec::new()
    }
}

fn render_turn_text(
    turn: &TurnSlice,
    sides: SelSides,
    cfg: &RichnessCfg,
    cap_override: Option<usize>,
    emit: &mut dyn FnMut(String),
) {
    if let Some(u) = shown_user(turn, sides) {
        emit_unit_text(u, cap_override, emit);
    }
    if matches!(sides, SelSides::Both) && turn.tool_calls > 0 {
        emit(format!("  [{} tool calls]", turn.tool_calls));
    }
    for entry in shown_agent_lane(turn, sides, cfg) {
        match entry {
            AgentRender::Kept(a) => emit_unit_text(&a.unit, cap_override, emit),
            AgentRender::Placeholder(s) => emit(agent_placeholder_line(&s)),
        }
    }
}

/// Emit a unit's header line + rendered (possibly truncated) body. The header string is
/// produced by [`unit_header_line`] — the SAME function the cost model charges — so the
/// emitted line is byte-for-byte what the budget accounted.
fn emit_unit_text(unit: &TurnUnit, cap_override: Option<usize>, emit: &mut dyn FnMut(String)) {
    emit(unit_header_line(unit));
    let r = render_unit_body(unit, cap_override);
    emit(r.body);
}

/// Count selected user + assistant UNITS in a plan (an assistant unit = one KEPT agent
/// message; collapsed placeholders are not units). With the richness model a turn's
/// assistant side can contribute more than one kept message, so this walks the lane.
fn count_sides(plan: &SessionPlan, cfg: &RichnessCfg) -> (usize, usize) {
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

/// The fan-out scope of an in-scope-session set. `--budget` is applied PER session, so a
/// `--include-subagents` query that spans S subagents realizes up to `budget × (1 + S)`
/// chars. The banner must report the TRUE scope (every discovered session) — NOT only what
/// fit in the budget — so a rendering knob (`--budget`) can never silently rewrite "scope"
/// and a targeted top-level uuid can never read as `0 top-level`. Returns the full
/// breakdown: how many sessions are in scope (split top-level vs subagent over ALL of them)
/// and how many actually rendered within budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScopeCounts {
    /// All discovered sessions in scope (regardless of whether they fit the budget).
    in_scope: usize,
    /// Top-level (`<uuid>.jsonl`) sessions among those in scope.
    in_scope_top: usize,
    /// Subagent (bare-hex) transcripts among those in scope.
    in_scope_sub: usize,
    /// How many of the in-scope sessions produced a non-empty plan (rendered within budget).
    rendered: usize,
}

fn scope_summary(sessions: &[ScanResult], plans: &[SessionPlan]) -> ScopeCounts {
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
fn min_render_chars(sr: &ScanResult, budget: usize, cfg: &RichnessCfg) -> Option<usize> {
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
fn count_automation(plan: &SessionPlan) -> usize {
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
const AUTOMATION_KINDS: [crate::model::AutomationKind; 5] = [
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
fn automation_by_kind(plans: &[SessionPlan]) -> [usize; 5] {
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
fn automation_in_scope_by_kind(plans: &[SessionPlan]) -> [usize; 5] {
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
fn automation_breakdown_text(by: &[usize; 5]) -> String {
    by.iter()
        .zip(AUTOMATION_KINDS.iter())
        .filter(|(n, _)| **n > 0)
        .map(|(n, k)| format!("{n} {}", k.slug()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_json(
    ctx: &RenderCtx,
    sessions: &[ScanResult],
    plans: &[SessionPlan],
    out_path: Option<&Path>,
) -> Result<()> {
    use serde_json::json;
    let mut out_blob = String::new();

    // A machine-readable HEADER object so a JSON consumer can recover the human/automation
    // split + the budget fan-out WITHOUT the text-only `selected N user (M automation
    // triggers)` line (which was previously absent from the JSON stream entirely). It is the
    // FIRST line; its `kind` discriminator (`session_header`) matches the existing
    // `compaction_boundary` / `collapsed_agents` boundary-object convention.
    let sc = scope_summary(sessions, plans);
    let total_user: usize = plans
        .iter()
        .filter(|p| !p.selected.is_empty())
        .map(|p| count_sides(p, &ctx.cfg).0)
        .sum();
    let total_automation: usize = plans
        .iter()
        .filter(|p| !p.selected.is_empty())
        .map(count_automation)
        .sum();
    // Per-class automation breakdown (the lens-required attribution composition), keyed by
    // the stable `AUTOMATION_KINDS` order, emitted as a `by_kind` object so a consumer never
    // re-derives it from the per-unit `trigger_kind` fields.
    let by = automation_by_kind(plans);
    let by_kind: serde_json::Map<String, serde_json::Value> = AUTOMATION_KINDS
        .iter()
        .zip(by.iter())
        .map(|(k, n)| (k.slug().to_string(), json!(n)))
        .collect();
    // The whole-session composition, INDEPENDENT of budget selection — so a monitor-dominated
    // session never reports `monitor:0` just because the recency window didn't reach the deep
    // pulses (the selected `automation_by_kind` can read 0 for a class that has dozens in
    // scope). A reader compares the two to see "much monitor activity exists, little selected".
    let in_scope_by = automation_in_scope_by_kind(plans);
    let in_scope_by_kind: serde_json::Map<String, serde_json::Value> = AUTOMATION_KINDS
        .iter()
        .zip(in_scope_by.iter())
        .map(|(k, n)| (k.slug().to_string(), json!(n)))
        .collect();
    // `sessions_in_scope` is the TRUE scope (every discovered session); `sessions_rendered` is
    // how many fit the budget. Keeping them distinct stops a `--budget` knob from silently
    // rewriting "scope" and keeps a targeted top-level uuid from reading as `0 top-level`.
    let header = json!({
        "kind": "session_header",
        "sessions_in_scope": sc.in_scope,
        "sessions_rendered": sc.rendered,
        "top_level_sessions": sc.in_scope_top,
        "subagent_sessions": sc.in_scope_sub,
        "budget_chars": ctx.budget_chars,
        "budget_is_per_session": true,
        "max_total_chars": ctx.budget_chars.saturating_mul(sc.rendered.max(1)),
        "selected_user": total_user,
        "automation_triggers": total_automation,
        "automation_by_kind": by_kind,
        "automation_in_scope_by_kind": in_scope_by_kind,
    });
    {
        let s = serde_json::to_string(&header)?;
        println!("{s}");
        out_blob.push_str(&s);
        out_blob.push('\n');
    }

    for (sr, plan) in sessions.iter().zip(plans.iter()) {
        if plan.selected.is_empty() {
            continue;
        }
        let mut prev_comp: Option<usize> = None;
        for sel in &plan.selected {
            let Some(turn) = find_turn(plan, sel.turn_index) else {
                continue;
            };
            // Interleave boundary records the same way the text banners do.
            maybe_boundary_json(
                &mut prev_comp,
                turn.compactions_before,
                &sr.summaries,
                &mut |line_no, summary_chars| {
                    let obj = json!({
                        "kind": "compaction_boundary",
                        "line_no": line_no,
                        "summary_chars": summary_chars,
                    });
                    let s = serde_json::to_string(&obj).expect("serialize boundary");
                    println!("{s}");
                    out_blob.push_str(&s);
                    out_blob.push('\n');
                },
            );

            if let Some(u) = shown_user(turn, sel.sides) {
                emit_unit_json(sr, turn, u, &mut out_blob)?;
            }
            // The assistant lane: one object per KEPT agent message, plus a
            // `collapsed_agents` placeholder object per contiguous dropped span (carrying
            // X/Y/Z + the fetchable line range), in ascending agent order.
            for entry in shown_agent_lane(turn, sel.sides, &ctx.cfg) {
                match entry {
                    AgentRender::Kept(a) => emit_unit_json(sr, turn, &a.unit, &mut out_blob)?,
                    AgentRender::Placeholder(s) => {
                        emit_placeholder_json(sr, turn, &s, &mut out_blob)?
                    }
                }
            }
        }
    }

    // Trailing terminator object, emitted UNCONDITIONALLY (even when 0) so a JSONL consumer
    // can reliably detect end-of-stream for turns — matching search/files/recover, which
    // always close with a trailing summary. The key is `skipped_lines` (was a one-off `count`
    // alias, emitted only when > 0; both divergences are now removed for cross-subcommand
    // consistency).
    let term = json!({"kind":"skipped_lines","skipped_lines": ctx.skipped_lines});
    println!("{}", serde_json::to_string(&term)?);
    if let Some(p) = out_path {
        crate::recover::write_out_guarded(p, &out_blob)?;
    }
    Ok(())
}

/// Emit one unit as a JSON object. The `text` field is ALWAYS the full verbatim
/// message (json is for machines that do their own windowing); the truncation metadata
/// describes what the TEXT render would show.
fn emit_unit_json(
    sr: &ScanResult,
    turn: &TurnSlice,
    unit: &TurnUnit,
    out_blob: &mut String,
) -> Result<()> {
    use serde_json::json;
    let r = render_unit_body(unit, None);
    let mut obj = json!({
        "session_id": sr.session_id,
        // Id-domain discriminator (the r5 shape): `is_subagent` flags a bare-hex subagent
        // unit; `parent_session_id` is the always-re-feedable owning uuid (= session_id for
        // a top-level unit). A subagent `session_id` is NOT a `--session` target.
        "is_subagent": sr.is_subagent,
        "parent_session_id": sr.parent_session_id,
        "turn_index": turn.turn_index,
        "line_no": unit.line_no,
        "role": unit.role.label(),
        "ts_utc": unit.ts_utc,
        "ts_local": unit.ts_utc.as_deref().and_then(local_iso),
        "tool_calls": turn.tool_calls,
        "full_chars": unit.full_chars,
        "rendered_chars": r.rendered_chars,
        "truncated": r.truncated,
        "elided_chars": r.elided_chars,
        "elided_lines": r.elided_lines,
        "also_in_summary": unit.also_in_summary,
        "compactions_before": turn.compactions_before,
        "text": unit.text,
    });
    // STRUCTURED automation attribution on a USER segment: a machine pulse opener carries
    // `is_automation:true` + the parsed trigger CLASS / id / status as fields, so a JSON
    // consumer distinguishes a human turn from an automation pulse WITHOUT regexing the
    // `[<kind> …]` text prefix out of the prose. A human user turn carries
    // `is_automation:false` and omits the trigger fields. (An assistant unit is never an
    // automation opener, so it always renders `is_automation:false`.)
    if let Some(map) = obj.as_object_mut() {
        let is_user_automation = unit.role == Role::User && turn.is_automation;
        map.insert("is_automation".into(), json!(is_user_automation));
        if is_user_automation {
            if let Some(t) = turn.automation.as_ref() {
                map.insert("trigger_kind".into(), json!(t.kind.slug()));
                map.insert("task_id".into(), json!(t.task_id));
                map.insert("status".into(), json!(t.status));
                // `event` is the Monitor/ScheduleWakeup real-outcome tag (null on non-monitor
                // pulses). Surfaced so a JSON consumer sees a timed-out / event-bearing monitor
                // verbatim rather than inferring `completed` from an absent status.
                map.insert("event".into(), json!(t.event));
            }
        }
    }
    let s = serde_json::to_string(&obj)?;
    println!("{s}");
    out_blob.push_str(&s);
    out_blob.push('\n');
    Ok(())
}

/// Emit a collapsed-agent-span placeholder as a JSON record (the machine twin of the
/// text `△ L… [X agent messages, Y tool calls, Z failed]` line). Carries the exact X/Y/Z
/// counts + the fetchable first/last jsonl line so a consumer can `Read` the raw range.
fn emit_placeholder_json(
    sr: &ScanResult,
    turn: &TurnSlice,
    span: &PlaceholderSpan,
    out_blob: &mut String,
) -> Result<()> {
    use serde_json::json;
    let obj = json!({
        "kind": "collapsed_agents",
        "session_id": sr.session_id,
        "is_subagent": sr.is_subagent,
        "parent_session_id": sr.parent_session_id,
        "turn_index": turn.turn_index,
        "agent_messages": span.messages,
        "tool_calls": span.tool_calls,
        "failed": span.failed,
        "first_line": span.first_line,
        "last_line": span.last_line,
        "compactions_before": turn.compactions_before,
    });
    let s = serde_json::to_string(&obj)?;
    println!("{s}");
    out_blob.push_str(&s);
    out_blob.push('\n');
    Ok(())
}

/// JSON twin of [`maybe_boundary_banner`]: invoke `emit(line_no, summary_chars)` for
/// each crossed boundary, in ascending line order, exactly once each.
fn maybe_boundary_json(
    prev: &mut Option<usize>,
    current: usize,
    summaries: &[SummaryInfo],
    emit: &mut dyn FnMut(usize, usize),
) {
    for s in crossed_summaries(summaries, *prev, current) {
        emit(s.line_no, s.body_chars);
    }
    *prev = Some(current);
}

#[cfg(test)]
mod tests;
