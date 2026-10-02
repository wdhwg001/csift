//! The JSON keys a `show --format json` RECORD row carries, as a table with a family
//! column rather than as one sentence in the help text.
//!
//! The enumeration inside `show --help` is a contract: a consumer reads it to know which
//! keys exist. Six taxonomy families add keys to that row, so the list is a TABLE rather
//! than a string: each family appends its own rows and the help renders what the table
//! says, where one string would put all six families on one line.
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
    k(Family::Shared, "superseded_draft"),
    k(Family::Shared, "line (null for a sidecar-merged record)"),
    k(Family::Shared, "uuid"),
    k(Family::Shared, "label"),
    k(Family::Shared, "labels:[…]"),
    k(Family::Shared, "tool_name"),
    k(Family::Shared, "from"),
    k(Family::Shared, "to"),
    k(Family::Shared, "pairing (paired | pending | orphan | null)"),
    k(Family::Shared, "tool_use_id"),
    k(Family::Shared, "queue_operation"),
    k(Family::Shared, "queue_reason"),
    // The per-record facts a `search` hit carried and this row did not, until both were
    // built by one projector: a row here is what a hit's `refetch` prints.
    k(Family::Shared, "delivered"),
    k(Family::Shared, "is_error"),
    k(Family::Shared, "denial_kind"),
    k(Family::Shared, "task_ids:[…]"),
    k(Family::Shared, "orphan_kind"),
    k(Family::Shared, "resume_paired"),
    k(Family::Shared, "scheduled_at"),
    k(Family::Shared, "source (\"elicitation-sidecar\" | null)"),
    k(Family::Shared, "survival (live | pre-cut | abandoned)"),
    k(Family::Shared, "abandoned_root_line"),
    k(Family::Shared, "replay_copy_of"),
    // A draft row's distance from the message that replaced it, and that message's
    // address; null on every other row. The row carried them before the list named them.
    k(Family::Shared, "superseding_line"),
    k(Family::Shared, "superseding_uuid"),
    k(Family::Shared, "diff_chars"),
    k(Family::Shared, "diff_pct"),
    k(Family::Shared, "diff_exact"),
    k(Family::Shared, "ts_utc"),
    k(Family::Shared, "ts_local"),
    k(Family::Shared, "text"),
    k(Family::Shared, "body"),
    k(Family::Shared, "image_ids:[…]"),
    k(Family::Shared, "mode"),
    k(Family::Shared, "compact_metadata"),
    // The PROVENANCE edge, and it is SHARED. Four families build edges and every one of
    // them folds into this ONE array, so the key belongs to no family: four families
    // listing it produced four rows spelled four ways, which reads as four keys. A family
    // that starts carrying edges adds no row at all.
    k(
        Family::Shared,
        "for:[{kind,basis,target:{line,uuid,session,note},reason}]",
    ),
    // The provenance ROOT, and it is on EVERY record row: which message this one
    // corresponds to. Always an object - a unit that opens its own turn says so in `note`
    // rather than leaving the key out, because "this is the opener" and "csift did not
    // look" are different answers.
    k(
        Family::Shared,
        "for_root:{line,uuid,hops,note} (the record that opened this unit's turn)",
    ),
    // The family's own one, on the line whose relation names an INSTANT rather than a
    // record or a file - the one relation no `for` target can express.
    k(Family::Bookkeeping, "checkpoint_closes_at"),
    k(Family::Bookkeeping, "checkpoint_join"),
    k(
        Family::Thinking,
        "signature_len (thinking blocks; null when the block carries no signature field)",
    ),
    k(Family::Thinking, "thinking_text_absent"),
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
    // The ATTACHMENT family: which of an attachment record's two copies of itself the body
    // shows, whether a second copy for a human turn differs from it, and what produced the
    // record when the producer is a command.
    k(
        Family::Attachments,
        "body_source (\"rendered\" | \"payload\")",
    ),
    k(Family::Attachments, "human_turn_render_differs"),
    k(
        Family::Attachments,
        "produced_by:{command,hook_name,payload_type}",
    ),
    // The SYSTEM family's per-event keys, in the order each leaf emits them, each present
    // only when the line carried it. `error` carries the failure's type and message ONLY -
    // an `api_error` line's `error.headers` reaches no rendered or JSON path, and `--raw`
    // is the way to those bytes.
    k(Family::System, "retried"),
    k(Family::System, "original_model"),
    k(Family::System, "fallback_model"),
    k(Family::System, "category"),
    k(Family::System, "status"),
    k(Family::System, "retry_attempt"),
    k(Family::System, "max_retries"),
    k(Family::System, "retry_in_ms"),
    k(
        Family::System,
        "error (type and message only; never headers)",
    ),
    k(Family::System, "task_id"),
    k(Family::System, "cron"),
    k(Family::System, "prompt (cut at 200 chars)"),
    k(Family::System, "served_by"),
    k(Family::System, "fell_back_from"),
    // The EXPOSED KEYS: facts a record states about the CALL behind it. Each is absent
    // from a record that does not name it - the absence rule is read at key level.
    k(
        Family::Keys,
        "attribution:{agent skill plugin mcp_server mcp_tool}",
    ),
    k(Family::Keys, "request_id"),
    k(Family::Keys, "api_block_index"),
    k(Family::Keys, "tool_ends_turn"),
    k(Family::Keys, "user_feedback"),
    k(
        Family::Keys,
        "api_failure:{error status quota{limit_type resets_at_utc resets_at_local} \
         truncated_after_output aborted_mid_stream}",
    ),
    // The two OBJECT-VALUED keys of this family sit together, ahead of the scalar joins,
    // so a reader meets the shapes first and the ids after. Position is a choice and
    // nothing more, and the pin is what keeps it one: a reader that re-parses the rendered
    // list by trimming every brace off both ends cannot read a row whose form ends in `}`
    // when it sits LAST (`cli::tests::docs::an_object_valued_key_parses_last` is the pin).
    k(
        Family::Keys,
        "stop_details:{type category explanation fallback_has_prefill_claim}",
    ),
    k(Family::Keys, "stop_reason"),
    k(Family::Keys, "pdf_pages:{first last count path}"),
    k(Family::Keys, "interrupted_message_id"),
    k(Family::Keys, "hook_run_id"),
    k(Family::Keys, "scheduled_task_id"),
    k(Family::Keys, "scheduled_fire_id"),
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
