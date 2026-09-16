//! The SYSTEM family: the five `harness.system.*` event kinds and the scheduler's own
//! `harness.schedule.event` record.
//!
//! These records carry no `message{}`, and the fields that make each one USEFUL - which
//! model refused, which one served the retry, how many attempts the request had left,
//! which task fired - live in top-level keys `Record` does not model. So the render reads
//! them off the SOURCE LINE: the struct deserializes what csift uses across every
//! surface, and these keys are used on one.
//!
//! TWO laws sit on this file.
//!
//! **`error.headers` never reaches a rendered or JSON output path.** An `api_error`
//! record's `error` object can carry the response headers verbatim, and those headers
//! carry a session cookie, an organization id and a CDN trace id. This family is the ONE
//! that stays OUTSIDE the shared matchable-text law ([`super::verbatim_body`]) for exactly
//! that reason: it renders the FIELDS it names - status, the retry counters, the error's
//! own type and message - and never the line. `--raw` stays the one documented way to the
//! bytes, and it says so.
//!
//! **Every fabricated word here is re-renderable from the line alone.** The heads
//! (`[refusal ...]`, `[api-error ...]`, `[notice ...]`, `[kill]`, `[command]`,
//! `[scheduled ...]`) and the `key=` words are not byte substrings of the source line, so
//! the section-7f whole-file gate would prune a file whose only hit lies in one of them.
//! The key-only `"subtype"` synth marker (`matcher::synth::synth_marker_finders`, active
//! under exactly the predicate that admits these lines) marks such a line for the stage-2
//! re-render, which is handed the SAME source line this render reads - so the gate sees
//! what the emission would emit.
//!
//! ONE PARSE PER ENTRY. [`payload`] runs once in [`render`] and once in [`facts`], and
//! the map is handed down to the heads and the edges rather than re-read. The two entries
//! are reached from different engine sites and neither can see the other's work, so two
//! is the floor a family can hit without a cache on `Record` itself; both are bounded to
//! GATED and ADDRESSED lines, so a default scan pays neither.
//!
//! WHAT THE SECTION-7F GATE PAYS FOR THIS, stated because filling a render is what makes
//! the gate pay anything at all. The gate's stage-2 re-render is reached only for a line
//! carrying a synth MARKER, and this family's marker is the key-only `"subtype"` needle,
//! pushed only under `scans_system_lines` - so a flagless scan, a `-t user` and a
//! `-t agent` never reach a single one of these renders. When a `-t harness` or a system
//! leaf IS selected, such a line has a leaf of its OWN ([`super::render_leaf`] reads
//! `promoted_class`), so the gate renders it ONCE rather than walking
//! [`super::rendering_classes`]; the added cost against the head this replaced is one
//! `serde_json::from_str` of that line. Bounded, but not zero: see the deviation in this
//! lane's report - the acid rows the amendment asks a render-filling family to re-measure
//! are the integration harness's, not this lane's.

use super::*;

mod edges;
mod fallback;
mod heads;

/// What a `type:"system"` event line says - the family's half of the render seam.
///
/// Called by [`super::family_render`] for every unit whose leaf sits in `harness.system`
/// or `harness.schedule`, BEFORE the engine's generic arms; `None` falls through to them,
/// which is also how a `harness.schedule` leaf that is NOT the scheduler's own record
/// reaches the injection family behind this one.
///
/// The gate is the RECORD's own leaf ([`Record::promoted_class`]), never the class the
/// caller handed in: a record with no leaf of its own arrives carrying nothing but the
/// caller's view, and a render that answered on that would hand back a stranger's record.
/// Every other system subtype - `compact_boundary` above all - has its leaf in another
/// category, so this answers `None` there and the record keeps the render it always had.
pub(crate) fn render(
    rec: &Record,
    raw: Option<&str>,
    _class: Class,
    _ctx: &ClassifyCtx,
) -> Option<String> {
    let class = rec.promoted_class()?;
    heads::system_event_text(rec, class, payload(raw).as_ref())
}

/// The system family's own facts: the exposed keys of one system EVENT record.
///
/// ONE VARIANT PER EVENT KIND rather than one per key, because the keys of a kind are
/// read off one line together and weighed together - `retry_attempt` without
/// `max_retries` is half a counter. Each variant projects only the keys its line actually
/// carried, so an absent key is absent rather than a fabricated null.
///
/// None of them renders a label-zone tag: every value here is already IN the rendered
/// head or body (`originalModel=`, `status=`, `cron=`, `prompt=`), and a tag would print
/// the same field twice on the one line a reader scans.
///
/// An EDGE to another record is never a variant here. Edges are SHARED on [`HitFact`]
/// itself, because several families carry them and a reader has to read every one the
/// same way; a family builds one through [`super::for_edge`] instead of describing the
/// shape again.
#[derive(Debug, Clone)]
pub(crate) enum Fact {
    /// `harness.system.refusal`: which model was asked, which served the retry (absent on
    /// the no-fallback subtype), the harness's refusal category, and whether a second
    /// model was tried at all - the last read off the SUBTYPE rather than off a key,
    /// which is why it rides even when the caller holds no source line.
    Refusal {
        /// True for `model_refusal_fallback`, false for `model_refusal_no_fallback`.
        retried: bool,
        /// `originalModel` - the model the refused request was sent to.
        original_model: Option<String>,
        /// `fallbackModel` - the model that served the retry.
        fallback_model: Option<String>,
        /// `apiRefusalCategory` - the harness's own word for why.
        category: Option<String>,
    },
    /// `harness.system.api-error`: the HTTP status, the three retry counters, and the
    /// error's own `type` and `message`. NEVER `error.headers` - see the module doc.
    ApiError {
        /// `error.status` - the status is inside the error object, not beside it.
        status: Option<serde_json::Value>,
        /// `retryAttempt`.
        retry_attempt: Option<serde_json::Value>,
        /// `maxRetries`.
        max_retries: Option<serde_json::Value>,
        /// `retryInMs` - a jittered float, carried as the line's own number.
        retry_in_ms: Option<serde_json::Value>,
        /// The error object reduced to `type` and `message`.
        error: Option<serde_json::Value>,
    },
    /// `harness.schedule.event`, the CRON arm: the scheduler's own task id, the schedule
    /// that armed it, and the armed prompt cut to the shared preview budget. The `/loop`
    /// wakeup arm carries none of the three.
    ScheduleArm {
        /// `taskId` - the scheduler's id, which is also the `armed_by` join key.
        task_id: Option<String>,
        /// `cron` - the schedule expression.
        cron: Option<String>,
        /// `prompt`, cut at [`PROMPT_MAX`].
        prompt: Option<String>,
    },
    /// The two model ids of an assistant turn the API served from a FALLBACK model. The
    /// odd one out here: its record is an assistant message rather than a `type:"system"`
    /// line, and its record carries no head of its own to put the ids in - so the ids
    /// reach the label zone through a [`Marker`] beside this fact rather than through
    /// [`Fact::label_zone`] (see [`fallback`]).
    Fallback {
        /// `to.model` - the model that answered.
        served_by: String,
        /// `from.model` - the model the turn was asked of.
        fell_back_from: String,
    },
}

impl Fact {
    /// The label-zone tag this fact adds: NONE, for every variant. The three event
    /// variants carry values that are already in the rendered head, so a tag would print
    /// the same field twice on the one line a reader scans; the fallback turn has no head
    /// of its own, and its ids reach the label zone through the [`Marker`] pushed beside
    /// it - the other extension point, and the one meant for a word rather than a value.
    pub(crate) fn label_zone(&self) -> Option<String> {
        None
    }

    /// The JSON key(s) and value(s) this fact adds to its hit object - only the keys the
    /// line carried, so a consumer reading a key knows the record held it.
    pub(crate) fn json(&self) -> Vec<(&'static str, serde_json::Value)> {
        let mut out: Vec<(&'static str, serde_json::Value)> = Vec::new();
        let mut put = |key: &'static str, v: Option<serde_json::Value>| {
            if let Some(v) = v {
                out.push((key, v));
            }
        };
        match self {
            Fact::Refusal {
                retried,
                original_model,
                fallback_model,
                category,
            } => {
                put("retried", Some(serde_json::json!(retried)));
                put("original_model", original_model.clone().map(Into::into));
                put("fallback_model", fallback_model.clone().map(Into::into));
                put("category", category.clone().map(Into::into));
            }
            Fact::ApiError {
                status,
                retry_attempt,
                max_retries,
                retry_in_ms,
                error,
            } => {
                put("status", status.clone());
                put("retry_attempt", retry_attempt.clone());
                put("max_retries", max_retries.clone());
                put("retry_in_ms", retry_in_ms.clone());
                put("error", error.clone());
            }
            Fact::ScheduleArm {
                task_id,
                cron,
                prompt,
            } => {
                put("task_id", task_id.clone().map(Into::into));
                put("cron", cron.clone().map(Into::into));
                put("prompt", prompt.clone().map(Into::into));
            }
            Fact::Fallback {
                served_by,
                fell_back_from,
            } => {
                put("served_by", Some(serde_json::json!(served_by)));
                put("fell_back_from", Some(serde_json::json!(fell_back_from)));
            }
        }
        out
    }
}

/// The system family's per-unit facts and edges - the one entry
/// [`crate::commands::search::family_facts`] dispatches to.
///
/// TWO record shapes reach it and they are not related: a `type:"system"` EVENT record,
/// whose exposed keys and provenance edge come off the source line, and an ASSISTANT
/// record carrying a `fallback` block, whose two model ids exist nowhere else. Every other
/// record leaves on two `type` compares, which is what keeps the stamping site free for
/// the conversation records that make up almost every scanned unit.
pub(crate) fn facts(
    rec: &Record,
    raw: Option<&str>,
    _section: &Section<'_>,
    ctx: &ClassifyCtx,
) -> (Vec<HitFact>, Vec<Marker>) {
    // A turn the API served from a fallback model. Per RECORD rather than per block: the
    // fact is about the whole turn, so a record that fell back AND spoke carries it on
    // each of its text units. The ids come off the parsed block, so this answers the same
    // on a scan (no source line) as under an address (one) - see the module doc there.
    if rec.is_type("assistant") {
        return fallback::notes(rec);
    }
    if !rec.is_type("system") {
        return (Vec::new(), Vec::new());
    }
    let Some(class) = rec.promoted_class() else {
        return (Vec::new(), Vec::new());
    };
    let p = payload(raw);
    let mut out = keys(class, rec, p.as_ref());
    out.extend(edges::edge(class, rec, p.as_ref(), ctx));
    (out, Vec::new())
}

/// The exposed KEYS one system EVENT record carries, as its family [`Fact`]. Empty for
/// every leaf outside the three that carry keys - the notice and the kill render their
/// whole payload as text and have nothing left to expose.
///
/// A caller holding no source line still gets the refusal fact, because `retried` is read
/// off the SUBTYPE, which is on the record; every other key is absent rather than null.
fn keys(
    class: Class,
    rec: &Record,
    p: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Vec<HitFact> {
    let s = |key: &str| p.and_then(|m| str_field(m, key));
    let v = |key: &str| p.and_then(|m| m.get(key).cloned());
    let fact = match class {
        Class::SystemRefusal => Fact::Refusal {
            retried: rec.subtype.as_deref() == Some("model_refusal_fallback"),
            original_model: s("originalModel"),
            fallback_model: s("fallbackModel"),
            category: s("apiRefusalCategory"),
        },
        Class::SystemApiError => {
            let error = p.and_then(|m| m.get("error"));
            Fact::ApiError {
                status: error.and_then(|e| e.get("status")).cloned(),
                retry_attempt: v("retryAttempt"),
                max_retries: v("maxRetries"),
                retry_in_ms: v("retryInMs"),
                error: heads::error_summary(error),
            }
        }
        Class::ScheduleEvent => Fact::ScheduleArm {
            task_id: s("taskId"),
            cron: s("cron"),
            // The armed text is the operator's whole prompt, so it is CUT to the
            // scannable-preview budget like every other long value on a hit - and the
            // SAME cut is rendered into the head, so the key and the matchable text carry
            // one value rather than two. The uncut text is a `--raw` away.
            prompt: s("prompt").map(|s| crate::text::truncate_excerpt(&s, PROMPT_MAX)),
        },
        _ => return Vec::new(),
    };
    vec![HitFact::System(fact)]
}

/// The armed prompt's render budget, on the hit AND in the head - the shared
/// scannable-preview cap the `list` and `agents` previews use, for the same reason: it is
/// operator prose of unbounded length riding a row a reader scans.
pub(crate) const PROMPT_MAX: usize = 200;

/// The record's own source line as a top-level JSON object. `None` when the caller holds
/// no line (every conversation record) or the line is not an object.
fn payload(raw: Option<&str>) -> Option<serde_json::Map<String, serde_json::Value>> {
    match serde_json::from_str(raw?) {
        Ok(serde_json::Value::Object(m)) => Some(m),
        _ => None,
    }
}

/// A top-level key read as a non-empty string; `None` for any other shape, so a rendered
/// `key=` never carries a JSON fragment.
fn str_field(m: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    let s = m.get(key)?.as_str()?;
    (!s.is_empty()).then(|| s.to_string())
}

/// A JSON scalar as the text the line itself holds - an integer as digits, a float with
/// its fraction, anything else as its compact JSON. Never rounded: a rendered retry delay
/// is a value a reader may compare against the line.
fn num_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests;
