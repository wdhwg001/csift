//! VerbatimArgs + WhoamiArgs.

use super::*;

#[derive(Debug, Args)]
#[command(
    about = "Turn-fidelity reconstruction: restore the verbatim user/assistant \
             back-and-forth a compaction summary clipped, within a char budget",
    long_about = "Turn-fidelity reconstruction: restore the verbatim user/assistant \
        back-and-forth that a Claude Code COMPACTION SUMMARY clipped. A summary preserves \
        TASK STATE (the 9-section synthesis: intent, file ledger, errors+fixes, plan, next \
        step) in high fidelity but PROVABLY LOSES turn fidelity; its \"All user messages\" \
        section clips ~22 real prose turns to ~17 `...`-truncated bullets, and the assistant \
        side collapses to a SINGLE verbatim quote. `verbatim` supplements (never replaces) the \
        summary: it re-emits the clipped user phrasings + discarded assistant end-of-turn \
        replies, IN ORIGINAL ORDER, each line carrying the jsonl LINE NUMBER so a consumer \
        can `Read` the raw transcript at the cited line.\n\n\
        NOT the tail-peek tool: to read a session's RECENT turns straight from the live \
        transcript (no compaction involved), use `show --turn N..` (e.g. `--turn -3..` = the \
        last 3 turns). `verbatim` is specifically for RESTORING the turns a compaction summary \
        already CLIPPED; its budget / round-trip / richness heuristics exist for that job.\n\n\
        SELECTION is recency-first (most-recent turns win the budget, what a resumed agent \
        most needs); the EMITTED document is sorted ascending so it reads as a forward \
        transcript. The backward walk is TRANSPARENT to compaction boundaries: a summary \
        record is a turn MEMBER, never a delimiter, so a 40K-char budget reaches back \
        across multiple boundaries by default (verified: 3 on one real sample, 2 on \
        another). `--max-compactions` only caps how far.\n\n\
        BUDGET (`--budget`, default 40000) bounds EACH session's reconstruction in CHARS \
        (sizing rule of thumb: ≈4 chars/token); it is applied PER session \
        in scope. The header's `spanned K of N compaction boundaries in scope` is \
        budget-relative on the K side: K counts the boundaries the SELECTED window crossed \
        (a small budget on a compaction-heavy session can legitimately read `0 of 4`: the \
        backward-from-EOF selection didn't reach them), while N is the session's true \
        total; the session-wide count is also `stats`' `compactions` (unwindowed). `verbatim` defaults to the TOP-LEVEL thread only, so a bare-uuid run realizes \
        just `budget` chars; with `--subagents` a target that spans S subagents \
        realizes up to `budget × (1 + S)` chars total (a scope banner surfaces the \
        multiplier). `--round-trip-fraction` \
        (default 0.5) is a HARD FLOOR: that fraction of the budget can ONLY be spent on \
        COMPLETE round-trips (user → [N tool calls] → assistant EOT), never on user-only / \
        assistant-only fragments; without it an assistant-heavy tail recovers ZERO human \
        turns. Over-cap units are MIDDLE-truncated (head+tail kept) with an explicit \
        `… [+K chars, L lines elided] …` marker; the assistant head is larger than the \
        user head (its prose front-loads context, back-loads the decision). Nothing is \
        ever fabricated or silently dropped.\n\n\
        AGENT MESSAGES (`--agent-msgs`, default `eot-only` = non-breaking): a single \
        genuine-user turn can own a LONG run of agent messages (a debugging/build chain \
        the model narrates) that the summary clips to one §9 quote. `eot-only` restores \
        just the last (today's behavior). `rich` ALSO restores the first/middle messages \
        that carry important info: a count, a commit hash, a `file.rs:NNN` ref, backtick \
        code, or a finding/decision lexeme, or that are clearly long, collapsing pure \
        \"let me look into this\" declarations into a `△ L… [X agent messages, Y tool \
        calls, Z failed]` placeholder (only on runs longer than the mode's threshold; \
        default 6; `--profile heavy` 4 / `light` 8). `all` keeps every agent message. \
        `--profile heavy|light` is the WHOLE tuning surface (per-knob flags are gone).\n\n\
        DEDUP: a turn the NEWEST summary already quotes verbatim is flagged `(also in \
        summary)` and DEMOTED (selected only after non-dup turns); never silently dropped \
        (a false positive must not lose a real turn).\n\n\
        AUTOMATION TRIGGERS: a machine `<task-notification>` (a background-command / \
        workflow / spawned-agent / monitor-tick COMPLETION pulse) OPENS a turn just like a \
        human message, and is rendered as a parsed attribution label `[<kind> <task-id> \
        <status>] <summary>` (kind = background-command | workflow | agent | monitor | task) \
        instead of the raw XML. The header reports the human/automation split (`selected N \
        user (M automation triggers) + …`). These pulses are EXCLUDED from the \
        `--round-trip-fraction` HARD FLOOR (that lane is reserved for human exchanges) but \
        can still be selected as Phase-2 fill.\n\n\
        BUDGET FAN-OUT: `--budget` is applied PER session in scope. `verbatim` defaults to the \
        top-level thread only, so a bare-uuid run is a single session at `budget` chars. With \
        `--subagents` the target also spans that session's subagents, so the realized \
        output is `budget × (sessions in scope)`; a top-of-output scope banner then names the \
        TRUE scope (all discovered top-level + subagent sessions), how many rendered within \
        budget, and the realized multiplier.\n\n\
        WINDOWING: `--turn (N|A..B|N..|-k)` (inclusive, 0-based genuine-user order) \
        intersects (AND) with `--since`/`--until` (ISO8601 / relative `2h`,`3d`,…). NOTE \
        `verbatim` TEXT prints `L<line>` per unit, NOT a turn marker; to pick a value for \
        `--turn` read the index from `csift search` text (`s·t<n>` header) or this \
        command's own `--format json` (`turn_index`). \
        `--out <PATH>` captures the SAME rendered reconstruction that prints to stdout into a \
        file (byte-identical; verbatim does NOT line-truncate stdout, so `--out` differs only in \
        going to a file rather than the terminal; over-cap units are middle-truncated with an \
        explicit `… [+K chars, L lines elided] …` marker in BOTH). For UN-truncated unit \
        bodies use `--format json`, which emits one VERBATIM object per unit (full `text`, no \
        per-unit cap) plus interleaved compaction-boundary records.",
    after_help = "EXAMPLES\n  \
          csift verbatim .                                     # default 40K-char reconstruction, top-level thread\n  \
          csift verbatim @<uuid> --budget 12000                # recover JUST my thread (~10-15K, no fan-out)\n  \
          csift verbatim @<uuid> --subagents --format json     # ALSO span subagents (budget × N, line-numbered)\n  \
          csift verbatim @<uuid> --round-trip-fraction 0.6     # weight harder toward complete round-trips\n  \
          csift verbatim . --budget 40000 --out /tmp/verbatim.md  # full reconstruction to a file\n  \
          csift verbatim . --budget 36000 --window 9000 --slice 1   # 1st ≤9000-char chunk for a SessionStart hook (fan slices 1..4)\n  \
          csift verbatim @<uuid> --budget 8000 --max-compactions 1  # stay within one compaction boundary\n  \
          csift verbatim @<uuid> --agent-msgs eot-only         # last-message-only per turn\n  \
          csift verbatim @<uuid> --profile heavy               # lower thresholds (max fidelity)\n  \
          csift verbatim @<uuid> --agent-msgs all              # every agent message, no filtering\n\n\
        AUTOMATION TRIGGERS\n  \
          A machine `<task-notification>` (a background-command / workflow / spawned-agent /\n  \
          monitor-tick COMPLETION pulse Claude Code injects as a `type:\"user\"` record) OPENS\n  \
          a turn like a human message, so it appears in the reconstruction. It renders as a\n  \
          PARSED attribution label `[<kind> <task-id> <status>] <summary>` (kind =\n  \
          background-command | workflow | agent | monitor | task, read from the summary);\n  \
          never the raw `<task-id>` / `<output-file>` XML. The header reports the\n  \
          human/automation split, e.g. `selected 16 user (3 automation triggers) + 58\n  \
          assistant units`. These pulses are EXCLUDED from the `--round-trip-fraction` HARD\n  \
          FLOOR (reserved for human exchanges) but can still be selected as Phase-2 fill.\n  \
          LIMITATION: only `<task-notification>` COMPLETION pulses are segmented + attributed.\n  \
          The isMeta ScheduleWakeup WAKEUP-TICK *prompts* (a monitor/cron tick FIRING, e.g.\n  \
          `MONITOR TICK`) bypass this; they are isMeta records that do NOT open a\n  \
          turn, so the agent run a tick triggers currently groups under the PRECEDING\n  \
          genuine-user turn (not yet split into per-tick segments). In an automation-heavy\n  \
          monitor session this lumps the dominant tick-driven work onto one human turn.\n\n\
        BUDGET FAN-OUT\n  \
          `--budget` is PER session in scope. `verbatim` defaults to the TOP-LEVEL thread only, so\n  \
          a bare-uuid run is one session at `budget` chars. Add `--subagents` to also\n  \
          span that session's subagents: the realized output is then budget × (sessions in\n  \
          scope), and a top-of-output `scope` line names the TRUE scope (all top-level +\n  \
          subagent sessions discovered), how many rendered within budget, and the multiplier.\n  \
          A targeted top-level session that does not fit `budget` is reported with an explicit\n  \
          `skipped; needs ≥ N chars` note, never silently dropped.\n\n\
        JSON SCHEMA (per --format json)\n  \
          A leading `{kind:\"header\", sessions_in_scope, sessions_rendered,\n  \
          top_level_sessions, subagent_sessions, budget_chars, budget_is_per_session,\n  \
          max_total_chars, round_trip_fraction, chars_used, boundaries_spanned,\n  \
          boundaries_total, selected_user, selected_assistant, automation_triggers,\n  \
          automation_by_kind, automation_in_scope_by_kind, with_elicitation_sidecar}`\n  \
          object (the full budget accounting the text header shows;\n  \
          `boundaries_spanned` is budget-window-relative, `boundaries_total` the scope's\n  \
          true total);\n  \
          `sessions_in_scope` is the TRUE scope (every discovered session), `sessions_rendered`\n  \
          is how many fit the budget, the top_level/subagent split is over ALL in scope,\n  \
          `budget_chars`/`max_total_chars` are ALWAYS in CHARS,\n  \
          `automation_by_kind` breaks the SELECTED `automation_triggers` total down per class\n  \
          ({background-command,agent,workflow,monitor,task}), and `automation_in_scope_by_kind`\n  \
          is the SAME breakdown over EVERY in-scope automation pulse REGARDLESS of budget\n  \
          selection, so a monitor-heavy session is never read as `monitor:0` just because the\n  \
          recency window selected none of its deep pulses (compare the two to see how much\n  \
          automation exists vs was rendered). Then one object PER emitted unit:\n  \
          {session_id, is_subagent, parent_session_id, turn_index, line_no, role, ts_utc,\n  \
          ts_local, tool_calls, full_chars, rendered_chars, truncated, elided_chars,\n  \
          elided_lines, also_in_summary, compactions_before, text, is_automation} (is_subagent\n  \
          flags a bare-hex subagent unit; re-feed parent_session_id, never the bare session_id);\n  \
          an automation USER unit additionally\n  \
          carries {trigger_kind, task_id, status, event} (event = the Monitor/ScheduleWakeup\n  \
          outcome tag, null on non-monitor pulses). Boundary objects are tagged\n  \
          {kind:\"compaction_boundary\",…} / {kind:\"collapsed_agents\",…}; and a trailing\n  \
          {kind:\"summary\", skipped_lines} ALWAYS closes the stream (even when 0). The envelope\n  \
          is UNIFORM tool-wide (envelope v2): EVERY command's stream is `{kind:\"header\",…}` →\n  \
          kind-tagged rows → `{kind:\"summary\",…}`, so `tail -1 | jq 'select(.kind==\
          \"summary\")'` reads the footer of ANY command identically."
)]
pub struct VerbatimArgs {
    /// Project target(s) (actual cwd or encoded dir) whose session(s) to reconstruct
    /// turns from, OR an `@<uuid>` session token. Repeatable; with none, every project is
    /// scanned. `@<uuid>` (8-4-4-4-12 hex) scopes to one session, so `csift verbatim @<uuid>` works
    /// as the EXAMPLES show; `@<agent-id>` reconstructs that ONE subagent's own thread (ids
    /// from `csift agents`); `@main`/`@trap:<marker>` resolve the calling session, and a
    /// `*.jsonl` file scopes to that transcript. A target is REQUIRED (a bare `csift verbatim`
    /// would realize `--budget` × every session of every project). NOTE: `verbatim` defaults to
    /// the TOP-LEVEL thread only (the single-thread recovery use case), so `csift verbatim
    /// @<uuid>` reconstructs just that conversation; add `--subagents` for the rare
    /// cross-fan-out reconstruction (`--budget` is then applied PER session in scope; see
    /// `--budget`).
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target
    )]
    pub paths: Vec<PathBuf>,

    /// Scope ALSO to the session ids in FILE (`-` = stdin): whitespace/newline-separated
    /// uuid / uuid-prefix / agent-id tokens, bare or `@`-prefixed, exactly the ids csift
    /// emits (`search -l`, JSON `transcript_ids` / `parent_session_id`). UNION with positional
    /// targets; each id resolves fail-loud like an `@` positional. An EMPTY list (an upstream
    /// stage that found nothing) scopes to NOTHING: honest empty, exit 0, never a silent
    /// widening to every project. The resolved ids then follow this command's normal
    /// span rules, exactly as if they were `@` positionals: on the span-by-default
    /// commands each session EXPANDS to its subagent transcripts (add `--no-subagents`
    /// to pin to exactly the listed ids).
    #[arg(long = "sessions-from", value_name = "FILE|-")]
    pub sessions_from: Option<std::path::PathBuf>,

    /// Character budget applied PER session in scope (chars, always; sizing rule ≈4
    /// chars/token). Each session's reconstruction is bounded by this; `verbatim` defaults to
    /// the top-level thread only, so a bare-uuid run realizes just `budget` chars. With
    /// `--subagents` the realized total is `budget × (sessions in scope)` and a scope banner
    /// surfaces the multiplier. Default 40000.
    #[arg(long, value_name = "N", default_value_t = 40000)]
    pub budget: usize,

    /// Fraction of the budget RESERVED to guarantee complete round-trips (user →
    /// [N tool calls] → assistant EOT), not user messages alone. A hard floor.
    /// Default 0.5; must be in the open interval (0.0, 1.0).
    #[arg(long = "round-trip-fraction", value_name = "F", default_value_t = 0.5)]
    pub round_trip_fraction: f64,

    /// Also span each in-scope session's SUBAGENT transcripts (built-in Task/Agent-tool,
    /// OMC, and Workflow agents) under `subagents/**`. Default OFF for `verbatim` (UNLIKE
    /// files/search): `verbatim` is a SINGLE-THREAD recovery tool and `--budget` MULTIPLIES per
    /// session in scope, so spanning hundreds of unrelated fan-out subagents by default would
    /// bury the thread you asked to restore under megabytes of noise. Opt in with
    /// `--subagents` only for the rare cross-fan-out reconstruction.
    #[arg(
        long = "subagents",
        overrides_with = "no_subagents",
        default_value_t = false
    )]
    pub include_subagents: bool,

    /// Exclude subagent transcripts: reconstruct only from the top-level session. This is
    /// already the `verbatim` DEFAULT; the flag is kept for symmetry with the other subcommands
    /// and to explicitly cancel an earlier `--subagents` (last flag wins).
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Stop walking back after crossing N compaction boundaries (0 = unlimited;
    /// default 0). A guard, not a target.
    #[arg(long = "max-compactions", value_name = "N", default_value_t = 0)]
    pub max_compactions: usize,

    /// Per-turn agent-message policy (MASTER switch for the multi-agent-message model).
    /// `longest` (DEFAULT) keeps the LONGEST agent message: the substantive Rich Response,
    /// which is frequently a MIDDLE message, not the last ~50-char throwaway wrap-up that
    /// the old `agents.last()` default silently kept, PLUS the first message when it is
    /// substantive (>= the mode's rich-min threshold) PLUS each rich middle (a number, commit
    /// hash, file:line, backtick code, or finding/decision lexeme, or clearly long),
    /// collapsing the rest into a placeholder. `eot-only` forces the old single-EOT
    /// behavior (only each turn's last agent message; byte-identical to the pre-feature
    /// output). `rich` keeps the last always + the first by position privilege + each
    /// non-droppable middle, only on a long run (over the mode's run threshold).
    /// `all` keeps every agent message.
    #[arg(long = "agent-msgs", value_enum, default_value_t = crate::turns::AgentMsgMode::Longest)]
    pub agent_msgs: crate::turns::AgentMsgMode,

    /// Convenience threshold bundle, applied BEFORE the individual flags (so an explicit
    /// flag overrides the profile). `heavy` = maximal fidelity (threshold 4, rich-min 200,
    /// declaration-max 140); `light` = lean (threshold 8, rich-min 360, declaration-max
    /// 240). Neither changes the master `--agent-msgs` mode: with no `--agent-msgs` the
    /// mode stays the default `longest` (with the profile's thresholds); add `--agent-msgs
    /// rich` to also switch the keep-set.
    #[arg(long = "profile", value_enum)]
    pub profile: Option<crate::turns::Profile>,

    /// Inclusive turn-index range in the shared grammar: `N` (one turn) · `A..B` · `N..` (to the end) · `..N` · `-k` from the end (`-3..` = the last 3) - 0-BASED: turn 0 is the pre-first-user
    /// lead (the session's opening context), so `1..N` SKIPS it. A turn opens on a genuine
    /// user message, an answered AskUserQuestion, or a plan-rejection-with-message.
    /// Discover turn indices from the `<id>·t<n>` exchange header in `csift search` text output, or the
    /// `turn_index` field in any `--format json` record. Intersects (AND) with `--since` /
    /// `--until`.
    #[arg(
        long = "turn",
        value_name = "N|A..B|N..|-k",
        allow_hyphen_values = true
    )]
    pub turn_range: Option<String>,

    /// Lower time bound. WHEN grammar (system-local tz): a relative `Ns`/`Nm`/`Nh`/`Nd`/`Nw`
    /// = that many seconds/minutes/hours/days/weeks AGO (`45s`, `90m`, `2h`, `3d`, `1w`);
    /// an ISO8601 instant (`2026-06-01T05:00:00Z` / `…+10:00`); a BARE datetime
    /// (`2026-06-01T05:00:00`) = that LOCAL wall-clock time; or a bare date (`2026-06-01`)
    /// = LOCAL MIDNIGHT that day. Intersects (AND) with --turn (both filters apply).
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as --since). Intersects (AND) with --turn (both filters apply).
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Capture the SAME rendered reconstruction that prints to stdout into this file
    /// (byte-identical; verbatim does NOT truncate stdout; over-cap units are middle-truncated
    /// with a `… [+K chars, L lines elided] …` marker in BOTH). The summary still prints to
    /// stdout. For UN-truncated unit bodies use `--format json` (full verbatim `text`).
    #[arg(long, value_name = "PATH")]
    pub out: Option<PathBuf>,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// CHUNKED OUTPUT: paginate the recovered DOCUMENT (the verbatim turns: the same content
    /// `--out` writes) into ≤`--window`-CHARACTER chunks and print ONLY the Nth chunk (1-based)
    /// to stdout, with NO operational chrome (scope banner / SESSION header / footer). Built for
    /// fanning a >10K reconstruction across several SessionStart hooks: Claude Code caps EACH
    /// hook's `additionalContext` at 10,000 CHARACTERS (over-cap is replaced by a file-path +
    /// short preview, i.e. the body is effectively LOST to the model), so one hook per slice
    /// keeps every injected chunk under the wall. Slicing is DETERMINISTIC (same session +
    /// budget ⇒ identical chunk boundaries), so N independent hooks can each request their own
    /// slice; the lock/ordering lives in the hook shell. An out-of-range N prints nothing (exit
    /// 0); surplus hooks simply inject nothing. Text format only; not combinable with `--out`.
    #[arg(long, value_name = "N")]
    pub slice: Option<usize>,

    /// Chunk size for `--slice`, in CHARACTERS (Unicode scalars: the unit Claude Code's
    /// 10,000-char `additionalContext` cap counts, so a CJK-heavy document is NOT 3×
    /// over-counted the way a byte budget would). Default 10000 = the cap; pass a little under
    /// (e.g. `--window 9000`) to leave headroom for any wrapper text the hook adds around the
    /// chunk. Lines are packed greedily up to the window at LINE boundaries; a single line
    /// longer than the window is hard-split on a char boundary so no chunk ever exceeds it.
    /// Ignored without `--slice`.
    #[arg(long, value_name = "N", default_value_t = 10000)]
    pub window: usize,

    /// FIXED-FLEET mode: pin the reconstruction to AT MOST N slices of `--window` chars each,
    /// instead of letting `--budget` decide a VARIABLE number of chunks. A hook fleet is a fixed
    /// set of registered `SessionStart` hooks, so the slice COUNT, not the char budget, is the
    /// hard constraint: it must NOT drift to 5/6/7 as the turns grow. With `--slices N`, csift
    /// fills the N newest-first slices with WHOLE turns (the per-role 600/900 caps are dropped; a
    /// turn is ellipsized ONLY if it alone exceeds one window), and DISCARDS the oldest turns that
    /// don't fit, so the emitted count is ALWAYS ≤N no matter how big the turns are. Requires
    /// `--slice i` to pick which chunk; the budget becomes N×`--window`, so `--budget` is ignored.
    /// Without `--slices`, `--slice` keeps its legacy budget-driven, variable-chunk-count behavior.
    #[arg(long, value_name = "N")]
    pub slices: Option<usize>,
}

impl VerbatimArgs {
    /// Resolve the include/exclude flags into a single decision. UNLIKE the other
    /// subcommands, `verbatim` defaults to TOP-LEVEL-ONLY (`include_subagents` defaults false):
    /// spanning is opt-in via `--subagents`, and a trailing `--no-subagents` still
    /// forces it off. So a `csift verbatim @<uuid>` reconstructs just that one thread.
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.include_subagents && !self.no_subagents
    }

    /// Resolve the agent-message policy into a [`crate::turns::RichnessCfg`]. The
    /// per-knob tuning flags are GONE (0-backcompat surface diet): `--profile` bundles are
    /// the only tuning: `heavy` (4/200/140) / `light` (8/360/240) / none (6/280/200).
    #[must_use]
    pub fn richness_cfg(&self) -> crate::turns::RichnessCfg {
        use crate::turns::{Profile, RichnessCfg};
        let (run_threshold, rich_min_chars, declaration_max_chars) = match self.profile {
            Some(Profile::Heavy) => (4, 200, 140),
            Some(Profile::Light) => (8, 360, 240),
            None => (6, 280, 200),
        };
        RichnessCfg {
            mode: self.agent_msgs,
            run_threshold,
            rich_min_chars,
            declaration_max_chars,
            keep_first: true,
        }
    }
}

#[derive(Debug, Args)]
#[command(
    about = "Identify the calling Claude Code session (via CLAUDE_CODE_SESSION_ID, falling \
             back to CODEX_COMPANION_SESSION_ID)",
    long_about = "Identify the CALLING Claude Code session, false-positive-safe.\n\n\
        Claude Code exports `CLAUDE_CODE_SESSION_ID` into every Bash-tool environment, \
        and its value equals the calling session's own jsonl basename exactly. That \
        canonical var is the signal csift trusts: per-session, version-independent, \
        survives bash nesting, zero false positives. When it is absent, csift falls back \
        to `CODEX_COMPANION_SESSION_ID` (the alias the Codex companion plugin sets) before \
        giving up. (The exact var names are matched; never a loose /session/i regex, which \
        would false-positive on macOS's SECURITYSESSIONID.)\n\n\
        When NEITHER var is set (an old CC build, or running outside CC/Codex), whoami \
        does NOT guess; it errors with guidance to pass an explicit `@<uuid>` target. \
        Most-recent-mtime and process-tree walking are FORBIDDEN: many CC sessions \
        may be live at once, so mtime is almost always wrong. It is acceptable for \
        whoami to often say \"ambiguous, pass `@<uuid>`\".\n\n\
        SUBAGENT CAVEAT: which session $CLAUDE_CODE_SESSION_ID names depends on HOW the \
        subagent was spawned. In a built-in Task/Agent SUBAGENT it is the SUBAGENT's OWN id \
        (so `whoami` identifies the subagent, not the main session). In an \
        ORCHESTRATED/workflow subagent (e.g. an OMC Workflow `agent()`) it is the PARENT \
        session's id (so `whoami` resolves the ROOT, not a subagent). Do NOT assume which; \
        disambiguate via the recovery: feed the resolved id to `csift agents --agent <id> \
        --format json`; if it returns the node, read `parent_session_id` for the ROOT (one \
        call); if it errors `no subagent matched`, the id is ALREADY top-level; use it \
        directly. (Or scan `csift agents .` / `csift list .` on the project PATH and find the \
        parent uuid.) The ENV form's JSON carries `{session_id, path}`; the `@trap:<marker>` form \
        instead returns the UPSTREAM CHAIN `{chain:[{session_id, is_subagent, parent_session_id, \
        depth, path}, …]}` (self first, top-level root last), so a subagent reads is_subagent / \
        parent_session_id directly, no `agents` round-trip.\n\n\
        FLAG NOTE: whoami's only positional is the optional SELF token `@trap:<marker>` / `@main`; \
        the `path` line is ALWAYS printed. Every OTHER session-operating subcommand takes a \
        general POSITIONAL `[PATH]...` / `@`-token; there is no target FLAG.",
    after_help = "SESSION-ID SOURCE\n  \
          The canonical env var CLAUDE_CODE_SESSION_ID (CC sets it per Bash-tool process; \
        its value IS the calling session's jsonl basename). If absent, csift falls back to \
        CODEX_COMPANION_SESSION_ID (the Codex companion plugin's alias). If NEITHER is set, \
        whoami errors with guidance to pass an `@<uuid>` target; it never guesses by mtime.\n\n\
        SUBAGENT CAVEAT\n  \
          What the env var names depends on HOW the subagent was spawned and has CHANGED \
        across CC versions: current CC hands an Agent-tool subagent the PARENT session's id \
        (verified live 2026-07-12: the subagent's OWN id is withheld from its env), and an \
        ORCHESTRATED/workflow subagent (e.g. an OMC Workflow `agent()`) likewise holds the \
        PARENT id; older builds handed a built-in Task subagent its OWN id. So from a \
        subagent, plain `whoami` usually resolves the ROOT, not you. For a DEFINITIVE, \
        env-INDEPENDENT answer use `whoami @trap:<marker>`: you embed a one-shot literal marker in \
        that very command and csift maps it to your subagent hex and walks the UPSTREAM CHAIN up to \
        the top-level root (the @trap form's JSON is `{chain:[{session_id, is_subagent, \
        parent_session_id, depth, path}, …]}`). @trap TIMING: a subagent's transcript records the \
        command mid-run (a first try resolves); the MAIN conversation flushes its own record only \
        AFTER the command completes, so a top-level FIRST use always misses; from the main thread \
        use `@main` (no race), or re-run the SAME command with the SAME marker. \
        WITHOUT @trap, don't assume which id the env gave; feed it to `csift agents --agent <id> \
        --format json`; a returned node → read `parent_session_id` for the ROOT; `no subagent \
        matched` → the id is ALREADY top-level. Plain `whoami` JSON carries only {session_id, \
        path}; from the env alone it canNOT know is_subagent / parent_session_id (that is exactly \
        what `@trap` resolves).\n\n\
        FLAG NOTE\n  \
          whoami takes an OPTIONAL positional SELF target: `@trap:<marker>` or `@main` ONLY; the \
        `path` line is ALWAYS printed. Every OTHER session-operating subcommand takes a general \
        POSITIONAL [PATH]... / `@`-token. There is no target flag anywhere.\n\n\
        EXAMPLES\n  \
          csift whoami                  # the calling session's uuid + its jsonl path\n  \
          csift whoami --format json    # {\"session_id\":\"…\",\"path\":\"…\"}\n  \
          csift whoami @trap:<invent-a-fresh-3word-4digit-marker>   # which SUBAGENT am I? -> upstream chain (self -> ... -> top-level root); the marker is YOURS to invent, never a copied literal\n  \
          # FALLBACK (no @trap): map this subagent's bare hex to its ROOT (read parent_session_id):\n  \
          csift agents --agent \"$(csift whoami --format json | jq -r 'select(.kind==\"identity\").session_id')\" --format json\n  \
          # …then scope the whole conversation with that parent uuid:\n  \
          csift search \"<pattern>\" \"@$(csift whoami --format json | jq -r 'select(.kind==\"identity\").parent_session_id')\"\n\n\
        JSON SCHEMA (per --format json)\n  \
          Envelope: header → {kind:\"identity\", session_id, is_subagent, parent_session_id, \
        depth, path} rows → a {kind:\"summary\", identities} terminator. One identity row for \
        a plain run; `@trap:<marker>` emits the full upstream chain, depth 0 (yourself) up \
        to the top-level root. Select with `jq 'select(.kind==\"identity\")'`."
)]
pub struct WhoamiArgs {
    /// Optional SELF target: `@trap:<marker>` or `@main` (nothing else). With NO target, identify
    /// the calling session from `$CLAUDE_CODE_SESSION_ID` (the historical behavior; `@main` is its
    /// explicit spelling). `@trap:<marker>` answers "which SUBAGENT am I?": a running subagent
    /// (whose own id CC withholds from the env) embeds a unique, literal, one-shot marker in THIS
    /// very csift command and csift maps it to the subagent's bare hex and walks the UPSTREAM
    /// ancestry CHAIN up to the top-level root (the walk-UP mirror of `agents`' walk-DOWN);
    /// env-INDEPENDENT, so it is reliable for a built-in Task AND a workflow subagent (whose env id
    /// is the PARENT). To inspect a DIFFERENT session, use `list`/`agents`, not `whoami`.
    #[arg(value_name = "SELF")]
    pub self_target: Option<String>,

    /// Emit JSON instead of text.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}
