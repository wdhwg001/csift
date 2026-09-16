//! The closing notes of `search --help`: the two parentheticals that end the JSON schema
//! paragraph. They follow [`super::intro::INTRO`] directly, so the block reads exactly as
//! it did before the split, and the six family regions come after the whole block - each
//! its own column-0 head, the way every other head on this page sits.
//!
//! The one leading SPACE is this file's, not the intro's. A space at the intro's tail
//! would trail the JSON paragraph on the runs where these notes are the last thing on it.

/// See the module doc.
pub(crate) const OUTRO: &str =
    " (`transcript_ids` is the per-TRANSCRIPT matching-id set, named apart from `-l`'s \
        owning-session ids.) (Whole-document `json.load` fails; parse line-by-line as JSONL: N \
        envelopes then the footer.)";
