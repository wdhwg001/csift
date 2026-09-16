//! The EXPOSED KEYS: what a record says about the CALL behind it - which agent
//! definition, skill, plugin or MCP tool the request ran under, which API request and
//! which block of its response, why the call failed, what a person typed refusing a
//! tool, which scheduled task submitted the prompt, which hook run closed the turn.
//!
//! They belong to no single leaf, which is why they are a family of their own rather
//! than fields of one classification.
//!
//! WIDTH IS THE DESIGN CONSTRAINT, and the shape below is what satisfies it and the
//! one-pass rule at once. Every parsed line of every scan carries a whole `Record`, so
//! eighteen `Option`s in that struct are eighteen fields on every row - the C-41 lesson,
//! which narrowed a spine row to 72 bytes for exactly that reason. They live in ONE
//! [`Exposed`] behind ONE `Box` instead, so the row grows by a pointer and the allocation
//! is paid only by a record that actually names a key; `#[serde(flatten)]` is what keeps
//! it to the SAME pass serde already makes, reading the keys where they sit at the top
//! level. Inside `Exposed` the width is still paid down where that is free, because a
//! record carrying one key carries the whole struct: every string is `Box<str>` rather
//! than `String` (8 bytes apiece, the trade `parse::SpineRow` makes), the two numbers are
//! `u32`, and `quotaLimits` is deserialized straight into the two-member summary that is
//! all csift reads of it rather than kept as a payload.
//!
//! The alternative - lifting the keys from the source line on demand - was built and
//! measured rather than argued about, and it is why this is a serde shape at all. A
//! depth-1 key walk at the one place a `Record` is built from bytes, behind an
//! Aho-Corasick gate over the key names, cost 48 to 79 percent of user CPU on three
//! section 7 acid rows over a 415 MiB transcript, because it is a second whole-line pass
//! on top of the one serde already makes. A lazier read at EMISSION time cannot reach the
//! bytes at all: the engine keeps a record's source line only for a line whose `Record`
//! cannot answer for it, which is the opposite of the population these keys ride.

use super::lenient;
use super::*;

/// The eighteen EXPOSED KEYS of one record, as one value behind one pointer.
///
/// Built only for a record that names at least one of them ([`exposed_keys`]), so the
/// common record - which names none - costs the 8 bytes of a null pointer and no
/// allocation at all.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Exposed {
    /// `attributionAgent` - the agent DEFINITION the request ran under. A sidechain fact:
    /// measured over one projects root, 413,510 of 413,510 carriers have `isSidechain`
    /// true, which is why csift reports it as data and renders no marker for it.
    #[serde(
        default,
        rename = "attributionAgent",
        deserialize_with = "lenient::string_only"
    )]
    pub attribution_agent: Option<Box<str>>,

    /// `attributionSkill` - the skill that was active for the call.
    #[serde(
        default,
        rename = "attributionSkill",
        deserialize_with = "lenient::string_only"
    )]
    pub attribution_skill: Option<Box<str>>,

    /// `attributionPlugin` - the plugin that owns that skill, derived from it by the
    /// composer rather than supplied beside it.
    #[serde(
        default,
        rename = "attributionPlugin",
        deserialize_with = "lenient::string_only"
    )]
    pub attribution_plugin: Option<Box<str>>,

    /// `attributionMcpServer` - the MCP server behind the tool. Written only for a main
    /// or subagent query source, together with the tool below.
    #[serde(
        default,
        rename = "attributionMcpServer",
        deserialize_with = "lenient::string_only"
    )]
    pub attribution_mcp_server: Option<Box<str>>,

    /// `attributionMcpTool` - the MCP tool itself.
    #[serde(
        default,
        rename = "attributionMcpTool",
        deserialize_with = "lenient::string_only"
    )]
    pub attribution_mcp_tool: Option<Box<str>>,

    /// `requestId` - the API request this record's turn came back on. Every record built
    /// from one response carries the same value, so it is the join key for "which records
    /// are one API message". Rides assistant records and the two model-refusal `system`
    /// subtypes; the synthetic failure placeholder carries it explicitly undefined.
    #[serde(
        default,
        rename = "requestId",
        deserialize_with = "lenient::string_only"
    )]
    pub request_id: Option<Box<str>>,

    /// `apiBlockIndex` - WHICH content block of that response this record is. Has a
    /// version floor: absent from every record of a build below 2.1.257.
    #[serde(
        default,
        rename = "apiBlockIndex",
        deserialize_with = "lenient::u32_only"
    )]
    pub api_block_index: Option<u32>,

    /// `toolEndsTurn` - the assistant's turn stopped at this tool's result. Written on
    /// the `type:"user"` record that carries the result, and measured for exactly one
    /// tool (`StructuredOutput`) on two projects roots.
    #[serde(
        default,
        rename = "toolEndsTurn",
        deserialize_with = "lenient::bool_only"
    )]
    pub tool_ends_turn: Option<bool>,

    /// `userFeedback` - the words a person typed when refusing a tool call. A TOP-LEVEL
    /// key of the synthesised refusal record, not a member of its `toolUseResult`, which
    /// is a plain string on every measured carrier.
    #[serde(
        default,
        rename = "userFeedback",
        deserialize_with = "lenient::string_only"
    )]
    pub user_feedback: Option<Box<str>>,

    /// `error` - the failure KIND of a synthetic API-failure record, STRING form only.
    /// The same key is an OBJECT on a `type:"system"` `api_error` record and that object
    /// carries the failed request's whole HTTP response header set, so the decoder keeps
    /// the string and answers `None` for anything else: the sensitive-headers law of
    /// AGENTS section 4, enforced where the value enters rather than where it renders.
    #[serde(default, deserialize_with = "lenient::string_only")]
    pub error: Option<Box<str>>,

    /// `apiErrorStatus` - the HTTP status of the failed call.
    #[serde(
        default,
        rename = "apiErrorStatus",
        deserialize_with = "lenient::u32_only"
    )]
    pub api_error_status: Option<u32>,

    /// `quotaLimits` - a 429's quota object, deserialized straight into the two members
    /// csift reads of it (which limit, and when it reopens) rather than carried whole:
    /// the measured object is ten keys, and the other eight are account bookkeeping.
    #[serde(
        default,
        rename = "quotaLimits",
        deserialize_with = "lenient::quota_only"
    )]
    pub quota_limits: Option<Box<QuotaSummary>>,

    /// `truncatedAfterOutput` - the stream died after output had begun, so the assistant
    /// text before this placeholder is partial.
    #[serde(
        default,
        rename = "truncatedAfterOutput",
        deserialize_with = "lenient::bool_only"
    )]
    pub truncated_after_output: Option<bool>,

    /// `isAbortedMidStream` - a partial record kept after an abort.
    #[serde(
        default,
        rename = "isAbortedMidStream",
        deserialize_with = "lenient::bool_only"
    )]
    pub is_aborted_mid_stream: Option<bool>,

    /// `interruptedMessageId` - the API message an interruption cut short. An API
    /// `message.id`, never a record `uuid`: measured, 0 of 272 and 0 of 418 values equal
    /// a `uuid` of their own file.
    #[serde(
        default,
        rename = "interruptedMessageId",
        deserialize_with = "lenient::string_only"
    )]
    pub interrupted_message_id: Option<Box<str>>,

    /// `scheduledTaskId` - the scheduled task whose prompt this record is. Equal to the
    /// `taskId` of the scheduler's own `scheduled_task_fire` record in the same file.
    #[serde(
        default,
        rename = "scheduledTaskId",
        deserialize_with = "lenient::string_only"
    )]
    pub scheduled_task_id: Option<Box<str>>,

    /// `scheduledFireId` - that task's individual firing, written together with the task
    /// id and never alone.
    #[serde(
        default,
        rename = "scheduledFireId",
        deserialize_with = "lenient::string_only"
    )]
    pub scheduled_fire_id: Option<Box<str>>,

    /// `toolUseID` - on a `stop_hook_summary` record this is the HOOK RUN id, not an API
    /// tool_use id, which is why [`Record::hook_run_id`] reads it on that subtype alone.
    #[serde(
        default,
        rename = "toolUseID",
        deserialize_with = "lenient::string_only"
    )]
    pub tool_use_id_key: Option<Box<str>>,
}

impl Exposed {
    /// Does this record name any exposed key at all? The gate on the allocation: a
    /// transcript is mostly records that name none, and an all-`None` box would be a heap
    /// object per line saying nothing.
    fn says_something(&self) -> bool {
        self.attribution_agent.is_some()
            || self.attribution_skill.is_some()
            || self.attribution_plugin.is_some()
            || self.attribution_mcp_server.is_some()
            || self.attribution_mcp_tool.is_some()
            || self.request_id.is_some()
            || self.api_block_index.is_some()
            || self.tool_ends_turn.is_some()
            || self.user_feedback.is_some()
            || self.error.is_some()
            || self.api_error_status.is_some()
            || self.quota_limits.is_some()
            || self.truncated_after_output.is_some()
            || self.is_aborted_mid_stream.is_some()
            || self.interrupted_message_id.is_some()
            || self.scheduled_task_id.is_some()
            || self.scheduled_fire_id.is_some()
            || self.tool_use_id_key.is_some()
    }
}

/// The `#[serde(flatten)]` hook on [`Record::exposed`]: read the eighteen keys out of the
/// record's own top level, and BOX the result only when at least one of them is there.
///
/// `Exposed` is built on the stack either way - that is what reading eighteen keys in one
/// pass costs - and only a record that says something pays for a heap object that outlives
/// the call.
pub(super) fn exposed_keys<'de, D>(de: D) -> std::result::Result<Option<Box<Exposed>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let keys = Exposed::deserialize(de)?;
    Ok(keys.says_something().then(|| Box::new(keys)))
}

/// The five `attribution*` keys of one record, as one value.
///
/// `agent` is measured on the SIDECHAIN population only - 413,510 of 413,510 carriers on
/// one projects root have `isSidechain` true - which is why it is data rather than a
/// rendered marker: on a subagent transcript every record would carry the same word.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attribution {
    /// `attributionAgent` - the agent DEFINITION the request ran under.
    pub agent: Option<String>,
    /// `attributionSkill` - the skill that was active.
    pub skill: Option<String>,
    /// `attributionPlugin` - the plugin that owns the skill.
    pub plugin: Option<String>,
    /// `attributionMcpServer` - the MCP server behind the tool.
    pub mcp_server: Option<String>,
    /// `attributionMcpTool` - the MCP tool itself.
    pub mcp_tool: Option<String>,
}

impl Attribution {
    /// The most SPECIFIC non-agent attribution, as `(kind, name)` - a skill outranks the
    /// plugin that owns it, and both outrank the MCP pair. `None` when the record names
    /// only an agent (or nothing), which is what keeps the marker off the per-record
    /// stream of a subagent transcript where every line names the same agent.
    #[must_use]
    pub fn specific(&self) -> Option<(&'static str, &str)> {
        if let Some(s) = self.skill.as_deref() {
            return Some(("skill", s));
        }
        if let Some(p) = self.plugin.as_deref() {
            return Some(("plugin", p));
        }
        // The SERVER names the endpoint the tool lives behind, so it is the stable half
        // of the pair; a record carrying only the tool still names that.
        self.mcp_server
            .as_deref()
            .or(self.mcp_tool.as_deref())
            .map(|m| ("mcp", m))
    }
}

/// The two members of a 429 `quotaLimits` object that answer "which limit, and when does
/// it reopen". The rest of that object is account-level bookkeeping (the measured key sets
/// are ten keys and seven), and it is dropped at deserialization rather than carried.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct QuotaSummary {
    /// `rateLimitType` - which window was hit.
    #[serde(rename = "rateLimitType", deserialize_with = "lenient::string_only")]
    pub limit_type: Option<Box<str>>,
    /// `resetsAt` - epoch SECONDS at which that window reopens.
    #[serde(rename = "resetsAt", deserialize_with = "lenient::u64_only")]
    pub resets_at: Option<u64>,
}

impl QuotaSummary {
    /// Does this summary name either member? An empty one would widen the wire while
    /// answering nothing, so [`Record::api_failure`] drops it.
    fn says_something(&self) -> bool {
        self.limit_type.is_some() || self.resets_at.is_some()
    }
}

/// What a SYNTHETIC API-FAILURE assistant record says about the call it replaced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApiFailure {
    /// The failure kind: `rate_limit`, `server_error`, `authentication_failed`, ...
    pub error: Option<String>,
    /// `apiErrorStatus` - the HTTP status of the failed call.
    pub status: Option<u32>,
    /// The two members a 429's `quotaLimits` is summarised to.
    pub quota: Option<QuotaSummary>,
    /// The stream died after output had begun: the preceding assistant text is partial.
    pub truncated_after_output: bool,
    /// A partial record kept after an abort.
    pub aborted_mid_stream: bool,
}

impl ApiFailure {
    /// The label-zone parts, in render order - and the SAME list [`ApiFailure::headline`]
    /// joins and [`Record::api_failure`] gates on, which is the point of there being one
    /// list. Two functions counting different fields is how a record that named only its
    /// quota came to render a headline reading `unknown`; there is no floor word here,
    /// because a failure with no parts is never built.
    fn parts(&self) -> Vec<String> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(e) = &self.error {
            parts.push(e.clone());
        }
        if let Some(s) = self.status {
            parts.push(format!("status {s}"));
        }
        if let Some(q) = &self.quota {
            // Which WINDOW was hit is the half a reader acts on; the reset instant is an
            // epoch second, which reads as noise in a label zone and rides JSON instead.
            match q.limit_type.as_deref() {
                Some(t) => parts.push(format!("quota {t}")),
                None => parts.push("quota".to_string()),
            }
        }
        if self.truncated_after_output {
            parts.push("truncated after output".to_string());
        }
        if self.aborted_mid_stream {
            parts.push("aborted mid-stream".to_string());
        }
        parts
    }

    /// The one-line headline a label zone carries, or `None` when the record named
    /// nothing at all - which is exactly when [`Record::api_failure`] builds no failure.
    #[must_use]
    pub fn headline(&self) -> Option<String> {
        let parts = self.parts();
        (!parts.is_empty()).then(|| parts.join(" "))
    }
}

/// The subtype whose `toolUseID` is a HOOK RUN id.
///
/// The key is gated on it because the name reads like an API tool_use id and is not one:
/// measured over two projects roots, all 6,849 and all 6,308 carriers sit on this subtype
/// and none of them resolves to a `uuid` of its own file.
const HOOK_RUN_SUBTYPE: &str = "stop_hook_summary";

impl Record {
    /// The exposed keys this record names, or `None` when it names none. Every accessor
    /// below reads through it, so "the record does not say" and "the key is absent" are
    /// one answer rather than two.
    #[must_use]
    pub fn exposed_keys(&self) -> Option<&Exposed> {
        self.exposed.as_deref()
    }

    /// The five attribution keys, when the record carries at least one. `None` on every
    /// record that names none - which is every record of a main-lane transcript with no
    /// skill, plugin or MCP tool in play.
    #[must_use]
    pub fn attribution(&self) -> Option<Attribution> {
        let k = self.exposed_keys()?;
        let any = k.attribution_agent.is_some()
            || k.attribution_skill.is_some()
            || k.attribution_plugin.is_some()
            || k.attribution_mcp_server.is_some()
            || k.attribution_mcp_tool.is_some();
        let own = |v: &Option<Box<str>>| v.as_deref().map(str::to_string);
        any.then(|| Attribution {
            agent: own(&k.attribution_agent),
            skill: own(&k.attribution_skill),
            plugin: own(&k.attribution_plugin),
            mcp_server: own(&k.attribution_mcp_server),
            mcp_tool: own(&k.attribution_mcp_tool),
        })
    }

    /// The API-failure facts of a SYNTHETIC assistant record.
    ///
    /// TWO gates, not one. `isApiErrorMessage` is the flag the one failure factory sets,
    /// so it says the record IS a failure placeholder - and a `system`/`api_error` record,
    /// which carries none, never reaches here. The second gate refuses a failure that SAYS
    /// nothing, because five members reading four nulls and a `false` answer no question
    /// while the `false` pair reads as a positive "no" where the record simply did not say.
    /// Measured, the second gate changes no live record: 925 of 925 flag carriers on one
    /// projects root and 998 of 998 on another name at least one part.
    #[must_use]
    pub fn api_failure(&self) -> Option<ApiFailure> {
        if self.is_api_error_message != Some(true) {
            return None;
        }
        let k = self.exposed_keys()?;
        let f = ApiFailure {
            error: self.error_kind().map(str::to_string),
            status: k.api_error_status,
            quota: k
                .quota_limits
                .as_deref()
                .filter(|q| QuotaSummary::says_something(q))
                .cloned(),
            truncated_after_output: k.truncated_after_output.unwrap_or(false),
            aborted_mid_stream: k.is_aborted_mid_stream.unwrap_or(false),
        };
        // ONE list decides both the gate and the headline, so a failure that renders can
        // never say nothing and one that says nothing can never render.
        f.headline().is_some().then_some(f)
    }

    /// The failure KIND. `error` is TWO shapes under one name - a STRING on the synthetic
    /// assistant record and an OBJECT on a `type:"system"` `api_error` record - and the
    /// field's decoder keeps only the string, so no path can reach the object's `headers`.
    #[must_use]
    pub fn error_kind(&self) -> Option<&str> {
        self.exposed_keys()?.error.as_deref()
    }

    /// The HOOK RUN a `stop_hook_summary` record reports on. `toolUseID` is read on that
    /// subtype and on no other: the name reads like an API tool_use id, and a later build
    /// putting a real one under the same name on another shape must not be misread.
    #[must_use]
    pub fn hook_run_id(&self) -> Option<&str> {
        if self.subtype.as_deref() != Some(HOOK_RUN_SUBTYPE) {
            return None;
        }
        self.exposed_keys()?.tool_use_id_key.as_deref()
    }
}
