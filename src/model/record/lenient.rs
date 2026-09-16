//! TOLERANT decoders for the EXPOSED KEYS: a value of an unexpected JSON type answers
//! `None` instead of failing the line.
//!
//! The rule these enforce is AGENTS section 4's, and the failure they exist to prevent is
//! specific rather than theoretical. A strictly typed `Option<u32>` field rejects
//! `"apiBlockIndex":"3"`, and serde's derive reports that as an error for the WHOLE record,
//! so csift books the line malformed and every surface loses its TEXT rather than just the
//! one key. That is the opposite trade: a key csift models for a marker is worth less than
//! the conversation on the same line. The crate already makes this trade twice, at
//! [`crate::model::Message::model`] (a raw `Value`, so an unexpected shape can never fail
//! the record) and at `Record::tur_probe` (every field typed to accept ANY JSON), and these
//! are the same discipline applied to a field that has a declared type.
//!
//! TOLERANT IS NOT COERCING. `"3"` decodes to `None`, never to `3`: csift reports what a
//! record says, and reading a string where the harness writes a number is a guess about
//! which of the two the writer meant. An absent key and a key of the wrong type therefore
//! give the SAME answer - this record does not say - which is the absence rule the family
//! already states at key level.
//!
//! Each decoder is a `deserialize_any` visitor rather than a `serde_json::Value`
//! round-trip: the exposed keys arrive through the `#[serde(flatten)]` map, where their
//! values are already buffered, and building a second tree per key to throw most of it
//! away would pay twice for the same bytes.

use super::keys::QuotaSummary;
use serde::de::{IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;

/// Drain a map and answer `None` - the arm every decoder takes for an object it does not
/// model. Draining rather than returning early is what keeps the parse positioned.
fn skip_map<'de, A: MapAccess<'de>>(mut map: A) -> std::result::Result<(), A::Error> {
    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
    Ok(())
}

/// The same for a sequence.
fn skip_seq<'de, A: SeqAccess<'de>>(mut seq: A) -> std::result::Result<(), A::Error> {
    while seq.next_element::<IgnoredAny>()?.is_some() {}
    Ok(())
}

/// A JSON string, or `None` for any other shape.
struct StrOrNone;

impl<'de> Visitor<'de> for StrOrNone {
    type Value = Option<Box<str>>;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a string, or any other JSON value (which reads as absent)")
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
        Ok(Some(v.into()))
    }

    fn visit_string<E: serde::de::Error>(self, v: String) -> std::result::Result<Self::Value, E> {
        Ok(Some(v.into_boxed_str()))
    }

    fn visit_bool<E: serde::de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_i64<E: serde::de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_u64<E: serde::de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: serde::Deserializer<'de>>(
        self,
        de: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        de.deserialize_any(self)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<Self::Value, A::Error> {
        skip_map(map)?;
        Ok(None)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<Self::Value, A::Error> {
        skip_seq(seq)?;
        Ok(None)
    }
}

/// A non-negative JSON integer, or `None` for any other shape (a negative, a float, a
/// string, an object). The keys that use it are an HTTP status, a block index and an epoch
/// second, none of which has a meaning below zero.
struct UintOrNone;

impl<'de> Visitor<'de> for UintOrNone {
    type Value = Option<u64>;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a non-negative integer, or any other JSON value (which reads as absent)")
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> {
        Ok(Some(v))
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> {
        Ok(u64::try_from(v).ok())
    }

    fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_bool<E: serde::de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_str<E: serde::de::Error>(self, _: &str) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: serde::Deserializer<'de>>(
        self,
        de: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        de.deserialize_any(self)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<Self::Value, A::Error> {
        skip_map(map)?;
        Ok(None)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<Self::Value, A::Error> {
        skip_seq(seq)?;
        Ok(None)
    }
}

/// A JSON boolean, or `None` for any other shape. `"yes"` is not `true` here for the same
/// reason `"3"` is not `3`: the record did not say what the field's type says it says.
struct BoolOrNone;

impl<'de> Visitor<'de> for BoolOrNone {
    type Value = Option<bool>;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a boolean, or any other JSON value (which reads as absent)")
    }

    fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<Self::Value, E> {
        Ok(Some(v))
    }

    fn visit_str<E: serde::de::Error>(self, _: &str) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_i64<E: serde::de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_u64<E: serde::de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: serde::Deserializer<'de>>(
        self,
        de: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        de.deserialize_any(self)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<Self::Value, A::Error> {
        skip_map(map)?;
        Ok(None)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<Self::Value, A::Error> {
        skip_seq(seq)?;
        Ok(None)
    }
}

/// A JSON object read as the two-member quota summary, or `None` for any other shape.
struct QuotaOrNone;

impl<'de> Visitor<'de> for QuotaOrNone {
    type Value = Option<Box<QuotaSummary>>;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("an object, or any other JSON value (which reads as absent)")
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> std::result::Result<Self::Value, A::Error> {
        // The members are themselves tolerant, so this can only fail on a malformed
        // document, which is the caller's error rather than a shape question.
        QuotaSummary::deserialize(serde::de::value::MapAccessDeserializer::new(map))
            .map(|q| Some(Box::new(q)))
    }

    fn visit_str<E: serde::de::Error>(self, _: &str) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_bool<E: serde::de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_i64<E: serde::de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_u64<E: serde::de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_f64<E: serde::de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: serde::Deserializer<'de>>(
        self,
        de: D,
    ) -> std::result::Result<Self::Value, D::Error> {
        de.deserialize_any(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> std::result::Result<Self::Value, A::Error> {
        skip_seq(seq)?;
        Ok(None)
    }
}

/// A JSON string value, or `None` for ANY other shape.
///
/// TWO rules meet in this one decoder. It is the tolerance rule above for the eleven
/// exposed keys that name something, and it is the SENSITIVE-HEADERS law of AGENTS
/// section 4 for `error`: that key is a STRING on the synthetic failure record and an
/// OBJECT on a `type:"system"` `api_error` record, and the object's `headers` member holds
/// the failed request's whole HTTP response header set. Refusing the object HERE keeps it
/// out of the record model altogether, rather than reading it in and remembering at every
/// render not to print it.
pub(super) fn string_only<'de, D>(de: D) -> std::result::Result<Option<Box<str>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    de.deserialize_any(StrOrNone)
}

/// A `u32`, from a non-negative integer that fits one. A value too large for the type
/// reads as absent for the same reason a string does: the record said something this field
/// cannot carry.
pub(super) fn u32_only<'de, D>(de: D) -> std::result::Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(de
        .deserialize_any(UintOrNone)?
        .and_then(|n| u32::try_from(n).ok()))
}

/// A `u64`, from a non-negative integer.
pub(super) fn u64_only<'de, D>(de: D) -> std::result::Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    de.deserialize_any(UintOrNone)
}

/// A boolean.
pub(super) fn bool_only<'de, D>(de: D) -> std::result::Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    de.deserialize_any(BoolOrNone)
}

/// The two-member quota summary, from an object.
pub(super) fn quota_only<'de, D>(de: D) -> std::result::Result<Option<Box<QuotaSummary>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    de.deserialize_any(QuotaOrNone)
}
