//! The JSON keys a `show --format json` RECORD row carries, as a table with a family
//! column rather than as one sentence in the help text.
//!
//! The enumeration inside `show --help` is a contract: a consumer reads it to know which
//! keys exist. Six taxonomy families add keys to that row, and while the list was a
//! string every one of them had to edit the same line - so the list is a TABLE now, each
//! family appends its own rows, and the help renders what the table says.
//!
//! The family column is what ORDERS the rendering: the shared keys print first, in the
//! order they always have, then each family's own block in the taxonomy's family order.
//! So a row appended anywhere in the table lands in its owner's block, and two lanes
//! appending at once cannot interleave their keys.

/// Which lane owns a row. `Shared` is the engine's own key set - the id trio, the
/// address, the label, the text - which no family may edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    Shared,
    Bookkeeping,
    Attachments,
    Injections,
    System,
    Thinking,
    Keys,
}

/// One key on a `show` record row: its rendered spelling and the lane that owns it.
///
/// The spelling is what the help prints, so a key whose VALUE set is worth naming carries
/// it here (`pairing (paired | pending | orphan | null)`) exactly as the sentence did.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RecordKey {
    /// The lane that owns this row.
    pub(crate) family: Family,
    /// The key as `show --help` prints it, value set included where the sentence had one.
    pub(crate) rendered: &'static str,
}

/// One row per key, in RENDER order. The `Shared` rows are the engine's own and are the
/// list the help carried before the table existed, verbatim and in the same order; a
/// family appends its rows at the end of the block it owns.
pub(crate) const RECORD_KEYS: &[RecordKey] = &[
    k(Family::Shared, "session_id"),
    k(Family::Shared, "is_subagent"),
    k(Family::Shared, "parent_session_id"),
    k(Family::Shared, "turn_index"),
    k(Family::Shared, "line (null for a sidecar-merged record)"),
    k(Family::Shared, "uuid"),
    k(Family::Shared, "label"),
    k(Family::Shared, "labels:[…]"),
    k(Family::Shared, "tool_name"),
    k(Family::Shared, "from"),
    k(Family::Shared, "to"),
    k(Family::Shared, "pairing (paired | pending | orphan | null)"),
    k(Family::Shared, "tool_use_id"),
    k(Family::Shared, "source (\"elicitation-sidecar\" | null)"),
    k(Family::Shared, "survival (live | pre-cut | abandoned)"),
    k(Family::Shared, "abandoned_root_line"),
    k(Family::Shared, "replay_copy_of"),
    k(Family::Shared, "ts_utc"),
    k(Family::Shared, "ts_local"),
    k(Family::Shared, "text"),
    k(Family::Shared, "body"),
    k(Family::Shared, "image_ids:[…]"),
    k(Family::Shared, "mode"),
    k(Family::Shared, "compact_metadata"),
    // The SHARED provenance edge, listed under the first family to build one: a
    // `harness.bookkeeping` unit is the first record row that can carry `for`, and the
    // key is one array whatever family filled it, so a later family finds this row
    // already here and adds none of its own.
    k(
        Family::Bookkeeping,
        "for:[…] (kind/basis/reason + target{line/uuid/session/note})",
    ),
    // The family's own two, on the two lines whose relation names no record.
    k(Family::Bookkeeping, "continued_in"),
    k(Family::Bookkeeping, "checkpoint_closes_at"),
    k(Family::Bookkeeping, "checkpoint_join"),
    k(
        Family::Thinking,
        "signature_len (thinking blocks; null when the block carries no signature field)",
    ),
    k(Family::Thinking, "thinking_text_absent"),
    // A rendered spelling carries no `", "`: the help joins the rows with exactly that
    // separator, so an inner one would read as two keys.
    k(
        Family::Injections,
        "for:[{kind|basis|target|reason}] (what an injection was written for)",
    ),
    k(
        Family::Injections,
        "relayed (a person's message the harness delivered into this lane)",
    ),
    k(
        Family::Injections,
        "peer_sender_task_id (the lane a peer relay came from)",
    ),
    k(
        Family::Injections,
        "image_only:{images|documents} (a textless harness.context.image record)",
    ),
];

/// One row, positionally - a `const fn` so the table stays a `const` slice.
pub(crate) const fn k(family: Family, rendered: &'static str) -> RecordKey {
    RecordKey { family, rendered }
}

/// The RENDER order of the blocks: the engine's own keys, then each family in the
/// taxonomy's family order. Two lanes appending at the same time land in their own
/// blocks whatever order their rows end up in the table.
pub(crate) const FAMILY_ORDER: [Family; 7] = [
    Family::Shared,
    Family::Bookkeeping,
    Family::Attachments,
    Family::Injections,
    Family::System,
    Family::Thinking,
    Family::Keys,
];

/// The keys as `show --help` prints them: `{a, b, c}`, block by block.
///
/// Built rather than typed, so a family that appends a row changes the help by changing
/// the table - which is the whole point of the table.
pub(crate) fn record_row_keys() -> String {
    let names: Vec<&str> = FAMILY_ORDER
        .iter()
        .flat_map(|f| {
            RECORD_KEYS
                .iter()
                .filter(move |r| r.family == *f)
                .map(|r| r.rendered)
        })
        .collect();
    format!("{{{}}}", names.join(", "))
}
