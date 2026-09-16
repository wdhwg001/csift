//! The ATTACHMENT family: the nine `harness.context` leaves, `harness.hook.{context,
//! outcome}`, `agent.message.structured` and the person's mid-turn `queued_command`.
//!
//! An attachment line is the harness attaching something to the turn, and the harness
//! keeps TWO copies of it: the payload it stored, and - on a build that writes the field -
//! the text its own converter produced for the model, persisted back as a TOP-LEVEL
//! `rendered` (a sibling of `attachment`, never a key inside it). The two are not the same
//! text, so a unit says WHICH it is showing: [`BODY_RENDERED`] or [`BODY_PAYLOAD`].
//!
//! A `queued_command` can carry a THIRD copy, `renderedInHumanTurn`. csift does not choose
//! between it and `rendered`, because the record does not say which one the model got: the
//! harness reader picks on its CALLER's own `inHumanTurn` option, which describes the turn
//! being assembled and is no field of the record. So the body stays the ordinary
//! conversion and the unit REPORTS the second copy ([`Fact::HumanTurnRender`]) instead of
//! picking one.
//!
//! WHAT KEEPS THE SECTION 7 PREFILTERS SOUND over a converted body is the NEEDLE law, not
//! a substring. The persisted conversion is a JSON STRING, so the DECODED text as a whole
//! is NOT a byte substring of the line that stores it - every measured conversion opens
//! `<system-reminder>` and runs on past a newline, which the raw bytes carry as `\n`. What
//! IS guaranteed is what the prefilter actually extracts: `matcher::required_needles`
//! derives a needle only from a run with no whitespace and no JSON-escaped character (the
//! R13 predicate), and such a run appears VERBATIM inside the raw line's JSON string - so a
//! needle present in the converted text is present in the raw bytes, and the 7d line
//! prefilter and the 7f whole-file gate keep their meaning with no synthesized-marker
//! registration. It is the same reading every decoded `message.content` render at this seam
//! already rests on. The label-zone markers are fabricated and therefore display-only,
//! which is exactly what [`Marker`] is for.

use super::*;

/// The body came from the harness's OWN converted text: what the model received, and what
/// a resume replays.
pub(crate) const BODY_RENDERED: &str = "rendered";

/// The body came from the attachment PAYLOAD on disk: either a field of it (a hook run's
/// stdout, a hook's injected strings) or the verbatim payload JSON.
pub(crate) const BODY_PAYLOAD: &str = "payload";

/// What an ATTACHMENT line says - the family's half of the render seam.
///
/// Called by [`super::family_render`] for every unit whose leaf sits in one of this
/// family's categories, BEFORE the engine's generic arms; `None` falls through to them.
///
/// THE GATE IS THE RECORD'S OWN LINE TYPE, and it is not redundant with the dispatch. The
/// seam reads `render_leaf(rec).or(class)`, so a record with NO leaf of its own - every
/// isMeta injection, every `<task-notification>` - arrives here carrying only the CALLER's
/// class, and a render that answered unconditionally would hand back a stranger's record.
/// `harness.hook` and `harness.context` are exactly such shared ground: a `.feedback`
/// injection and an isMeta `<system-reminder>` both arrive under this family's categories
/// and are the injections family's records. [`attachment_body`] carries that gate.
pub(crate) fn render(
    rec: &Record,
    _raw: Option<&str>,
    _class: Class,
    _ctx: &ClassifyCtx,
) -> Option<String> {
    attachment_body(rec).map(|(text, _)| text)
}

/// The body an attachment record renders, and the word that says where it came from.
///
/// THREE sources, in this order, and the order is the point.
///
/// 1. The converter's own output (`rendered`): the text the model received, and - because
///    the assembler reads the persisted value back rather than re-converting - the text a
///    resume replays. It leads because a reader asking what an attachment said means this.
/// 2. The per-leaf body a payload carries in a field of its own: a hook RUN's `stdout`, a
///    hook's injected `content` strings.
/// 3. The verbatim payload JSON, which is what every attachment rendered before.
///
/// `None` for every non-attachment record, so no other line shape moves.
#[must_use]
pub(crate) fn attachment_body(rec: &Record) -> Option<(String, &'static str)> {
    if !rec.is_type("attachment") {
        return None;
    }
    if let Some(text) = rec.converted_text().and_then(|c| c.rendered.clone()) {
        return Some((text, BODY_RENDERED));
    }
    if let Some(text) = payload_field_body(rec) {
        return Some((text, BODY_PAYLOAD));
    }
    rec.attachment_payload_text().map(|t| (t, BODY_PAYLOAD))
}

/// The body a payload carries in a field of its own, for the two types whose payload JSON
/// would otherwise bury the one thing a reader wants. `None` for every other type, which
/// falls through to the verbatim payload JSON.
fn payload_field_body(rec: &Record) -> Option<String> {
    match rec.attachment_type()?.as_str() {
        // A hook RUN's record: the run's own stdout is its text. An empty stdout is not a
        // body, so the unit falls through to the payload JSON and still renders the exit
        // code and the duration in its label zone.
        "hook_success" => {
            let out = payload_str(rec, "stdout")?.trim();
            (!out.is_empty()).then(|| out.to_string())
        }
        // The context a hook injected: the strings it wrote, joined. This is the body
        // csift has always rendered for the leaf; it stays the fallback for a build below
        // the `rendered` floor, where no converted copy exists to prefer.
        "hook_additional_context" => rec.hook_additional_context_text(),
        _ => None,
    }
}

/// The ATTACHMENT family's own facts.
///
/// An EDGE to another record is never a variant here - [`super::for_edge`] builds those, so
/// every family's provenance edge resolves and renders alike. What IS here is what only
/// this family knows: which of a record's two copies of itself the body shows.
#[derive(Debug, Clone)]
pub(crate) enum Fact {
    /// Which of an attachment record's two copies of itself the unit's BODY came from:
    /// the harness converter's own output ([`BODY_RENDERED`]) or the payload on disk
    /// ([`BODY_PAYLOAD`]). Display-only in the label zone, `body_source` in JSON.
    Body(&'static str),
    /// The record carries a SECOND conversion for a human turn (`renderedInHumanTurn`)
    /// whose text differs from the body's. Which copy the model received depends on the
    /// turn the record was replayed into, which the record does not say - so the unit
    /// reports the second copy rather than choosing.
    HumanTurnRender,
}

impl Fact {
    /// The label-zone tag this fact adds, if any.
    pub(crate) fn label_zone(&self) -> Option<String> {
        match self {
            Fact::Body(source) => Some(format!("[{source}]")),
            Fact::HumanTurnRender => Some("[human-turn render differs]".to_string()),
        }
    }

    /// The JSON key(s) and value(s) this fact adds to its hit object.
    pub(crate) fn json(&self) -> Vec<(&'static str, serde_json::Value)> {
        match self {
            Fact::Body(source) => vec![("body_source", serde_json::json!(source))],
            Fact::HumanTurnRender => {
                vec![("human_turn_render_differs", serde_json::json!(true))]
            }
        }
    }
}

/// The ATTACHMENT family's per-unit facts and markers.
///
/// Called for EVERY emitted unit through [`crate::commands::search::family_facts`], so the
/// first thing it does is refuse a unit that is not its own - one type compare, which is
/// what every record of every other family pays here.
///
/// The body fact and the leaf marker ride only a unit whose text this family produced: a
/// csift-channel delivery renders its own envelope verbatim, so stamping
/// `[rendered]`/`[payload]` on it would describe a body it is not showing.
pub(crate) fn facts(
    rec: &Record,
    _raw: Option<&str>,
    section: &Section<'_>,
    _ctx: &ClassifyCtx,
) -> (Vec<HitFact>, Vec<Marker>) {
    if !rec.is_type("attachment") {
        return (Vec::new(), Vec::new());
    }
    let mut facts = Vec::new();
    let mut markers = Vec::new();
    if section.class == Some(Class::CommChannel) {
        return (facts, markers);
    }
    if let Some(source) = body_source(rec, section.class) {
        facts.push(HitFact::Attachments(Fact::Body(source)));
        // The second conversion is reported only when it DIFFERS from the body, because an
        // identical copy tells a reader nothing - and only when the body IS the ordinary
        // conversion, which is what the difference is a difference from.
        let conv = rec.converted_text();
        let human = conv.and_then(|c| c.human_turn.as_deref());
        if source == BODY_RENDERED
            && human.is_some()
            && human != conv.and_then(|c| c.rendered.as_deref())
        {
            facts.push(HitFact::Attachments(Fact::HumanTurnRender));
        }
    }
    if let Some(m) = section.class.and_then(|c| leaf_marker(rec, c)) {
        markers.push(m);
    }
    (facts, markers)
}

/// WHERE THE TEXT THIS UNIT SHOWS CAME FROM - which is not always what [`attachment_body`]
/// would produce, because one view of an attachment record is rendered by the shared
/// opener path rather than by this family's render.
///
/// The person's mid-turn steer is that view. `record_text_emission` resolves a
/// `user.message` unit as the reconstructed opener text, then the VERBATIM payload, and
/// only then the seam this family answers at - so a steer whose record also carries a
/// persisted conversion shows the payload JSON and a `[rendered]` fact on it would name a
/// body the unit is not showing. The measured size of that gap, over one projects root
/// counted one parse per non-blank line: 20 of the 164 `queued_command` lines whose
/// payload `origin.kind` is `human` carry a `rendered`, and 0 of them carry the
/// human-turn conversion at all.
///
/// `None` for a record with no body of its own, which is every non-attachment line.
fn body_source(rec: &Record, class: Option<Class>) -> Option<&'static str> {
    let (_, source) = attachment_body(rec)?;
    Some(match class {
        Some(Class::UserMessage) => BODY_PAYLOAD,
        _ => source,
    })
}

/// What the unit IS, named for the label zone. Keyed on the unit's own leaf, because one
/// attachment line can carry two views; `None` for a leaf this family does not own and for
/// the unmodeled unit, which has no leaf to name.
#[must_use]
fn leaf_marker(rec: &Record, class: Class) -> Option<Marker> {
    let ty = rec.attachment_type();
    match class {
        // The context a hook injected, named by the event it fired on and the hook that
        // wrote it - two fields that sit OUTSIDE the searchable body, so without this a
        // reader cannot tell one injection from another.
        Class::HookContext => Some(Marker {
            head: "hook",
            detail: Some(hook_identity(rec)),
        }),
        // A hook RUN's own record. `hook_success` carries the whole outcome (the event, the
        // hook, its exit code and how long it took); the other six payload types are
        // outcomes with no such fields, so the marker names the type and stops there.
        Class::HookOutcome => Some(Marker {
            head: "hook",
            detail: Some(match ty.as_deref() {
                Some("hook_success") => hook_outcome_detail(rec),
                other => other.unwrap_or("outcome").to_string(),
            }),
        }),
        // The nine context leaves fan several payload types into one leaf each, so the unit
        // names the payload type it actually came from. An isMeta injection classified
        // under one of these leaves is another family's record and never reaches here.
        c if c.category() == "harness.context" => ty.map(|t| Marker {
            head: "context",
            detail: Some(t),
        }),
        // The model's own schema-bound answer.
        Class::AgentMessageStructured => Some(Marker::plain("structured")),
        // The person's own mid-turn steer: an ordinary `user.message` that reached the
        // model in the MIDDLE of a turn rather than opening one.
        Class::UserMessage => Some(Marker::plain("mid-turn")),
        _ => None,
    }
}

/// `<hookEvent> <hookName>`, with the honest word in either slot the record leaves empty.
fn hook_identity(rec: &Record) -> String {
    let field = |key: &str| -> String { payload_str(rec, key).unwrap_or("unknown").to_string() };
    format!("{} {}", field("hookEvent"), field("hookName"))
}

/// `<hookEvent> <hookName> exit <code> <ms>ms` for a hook RUN, dropping either tail the
/// record does not carry rather than printing a zero it never wrote.
fn hook_outcome_detail(rec: &Record) -> String {
    let payload = rec.attachment_value();
    let num = |key: &str| -> Option<i64> { payload.and_then(|p| p.get(key)?.as_i64()) };
    let mut out = hook_identity(rec);
    if let Some(code) = num("exitCode") {
        out.push_str(&format!(" exit {code}"));
    }
    if let Some(ms) = num("durationMs") {
        out.push_str(&format!(" {ms}ms"));
    }
    out
}

/// One string field of the attachment PAYLOAD, through the memoised parse.
///
/// The payload is where every key this family reads lives, and the TOP level of the line is
/// not a fallback for any of them: `toolUseID` occurs on no line at the top level at all,
/// so reading the payload IS the rule.
fn payload_str<'a>(rec: &'a Record, key: &str) -> Option<&'a str> {
    rec.attachment_value()?.get(key)?.as_str()
}

#[cfg(test)]
mod tests;
