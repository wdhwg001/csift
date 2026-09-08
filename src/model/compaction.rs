//! Compaction MODE: which gesture minted a boundary + summary pair.
//!
//! Claude Code compacts a conversation three ways and writes the SAME two records for all
//! three - a `compact_boundary` system record carrying the metrics and a `type:"user"`
//! record carrying `isCompactSummary:true`. The two `/rewind` summarize gestures are
//! distinguished by ONE key, and it sits on the SUMMARY, not the boundary: the writer
//! emits `summarizeMetadata:{messagesSummarized, userContext, direction}` INSTEAD of
//! `isVisibleInTranscriptOnly` (a genuine either/or), with `direction` reading `"up_to"`
//! for "Summarize up to here" and `"from"` for "Summarize from here". The BOUNDARY of a
//! summarize carries `compactMetadata.messagesSummarized`, which an ordinary compaction
//! never writes, but it carries no direction - so a boundary learns its mode only by
//! pairing with the summary that follows it, which is what [`SummarizeIndex`] does.
//!
//! Consequence for the rest of csift: a summarize IS a compaction. It clips turns exactly
//! like an auto-compact, so `verbatim` reconstructs across it unchanged; the mode is a
//! label on the event, never a different event.

use super::*;
use std::collections::HashMap;

/// The compaction gesture a boundary + summary pair came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummarizeMode {
    /// An ordinary compaction (auto or manual `/compact`): the summary carries
    /// `isVisibleInTranscriptOnly` and no `summarizeMetadata`.
    Compact,
    /// `/rewind` -> "Summarize from here": everything from the selected message onward is
    /// summarized and the head is kept (`summarizeMetadata.direction == "from"`).
    FromHere,
    /// `/rewind` -> "Summarize up to here": everything up to the selected message is
    /// summarized and the tail is kept (`summarizeMetadata.direction == "up_to"`).
    UpToHere,
}

impl SummarizeMode {
    /// The stable machine slug (JSON `mode`).
    #[must_use]
    pub fn slug(self) -> &'static str {
        match self {
            SummarizeMode::Compact => "compact",
            SummarizeMode::FromHere => "summarize-from-here",
            SummarizeMode::UpToHere => "summarize-up-to-here",
        }
    }

    /// The harness's own `summarizeMetadata.direction` value; `None` for a plain compaction
    /// (which writes no `summarizeMetadata` at all).
    #[must_use]
    pub fn direction(self) -> Option<&'static str> {
        match self {
            SummarizeMode::Compact => None,
            SummarizeMode::FromHere => Some("from"),
            SummarizeMode::UpToHere => Some("up_to"),
        }
    }

    /// Map a verbatim `direction` value to a mode. An UNMODELED value yields `None` rather
    /// than a guess - a new gesture must not silently render as one of the two known ones.
    #[must_use]
    pub fn from_direction(d: &str) -> Option<Self> {
        match d {
            "from" => Some(SummarizeMode::FromHere),
            "up_to" => Some(SummarizeMode::UpToHere),
            _ => None,
        }
    }
}

impl Record {
    /// The verbatim `summarizeMetadata.direction` string when this record carries one.
    /// Read as-is (never mapped) so an unmodeled gesture still renders its own word.
    #[must_use]
    pub fn summarize_direction(&self) -> Option<&str> {
        self.summarize_metadata.as_ref()?.get("direction")?.as_str()
    }

    /// The compaction mode of a SUMMARY record: [`SummarizeMode::Compact`] when it carries no
    /// `summarizeMetadata`, the direction's mode when it carries a modeled one. `None` when
    /// this is not a compaction summary at all, or when the direction is a value csift does
    /// not model (the honest "unknown", never a guessed `compact`).
    #[must_use]
    pub fn summary_compaction_mode(&self) -> Option<SummarizeMode> {
        if !self.is_compact_summary.unwrap_or(false) {
            return None;
        }
        match self.summarize_direction() {
            None => Some(SummarizeMode::Compact),
            Some(d) => SummarizeMode::from_direction(d),
        }
    }
}

/// Boundary -> mode pairing for ONE transcript. A boundary's own metadata names no
/// direction, so its mode comes from the compaction SUMMARY that follows it: the first
/// `isCompactSummary` record after the boundary and before the NEXT boundary.
///
/// WINDOW-INDEPENDENT BY CONSTRUCTION: callers build this from the whole parsed record set,
/// BEFORE any `--turn` window, `--max-count` cap or `show` address narrows what is emitted,
/// so addressing a boundary alone still reports its mode. A windowed read must not turn a
/// known gesture into an unknown one. What DOES leave a boundary unpaired is the file
/// itself: no summary after it (a clone head, a truncated tail) or a second boundary first.
/// Such a boundary stays `None` - never a guessed `compact`.
#[derive(Debug, Default)]
pub struct SummarizeIndex {
    by_boundary_uuid: HashMap<String, SummarizeMode>,
}

impl SummarizeIndex {
    /// Build the pairing from a transcript's records IN FILE ORDER. One pass, one map that
    /// allocates only on the first pair (compactions are rare: the map stays empty on the
    /// overwhelming majority of transcripts).
    ///
    /// PERF (SPEC section 7): this runs on every record of every scanned file, so the loop
    /// body must be two enum-TAG loads on the common record and nothing more. A boundary is
    /// the only shape carrying a `subtype`, and `subtype.is_some()` is a discriminant read,
    /// so it short-circuits the string compare inside [`Record::is_compact_boundary`] for
    /// every user and assistant record; the summary arm compares an `Option<bool>` before
    /// reaching [`Record::summary_compaction_mode`]. Never reorder these so the string
    /// compare runs first.
    pub fn from_records<'a>(records: impl IntoIterator<Item = &'a Record>) -> Self {
        let mut by_boundary_uuid = HashMap::new();
        let mut pending: Option<&str> = None;
        for rec in records {
            if rec.subtype.is_some() && rec.is_compact_boundary() {
                // A second boundary before any summary leaves the first one unpaired.
                pending = rec.uuid.as_deref();
                continue;
            }
            if rec.is_compact_summary != Some(true) {
                continue;
            }
            if let Some(mode) = rec.summary_compaction_mode() {
                if let Some(uuid) = pending.take() {
                    by_boundary_uuid.insert(uuid.to_string(), mode);
                }
            }
        }
        SummarizeIndex { by_boundary_uuid }
    }

    /// The mode paired to this boundary uuid; `None` when unpaired or unknown.
    #[must_use]
    pub fn mode_for_boundary(&self, uuid: Option<&str>) -> Option<SummarizeMode> {
        self.by_boundary_uuid.get(uuid?).copied()
    }
}
