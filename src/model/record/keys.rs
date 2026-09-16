//! The EXPOSED KEYS: what a record says about the CALL behind it - which agent
//! definition, skill, plugin or MCP tool the request ran under, which API request and
//! which block of its response, why the call failed, what a person typed refusing a
//! tool, which scheduled task submitted the prompt, which hook run closed the turn.
//!
//! They belong to no single leaf, which is why they are a family of their own rather
//! than fields of one classification.
//!
//! WIDTH IS THE DESIGN CONSTRAINT, and it is paid down rather than ignored. Every parsed
//! line of every scan carries a whole `Record`, so seventeen new fields are seventeen
//! fields on every row - the C-41 lesson, which narrowed a spine row to 72 bytes for
//! exactly that reason. So every string here is `Box<str>` rather than `String` (8 bytes
//! saved apiece - the same trade `parse::SpineRow` makes), the two numbers are `u32`
//! rather than `u64`, `quotaLimits` is deserialized straight into the two-member summary
//! that is all csift reads of it rather than kept as a payload, and `error` goes through a
//! STRING-ONLY decoder that keeps the other shape of that key out of the record model
//! entirely. The measured result is in the width pin (`parse::tests::spine`).
//!
//! The alternative - lifting the keys from the source line on demand - was built and
//! measured rather than argued about, and it is why these are serde fields. A depth-1 key
//! walk at the one place a `Record` is built from bytes, behind an Aho-Corasick gate over
//! the fourteen key names, cost 48 to 79 percent of user CPU on the section 7 acid rows,
//! because it is a second whole-line pass on top of the one serde already makes. Widening
//! the row cost nothing measurable on the same rows. A lazier read at EMISSION time cannot
//! reach the bytes at all: the engine keeps a record's source line only for a line whose
//! `Record` cannot answer for it, which is the opposite of the population these keys ride.

use super::*;

/// A JSON string value, or `None` for ANY other shape.
///
/// This is what keeps the `type:"system"` `api_error` record's `error` OBJECT - whose
/// `headers` member holds the failed request's whole HTTP response header set - out of the
/// record model, rather than reading it in and remembering not to render it. The
/// sensitive-headers law of AGENTS section 4, enforced by the decoder.
pub(super) fn string_only<'de, D>(de: D) -> std::result::Result<Option<Box<str>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(de)?;
    Ok(match v {
        serde_json::Value::String(s) => Some(s.into_boxed_str()),
        _ => None,
    })
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
    #[serde(rename = "rateLimitType")]
    pub limit_type: Option<String>,
    /// `resetsAt` - epoch SECONDS at which that window reopens.
    #[serde(rename = "resetsAt")]
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
    /// The five attribution keys, when the record carries at least one. `None` on every
    /// record that names none - which is every record of a main-lane transcript with no
    /// skill, plugin or MCP tool in play.
    #[must_use]
    pub fn attribution(&self) -> Option<Attribution> {
        let any = self.attribution_agent.is_some()
            || self.attribution_skill.is_some()
            || self.attribution_plugin.is_some()
            || self.attribution_mcp_server.is_some()
            || self.attribution_mcp_tool.is_some();
        let own = |v: &Option<Box<str>>| v.as_deref().map(str::to_string);
        any.then(|| Attribution {
            agent: own(&self.attribution_agent),
            skill: own(&self.attribution_skill),
            plugin: own(&self.attribution_plugin),
            mcp_server: own(&self.attribution_mcp_server),
            mcp_tool: own(&self.attribution_mcp_tool),
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
        let f = ApiFailure {
            error: self.error_kind().map(str::to_string),
            status: self.api_error_status,
            quota: self
                .quota_limits
                .as_deref()
                .filter(|q| QuotaSummary::says_something(q))
                .cloned(),
            truncated_after_output: self.truncated_after_output.unwrap_or(false),
            aborted_mid_stream: self.is_aborted_mid_stream.unwrap_or(false),
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
        self.error.as_deref()
    }

    /// The HOOK RUN a `stop_hook_summary` record reports on. `toolUseID` is read on that
    /// subtype and on no other: the name reads like an API tool_use id, and a later build
    /// putting a real one under the same name on another shape must not be misread.
    #[must_use]
    pub fn hook_run_id(&self) -> Option<&str> {
        if self.subtype.as_deref() != Some(HOOK_RUN_SUBTYPE) {
            return None;
        }
        self.tool_use_id_key.as_deref()
    }
}
