//! Marker constants + tiny string predicates for CC's synthetic record shapes.

/// The synthesized prefixes Claude Code writes into the `tool_result` answering an
/// `AskUserQuestion` (§4.4). CC has shipped (at least) TWO phrasings for the same
/// synthesized answer - verified across real `~/.claude/projects` data:
///
/// - `"User has answered your questions: \"<q>\"=\"<a>\". …"`
/// - `"Your questions have been answered: \"<q>\"=\"<a>\". …"`  (the dominant form
///   in current data; a single hardcoded marker missed it entirely)
///
/// Some sessions span a version transition and contain BOTH forms, so an AUQ
/// answer must be recognised if it carries EITHER prefix. Used to surface AUQ
/// answers under the `user` category without re-parsing `toolUseResult`.
pub const AUQ_ANSWER_MARKERS: &[&str] = &[
    "User has answered your questions",
    "Your questions have been answered",
];

/// True when `text` (a `tool_result`'s rendered content) is a synthesized
/// AskUserQuestion answer - i.e. it STARTS WITH a known AUQ-answer marker (§4.4).
///
/// START-anchored, not `contains`: a real AUQ answer is a CC-machine-synthesized
/// string that always LEADS with its marker (`"User has answered your questions: …"`
/// / `"Your questions have been answered: …"`). A `contains` check false-positives on
/// any tool_result that merely QUOTES the marker mid-content - e.g. csift's own dev
/// sessions Read/grep SPEC.md + fixtures that DOCUMENT these markers, which used to be
/// mislabeled `user.answer` and dumped whole files. `trim_start` tolerates leading
/// whitespace the renderer may prepend without admitting a mid-content quote.
#[must_use]
pub fn is_auq_answer_text(text: &str) -> bool {
    let head = text.trim_start();
    AUQ_ANSWER_MARKERS.iter().any(|m| head.starts_with(m))
}

/// True when `content` (a user record's string or joined-text body) is a
/// machine-synthesized marker the user never typed (§4.2.1–.3) and so must NOT count
/// as a genuine human turn: an exact interrupt marker, a `<local-command-stdout>…`
/// output, or a `<command-name>…` slash-command wrapper. CODEPOINT-SAFE - exact `==`
/// (interrupts, whole-token) or `starts_with` (the two ASCII-tag prefixes); never a
/// byte-offset slice.
#[must_use]
pub fn is_synthetic_user_marker(content: &str) -> bool {
    INTERRUPT_MARKERS.contains(&content)
        || content.starts_with(LOCAL_COMMAND_STDOUT_PREFIX)
        || is_slash_command_wrapper(content)
        || is_agents_stopped_notice(content)
}

/// The harness's "N background agent(s) were stopped by the user: ..." notice (v0.10.0):
/// a plain-string `type:"user"` record Claude Code writes when async agents are killed
/// from the UI. It names a count and truncated prompt prefixes, never an id, and it
/// triggers no generation - the model sees it only alongside the next real prompt. Not
/// the operator, never a turn opener; classifies `harness.notification.subagent`.
#[must_use]
pub fn is_agents_stopped_notice(content: &str) -> bool {
    let s = content.trim_start();
    // The singular template names the agent: `Background agent "<desc>" was stopped by
    // the user.` (no count).
    if s.starts_with("Background agent \"") && s.contains(" was stopped by the user") {
        return true;
    }
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return false;
    }
    let rest = &s[digits..];
    (rest.starts_with(" background agent was stopped by the user")
        || rest.starts_with(" background agents were stopped by the user"))
        && rest.contains(AGENTS_STOPPED_MARKER)
}

/// The raw-byte marker the agents-stopped notice always carries (the synth-marker
/// needle for its fabricated `[subagent stopped]` label prefix).
pub const AGENTS_STOPPED_MARKER: &str = "stopped by the user";

/// The two exact-content synthesized strings Claude Code writes when the user
/// interrupts (§4.2.1). They are a `type:"user"` `text`-block record whose content is
/// EXACTLY one of these - a machine-synthesized interrupt marker, NOT a human turn.
/// Verified across real `~/.claude/projects` data: 116 + 21 occurrences, all
/// non-`isMeta`, none carrying any extra prose (dropping them as turn boundaries loses
/// zero user content).
pub const INTERRUPT_MARKERS: &[&str] = &[
    "[Request interrupted by user]",
    "[Request interrupted by user for tool use]",
];

/// Prefix of a `<local-command-stdout>…` user record (§4.2.2) - local-command OUTPUT
/// (machine), not the user's prose. Non-`isMeta` string content (its sibling
/// `<local-command-caveat>` carries `isMeta` and is already excluded). Must NOT open a
/// turn.
pub const LOCAL_COMMAND_STDOUT_PREFIX: &str = "<local-command-stdout>";

/// Prefix of a `<command-name>/x…</command-name>` slash-command invocation record
/// (§4.2.3) - the machine-templated EXPANSION of a slash command, non-`isMeta`. The
/// templated wrapper must NOT open a turn; any genuine prose the user typed after the
/// command lives in the `<command-args>…</command-args>` body and is recovered
/// separately (see [`Record::slash_command_args`]).
pub const COMMAND_NAME_PREFIX: &str = "<command-name>";

/// The SAME slash-command wrapper in the NEWER tag order: current CC emits
/// `<command-message>…</command-message>\n<command-name>/x</command-name>\n<command-args>…`
/// -- the message tag FIRST (verified live 2026-07: 14 sessions new-order vs 35 old-order
/// in one real corpus; both orders coexist). Detection must accept EITHER leading tag:
/// anchoring on `<command-name>` alone silently reclassified every new-order record as
/// GENUINE user prose - raw wrapper XML surfaced as `user.message`, and a no-args
/// wrapper opened a turn.
pub const COMMAND_MESSAGE_PREFIX: &str = "<command-message>";

/// True when `content` is a slash-command wrapper (§4.2.3) in EITHER tag order.
#[must_use]
pub fn is_slash_command_wrapper(content: &str) -> bool {
    content.starts_with(COMMAND_NAME_PREFIX) || content.starts_with(COMMAND_MESSAGE_PREFIX)
}

/// Prefix of a `<task-notification>…</task-notification>` user record - a MACHINE-INJECTED
/// automation trigger (a background-command / workflow / spawned-task completion notice CC
/// inserts as a `type:"user"`, non-`isMeta`, STRING-content record). It LOOKS like a human
/// turn to [`Record::is_genuine_user`] (it passes every gate), so it DOES open a turn - but
/// it is an automation pulse, not the operator's prose. [`Record::automation_trigger`]
/// classifies it so surfaces can LABEL the segment (`[workflow <id> completed] <summary>`)
/// instead of dumping the raw `<task-id>`/`<output-file>`/`<status>` XML wrapper.
pub const TASK_NOTIFICATION_PREFIX: &str = "<task-notification>";

/// The synthesized marker Claude Code writes into the `tool_result` when the user
/// REJECTS a tool use (§4.2.4) - fires for ANY rejected tool_use (ExitPlanMode plan
/// kick-backs AND rejected AskUserQuestion / Edit / etc.). On its own it is NOT a user
/// turn; it becomes one only when followed by the [`PLAN_REJECTION_USER_PREFIX`] tail
/// (a real typed user instruction).
pub const PLAN_REJECTION_MARKER: &str = "The user doesn't want to proceed with this tool use";

/// The fixed ASCII delimiter that precedes the user's typed instruction in a
/// rejection-with-message (§4.2.4): everything AFTER it is the genuine user message.
/// A rejection WITHOUT this delimiter (the `STOP what you are doing and wait…` form)
/// carries no typed message and must NOT open a turn.
pub const PLAN_REJECTION_USER_PREFIX: &str = "To tell you how to proceed, the user said:\n";

/// The opening tag of an inbound `<teammate-message …>` peer-agent message (GOLD §5). A
/// teammate (another Claude session on the same team / FleetView) sends prose or a control
/// signal that Claude Code delivers as a `type:"user"`, `role:"user"`, STRING-content record
/// -- so it LOOKS like a human turn ([`Record::is_genuine_user`] used to return `true` for it,
/// counting 106 peer messages as the human user in one real session). It is a peer message,
/// never the operator: classified `agent.communication.{inbox,signal}`, never `user`.
pub const TEAMMATE_MESSAGE_OPEN: &str = "<teammate-message";

/// The preamble Claude Code prepends to a peer message it relays into a session (GOLD §5):
/// `Another Claude session sent a message:\n<teammate-message …>`. A peer tag IMMEDIATELY after
/// this preamble is at a section BOUNDARY (FINDING-1, [`is_section_boundary`]) - so a real relayed
/// peer message is recognized while a tag merely QUOTED mid-prose is not.
pub const PEER_MESSAGE_PREAMBLE: &str = "Another Claude session sent a message:";

/// The opening tag of an inbound `<agent-message from="…">` peer form (P1c M1) - a DISTINCT
/// inbound peer message from [`TEAMMATE_MESSAGE_OPEN`], seen `isMeta` in real data (e.g. an OMC
/// agent replying to a peer, relayed into this session: `<agent-message
/// from="oh-my-claudecode:architect">…`). Classifies `agent.communication.inbox` (the
/// `from="…"` attribute ⇨ self), never `user.message`.
pub const AGENT_MESSAGE_OPEN: &str = "<agent-message";

/// Section CLOSE tags (FINDING-1). A peer / `<task-notification>` open tag that sits right after
/// one of these (modulo whitespace) is at a section BOUNDARY ([`is_section_boundary`]), so a
/// BATCHED record's later sections are still recognized - while a tag QUOTED mid-prose (a genuine
/// user message that merely mentions the literal tag, common in csift's OWN dev sessions) is NOT a
/// boundary and never starts a section. Kept beside their open-tag constants so the pair never drift.
pub(crate) const TASK_NOTIFICATION_CLOSE: &str = "</task-notification>";
pub(crate) const TEAMMATE_MESSAGE_CLOSE: &str = "</teammate-message>";
pub(crate) const AGENT_MESSAGE_CLOSE: &str = "</agent-message>";

/// The leading sentence of an ASYNC/background `Agent` spawn's launch-confirmation tool_result
/// (`"Async agent launched successfully.\nagentId: …"`). This is a launch ACK, NOT the child's
/// report - the report arrives LATER via the `<task-notification>` `<result>` pulse (G1 → inbox).
/// On disk the ack also carries the structured `toolUseResult.{isAsync:true, status:"async_launched"}`
/// shape ([`Record::is_async_launch_ack`] prefers the structured signal, falls back to this prefix).
pub const ASYNC_LAUNCH_ACK_PREFIX: &str = "Async agent launched successfully";

/// The fixed harness-injected continuation marker (GOLD §5) - `harness.schedule.continuation`.
/// A `type:"user"` (`isMeta`) record CC injects to resume a session from where it left off.
/// Verified across real `~/.claude/projects` data (522 occurrences), exact content.
pub const SCHEDULE_CONTINUATION_MARKER: &str = "Continue from where you left off.";

/// The `ScheduleWakeup` TIMER's fired-prompt sentinel (GOLD §5) - `harness.schedule.wakeup`.
/// When a `ScheduleWakeup` tool fires, the harness injects its `prompt`; this fixed sentinel is
/// that injected prompt. This is the SCHEDULER timer firing - DISTINCT from the autonomous-loop
/// DRIVER ticks (`# Autonomous loop tick` / `Run the autonomous check`), which are
/// `harness.meta.loop` ([`AUTONOMOUS_LOOP_TICK_PREFIX`] / [`AUTONOMOUS_CHECK_MARKER`]); the two
/// must not be conflated. Together with [`SCHEDULE_WAKEUP_LOOP_CHECK_PREFIX`] /
/// [`SCHEDULE_WAKEUP_TIMER_MARKER`] these are the fixed wakeup-tick markers - a generic
/// cron/monitor tick's injected prompt is still operator-authored free text with no universal
/// marker (the `ScheduleWakeup` *tool_use* that ARMS a wakeup is the agent's action, classified
/// `agent.tool.use`, not the fired tick). See the GOLD-gap note in the module docs.
pub const SCHEDULE_WAKEUP_MARKER: &str = "<<autonomous-loop-dynamic>>";

/// The header of the harness-injected FIRED autonomous-loop / `ScheduleWakeup` timer tick (P1c
/// M2a / oracle D12) - `harness.schedule.wakeup`. When the timer FIRES, the harness injects an
/// `isMeta` `type:"user"` record whose content opens `# Autonomous loop check\n\nYou're being
/// invoked on a timer …`. DISTINCT from the `meta.loop` DRIVER ticks
/// ([`AUTONOMOUS_LOOP_TICK_PREFIX`] = `# Autonomous loop tick` / [`AUTONOMOUS_CHECK_MARKER`]):
/// `check` ≠ `tick`, so the two prefixes never collide. The wakeup arm is matched BEFORE the
/// meta.loop arm in [`Record::classify`], so the fired tick routes to `schedule.wakeup`.
pub const SCHEDULE_WAKEUP_LOOP_CHECK_PREFIX: &str = "# Autonomous loop check";

/// See [`SCHEDULE_WAKEUP_LOOP_CHECK_PREFIX`] - the fired-timer body sentence (matched anywhere,
/// as it follows the `# Autonomous loop check` header after a blank line). Verified verbatim
/// against real `~/.claude/projects` data (straight ASCII apostrophe).
pub const SCHEDULE_WAKEUP_TIMER_MARKER: &str = "You're being invoked on a timer";

/// `harness.meta.hook` markers (GOLD §2, edge-fixtures G2) - hook-injected feedback, NOT the
/// operator: a stop-hook feedback message, a `<local-command-caveat>` wrapper, or the
/// edit-failed-retry notice CC injects when an Edit's target changed under it. (These are
/// `isMeta` user records that would otherwise fall through to `user.message`.)
pub const STOP_HOOK_FEEDBACK_PREFIX: &str = "Stop hook feedback:";
/// See [`STOP_HOOK_FEEDBACK_PREFIX`] - the `<local-command-caveat>…` hook wrapper.
pub const LOCAL_COMMAND_CAVEAT_PREFIX: &str = "<local-command-caveat>";
/// See [`STOP_HOOK_FEEDBACK_PREFIX`] - the edit-failed-retry notice (matched anywhere, as it
/// also rides inside a `Stop hook feedback:` body).
pub const EDIT_RETRY_MARKER: &str = "The last Edit failed because the target file was modified";

/// `harness.meta.loop` markers (GOLD §2, edge-fixtures G2) - autonomous-loop drivers (distinct
/// from the [`SCHEDULE_WAKEUP_MARKER`] sentinel, which stays `harness.schedule.wakeup`).
pub const AUTONOMOUS_LOOP_TICK_PREFIX: &str = "# Autonomous loop tick";
/// See [`AUTONOMOUS_LOOP_TICK_PREFIX`] - matched anywhere (it can sit mid-prompt).
pub const AUTONOMOUS_CHECK_MARKER: &str = "Run the autonomous check";

/// An `isMeta` `[Image: source:…]` pseudo-record (GOLD §2, edge-fixtures G2) - EXCLUDED from
/// the taxonomy entirely (classify yields no label), so it is never mislabeled `user.message`.
pub const IMAGE_SOURCE_PREFIX: &str = "[Image: source:";
