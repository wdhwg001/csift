//! Record-text resolution: class views, raw text, persisted output, excerpts.
//!
//! A PAIRED ROOT: the shared resolution lives here and the per-family renders live in
//! the children, so a new family is a new file rather than a widening of this one.

mod promoted;
mod seam;

pub(crate) use promoted::*;
pub(crate) use seam::*;

// The six FAMILY modules the one stamping site ([`crate::commands::search::family_facts`])
// dispatches to, in that fixed order. Each owns its family's renders, markers and facts;
// they are declared (not glob-re-exported) because every one of them answers the same
// entry name `facts`, and six globs carrying one name would make it ambiguous at this
// root.
pub(crate) mod attachments;
pub(crate) mod bookkeeping;
pub(crate) mod injections;
pub(crate) mod keys;
pub(crate) mod system;
pub(crate) mod thinking;

/// A seventh dispatch arm that exists only under `cfg(test)` (see the module doc there):
/// a stub returning empty vectors cannot demonstrate that the site is reached, so the
/// probe answers on a fixture sentinel and the unit tests read it back from both
/// hit-construction sites. Never compiled into the binary.
#[cfg(test)]
pub(crate) mod probe;

use super::*;

/// True for the four `agent.communication.*` leaves (they render a `from ⇨ to` direction).
pub(crate) fn is_comm_class(c: Class) -> bool {
    matches!(
        c,
        Class::CommInbox | Class::CommSent | Class::CommSignal | Class::CommChannel
    )
}

/// Render the transcript owner's own id as the literal `self` on either side of a comm direction
/// (`self ⇨ to` outbound, `from ⇨ self` inbound) - a verbose session uuid / bare agent hex on
/// the self side becomes `self`, while a peer id/name on the OTHER side is kept verbatim (a peer
/// never equals the owner). No-op when `owner_id` is `None`.
pub(crate) fn alias_self(
    dir: Option<(String, String)>,
    owner_id: Option<&str>,
) -> Option<(String, String)> {
    let Some(owner) = owner_id else {
        return dir;
    };
    dir.map(|(from, to)| {
        let sub = |s: String| if s == owner { "self".to_string() } else { s };
        (sub(from), sub(to))
    })
}

/// True for a RECORD-LEVEL text class - one classified from a record's string / text-block
/// content (NOT a per-block agent class, and NOT the tool_result duals `user.answer`/
/// `user.rejection`, which are handled in the ToolResult arm). Drives [`record_text_emission`].
pub(crate) fn is_record_text_class(c: Class) -> bool {
    matches!(
        c,
        Class::UserMessage
            | Class::UserUnsent
            | Class::UserRewound
            | Class::CommInbox
            | Class::CommChannel
            | Class::NotificationWorkflow
            | Class::NotificationMonitor
            | Class::NotificationSubagent
            | Class::NotificationBackgroundCommand
            | Class::NotificationTask
            | Class::CompactionSummary
            | Class::CompactionBoundary
            | Class::CommandInvocation
            | Class::CommandStdout
            | Class::InterruptUser
            | Class::InterruptTool
            | Class::ScheduleWakeup
            | Class::ScheduleFire
            | Class::ResumePrompt
            // The resume PLACEHOLDER is an assistant record classified at the RECORD level
            // (the block loop only emits agent classes), so it renders through this path -
            // verbatim, since `record_raw_text` joins its one text block unchanged.
            | Class::ResumePlaceholder
            | Class::HookContext
            | Class::HookOutcome
            | Class::HookFeedback
            | Class::CommandCaveat
            | Class::ScheduleTick
            | Class::ScheduleEvent
            | Class::AgentMessageStructured
            | Class::ContextReminder
            | Class::ContextListing
            | Class::ContextPrompt
            | Class::ContextFile
            | Class::ContextMode
            | Class::ContextIde
            | Class::ContextTask
            | Class::ContextImage
            | Class::ContextSkill
            | Class::EnforceStructuredOutput
            | Class::EnforceBrief
            | Class::EnforceHandback
            | Class::EnforceRetry
            | Class::UserQueued
            | Class::TurnDuration
            | Class::TurnRecap
            | Class::TurnStopHooks
            | Class::SystemRefusal
            | Class::SystemApiError
            | Class::SystemNotice
            | Class::SystemKill
            | Class::SystemCommand
            | Class::BookkeepingLastPrompt
            | Class::BookkeepingMode
            | Class::BookkeepingName
            | Class::BookkeepingAtis
            | Class::BookkeepingBridge
            | Class::BookkeepingSuppression
            | Class::BookkeepingPrLink
            | Class::BookkeepingArtifact
            | Class::BookkeepingFileHistory
            | Class::BookkeepingCost
            | Class::BookkeepingFork
            | Class::BookkeepingContinued
            | Class::BookkeepingQueue
    )
}

/// The richest SELECTED record-level text class + its display text. Iterates the
/// record's labels in `classify`'s richest-first order, taking the first record-text class that
/// is selected, then resolving its text source: a `<task-notification>` -> `automation_label`; a
/// genuine/AUQ/rejection/teammate-prose/subagent-opener -> `reconstructed_user_text`; any other
/// harness marker / compaction summary -> the raw string. `None` => no record-text class is
/// selected (or it has no text).
///
/// `agent.communication.signal` is deliberately NOT in the class list, and the reason is a
/// measurement rather than a judgement. This function runs only when a record produced no
/// SECTIONS, and a record carrying that leaf always produces one: a peer control payload is
/// a peer section, and a `SendMessage` control call is a tool BLOCK the block loop emits.
/// Instrumenting the arm to return a sentinel and scanning one projects root found 397
/// records under `-t agent.communication.signal` and 0 reaching it. Listing an
/// unreachable class here is a promise about a path nothing takes.
pub(crate) fn record_text_emission(
    rec: &Record,
    raw: Option<&str>,
    labels: &[Class],
    filter: LabelFilter<'_>,
    plan_index: &PlanIndex,
    ctx: &ClassifyCtx,
) -> Option<(Class, String)> {
    for &c in labels {
        if !is_record_text_class(c) || !filter.selected(c.path()) {
            continue;
        }
        let text = match c {
            Class::NotificationWorkflow
            | Class::NotificationMonitor
            | Class::NotificationSubagent
            | Class::NotificationBackgroundCommand
            | Class::NotificationTask => rec.automation_label(),
            Class::UserMessage | Class::UserUnsent | Class::UserRewound | Class::CommInbox => rec
                .reconstructed_user_text(Some(plan_index))
                // TWO carriers reach this arm with no opener body to render, and the
                // fallback is the RENDER SEAM for both - the one place a line's own family
                // decides what it says.
                //
                // The person's mid-turn STEER rides an attachment (`queued_command` whose
                // payload `origin.kind` is `human`), and an attachment carries no
                // `message{}` for the opener render to read - so without a fallback the
                // record classified `user.message` and then emitted nothing at all: no
                // hit, no census key, no `-t user` result, reachable only by address. The
                // COORDINATOR relay is the same defect on a second carrier: an isMeta
                // record carrying `agent.communication.inbox` opens no turn, so the opener
                // render answers None, and it is no attachment either. 141 such records
                // over 89 files of one projects root reached a census as zero.
                //
                // The seam answers BOTH, and it answers the steer BETTER than the payload
                // text did: the attachment family's body rule prefers the conversion the
                // harness persisted - what the model actually received - over the payload
                // JSON it replaced, which is the same body every other view of that line
                // renders. Calling `attachment_payload_text` first here bypassed the seam
                // and pinned this ONE view to the payload, so a reader who found the steer
                // under `-t user` read different bytes from a reader who addressed the
                // same line. `record_raw_text` still falls through to the verbatim payload
                // for a record no family answers for, so nothing that had a body loses one.
                .or_else(|| record_raw_text(rec, raw, Some(c), ctx)),
            // A csift-channel delivery renders the envelope chunk VERBATIM (header line +
            // body), never a rewrite: the header is the receipt a reader audits, and a
            // verbatim byte substring keeps the 7d/7f prefilter laws sound with no synth
            // marker. The `harness.hook.context` view of the SAME record takes the attachment
            // family's ordinary body rule instead, which is the conversion the harness
            // persisted when the record carries one and the joined `content` when it does
            // not - so the two views of a channel delivery can show different text, and
            // each says which.
            //
            // The CHANNEL view is the one that must not move: a reader audits the envelope
            // header, and a conversion is the harness's own rewrite of it.
            Class::CommChannel => rec.csift_channel_text(),
            _ => record_raw_text(rec, raw, Some(c), ctx),
        };
        if let Some(text) = text {
            return Some((c, text));
        }
    }
    None
}

/// The raw textual body of a record for harness-marker matching: the bare string, or the text
/// blocks joined with `\n` (mirrors the engine's `raw_message_text`). For a MESSAGE-LESS record (a
/// `type:"system"` record - e.g. the `compact_boundary` metrics record) it falls back to the
/// top-level `content` plus a readable `compactMetadata` excerpt, so the boundary is BOTH matchable
/// and rendered. `None` when there is no text anywhere.
/// `class` is the leaf the CALLER is emitting under, when it holds one; it only stands in
/// for a line whose own type names none (an isMeta injection). `None` is for the caller
/// that genuinely has no view - the section 7f gate's re-render - and makes the seam read
/// the line alone, which is all it ever reads for the non-transcript shapes.
pub(crate) fn record_raw_text(
    rec: &Record,
    raw: Option<&str>,
    class: Option<Class>,
    ctx: &ClassifyCtx,
) -> Option<String> {
    // THE RENDER SEAM, first: the family that owns this line's leaf answers, and a `Some`
    // wins. The LINE's own leaf leads and the caller's class is only the fallback, so a
    // line renders the same text under every view of it.
    if let Some(text) = render_leaf(rec)
        .or(class)
        .and_then(|c| family_render(rec, raw, c, ctx))
    {
        return Some(text);
    }
    // An attachment payload csift models NO leaf for: the seam had no leaf to dispatch
    // on, so no family was offered it, and the VERBATIM payload JSON is the engine's own
    // honest body for a shape csift has not caught up with. (Every MODELED payload
    // renders through `attachments::render` above, which produces these same bytes for
    // everything but a hook's injected context.) Raw by design, and a byte substring of
    // the source line, so the literal prefilter and the whole-file gate hold with no
    // synthesized markers.
    if let Some(text) = rec.attachment_payload_text() {
        return Some(text);
    }
    // Promoted non-message lines: verbatim content where the line has one
    // (queued text, away recap), a fabricated key=value excerpt otherwise (turn
    // duration, stop hooks) - each fabricated form is registered as a synth marker
    // (`synth_marker_finders`) so the whole-file gate stays sound.
    if let Some(text) = promoted_record_text(rec) {
        return Some(text);
    }
    let Some(msg) = rec.message.as_ref() else {
        // No `message` blocks -> a system record: the boundary's content + compactMetadata.
        return system_record_text(rec);
    };
    match msg.content.as_ref()? {
        Content::Text(s) => Some(s.clone()),
        Content::Blocks(blocks) => {
            let parts: Vec<&str> = blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
    }
}

/// The MATCHABLE-TEXT LAW: the body of a unit whose content lives in fields `Record`
/// does not model is the line's OWN VERBATIM JSON.
///
/// A key=value head is a summary, and a summary is lossy by construction: it shortens a
/// uuid to eight characters, prints three of a line's twelve keys, and formats a number.
/// Rendering only the head makes the elided material UNSEARCHABLE - `search` matches
/// rendered text, so a full uuid absent from every rendered string cannot be found at
/// all. The verbatim line is also free of every section 7 hazard for the same
/// reason `--attachments` renders a payload verbatim: it is a byte substring of the
/// source line, so the 7d literal prefilter and the 7f whole-file gate hold with no
/// synthesized-marker registration.
///
/// So a bookkeeping unit renders HEAD then BODY ([`head_and_body`]): the summary a reader
/// scans, and the bytes a pattern searches.
///
/// ONE family is deliberately outside the law, and it is not an oversight. A
/// `system`/`api_error` line carries the whole HTTP response header set inside its
/// `error` object, and csift does not print those on any path but `--raw`; the system
/// family therefore renders the FIELDS it names rather than the line, and `--raw` stays
/// the way to the bytes there. Any family whose lines carry no such material uses this.
///
/// `None` when the caller has no source line - every conversation record, where the
/// question does not arise because `message` holds the text.
pub(crate) fn verbatim_body(raw: Option<&str>) -> Option<String> {
    raw.map(str::to_string)
}

/// Join a family's key=value HEAD to the verbatim BODY of the same line, newline between
/// them so a `--no-truncate` body keeps the two apart (the excerpt collapses whitespace
/// anyway). Either half alone is the whole text when the other is absent; `None` only
/// when there is neither, which is the honest answer for a line with no render and no
/// source bytes in hand.
pub(crate) fn head_and_body(head: Option<String>, body: Option<String>) -> Option<String> {
    match (head, body) {
        (Some(h), Some(b)) => Some(format!("{h}\n{b}")),
        (Some(h), None) => Some(h),
        (None, body) => body,
    }
}

/// The communication [`Class`] a `tool_use` block carries: a `SendMessage` ->
/// `...sent`/`...signal`; a `Task`/`Agent`/`Workflow` spawn -> `...sent`. `None` for any other tool.
/// REPLICATES the engine's per-record decision per-BLOCK (so a record with mixed comm/non-comm
/// tool_use blocks labels each correctly) - kept faithful to model.rs `classify_assistant`.
pub(crate) fn tool_use_comm_class(
    name: Option<&str>,
    input: Option<&serde_json::Value>,
) -> Option<Class> {
    match name? {
        "SendMessage" => Some(if send_message_is_signal(input) {
            Class::CommSignal
        } else {
            Class::CommSent
        }),
        n if is_spawn_tool_name(n) => Some(Class::CommSent),
        _ => None,
    }
}

/// Replica of the engine's spawn-tool set (model.rs `is_spawn_tool_name`).
pub(crate) fn is_spawn_tool_name(name: &str) -> bool {
    matches!(name, "Task" | "Agent" | "Workflow")
}

/// Replica of the engine's `send_message_is_signal` (model.rs): a `SendMessage` whose top-level
/// (or nested `message`) `type` is present and is NOT `message`/`direct` is a control SIGNAL.
pub(crate) fn send_message_is_signal(input: Option<&serde_json::Value>) -> bool {
    let Some(input) = input else {
        return false;
    };
    let type_at = |v: &serde_json::Value| {
        v.get("type")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    if let Some(t) = type_at(input) {
        return !matches!(t.as_str(), "message" | "direct");
    }
    if let Some(t) = input.get("message").and_then(type_at) {
        return !matches!(t.as_str(), "message" | "direct");
    }
    false
}

/// Render a `tool_use` block to searchable text: `name {json-input}`. The name is
/// matched first so `csift search AskUserQuestion -t agent.tool.use` works; the input JSON is
/// included so a regex can match arguments too.
pub(crate) fn render_tool_use(name: Option<&str>, input: Option<&serde_json::Value>) -> String {
    let mut s = String::new();
    if let Some(n) = name {
        s.push_str(n);
    }
    if let Some(v) = input {
        s.push(' ');
        s.push_str(&v.to_string());
    }
    s
}

/// Resolve a `<persisted-output>` pointer (section 4.6) to the referenced file's content.
/// On a read failure the inline text is kept and an explicit note appended - a
/// missing persisted file is reported, never fatal (SPEC section 4.6).
pub(crate) fn resolve_persisted_text(path: &str, inline: &str) -> String {
    match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => format!("{inline}\n[csift: could not resolve persisted output {path}: {e}]"),
    }
}

/// The synthesized AUQ-answer string from a carrier's `tool_result` (section 4.4). Matches
/// any known AUQ-answer marker (every shipped phrasing, see `model::AUQ_ANSWER_MARKERS`).
/// Test-only: production surfaces the AUQ answer via the model's reconstructed unit
/// ([`Record::reconstructed_user_text`] -> [`Record::auq_exchange`]), which prefers the
/// clean structured `toolUseResult.answers`; this helper backs the legacy-shape tests.
#[cfg(test)]
pub(crate) fn auq_answer_text(rec: &Record) -> Option<String> {
    let blocks = rec.blocks()?;
    for b in blocks {
        if let Block::ToolResult {
            content: Some(c), ..
        } = b
        {
            let t = tool_result_content_text(c);
            if crate::model::is_auq_answer_text(&t) {
                return Some(t);
            }
        }
    }
    None
}

/// Truncate to [`EXCERPT_MAX`] chars with the shared explicit `… (+N chars)` marker.
/// Test-only: production excerpting goes through [`match_excerpt`], which carries the
/// caller's (possibly `--no-truncate`) budget; this fixed-budget wrapper backs the unit tests.
#[cfg(test)]
pub(crate) fn truncate_excerpt(s: &str) -> String {
    crate::text::truncate_excerpt(s, EXCERPT_MAX)
}

/// The [`Hit::body`] companion of [`match_excerpt`]: the SAME rendered section text,
/// verbatim, when the caller's budget is the uncapped `usize::MAX` that `--no-truncate`
/// and a `show --line`/`--uuid` address install - else `None`.
///
/// Keyed on the budget rather than on a flag so the two fields can never disagree: an
/// excerpt built under a real cap is a match-centered fragment, and pairing it with a
/// full second copy of every matched record would multiply the default stream for a
/// consumer that did not ask for the body. `match_excerpt` normalizes whitespace (the
/// excerpt is one line by contract); this does not, so a body keeps every newline its
/// renderer produced.
pub(crate) fn uncapped_body(text: &str, max: usize) -> Option<String> {
    (max == usize::MAX).then(|| text.to_string())
}

/// Build the inline excerpt, CENTERED on the match so a hit DEEP in a long message is
/// actually visible - not just the message head: a head-only excerpt silently hides any
/// match past the first `max` chars and sends a reader back to the raw jsonl.
///
/// `span` is the first match's BYTE range, or `None` for the pure filter (no specific
/// match -> show the head). When the message fits in `max` chars it is shown whole.
/// Otherwise a `max`-char window is taken around the match (a quarter of the budget as
/// leading context), whitespace-normalized, with a leading `…` when content precedes
/// the window and the shared `… (+N chars)` marker when content follows - so clipping
/// on either side is explicit, never silent (SPEC section 0).
///
/// Returns `(excerpt, truncated)` - `truncated` is true iff content was CLIPPED to fit `max`
/// (the head form when the normalized text exceeds `max`, or any match-centered window). Under
/// `--no-truncate`'s `usize::MAX` budget nothing is ever clipped, so `truncated` is always false there.
pub(crate) fn match_excerpt(
    text: &str,
    span: Option<(usize, usize)>,
    max: usize,
) -> (String, bool) {
    let total = text.chars().count();
    // Pure filter, or the whole message already fits (incl. `--no-truncate`'s `usize::MAX`): keep
    // the head-anchored form, capped at `max` (uncapped under `--no-truncate`). Truncated iff the
    // normalized body still overruns `max`.
    let head_form = |text: &str| -> (String, bool) {
        let norm = normalize_line(text);
        let truncated = norm.chars().count() > max;
        (crate::text::truncate_excerpt(&norm, max), truncated)
    };
    let start_byte = match span {
        Some((s, _)) if total > max => s,
        _ => return head_form(text),
    };
    // Char index of the match start; a non-char-boundary byte offset (possible with a
    // raw-byte regex) falls back to the head rather than panicking.
    let Some(prefix) = text.get(..start_byte) else {
        return head_form(text);
    };
    let match_char = prefix.chars().count();
    let win_start = match_char.saturating_sub(max / 4);
    let window: String = text.chars().skip(win_start).take(max).collect();
    let body = normalize_line(&window);
    let after = total.saturating_sub(win_start + max);
    let mut out = String::new();
    if win_start > 0 {
        out.push('…');
    }
    out.push_str(&body);
    if after > 0 {
        out.push_str(&format!("… (+{after} chars)"));
    }
    // The window form is only reached when `total > max`, so a `max`-char window necessarily
    // dropped surrounding content - this is always a truncated fragment.
    (out, true)
}

/// Parse a `--turn` token into a [`RangeSpec`] (the shared grammar: `N`/`A..B`/`N..`/
/// `..N`/`-k`), resolved per-file against that transcript's turn count (0-based).
pub(crate) fn parse_turn_range(s: &str) -> Result<crate::text::RangeSpec> {
    crate::text::parse_range_spec(s, "--turn", false)
}

// -- Rendering --
//
// Timestamp formatting (system-local + raw UTC) lives in `crate::timez`, shared
// with `list` so the local-timezone choice is defined once.
