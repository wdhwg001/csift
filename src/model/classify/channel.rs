//! The csift-channel leaf: a delivery envelope carried inside a hook-context attachment.

use super::*;

impl Record {
    /// The envelope header prefix EVERY csift-channel chunk opens with. The trailing space
    /// is part of the marker: it separates the version token from the first `key=value`
    /// field, so a future `v11` header can never be read as a `v1` one.
    pub(crate) const CSIFT_CHANNEL_HEADER_PREFIX: &'static str = "[csift-channel v1 ";

    /// The RAW-BYTES candidate needle for the same envelope (the prefilter form, without
    /// the opening bracket). Kept beside the prefix so the byte scan and the classifier
    /// can never drift apart; a unit test pins that the needle is a run of the prefix.
    /// Safe as a candidate needle by the SPEC 7d/R13 laws: it is a bare VALUE substring
    /// of the injected content, carries no JSON-escaped character, and survives any
    /// reserialization of the surrounding object.
    pub(crate) const CSIFT_CHANNEL_NEEDLE: &'static str = "csift-channel v1";

    /// The FIRST content string of a hook-injected `hook_additional_context` attachment,
    /// when that string opens with the csift-channel envelope header - the delivery text
    /// (header line + body chunk) exactly as the lane received it. `None` for every other
    /// record.
    ///
    /// VERBATIM by design, nothing fabricated: the text is a byte substring of the source
    /// line's decoded content, so the SPEC 7d literal prefilter and the 7f whole-file gate
    /// stay sound with no synthesized-marker machinery.
    ///
    /// The FIRST string only. One hook event can inject several blocks and the joined form
    /// is the `harness.meta.hook` view; only the block the header opens is the delivery.
    pub(crate) fn csift_channel_text(&self) -> Option<String> {
        // Cheap raw-byte gate before any payload parse: the needle is ASCII with no
        // JSON-escaped character, so a decoded match implies a raw match. An ordinary
        // hook context therefore pays one substring scan, never a second parse.
        if !self
            .attachment
            .as_ref()
            .is_some_and(|raw| raw.get().contains(Self::CSIFT_CHANNEL_NEEDLE))
        {
            return None;
        }
        let first = self.hook_context_first_content()?;
        first
            .starts_with(Self::CSIFT_CHANNEL_HEADER_PREFIX)
            .then_some(first)
    }

    /// The label set of a hook-injected `hook_additional_context` attachment. A csift-channel
    /// delivery is a MESSAGE addressed at this lane, so it LEADS (the richest-view law) and
    /// the harness leaf follows on the same record - the delivery is still hook machinery on
    /// disk. Every other hook context carries `harness.meta.hook` alone.
    pub(crate) fn push_hook_classes(&self, out: &mut Vec<Class>) {
        if self.csift_channel_text().is_some() {
            push_unique(out, Class::CommChannel);
        }
        push_unique(out, Class::MetaHook);
    }

    /// `<sender lane> ⇨ self` for a csift-channel delivery, the sender read from the
    /// envelope header's `from=` field. `None` for every other record AND for a
    /// CONTINUATION chunk, whose header carries only `id=` and `part=`: an unnamed sender
    /// is reported as no direction rather than guessed.
    pub(crate) fn csift_channel_direction(&self, ctx: &ClassifyCtx) -> Option<(String, String)> {
        let from = csift_channel_from(&self.csift_channel_text()?)?;
        Some((from, ctx.owner_id.unwrap_or("self").to_string()))
    }

    /// The first content STRING of a `hook_additional_context` attachment payload (a bare
    /// string payload is tolerated, as in [`Record::hook_additional_context_text`]).
    /// `None` off any other record shape.
    fn hook_context_first_content(&self) -> Option<String> {
        if !self.is_type("attachment") {
            return None;
        }
        let v = self.attachment_value()?;
        let att = v.as_object()?;
        if att.get("type").and_then(serde_json::Value::as_str) != Some("hook_additional_context") {
            return None;
        }
        match att.get("content")? {
            serde_json::Value::String(s) => Some(s.clone()),
            // The first STRING element: a non-string element carries no injected text, so
            // skipping it reads the same block a receiver would have been shown first.
            serde_json::Value::Array(parts) => parts
                .iter()
                .find_map(serde_json::Value::as_str)
                .map(str::to_string),
            _ => None,
        }
    }
}

/// The `from=` value of a csift-channel envelope header: the run between ` from=` and the
/// next space or the header's closing `]`, read from the FIRST line only. `None` when the
/// header names no sender.
///
/// The needle is ` from=` with the `=`, so the neighbouring `from-session=` field can never
/// be read as the sender. An `external:<label>` sender whose label carries a space keeps its
/// first token - the header is space-delimited, so nothing past that token is recoverable,
/// and a truncated sender is preferable to swallowing the fields that follow it.
fn csift_channel_from(text: &str) -> Option<String> {
    let head = text.lines().next()?;
    let rest = head.split_once(" from=")?.1;
    let value = rest.split([' ', ']']).next()?;
    (!value.is_empty()).then(|| value.to_string())
}
