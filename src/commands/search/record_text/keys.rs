//! The EXPOSED KEYS family: facts Claude Code stamps on a record about the call behind
//! it - which agent definition, skill, plugin and MCP tool it ran under, which API
//! request and block it came from, what a person typed refusing a tool, why a call
//! failed. They belong to no single leaf, which is why they are a family of their own.
//!
//! This family has NO render: its facts are keys a record EXPOSES, never what a line
//! says, so the render seam does not offer it a record (`seam::render_families`) and
//! there is nothing here to answer with.
//!
//! PERF (SPEC section 7): the keys reach this family as [`Record::exposed_keys`] - the
//! nineteen of them in ONE [`crate::model::Exposed`] behind ONE `Box`, read by the same
//! serde pass that builds the record (`#[serde(flatten)]`) and allocated only for a record
//! that names at least one. That shape answers both halves of the RECORD-WIDTH law this
//! family is built around: the row every parsed line of every scan carries grows by a
//! pointer rather than by one `Option` per key (`size_of::<Record>()` 1368 -> 1376, and 1384 at
//! this tip with the API's stop-details object behind its own pointer; pinned in
//! `parse::tests::spine`), and nothing walks the line twice. The walk was the alternative,
//! and it was built and measured rather than argued about: a depth-1 key read at the one
//! place a `Record` is built from bytes cost 48 to 79 percent of user CPU on three section
//! 7 acid rows, being a second whole-line pass. A read at EMISSION time instead cannot
//! reach the bytes at all - the engine keeps a record's source line only for a line whose
//! `Record` cannot answer for it, which is the opposite of the population these keys ride.
//!
//! ONE fact reads a key `Record` does not model - the PDF page range, off
//! `toolUseResult.firstPage` and the result's own content elements - and it is the one
//! thing this family pays for. Per EMITTED UNIT it runs `Record::tur_probe`, which returns
//! on its first `?` for a record carrying no `toolUseResult` at all (every assistant turn,
//! every user message) but otherwise DESERIALIZES THE WHOLE RESULT BLOB to read six small
//! fields - so on a tool-result unit it is a typed walk of that blob, skipping the unlisted
//! values without allocating them rather than skipping the bytes. Only once that walk has
//! read `parts` does the full `tool_use_result_value()` parse follow, which is why the type
//! check and not the `firstPage` read is the short circuit. MEASURED: no difference on the
//! section 7 landmark, and +9% on a synthetic worst case of 64 KB of result blob per
//! emitted tool result - so the cost is real in principle and unmeasurable at the blob
//! sizes a transcript carries. Emission is bounded by matches, so it rides hits, never a
//! scan.

use super::*;
use crate::model::{ApiFailure, Attribution};

/// How much of a refusal's typed text the LABEL ZONE carries. The whole text rides JSON;
/// the label zone is one line beside a leaf path, and a person refusing a tool can type
/// a paragraph.
const FEEDBACK_MAX: usize = 60;

/// Why a Stop-hook summary that names no hook run has an edge at all: the leaf OWES one.
const NO_HOOK_RUN_REASON: &str = "no toolUseID on this stop_hook_summary record";

/// What an assistant record says about the API's own decision to stop it: the four
/// members of `message.stop_details`, plus the `stop_reason` beside them.
///
/// Held as owned values, like every other fact here, so the projections below are total.
#[derive(Debug, Clone)]
pub(crate) struct StopSummary {
    /// `stop_reason`, the coarse word - `refusal` where the details are written.
    pub(crate) reason: Option<String>,
    /// `stop_details.type`, the stop KIND.
    pub(crate) kind: Option<String>,
    /// `stop_details.category`, the classifier band.
    pub(crate) category: Option<String>,
    /// `stop_details.explanation`, the API's own sentence.
    pub(crate) explanation: Option<String>,
    /// `stop_details.fallback_has_prefill_claim`.
    pub(crate) fallback_has_prefill_claim: Option<bool>,
}

/// The exposed-keys family's own facts. One variant per key (or per key GROUP, where the
/// harness writes a set that only means anything together).
///
/// Each carries the value rather than the record, so the projections below are total and
/// the JSON shape is decided in one place per key.
#[derive(Debug, Clone)]
pub(crate) enum Fact {
    /// The five `attribution*` keys as one object. Boxed: it is five `Option<String>`s
    /// and the enum is carried by value on every hit that has any fact at all.
    Attribution(Box<Attribution>),
    /// `requestId` - the API request this record's turn came back on.
    RequestId(String),
    /// `apiBlockIndex` - which content block of that response this record is.
    ApiBlockIndex(u32),
    /// `toolEndsTurn` - the assistant's turn stopped at this tool's result.
    ToolEndsTurn,
    /// `userFeedback` - the words a person typed refusing a tool call, structured.
    UserFeedback(String),
    /// The API-failure set of a synthetic assistant record. Boxed for the same reason as
    /// the attribution.
    ApiFailure(Box<ApiFailure>),
    /// `interruptedMessageId` - the API message an interruption cut short.
    InterruptedMessage(String),
    /// `scheduledTaskId` - the scheduled task whose prompt this is.
    ScheduledTaskId(String),
    /// `scheduledFireId` - that task's individual firing.
    ScheduledFireId(String),
    /// The HOOK RUN a `stop_hook_summary` reports on.
    HookRunId(String),
    /// The API's own account of why it stopped this response, from `message.stop_details`
    /// and the `stop_reason` beside it. Boxed for the same reason as the two above.
    ///
    /// The tag it renders is LABEL-ZONE text and is therefore display-only, like the
    /// thinking family's: a pattern scan matches the rendered SECTION and never the zone,
    /// so searching for the word a refusal is tagged with finds nothing. The selector and
    /// the JSON key are the way to these records.
    StopDetails(Box<StopSummary>),
    /// The PDF page RANGE a `Read` with a `pages` parameter returned, read off the pair of
    /// keys that carry it: `toolUseResult.firstPage` for the offset and the result's own
    /// content elements for how many pages followed it. `path` is `file.filePath`.
    ///
    /// The tag it renders is DISPLAY-ONLY and needs no synthesized-text marker: the label
    /// zone is never matched, so no prefilter and no whole-file gate reasons about it.
    PdfPages {
        /// The document page of the first element after the leading text.
        first: usize,
        /// The document page of the last one.
        last: usize,
        /// How many pages the result carries.
        count: usize,
        /// The document, verbatim; `None` when the result named no path.
        path: Option<String>,
    },
}

impl Fact {
    /// The label-zone tag this fact adds, if any.
    ///
    /// Six of the twelve render a tag and six ride JSON alone, and the split is not
    /// arbitrary: a tag is worth a reader's line only when it says something about THIS
    /// record that a person scanning a result list would act on. An id is a join key, and
    /// `attributionAgent` is on every record of a subagent transcript, so a tag naming it
    /// would be the same word on every line.
    pub(crate) fn label_zone(&self) -> Option<String> {
        match self {
            // Only the SPECIFIC half: a skill, the plugin that owns it, or the MCP pair.
            Fact::Attribution(a) => a
                .specific()
                .map(|(kind, name)| format!("[via {kind} {name}]")),
            Fact::ToolEndsTurn => Some("[ends turn]".to_string()),
            // A label-zone tag is ONE line: a refusal typed over several lines is flattened
            // before it is cut, the rule every excerpt takes, and JSON keeps the words whole.
            Fact::UserFeedback(text) => Some(format!(
                "[feedback: {}]",
                crate::text::collapse_and_truncate(text, FEEDBACK_MAX)
            )),
            // `headline` is `None` exactly when the failure says nothing, and the model
            // layer never builds one that does - so this can render no floor word.
            Fact::ApiFailure(f) => f.headline().map(|h| format!("[api failure {h}]")),
            // The KIND, and the category when the record names one. The EXPLANATION is
            // the API's own prose about a response it withheld, which is the one member
            // a reader of a result list is not owed on the same line as a leaf path; it
            // rides JSON. A record naming no kind renders no tag: the zone would carry a
            // floor word where the record simply did not say.
            Fact::StopDetails(s) => s.kind.as_deref().map(|k| match s.category.as_deref() {
                Some(c) => format!("[api {k}: {c}]"),
                None => format!("[api {k}]"),
            }),
            Fact::RequestId(_)
            | Fact::ApiBlockIndex(_)
            | Fact::InterruptedMessage(_)
            | Fact::ScheduledTaskId(_)
            | Fact::ScheduledFireId(_)
            | Fact::HookRunId(_) => None,
            // The BASENAME, never the whole path: this tag sits beside a leaf path on one
            // line, and the JSON key carries the path verbatim for a reader who needs it.
            Fact::PdfPages {
                first, last, path, ..
            } => Some(
                match path.as_deref().map(crate::commands::image::basename) {
                    Some(name) => format!("[pdf pages {first}-{last} of {name}]"),
                    None => format!("[pdf pages {first}-{last}]"),
                },
            ),
        }
    }

    /// The JSON key(s) and value(s) this fact adds to its hit object.
    ///
    /// An object-valued fact names EVERY member, with null for one the record lacks, so a
    /// consumer reads presence from the KEY and never from an interior: `attribution`
    /// present means the record named at least one of the five, and a null `skill` inside
    /// it means this record did not name a skill.
    pub(crate) fn json(&self) -> Vec<(&'static str, serde_json::Value)> {
        match self {
            Fact::Attribution(a) => vec![(
                "attribution",
                serde_json::json!({
                    "agent": a.agent,
                    "skill": a.skill,
                    "plugin": a.plugin,
                    "mcp_server": a.mcp_server,
                    "mcp_tool": a.mcp_tool,
                }),
            )],
            Fact::RequestId(id) => vec![("request_id", serde_json::json!(id))],
            Fact::ApiBlockIndex(n) => vec![("api_block_index", serde_json::json!(n))],
            Fact::ToolEndsTurn => vec![("tool_ends_turn", serde_json::json!(true))],
            Fact::UserFeedback(text) => vec![("user_feedback", serde_json::json!(text))],
            Fact::ApiFailure(f) => vec![(
                "api_failure",
                serde_json::json!({
                    "error": f.error,
                    "status": f.status,
                    "quota": f.quota.as_ref().map(|q| serde_json::json!({
                        "limit_type": q.limit_type,
                        "resets_at": q.resets_at,
                    })),
                    "truncated_after_output": f.truncated_after_output,
                    "aborted_mid_stream": f.aborted_mid_stream,
                }),
            )],
            Fact::InterruptedMessage(id) => {
                vec![("interrupted_message_id", serde_json::json!(id))]
            }
            Fact::ScheduledTaskId(id) => vec![("scheduled_task_id", serde_json::json!(id))],
            Fact::ScheduledFireId(id) => vec![("scheduled_fire_id", serde_json::json!(id))],
            Fact::HookRunId(id) => vec![("hook_run_id", serde_json::json!(id))],
            // TWO keys: the coarse reason the API gave and its structured account of the
            // same stop. The object names every member with null for one the record
            // lacks, so a consumer reads presence from the KEY - the absence-at-key-level
            // rule this whole family follows.
            Fact::StopDetails(s) => vec![
                ("stop_reason", serde_json::json!(s.reason)),
                (
                    "stop_details",
                    serde_json::json!({
                        "type": s.kind,
                        "category": s.category,
                        "explanation": s.explanation,
                        "fallback_has_prefill_claim": s.fallback_has_prefill_claim,
                    }),
                ),
            ],
            Fact::PdfPages {
                first,
                last,
                count,
                path,
            } => vec![(
                "pdf_pages",
                serde_json::json!({
                    "first": first,
                    "last": last,
                    "count": count,
                    "path": path,
                }),
            )],
        }
    }
}

/// The exposed-keys family's per-unit facts and markers.
///
/// Called for EVERY emitted unit through [`crate::commands::search::family_facts`].
/// Unlike the other five this family is keyed on FIELDS rather than on a leaf, so the
/// body reads the record's own keys and returns nothing when it carries none - which is
/// the common case, and the reason an absent key means "this record does not say" rather
/// than "no".
///
/// The gate is the KEY, never the leaf: a key is a fact about the record whatever csift
/// labelled it, and keying the gate on a label would make the answer depend on which
/// selector the reader passed. The two exceptions are the ones the HARNESS itself scopes -
/// the API-failure set is read only through `isApiErrorMessage`, the flag its one factory
/// sets, and `toolUseID` is read only on the `stop_hook_summary` subtype, where it is a
/// hook-run id rather than the API tool_use id its name suggests.
///
/// The facts are RECORD-level, so the unit's own block is not consulted; `section` is read
/// for the ONE edge a LEAF owes rather than a key.
///
/// The family produces no [`Marker`]: every tag it renders belongs to a value, so it
/// rides that value's fact and the two surfaces stay one decision.
pub(crate) fn facts(
    rec: &Record,
    _raw: Option<&str>,
    section: &Section<'_>,
    ctx: &ClassifyCtx,
) -> (Vec<HitFact>, Vec<Marker>) {
    let mut out: Vec<HitFact> = Vec::new();
    let mut push = |f: Fact| out.push(HitFact::Keys(f));
    if let Some(a) = rec.attribution() {
        push(Fact::Attribution(Box::new(a)));
    }
    if let Some(f) = rec.api_failure() {
        push(Fact::ApiFailure(Box::new(f)));
    }
    // The API's own account of the stop, on EVERY section of the record that carries it -
    // one assistant message is several records and each repeats the envelope, so the fact
    // is the record's rather than one block's. The gate is the object, so a turn that
    // ended normally carries nothing here and the absent key means the record does not
    // say rather than that the API did not refuse.
    if let Some(s) = rec.stop_details() {
        push(Fact::StopDetails(Box::new(StopSummary {
            reason: rec.message.as_ref().and_then(|m| m.stop_reason.clone()),
            kind: s.r#type.as_deref().map(str::to_string),
            category: s.category.as_deref().map(str::to_string),
            explanation: s.explanation.as_deref().map(str::to_string),
            fallback_has_prefill_claim: s.fallback_has_prefill_claim,
        })));
    }
    if let Some(k) = rec.exposed_keys() {
        if k.tool_ends_turn == Some(true) {
            push(Fact::ToolEndsTurn);
        }
        if let Some(t) = k.user_feedback.as_deref() {
            push(Fact::UserFeedback(t.to_string()));
        }
        if let Some(id) = k.request_id.as_deref() {
            push(Fact::RequestId(id.to_string()));
        }
        if let Some(n) = k.api_block_index {
            push(Fact::ApiBlockIndex(n));
        }
        if let Some(id) = k.interrupted_message_id.as_deref() {
            push(Fact::InterruptedMessage(id.to_string()));
        }
        if let Some(id) = k.scheduled_task_id.as_deref() {
            push(Fact::ScheduledTaskId(id.to_string()));
        }
        if let Some(id) = k.scheduled_fire_id.as_deref() {
            push(Fact::ScheduledFireId(id.to_string()));
        }
    }
    if let Some(id) = rec.hook_run_id() {
        push(Fact::HookRunId(id.to_string()));
    }
    // The PDF page range, read through the ONE reader `image` derives its per-page number
    // from, so the two surfaces cannot drift: the gate, the offset and the count are that
    // function's, and this adds only the range. It is keyed on the result object like
    // every other fact here, never on the leaf.
    if let Some(p) = crate::commands::image::record_pdf_pages(rec) {
        push(Fact::PdfPages {
            first: p.first,
            last: p.last(),
            count: p.count,
            path: p.path,
        });
    }
    out.extend(for_edges(rec, section, ctx));
    (out, Vec::new())
}

/// The `for` EDGES this unit carries - at most one, and the order below is the precedence.
///
/// One per unit is the rule rather than an accident of which keys happen to be there: the
/// edge answers "which record is this FOR", and a unit with two answers has none. The two
/// this family owns sit on different line TYPES (a user record and a `system` record), so
/// a record carrying both keys is not a shape Claude Code writes.
///
/// Both ids are real and neither is a record uuid, so on an ordinary transcript both
/// resolve through [`record_text::for_edge`]'s middle answer - the id printed with `not in
/// this file`. That is measured rather than assumed: over two projects roots, 0 of 272 and
/// 0 of 418 `interruptedMessageId` values and 0 of 6,849 and 0 of 6,308 hook-run ids equal
/// a `uuid` of their own file. The resolving arm is still the one that runs, because
/// nothing but the uuid map decides it and a family may not build a second index.
fn for_edges(rec: &Record, section: &Section<'_>, ctx: &ClassifyCtx) -> Option<HitFact> {
    if let Some(id) = rec
        .exposed_keys()
        .and_then(|k| k.interrupted_message_id.as_deref())
    {
        return Some(for_edge(
            EdgeKind::Interrupted,
            Basis::Key,
            Some(id),
            None,
            ctx,
        ));
    }
    if let Some(id) = rec.hook_run_id() {
        return Some(for_edge(EdgeKind::HookRun, Basis::Key, Some(id), None, ctx));
    }
    // The edge is owed by the LEAF here, not by a key: every Stop-hook summary measured
    // names its run, so one that does not is a shape worth saying out loud rather than a
    // hit that quietly carries one fewer field.
    if section.class == Some(Class::TurnStopHooks) {
        return Some(HitFact::ForUnknown {
            kind: EdgeKind::HookRun,
            reason: NO_HOOK_RUN_REASON.to_string(),
        });
    }
    None
}
