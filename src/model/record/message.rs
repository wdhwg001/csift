//! The `message` object of a user or assistant record, and the token counts on it.
//!
//! Split from the record root at the 600-line structure gate. Nothing here changed in the
//! move: these are the same shapes the root carried, at the same bytes.

use super::*;

/// The `message` object on user / assistant records.
#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub role: Option<String>,

    /// Either a bare string (genuine text) or an array of typed blocks.
    #[serde(default)]
    pub content: Option<Content>,

    /// The model id on assistant records (`claude-...`). Kept as a raw `Value` so an
    /// unexpected shape can never fail the whole record (tolerance discipline); read
    /// via `as_str`. Consumed by `stats`.
    #[serde(default)]
    pub model: Option<serde_json::Value>,
    /// `stop_reason` on assistant messages (`tool_use` / `end_turn` / `stop_sequence` /
    /// `max_tokens` / `refusal` / null). Persisted per record; trustworthy on the MAIN
    /// lane (measured 0.0-0.3% null), NORMALLY null mid-message on subagent lanes (which
    /// flush per content block). Read by the live-truth surfaces. Additive + tolerant.
    #[serde(default, rename = "stop_reason")]
    pub stop_reason: Option<String>,

    /// `stop_details` on assistant messages: the API's own structured account of WHY it
    /// stopped, written when `stop_reason` reads `refusal`. The client does not build the
    /// value - it copies the object the response delta carried, after removing one member
    /// - so the shape is the API's and an added member survives into the record.
    ///
    /// BOXED, and [`Record`]'s width is why: every parsed line of every scan pays
    /// `size_of::<Record>()`, and `Message` sits inside it, so four `Option`s here would
    /// be four fields on every row for a key almost no record carries. Behind a pointer
    /// the field costs 8 bytes whatever the object grows to, and the allocation is paid
    /// only by a record that names it. Additive + tolerant: an unexpected JSON type reads
    /// as absent rather than failing the record.
    #[serde(
        default,
        rename = "stop_details",
        deserialize_with = "lenient::stop_details_only"
    )]
    pub stop_details: Option<Box<StopDetails>>,

    /// The API message id (`msg_...`). One API message spans MULTIPLE records (CC
    /// writes one line per content block and repeats the message envelope on each),
    /// so this is the usage-dedupe key for `stats`. Additive + tolerant.
    #[serde(default)]
    pub id: Option<String>,

    /// The token-usage echo on assistant records. Kept UNPARSED (same rationale as
    /// `Record::tool_use_result`): only `stats` reads it, via [`Message::token_usage`].
    #[serde(default)]
    pub usage: Option<Box<serde_json::value::RawValue>>,
}

/// The API's structured account of why it stopped a response - `message.stop_details`.
///
/// Four members, each tolerant, each answering `None` when the record does not name it or
/// names it with an unexpected type. The object is the API's: the client copies it from
/// the response delta onto every assistant record of the turn, strips exactly one member
/// it does not persist, and constructs its own only on paths that set every remaining
/// member to null. So a member csift models no field for still rides the line and is
/// reachable with `--raw`, and `fallback_has_prefill_claim` reads null on every record the
/// client built rather than received.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct StopDetails {
    /// The stop KIND - `refusal` on every measured record. Open set, kept verbatim.
    #[serde(rename = "type", deserialize_with = "lenient::string_only")]
    pub r#type: Option<Box<str>>,
    /// Which classifier band stopped it. Open set, kept verbatim.
    #[serde(deserialize_with = "lenient::string_only")]
    pub category: Option<Box<str>>,
    /// The API's own sentence about the stop, when it sent one.
    #[serde(deserialize_with = "lenient::string_only")]
    pub explanation: Option<Box<str>>,
    /// Whether a fallback response claimed a prefill. Null on every record the client
    /// constructed, so a `true` can only have come from the API.
    #[serde(
        rename = "fallback_has_prefill_claim",
        deserialize_with = "lenient::bool_only"
    )]
    pub fallback_has_prefill_claim: Option<bool>,
}

impl StopDetails {
    /// Does this object name anything at all? An object whose every member is null says
    /// only that the key was there, so [`Record::stop_details`] refuses it - the same gate
    /// the quota summary and the API-failure set already take.
    pub(crate) fn says_something(&self) -> bool {
        self.r#type.is_some()
            || self.category.is_some()
            || self.explanation.is_some()
            || self.fallback_has_prefill_claim.is_some()
    }
}

/// The token-usage fields `stats` sums (each optional + tolerant - a missing/odd
/// field reads as absent, never an error).
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
}

impl Message {
    /// Parse the raw `usage` blob into the typed probe on demand (`None` when absent
    /// or shaped unexpectedly - tolerant, never an error).
    #[must_use]
    pub fn token_usage(&self) -> Option<TokenUsage> {
        let raw = self.usage.as_ref()?;
        serde_json::from_str(raw.get()).ok()
    }

    /// The model id string, when present and a string.
    #[must_use]
    pub fn model_id(&self) -> Option<&str> {
        self.model.as_ref().and_then(serde_json::Value::as_str)
    }
}

/// Typed probe of the SMALL `toolUseResult` fields the hot paths consult (see
/// [`Record::tur_probe`]). Every field is an `Option<Value>` - a tiny scalar/map tree
/// that accepts ANY JSON type, so one oddly-typed field can never fail the whole probe
/// (each accessor then applies the same `as_str`/`as_bool`/`as_object` coercion the
/// former `.get(...)` chains did - byte-identical semantics). Crucially, the blob's HUGE
/// unlisted values (file bodies, stdout echoes, structured patches) are skipped by
/// serde's ignore path without ever being allocated.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct TurProbe {
    pub(crate) r#type: Option<serde_json::Value>,
    #[serde(rename = "filePath")]
    pub(crate) file_path: Option<serde_json::Value>,
    #[serde(rename = "persistedOutputPath")]
    pub(crate) persisted_output_path: Option<serde_json::Value>,
    pub(crate) status: Option<serde_json::Value>,
    #[serde(rename = "isAsync")]
    pub(crate) is_async: Option<serde_json::Value>,
    pub(crate) answers: Option<serde_json::Value>,
    /// AskUserQuestion's freeform answer: the text an answerer typed instead
    /// of a structured option. Written beside an EMPTY `answers` map, synthesized as
    /// `The user responded: <text>`; the TUI dialog never writes it (its Other entry
    /// lands in `answers`), so it comes from an answerer outside the dialog.
    pub(crate) response: Option<serde_json::Value>,
}
