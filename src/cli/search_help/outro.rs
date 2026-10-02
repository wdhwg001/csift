//! The closing notes of `search --help`: the two notes that end the JSON schema block.
//! They follow [`super::intro::INTRO`] directly, so the block reads as one piece, and the
//! six family regions come after the whole block - each its own column-0 head, the way
//! every other head on this page sits.
//!
//! The one leading SPACE is this file's, not the intro's. A space at the intro's tail
//! would trail the JSON paragraph on the runs where these notes are the last thing on it.

/// See the module doc.
pub(crate) const OUTRO: &str = " The footer's `transcript_ids` is the \
        per-TRANSCRIPT\n  matching-id set, named apart from `-l`'s owning-session ids. \
        Whole-document\n  `json.load` fails, so parse the stream line by line as JSONL, N \
        envelopes followed by\n  the footer.";
