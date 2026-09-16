//! The closing notes of `search --help`: the two parentheticals that end the JSON
//! schema paragraph. They sit AFTER every family section so a family's own sentences
//! land inside the schema discussion rather than after its last word.

/// See the module doc.
pub(crate) const OUTRO: &str =
    " (`transcript_ids` is the per-TRANSCRIPT matching-id set, named apart from `-l`'s \
        owning-session ids.) (Whole-document `json.load` fails; parse line-by-line as JSONL: N \
        envelopes then the footer.)";
